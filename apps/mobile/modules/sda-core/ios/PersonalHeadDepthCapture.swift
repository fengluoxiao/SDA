import Foundation
import AVFoundation
import UIKit
import ImageIO

struct PersonalHeadDepthFrame {
 let color: CVPixelBuffer
 let depth: AVDepthData?
 let timestamp: TimeInterval
 let orientation: CGImagePropertyOrientation
}
// A synchronized native front TrueDepth stream, including views without a
// tracked face. Capture does not use face geometry or a rear-camera fallback.
final class PersonalHeadDepthCapture: NSObject, AVCaptureDataOutputSynchronizerDelegate {
 private let session = AVCaptureSession()
 private let sessionQueue = DispatchQueue(label:"app.sda.head-depth-session",qos:.userInitiated)
 private let outputQueue = DispatchQueue(label:"app.sda.head-depth-frames",qos:.userInitiated)
 private let video = AVCaptureVideoDataOutput(), depth = AVCaptureDepthDataOutput()
 private var synchronizer: AVCaptureDataOutputSynchronizer?
 private var frameOrientation: () -> CGImagePropertyOrientation = {
  switch UIDevice.current.orientation {
  case .landscapeLeft: return .up
  case .landscapeRight: return .down
  case .portraitUpsideDown: return .right
  default: return .left
  }
 }
 private let lock = NSLock()
 private var cancelled = false, started = false
 private var frameCallback: ((PersonalHeadDepthFrame) -> Void)?
 private var failureCallback: ((Error) -> Void)?
 private var observers: [NSObjectProtocol] = []
 lazy var previewLayer: AVCaptureVideoPreviewLayer = {
  let layer = AVCaptureVideoPreviewLayer(session:session); layer.videoGravity = .resizeAspectFill; return layer
 }()
 private var isCancelled: Bool { lock.lock(); defer { lock.unlock() }; return cancelled }
 deinit { for observer in observers { NotificationCenter.default.removeObserver(observer) } }
 func start(frame: @escaping (PersonalHeadDepthFrame) -> Void, failure: @escaping (Error) -> Void) {
  lock.lock()
  guard !cancelled, !started else { lock.unlock(); return }
  started = true; frameCallback = frame; failureCallback = failure; lock.unlock()
  AVCaptureDevice.requestAccess(for:.video) { [weak self] allowed in
   guard let self else { return }
   self.sessionQueue.async {
    guard !self.isCancelled else { return }
    guard allowed else { self.report("需要相机权限才能采集头耳深度，可以在系统设置中开启"); return }
    do {
     try self.configure()
     guard !self.isCancelled else { return }
     self.observers = [AVCaptureSession.runtimeErrorNotification,AVCaptureSession.wasInterruptedNotification].map { name in
      NotificationCenter.default.addObserver(forName:name,object:self.session,queue:nil) { [weak self] _ in
       guard let self else { return }; self.sessionQueue.async { self.report("前置深度采集已中断，请保持应用在前台重新采集") }
      }
     }
     self.session.startRunning()
     if !self.session.isRunning { self.report("前置 TrueDepth 采集未能启动") }
    } catch { self.report(error.localizedDescription) }
   }
  }
 }
 func stop() {
  // Clear callbacks immediately, including while a permission request is open.
  // A later grant checks this flag before configuring/starting the camera.
  lock.lock(); cancelled = true; frameCallback = nil; failureCallback = nil; lock.unlock()
  sessionQueue.async { [self] in
   synchronizer?.setDelegate(nil,queue:nil)
   if session.isRunning { session.stopRunning() }
  }
 }
 private func report(_ message:String) {
  lock.lock()
  guard !cancelled else { lock.unlock(); return }
  let callback = failureCallback; cancelled = true; frameCallback = nil; failureCallback = nil; lock.unlock()
  synchronizer?.setDelegate(nil,queue:nil)
  if session.isRunning { session.stopRunning() }
  DispatchQueue.main.async { callback?(PersonalHrtfError.message(message)) }
 }
 private func configure() throws {
  guard let device = AVCaptureDevice.default(.builtInTrueDepthCamera,for:.video,position:.front) else {
   throw PersonalHrtfError.message("此设备没有可用的前置 TrueDepth 相机")
  }
  let input = try AVCaptureDeviceInput(device:device)
  // This video/depth-only session must not reconfigure music playback audio.
  session.automaticallyConfiguresApplicationAudioSession = false
  session.beginConfiguration(); defer { session.commitConfiguration() }
  guard session.canSetSessionPreset(.hd1280x720), session.canAddInput(input) else {
   throw PersonalHrtfError.message("无法配置前置 TrueDepth 视频输入")
  }
  session.sessionPreset = .hd1280x720; session.addInput(input)
  guard session.canAddOutput(video), session.canAddOutput(depth) else {
   throw PersonalHrtfError.message("此设备不能同步输出前置视频与深度")
  }
  session.addOutput(video); session.addOutput(depth)
  video.videoSettings = [kCVPixelBufferPixelFormatTypeKey as String:Int(kCVPixelFormatType_32BGRA)]
  video.alwaysDiscardsLateVideoFrames = true
  depth.isFilteringEnabled = false; depth.alwaysDiscardsLateDepthData = true
  let formats = device.activeFormat.supportedDepthDataFormats.filter { format in
   let dimensions = CMVideoFormatDescriptionGetDimensions(format.formatDescription)
   return CMFormatDescriptionGetMediaSubType(format.formatDescription) == kCVPixelFormatType_DepthFloat32 &&
    dimensions.width <= 640 && dimensions.height <= 640 &&
    format.videoSupportedFrameRateRanges.contains { $0.minFrameRate <= 30 && $0.maxFrameRate >= 30 }
  }
  guard let format = formats.max(by: {
   CMVideoFormatDescriptionGetDimensions($0.formatDescription).width < CMVideoFormatDescriptionGetDimensions($1.formatDescription).width
  }) else { throw PersonalHrtfError.message("缺少可用的 30 fps 前置标定深度格式") }
  try device.lockForConfiguration()
  device.activeVideoMinFrameDuration = CMTime(value:1,timescale:30)
  device.activeVideoMaxFrameDuration = CMTime(value:1,timescale:30)
  device.activeDepthDataFormat = format
  device.activeDepthDataMinFrameDuration = CMTime(value:1,timescale:30)
  device.isSubjectAreaChangeMonitoringEnabled = false
  device.unlockForConfiguration()
  guard let videoConnection = video.connection(with:.video), let depthConnection = depth.connection(with:.depthData) else {
   throw PersonalHrtfError.message("缺少同步视频／深度连接")
  }
  for connection in [videoConnection,depthConnection] {
   connection.isEnabled = true
   if connection.isVideoMirroringSupported { connection.automaticallyAdjustsVideoMirroring = false; connection.isVideoMirrored = false }
  }
  if let preview = previewLayer.connection, preview.isVideoMirroringSupported {
   preview.automaticallyAdjustsVideoMirroring = false; preview.isVideoMirrored = true
  }
  if #available(iOS 17.0, *) {
   let rotation = AVCaptureDevice.RotationCoordinator(device:device,previewLayer:nil)
   frameOrientation = {
    let degrees = rotation.videoRotationAngleForHorizonLevelCapture - videoConnection.videoRotationAngle
    switch (Int((degrees/90).rounded()) % 4 + 4) % 4 {
    case 1: return .right
    case 2: return .down
    case 3: return .left
    default: return .up
    }
   }
  }
  if videoConnection.isCameraIntrinsicMatrixDeliverySupported { videoConnection.isCameraIntrinsicMatrixDeliveryEnabled = true }
  let sync = AVCaptureDataOutputSynchronizer(dataOutputs:[video,depth]); synchronizer = sync
  sync.setDelegate(self,queue:outputQueue)
 }
 func dataOutputSynchronizer(_ synchronizer:AVCaptureDataOutputSynchronizer,didOutput data:AVCaptureSynchronizedDataCollection) {
  guard let color = data.synchronizedData(for:video) as? AVCaptureSynchronizedSampleBufferData,
        !color.sampleBufferWasDropped,
        let buffer = CMSampleBufferGetImageBuffer(color.sampleBuffer) else { return }
  let timestamp = CMTimeGetSeconds(color.timestamp)
  guard timestamp.isFinite else { return }
  var measured: AVDepthData?
  if let distance = data.synchronizedData(for:depth) as? AVCaptureSynchronizedDepthData,
     !distance.depthDataWasDropped, abs(timestamp-CMTimeGetSeconds(distance.timestamp)) <= 0.04 {
   let candidate = distance.depthData
   if !candidate.isDepthDataFiltered, candidate.depthDataAccuracy == .absolute, candidate.cameraCalibrationData != nil { measured = candidate }
  }
  lock.lock(); let callback = cancelled ? nil : frameCallback; lock.unlock()
  callback?(PersonalHeadDepthFrame(color:buffer,depth:measured,timestamp:timestamp,orientation:frameOrientation()))
 }
}
