import Foundation
import AVFoundation
import CoreMotion
import Metal
import UIKit
import StandardCyborgFusion

// Native front-TrueDepth reconstruction. No ARFaceGeometry ears, photo export,
// cloud API, or external computer. Output is measured support plus a CANDIDATE
// mesh: it still needs coverage, support, topology and ear-entrance validation.
final class PersonalHrtfReconstruction: NSObject, PersonalHeadReconstructor, SCReconstructionManagerDelegate {
 enum Failure: LocalizedError {
  case message(String)
  var errorDescription: String? { if case .message(let value) = self { return value }; return nil }
 }
 private enum Phase: Equatable { case idle, scanning, finalizing, meshing, checking, finished }
 private let lock = NSLock()
 private var phase = Phase.idle
 private var cancelled = false
 private var lastTimestamp = -1.0
 private var cameraPositions: [SIMD3<Float>] = []
 private let model: SCReconstructionManager
 private let motion = CMMotionManager()
 private let motionQueue = OperationQueue()
 private let meshQueue = OperationQueue()
 private var meshing: SCMeshingOperation?
 private let directory: URL
 private var observers: [NSObjectProtocol] = []
 private var completion: ((Result<PersonalHeadMeshCandidate,Error>) -> Void)?
 var progress: ((Int, String) -> Void)?

