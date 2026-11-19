import UIKit
import ARKit
import AVFoundation
import SceneKit
import CoreImage
import ImageIO
import simd

// Front TrueDepth only. Rear-camera world tracking is explicitly disabled.
// ARFaceGeometry is never substituted for measured head or ear surfaces.
// Camera alignment and coverage are verified by desktop RGB-D registration.
final class PersonalHeadScanController: UIViewController, ARSessionDelegate {
 static let maximumBytes = 96 * 1024 * 1024
 static func supported() -> Bool {
  ARFaceTrackingConfiguration.isSupported &&
   AVCaptureDevice.default(.builtInTrueDepthCamera,for:.video,position:.front) != nil
 }
 let session = ARSession()
 let worker = DispatchQueue(label:"app.sda.headscan",qos:.userInitiated)
 let preview = ARSCNView(frame:.zero)
 let message = UILabel(), countLabel = UILabel()
 let finishButton = UIButton(type:.system)
 let imageContext = CIContext(options:[.cacheIntermediates:false])
 var complete: ((Result<URL,Error>) -> Void)?
 private var reference: simd_float4x4?
 private var file: FileHandle?
 private var directory: URL?
 private var destination: URL?
 private var frames = 0, byteCount = 0, lastTime = -1.0, lastDepthTime = -1.0
 private var isFinished = false
 private var permissionCancelled = false // main queue only
 private var backgroundObserver: NSObjectProtocol?
 static func discardAbandonedScans() {
  let root = FileManager.default.temporaryDirectory.appendingPathComponent("SDAHeadScan",isDirectory:true)
  try? FileManager.default.removeItem(at:root)
 }
 func cancel() { fail("已取消，采集数据已删除") }
 override func viewDidLoad() {
  super.viewDidLoad(); view.backgroundColor = .black
  preview.session = session; preview.translatesAutoresizingMaskIntoConstraints = false
  view.addSubview(preview)
  message.textColor = .white; message.numberOfLines = 0; message.textAlignment = .center
  message.font = .preferredFont(forTextStyle:.body)
  message.text = "仅使用前置 TrueDepth。需要他人协助：移开遮耳头发，保持表情稳定；先正面对准，再缓慢围绕头部，覆盖两耳、后脑、头顶和下方。保持约 25–60 厘米距离。扫描导出后在电脑上配准并检查质量。"
  countLabel.textColor = .white; countLabel.textAlignment = .center
  finishButton.setTitle("完成并导出",for:.normal); finishButton.isEnabled = false
  finishButton.addTarget(self,action:#selector(finish),for:.touchUpInside)
  let cancel = UIButton(type:.system); cancel.setTitle("取消并删除",for:.normal)
  cancel.addTarget(self,action:#selector(cancelScan),for:.touchUpInside)
  let stack = UIStackView(arrangedSubviews:[message,countLabel,finishButton,cancel]); stack.axis = .vertical; stack.spacing = 14
  stack.backgroundColor = UIColor.black.withAlphaComponent(0.65); stack.isLayoutMarginsRelativeArrangement = true
  stack.layoutMargins = UIEdgeInsets(top:16,left:18,bottom:12,right:18); stack.translatesAutoresizingMaskIntoConstraints = false
  view.addSubview(stack)
  NSLayoutConstraint.activate([
   preview.topAnchor.constraint(equalTo:view.topAnchor),preview.bottomAnchor.constraint(equalTo:view.bottomAnchor),
   preview.leadingAnchor.constraint(equalTo:view.leadingAnchor),preview.trailingAnchor.constraint(equalTo:view.trailingAnchor),
   stack.leadingAnchor.constraint(equalTo:view.safeAreaLayoutGuide.leadingAnchor,constant:12),
   stack.trailingAnchor.constraint(equalTo:view.safeAreaLayoutGuide.trailingAnchor,constant:-12),
   stack.bottomAnchor.constraint(equalTo:view.safeAreaLayoutGuide.bottomAnchor,constant:-8)
  ])
  isModalInPresentation = true; UIApplication.shared.isIdleTimerDisabled = true
  backgroundObserver = NotificationCenter.default.addObserver(forName:UIApplication.didEnterBackgroundNotification,object:nil,queue:.main) { [weak self] _ in self?.fail("采集已暂停，临时数据已删除；请保持应用在前台重新采集。") }
  session.delegate = self; session.delegateQueue = worker
  AVCaptureDevice.requestAccess(for:.video) { [weak self] allowed in
   DispatchQueue.main.async {
    guard let self, !self.permissionCancelled else { return }
    guard allowed else { self.fail("需要相机权限才能采集头耳几何；可以在系统设置中开启。"); return }
    guard Self.supported() else { self.fail("此设备没有可用的前置 TrueDepth 深度相机。"); return }
    let config = ARFaceTrackingConfiguration(); config.isWorldTrackingEnabled = false
    config.isLightEstimationEnabled = false; config.maximumNumberOfTrackedFaces = 1
    self.session.run(config,options:[.resetTracking,.removeExistingAnchors])
   }
  }
 }
 override func viewDidDisappear(_ animated: Bool) {
  super.viewDidDisappear(animated); permissionCancelled = true; session.pause(); UIApplication.shared.isIdleTimerDisabled = false
  if let observer = backgroundObserver { NotificationCenter.default.removeObserver(observer); backgroundObserver = nil }
 }
 func session(_ session: ARSession,didFailWithError error: Error) { DispatchQueue.main.async { [weak self] in self?.fail(error.localizedDescription) } }
 func sessionWasInterrupted(_ session: ARSession) { DispatchQueue.main.async { [weak self] in self?.fail("采集被打断，请重新开始，以免深度帧不连续。") } }
 private func update(_ text: String) { DispatchQueue.main.async { [weak self] in self?.countLabel.text = text } }
 private func append(_ data: Data) throws {
  try PersonalHrtfStore.require(byteCount + data.count <= Self.maximumBytes,"采集已达到 96 MiB 上限，请取消后重新采集")
  try file?.write(contentsOf:data); byteCount += data.count
 }
 private func word(_ x: Int) -> Data { var value = UInt32(x).littleEndian; return withUnsafeBytes(of:&value) { Data($0) } }
 private func matrix(_ m: simd_float4x4) -> [[Float]] { (0..<4).map { row in (0..<4).map { col in m[col][row] } } }
 private func begin(_ head: simd_float4x4) throws {
  let base = FileManager.default.temporaryDirectory.appendingPathComponent("SDAHeadScan",isDirectory:true)
  try FileManager.default.createDirectory(at:base,withIntermediateDirectories:true)
  // A single active acquisition. Never retain old raw camera sessions.
  for url in try FileManager.default.contentsOfDirectory(at:base,includingPropertiesForKeys:nil) { try FileManager.default.removeItem(at:url) }
  let dir = base.appendingPathComponent(UUID().uuidString,isDirectory:true)
  try FileManager.default.createDirectory(at:dir,withIntermediateDirectories:false); directory = dir
  let url = dir.appendingPathComponent("head.sdascan"); destination = url
  _ = FileManager.default.createFile(atPath:url.path,contents:nil); file = try FileHandle(forWritingTo:url)
  let meta: [String:Any] = ["format":"sda-headscan","version":1,"units":"metre","depth":"float32-le-z-camera", "coordinates":"depth camera: right/down/forward; face hints in ARKit camera: right/up/back", "initialHeadToCamera":matrix(head),"cameraPolicy":"front-truedepth-only","alignment":"offline-rigid-rgbd-registration","device":UIDevice.current.model,"createdAt":ISO8601DateFormatter().string(from:Date())]
  let json = try JSONSerialization.data(withJSONObject:meta,options:[.sortedKeys]); try append(Data("SDASCAN1".utf8)); try append(word(json.count)); try append(json)
 }
 func session(_ session: ARSession,didUpdate frame: ARFrame) {
  guard !isFinished, frames < 180, frame.timestamp - lastTime >= 0.30 else { return }
  guard let raw = frame.capturedDepthData, frame.capturedDepthDataTimestamp > lastDepthTime,
   abs(frame.capturedDepthDataTimestamp - frame.timestamp) <= 0.025 else { return }
  do {
   if reference == nil {
    guard let face = frame.anchors.compactMap({$0 as? ARFaceAnchor}).first, face.isTracked else { update("请先正面对准头部，建立初始坐标"); return }
    let head = simd_inverse(frame.camera.transform) * face.transform
    reference = head; try begin(head)
   }
   let trackedFace = frame.anchors.compactMap({$0 as? ARFaceAnchor}).first(where:{$0.isTracked})
   let headToCamera = trackedFace.map { simd_inverse(frame.camera.transform) * $0.transform }
   let depth = raw.converting(toDepthDataType:kCVPixelFormatType_DepthFloat32)
   guard let calibration = depth.cameraCalibrationData, let distortion = calibration.lensDistortionLookupTable, !distortion.isEmpty else { throw PersonalHrtfError.message("缺少完整的前置 TrueDepth 镜头标定，无法导出可靠深度") }
   let map = depth.depthDataMap
   CVPixelBufferLockBaseAddress(map,.readOnly); defer { CVPixelBufferUnlockBaseAddress(map,.readOnly) }
   let width = CVPixelBufferGetWidth(map), height = CVPixelBufferGetHeight(map)
   guard width <= 640, height <= 640, let start = CVPixelBufferGetBaseAddress(map) else { return }
   let stride = CVPixelBufferGetBytesPerRow(map); var bytes = Data(capacity:width*height*4); var valid = 0
   for y in 0..<height {
    let row = start.advanced(by:y*stride).assumingMemoryBound(to:Float.self)
    for x in 0..<width {
     let v = row[x]; if v.isFinite && v >= 0.15 && v <= 1.0 { valid += 1 }
     var little = v.bitPattern.littleEndian; withUnsafeBytes(of:&little) { bytes.append(contentsOf:$0) }
    }
   }
   guard valid > width*height/5 else { update("有效深度不足，移开头发或调整距离"); return }
   let image = CIImage(cvPixelBuffer:frame.capturedImage)
   guard let jpeg = imageContext.jpegRepresentation(of:image,colorSpace:CGColorSpaceCreateDeviceRGB(),options:[CIImageRepresentationOption(rawValue:kCGImageDestinationLossyCompressionQuality as String):0.70]), jpeg.count <= 1500000 else { return }
   let k = calibration.intrinsicMatrix
   let intrinsic = (0..<3).map { row in (0..<3).map { col in k[col][row] } }
   let e = calibration.extrinsicMatrix
   let extrinsic = (0..<3).map { row in (0..<4).map { col in e[col][row] } }
   let referenceSize = calibration.intrinsicMatrixReferenceDimensions
   var metadata: [String:Any] = ["type":"frame","index":frames,"timestamp":frame.timestamp,"depthTimestamp":frame.capturedDepthDataTimestamp,
    "width":width,"height":height,"intrinsics":intrinsic,"intrinsicReference":[referenceSize.width,referenceSize.height],
    "depthExtrinsics":extrinsic,"distortionCenter":[calibration.lensDistortionCenter.x,calibration.lensDistortionCenter.y],
    "poseSource":headToCamera == nil ? "unregistered" : "face-anchor-hint"]
   if let hint = headToCamera { metadata["headToCamera"] = matrix(hint) }
   metadata["lensDistortion"] = distortion.base64EncodedString()
   let json = try JSONSerialization.data(withJSONObject:metadata,options:[.sortedKeys])
   try append(word(json.count)); try append(word(bytes.count)); try append(word(jpeg.count)); try append(json); try append(bytes); try append(jpeg)
   frames += 1; lastTime = frame.timestamp; lastDepthTime = frame.capturedDepthDataTimestamp
   update("已采集 \(frames)/180 帧 · \(byteCount/1048576) MiB · 配准与覆盖待电脑验证")
   let ready = frames >= 72
   DispatchQueue.main.async { [weak self] in self?.finishButton.isEnabled = ready }
  } catch { DispatchQueue.main.async { [weak self] in self?.fail(error.localizedDescription) } }
 }
 @objc private func finish() {
  session.pause()
  worker.async { [self] in
   guard !self.isFinished else { return }
   do {
    try PersonalHrtfStore.require(self.frames >= 72,"帧数不足，不能导出扫描")
    let json = try JSONSerialization.data(withJSONObject:["type":"sealed","frames":self.frames,"coverageVerified":false,"alignmentVerified":false])
    try self.append(self.word(json.count)); try self.append(self.word(0)); try self.append(self.word(0)); try self.append(json)
    try self.file?.close(); self.file = nil; self.isFinished = true
    guard let url = self.destination else { throw PersonalHrtfError.message("采集文件缺失") }
    DispatchQueue.main.async { self.dismiss(animated:true) { self.complete?(.success(url)); self.complete = nil } }
   } catch { DispatchQueue.main.async { self.fail(error.localizedDescription) } }
  }
 }
 @objc private func cancelScan() { fail("已取消，采集数据已删除") }
 private func fail(_ text: String) {
  permissionCancelled = true; session.pause()
  worker.async { [weak self] in
   guard let self, !self.isFinished else { return }; self.isFinished = true
   try? self.file?.close(); self.file = nil
   if let directory = self.directory { try? FileManager.default.removeItem(at:directory) }
   DispatchQueue.main.async { self.dismiss(animated:true) { self.complete?(.failure(PersonalHrtfError.message(text))); self.complete = nil } }
  }
 }
}
