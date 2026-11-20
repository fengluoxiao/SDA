import Foundation
import AVFoundation
import CoreMotion
import Metal
import UIKit
import ImageIO
import StandardCyborgFusion

// Native front-TrueDepth reconstruction. No ARFaceGeometry ears, photo export,
// cloud API, or external computer. Output is measured support plus a CANDIDATE
// mesh: it still needs coverage, support, topology and ear-entrance validation.
final class PersonalHrtfReconstruction: NSObject, PersonalHeadReconstructor, SCReconstructionManagerDelegate {
 enum Failure: LocalizedError {
  case message(String)
  var errorDescription: String? { if case .message(let value) = self { return value }; return nil }
 }
 private enum Phase: Equatable { case idle, scanning, captured, paused, finalizing, meshing, checking, finished }
 private let lock = NSLock()
 private var phase = Phase.idle
 private var cancelled = false
 private var nextFrameID: UInt64 = 0
 private var lastTimestamp = -1.0
 private var lastPoseTime = -1.0
 private var cameraPositions: [SIMD3<Float>] = []
 private let pointLimit: Int
 private var coverage: PersonalHrtfCaptureCoverage?
 private var guide = PersonalHeadCaptureGuide()
 private var bootstrapPending = false
 private var lastBootstrapAttempt = -Double.infinity
 private var failureReason: String?
 private let model: SCReconstructionManager
 private let motion = CMMotionManager()
 private let motionQueue = OperationQueue()
 private let meshQueue = OperationQueue()
 private var meshing: SCMeshingOperation?
 private let directory: URL
 private var observers: [NSObjectProtocol] = []
 private var completion: ((Result<PersonalHeadMeshCandidate,Error>) -> Void)?
 var captureGuide: ((PersonalHeadCaptureGuide) -> Void)?
 var captureReadiness: ((Bool, String) -> Void)?
 var captureLimit: ((Int, String) -> Void)?
 var progress: ((Int, String) -> Void)?