 init(frontTrueDepth: Void) throws {
  dispatchPrecondition(condition:.onQueue(.main))
  guard AVCaptureDevice.default(.builtInTrueDepthCamera,for:.video,position:.front) != nil,
        let device = MTLCreateSystemDefaultDevice(), let queue = device.makeCommandQueue(),
        let resources = SCFusionBundle.fusionBundle(),
        (try? device.makeDefaultLibrary(bundle:resources)) != nil else {
   throw Failure.message("需要可用的前置 TrueDepth 相机和 Metal；没有后置相机回退")
  }
  model = SCReconstructionManager(device:device,commandQueue:queue,maxThreadCount:2)
  directory = FileManager.default.temporaryDirectory.appendingPathComponent("SDAHeadScan",isDirectory:true).appendingPathComponent(UUID().uuidString,isDirectory:true)
  try FileManager.default.createDirectory(at:directory,withIntermediateDirectories:true)
  super.init()
  model.delegate = self
  model.flipsInputHorizontally = false
  model.includesDepthBuffersInMetadata = false
  model.includesColorBuffersInMetadata = false
  model.minDepth = 0.15
  model.setMaxDepth(0.65)
  model.icpDownsampleFraction = 0.1
  model.icpTolerance = 0.00002
  model.maxICPIterations = 32
  motionQueue.maxConcurrentOperationCount = 1
  meshQueue.maxConcurrentOperationCount = 1
  meshQueue.qualityOfService = .utility
  observers = [UIApplication.didEnterBackgroundNotification,UIApplication.didReceiveMemoryWarningNotification].map { name in
   NotificationCenter.default.addObserver(forName:name,object:nil,queue:.main) { [weak self] _ in self?.cancel() }
  }
 }
 deinit {
  motion.stopDeviceMotionUpdates()
  for observer in observers { NotificationCenter.default.removeObserver(observer) }
 }
 func start(completion: @escaping (Result<PersonalHeadMeshCandidate,Error>) -> Void) throws {
  lock.lock(); defer { lock.unlock() }
  guard phase == .idle else { throw Failure.message("头耳重建任务不能重复启动") }
  phase = .scanning; self.completion = completion
  if motion.isDeviceMotionAvailable {
   motion.deviceMotionUpdateInterval = 1.0 / 50
   motion.startDeviceMotionUpdates(using:.xArbitraryZVertical,to:motionQueue) { [weak self] sample,_ in
    if let sample { self?.model.accumulateDeviceMotion(sample) }
   }
  }
 }
 // Called directly from the synchronized AVFoundation frame callback. No unbounded dispatch
 // queue of retained images: the native reconstructor drops frames under load.
 func accumulate(_ frame: PersonalHeadDepthFrame) {
  let raw = frame.depth
  let depth = raw.converting(toDepthDataType:kCVPixelFormatType_DepthFloat32)
  guard let calibration = depth.cameraCalibrationData,
        let distortion = calibration.lensDistortionLookupTable, !distortion.isEmpty,
        CVPixelBufferGetWidth(depth.depthDataMap) <= 640,
        CVPixelBufferGetHeight(depth.depthDataMap) <= 640 else { return }
  lock.lock(); defer { lock.unlock() }
  guard phase == .scanning, !cancelled, frame.timestamp - lastTimestamp >= 0.15 else { return }
  lastTimestamp = frame.timestamp
  model.accumulate(depthBuffer:depth.depthDataMap,colorBuffer:frame.color,calibrationData:calibration)
 }
 func reconstructionManager(_ manager: SCReconstructionManager, didProcessWith metadata: SCAssimilatedFrameMetadata, statistics: SCReconstructionManagerStatistics) {
  lock.lock()
  guard phase == .scanning else { lock.unlock(); return }
  let failed = metadata.result == .failed || statistics.consecutiveLostTrackingCount > 5
  let accepted = metadata.result == .succeeded || metadata.result == .poorTracking
  if accepted {
   let p = simd_inverse(metadata.viewMatrix).columns.3
   if p.x.isFinite && p.y.isFinite && p.z.isFinite && cameraPositions.count < 600 {
    cameraPositions.append(SIMD3(p.x,p.y,p.z))
   }
  }
  let count = cameraPositions.count
  lock.unlock()
  if failed { fail("跟踪丢失，请放慢扫描并重新采集；不会用猜测的耳廓替代"); return }
  if count >= 600 { fail("扫描达到本机采集上限，请重新采集完整头耳覆盖"); return }
  if accepted && count % 24 == 0 {
   guard model.buildPointCloud().pointCount <= 500000 else { fail("扫描点数达到本机处理上限，请收紧头部取景范围后重试"); return }
  }
  progress?(count,accepted ? "正在手机上实时配准头耳深度" : "请缓慢移动，保持与已扫描表面重叠")
 }
 func reconstructionManager(_ manager: SCReconstructionManager,didEncounterAPIError error: Error) { fail(error.localizedDescription) }
 func finish() {
  lock.lock()
  guard phase == .scanning else { lock.unlock(); return }
  guard cameraPositions.count >= 72 else { lock.unlock(); fail("有效扫描不足，请完成头部各方向的覆盖"); return }
  phase = .finalizing; lock.unlock()
  motion.stopDeviceMotionUpdates()
  model.finalize { [self] in prepareMesh() }
 }
 func cancel() { fail("已取消本机头耳重建，未启用个人档案") }
 private func fail(_ message: String) {
  lock.lock()
  guard phase != .finished else { lock.unlock(); return }
  cancelled = true
  let old = phase
  if phase == .scanning || phase == .idle { phase = .finalizing }
  let operation = meshing
  lock.unlock()
  motion.stopDeviceMotionUpdates()
  if old == .scanning { model.finalize { [self] in end(.failure(Failure.message(message))) } }
  else if old == .idle { model.finalize { [self] in end(.failure(Failure.message(message))) } }
  else if old == .meshing { operation?.cancel() } // completion owns cleanup after writes stop
  // A finalize already in flight observes cancelled in prepareMesh.
 }
 private func prepareMesh() {
  lock.lock(); let stopped = cancelled; let poses = cameraPositions; lock.unlock()
  if stopped { end(.failure(Failure.message("已取消头耳重建"))); return }
  let cloud = model.buildPointCloud()
  let pointCount = cloud.pointCount
  guard (10000...500000).contains(cloud.pointCount) else { end(.failure(Failure.message("有效头耳表面不足或超出处理上限"))); return }
  let support = directory.appendingPathComponent("measured-support.ply")
  let mesh = directory.appendingPathComponent("candidate-head.ply")
  guard cloud.writeToPLY(atPath:support.path) else { end(.failure(Failure.message("无法写入本机临时测量表面"))); return }
  let operation = SCMeshingOperation(inputPLYPath:support.path,outputPLYPath:mesh.path)
  // Fine reconstruction is a candidate only. Poisson may close unmeasured
  // holes; the subsequent support validator MUST reject those closures.
  // Upstream maps UI resolution 6 to actual Poisson octree depth 9.
  // UI resolution 9 would mean depth 12, an unsuitable mobile allocation.
  operation.parameters.resolution = 6
  operation.parameters.smoothness = 1
  operation.parameters.surfaceTrimmingAmount = 0
  operation.parameters.closed = true
  operation.progressHandler = { [weak self] value in
   DispatchQueue.main.async { self?.progress?(poses.count,"正在手机上重建头耳表面 \(Int(value * 100))%") }
  }
  lock.lock(); meshing = operation; phase = .meshing; let stop = cancelled; lock.unlock()
  if stop { operation.cancel() }
  let gravity = cloud.gravity
  // Capture is finalized, and measured support is on disk. Do not retain a
  // second full surfel model while Poisson and mesh validation allocate.
  model.reset()
  operation.completionBlock = { [self, weak operation] in
   lock.lock(); let stopped = cancelled; lock.unlock()
   let size = (try? mesh.resourceValues(forKeys:[.fileSizeKey]).fileSize) ?? 0
   guard !stopped, operation?.isCancelled == false, (128...64 * 1024 * 1024).contains(size) else {
    end(.failure(Failure.message(stopped ? "已取消头耳重建" : "本机候选网格未成功生成"))); return
   }
   lock.lock(); phase = .checking; lock.unlock()
   do {
    let distance = try PersonalHrtfMeshCheck.screen(mesh:mesh,support:support,cancelled:{ [self] in
     lock.lock(); defer { lock.unlock() }; return cancelled
    })
    end(.success(PersonalHeadMeshCandidate(directory:directory,support:support,mesh:mesh,cameraPositions:poses,gravity:gravity,points:pointCount,geometryScreenVersion:1,maximumSupportDistance:distance)))
   } catch { end(.failure(error)) }
  }
  meshQueue.addOperation(operation)
 }
 private func end(_ result: Result<PersonalHeadMeshCandidate,Error>) {
  lock.lock()
  guard phase != .finished else { lock.unlock(); return }
  let answer: Result<PersonalHeadMeshCandidate,Error>
  if cancelled, case .success = result { answer = .failure(Failure.message("已取消本机重建")) }
  else { answer = result }
  phase = .finished; let callback = completion; completion = nil; meshing = nil
  lock.unlock()
  motion.stopDeviceMotionUpdates()
  model.reset()
  if case .failure = answer { try? FileManager.default.removeItem(at:directory) }
  DispatchQueue.main.async { callback?(answer) }
 }
}