 init(frontTrueDepth: Void) throws {
  dispatchPrecondition(condition:.onQueue(.main))
  guard AVCaptureDevice.default(.builtInTrueDepthCamera,for:.video,position:.front) != nil,
        let device = MTLCreateSystemDefaultDevice(), let queue = device.makeCommandQueue(),
        let resources = SCFusionBundle.fusionBundle(),
        (try? device.makeDefaultLibrary(bundle:resources)) != nil else {
   throw Failure.message("需要可用的前置 TrueDepth 相机和 Metal；没有后置相机回退")
  }
  coverage = try PersonalHrtfCaptureCoverage()
  let budget = sda_hrtf_recommended_budget(sda_hrtf_available_memory(),ProcessInfo.processInfo.physicalMemory)
  pointLimit = Int(sda_hrtf_scan_point_limit(budget))
  guard pointLimit > 0 else { throw Failure.message("当前可用内存不足以开始头耳采集，请稍后重试") }
  model = SCReconstructionManager(device:device,commandQueue:queue,maxThreadCount:2)
  directory = FileManager.default.temporaryDirectory.appendingPathComponent("SDAHeadScan",isDirectory:true).appendingPathComponent(UUID().uuidString,isDirectory:true)
  try FileManager.default.createDirectory(at:directory,withIntermediateDirectories:true)
  super.init()
  model.sdaMaximumSurfelCount = UInt(pointLimit)
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
  let depth = frame.depth?.converting(toDepthDataType:kCVPixelFormatType_DepthFloat32)
  let depthMap = depth?.depthDataMap
  let calibration = depth?.cameraCalibrationData
  let boundedDepth = depthMap.flatMap { CVPixelBufferGetWidth($0) <= 640 && CVPixelBufferGetHeight($0) <= 640 ? $0 : nil }
  lock.lock()
  guard phase == .scanning, !cancelled, frame.timestamp.isFinite, frame.timestamp > lastTimestamp,
        !bootstrapPending, let analysis = coverage else { lock.unlock(); return }
  lastTimestamp = frame.timestamp
  nextFrameID += 1; let frameID = nextFrameID
  let orientation = frame.orientation
  let region = analysis.captureRegion
  if region == nil {
   let now = ProcessInfo.processInfo.systemUptime
   guard now-lastBootstrapAttempt >= 0.3, let boundedDepth, let calibration,
         calibration.lensDistortionLookupTable?.isEmpty == false else { lock.unlock(); return }
   lastBootstrapAttempt = now; bootstrapPending = true; lock.unlock()
   analysis.prepareInitial(color:frame.color,depth:boundedDepth,orientation:orientation) { [weak self] seed in
    guard let self else { return }
    let masked = seed.flatMap { PersonalHrtfCaptureCoverage.maskedDepth(boundedDepth,seed:$0,orientation:orientation,region:nil) }
    self.lock.lock(); defer { self.lock.unlock() }
    guard self.phase == .scanning, !self.cancelled else { self.bootstrapPending = false; return }
    guard let masked else { self.bootstrapPending = false; return }
    // Until this seed is registered AND its reference is measured, retain only
    // this one frame. No inference backlog and no arbitrary background seed.
    analysis.offerRaw(color:frame.color,depth:boundedDepth,frameID:frameID,orientation:orientation,force:true) { [weak self] update in self?.receiveCoverage(update) }
    self.model.sdaAccumulate(depthBuffer:masked,colorBuffer:frame.color,calibrationData:calibration,frameID:frameID)
   }
   return
  }
  lock.unlock()
  defer { analysis.offerRaw(color:frame.color,depth:boundedDepth,frameID:frameID,orientation:orientation) { [weak self] update in self?.receiveCoverage(update) } }
  guard !model.sdaReachedSurfelLimit, let boundedDepth, let calibration,
        calibration.lensDistortionLookupTable?.isEmpty == false,
        let masked = PersonalHrtfCaptureCoverage.maskedDepth(boundedDepth,seed:nil,orientation:orientation,region:region) else { return }
  lock.lock(); defer { lock.unlock() }
  guard phase == .scanning, !cancelled else { return }
  model.sdaAccumulate(depthBuffer:masked,colorBuffer:frame.color,calibrationData:calibration,frameID:frameID)
 }
 func confirmEarView(_ target:PersonalHeadCaptureGuide.EarSide) {
  lock.lock(); defer { lock.unlock() }
  guard phase == .scanning, guide.targetEar == target, guide.canConfirmEar else { return }
  coverage?.confirmEarView(target,after:nextFrameID)
 }
 private func receiveCoverage(_ update:PersonalHrtfCaptureCoverage.Update) {
  DispatchQueue.main.async { [weak self] in
   guard let self else { return }
   self.lock.lock()
   guard self.phase == .scanning else { self.lock.unlock(); return }
   if update.registered { self.bootstrapPending = false }
   self.guide = update.state
   let ready = update.state.complete
   if ready { self.phase = .captured }
   self.lock.unlock()
   self.captureGuide?(update.state)
   self.captureReadiness?(ready,update.message)
   if ready { self.motion.stopDeviceMotionUpdates() }
  }
 }
 func reconstructionManager(_ manager: SCReconstructionManager, didProcessWith metadata: SCAssimilatedFrameMetadata, statistics: SCReconstructionManagerStatistics) {
  lock.lock()
  guard phase == .scanning else { lock.unlock(); return }
  let failed = metadata.result == .failed
  let accepted = metadata.result == .succeeded || metadata.result == .poorTracking
  // Keep coverage records bounded independently of the reconstruction cadence.
  let now = ProcessInfo.processInfo.systemUptime
  if accepted && now - lastPoseTime >= 0.15 {
   lastPoseTime = now
   let p = simd_inverse(metadata.viewMatrix).columns.3
   if p.x.isFinite && p.y.isFinite && p.z.isFinite && cameraPositions.count < 600 {
    cameraPositions.append(SIMD3(p.x,p.y,p.z))
   }
  }
  let count = cameraPositions.count
  let recognizingEars = guide.stage != .face
  lock.unlock()
  if accepted { coverage?.updatePose(view:metadata.viewMatrix,projection:metadata.projectionMatrix) }
  coverage?.register(frameID:metadata.sdaFrameID,accepted:accepted,view:metadata.viewMatrix,projection:metadata.projectionMatrix) { [weak self] update in self?.receiveCoverage(update) }
  if !accepted { lock.lock(); bootstrapPending = false; lock.unlock() }
  if model.sdaReachedSurfelLimit {
   lock.lock()
   guard phase == .scanning else { lock.unlock(); return }
   if guide.stage != .face {
    lock.unlock(); progress?(count,"本次深度已保留，可继续识别耳朵"); return
   }
   phase = .paused; lock.unlock()
   releaseCaptureAnalysis()
   motion.stopDeviceMotionUpdates()
   captureLimit?(count,"本次采集预算已用满（最多 \(pointLimit) 个点），已暂停并保留测量。三个步骤未完成时不能跳过；请取消后重新采集。")
   return
  }
  if failed { fail("配准器发生内部错误，无法继续本次采集"); return }
  let hint: String
  if recognizingEars { hint = "已记录 \(model.sdaSurfelCount) 个测量点" }
  else if accepted { hint = "正在配准头耳深度 · \(model.sdaSurfelCount)/\(pointLimit) 点" }
  else if manager.sdaNeedsTrackingRecovery {
   hint = "已采集区域已保留：保持头部不动，将手机小幅移回刚才能采集的位置，自动接续；无需重新正面校准或重扫已采集的耳朵"
  } else if manager.sdaIsAligning {
   hint = "正在自动接续：保持头部不动，让耳部与脸侧保留重叠，不用重新校准"
  } else { hint = "正在采集头耳深度 · \(model.sdaSurfelCount)/\(pointLimit) 点" }
  progress?(count,hint)
 }
 func reconstructionManager(_ manager: SCReconstructionManager,didEncounterAPIError error: Error) { fail(error.localizedDescription) }
 func finish() {
  lock.lock()
  guard phase == .scanning || phase == .captured || phase == .paused else { lock.unlock(); return }
  guard guide.complete else { lock.unlock(); fail("正脸与双耳尚未全部识别"); return }
  guard !cameraPositions.isEmpty, model.sdaSurfelCount >= 10000 else { lock.unlock(); fail("双耳已识别，但本次实测深度不足以重建表面，未生成个人档案"); return }
  phase = .finalizing; lock.unlock()
  releaseCaptureAnalysis()
  motion.stopDeviceMotionUpdates()
  model.finalize { [self] in meshQueue.addOperation { [self] in prepareMesh() } }
 }
 func cancel() { fail("已取消本机头耳重建，未启用个人档案") }
 private func releaseCaptureAnalysis() {
  // The native metadata delegate runs on main; never race its optional model.
  DispatchQueue.main.async { [weak self] in
   guard let self else { return }; self.lock.lock(); self.coverage = nil; self.lock.unlock()
  }
 }
 private func fail(_ message: String) {
  lock.lock()
  guard phase != .finished else { lock.unlock(); return }
  cancelled = true; failureReason = message
  let old = phase
  if phase == .scanning || phase == .captured || phase == .paused || phase == .idle { phase = .finalizing }
  let operation = meshing
  lock.unlock()
  releaseCaptureAnalysis()
  motion.stopDeviceMotionUpdates()
  if old == .scanning || old == .captured || old == .paused { model.finalize { [self] in end(.failure(Failure.message(message))) } }
  else if old == .idle { model.finalize { [self] in end(.failure(Failure.message(message))) } }
  else if old == .meshing { operation?.cancel() } // completion owns cleanup after writes stop
  // A finalize already in flight observes cancelled in prepareMesh.
 }
 private func prepareMesh() {
  lock.lock(); let stopped = cancelled; let poses = cameraPositions; lock.unlock()
  if stopped { end(.failure(Failure.message("已取消头耳重建"))); return }
  let cloud = model.buildPointCloud()
  let pointCount = cloud.pointCount
  guard (10000...pointLimit).contains(cloud.pointCount) else { end(.failure(Failure.message("有效头耳表面不足或超出处理上限"))); return }
  let support = directory.appendingPathComponent("measured-support.ply")
  let mesh = directory.appendingPathComponent("candidate-head.ply")
  guard cloud.writeToPLY(atPath:support.path) else { end(.failure(Failure.message("无法写入本机临时测量表面"))); return }
  let operation = SCMeshingOperation(inputPLYPath:support.path,outputPLYPath:mesh.path)
  // Fine reconstruction is a candidate only. Poisson may close unmeasured
  // holes; measurement review MUST discard those unmeasured closures.
  // Upstream maps UI resolution 6 to actual Poisson octree depth 9.
  // UI resolution 9 would mean depth 12, an unsuitable mobile allocation.
  operation.parameters.resolution = 6
  operation.parameters.smoothness = 1
  operation.parameters.surfaceTrimmingAmount = 0
  operation.parameters.closed = false
  operation.progressHandler = { [weak self] value in
   DispatchQueue.main.async { self?.progress?(poses.count,"正在手机上重建头耳表面 \(Int(value * 100))%") }
  }
  lock.lock(); meshing = operation; phase = .meshing; let stop = cancelled; lock.unlock()
  if stop { operation.cancel() }
  let gravity = cloud.gravity
  // Capture is finalized, and measured support is on disk. Do not retain a
  // second full surfel model while Poisson and mesh validation allocate.
  operation.completionBlock = { [self, weak operation] in
   lock.lock(); let stopped = cancelled; let reason = failureReason; lock.unlock()
   if stopped || operation?.isCancelled == true {
    end(.failure(Failure.message(reason ?? "已取消头耳重建"))); return
   }
   if let cause = operation?.sdaError {
    let error = cause as NSError
    end(.failure(Failure.message("本机网格重建失败（\(error.domain)/\(error.code)）：\(error.localizedDescription)"))); return
   }
   let size = (try? mesh.resourceValues(forKeys:[.fileSizeKey]).fileSize) ?? 0
   guard size >= 128 else { end(.failure(Failure.message("Poisson 未输出有效网格（\(pointCount) 个测量点）"))); return }
   guard size <= 256 * 1024 * 1024 else { end(.failure(Failure.message("候选网格文件超出本机读取范围（\(size / 1024 / 1024) MiB）"))); return }
   lock.lock(); phase = .checking; lock.unlock()
   do {
    let measured = try PersonalHrtfMeshCheck.load(mesh:mesh,support:support,cancelled:{ [self] in
     lock.lock(); defer { lock.unlock() }; return cancelled
    })
    let checked = try PersonalHrtfMeshCheck.measurement(measured,cancelled:{ [self] in
     lock.lock(); defer { lock.unlock() }; return cancelled
    })
    end(.success(PersonalHeadMeshCandidate(directory:directory,support:support,mesh:mesh,cameraPositions:poses,gravity:gravity,points:pointCount,geometryScreenVersion:2,surface:checked.surface,maximumSupportDistance:checked.distance)))
   } catch { end(.failure(error)) }
  }
  model.sdaReset { [self] in meshQueue.addOperation(operation) }
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
  model.sdaReset { [self] in
   if case .failure = answer { try? FileManager.default.removeItem(at:directory) }
   callback?(answer)
  }
 }
}
