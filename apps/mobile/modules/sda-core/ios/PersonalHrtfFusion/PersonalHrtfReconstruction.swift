import ARKit
import AVFoundation
import Foundation
import StandardCyborgFusion
import UIKit
import simd

final class PersonalHrtfReconstruction: NSObject, PersonalHeadReconstructor {
 private enum Phase { case idle, scanning, captured, processing, finished }
 private let lock = NSLock(), worker = DispatchQueue(label:"app.sda.faceframe.fusion",qos:.userInitiated,autoreleaseFrequency:.workItem)
 private var phase = Phase.idle, busy = false, cancelled = false, needsVertices = true
 private var completion: ((Result<PersonalHeadMeshCandidate,Error>)->Void)?
 private var poses = PoseHistory(), colors = [(time:Double,image:CVPixelBuffer)]()
 private var lastTimestamp = -Double.infinity, lastDepthTime = -Double.infinity
 private var observers = [NSObjectProtocol]()
 private var meshing: SCMeshingOperation?
 private let coverage: PersonalHrtfCaptureCoverage
 private var fusion: FusionEngine? = FusionEngine()
 private var support: PersonalHeadMeasuredSupport
 private var leftSupport = false, rightSupport = false
 private let budget: UInt64, pointLimit: Int
 var captureGuide: ((PersonalHeadCaptureGuide)->Void)?
 var captureReadiness: ((Bool,String)->Void)?
 var captureLimit: ((Int,String)->Void)?
 var progress: ((Int,String)->Void)?

 init(frontTrueDepth:Void) throws {
  guard PersonalHeadScanController.supported() else { throw PersonalHrtfError.message("此设备无法使用前置 TrueDepth") }
  coverage = try PersonalHrtfCaptureCoverage()
  let available = sda_hrtf_recommended_budget(sda_hrtf_available_memory(),ProcessInfo.processInfo.physicalMemory)
  let limit = Int(sda_hrtf_scan_point_limit(available))
  guard limit > 0 else { throw PersonalHrtfError.message("当前内存不足以开始头耳测量") }
  budget = available; pointLimit = limit; support = PersonalHeadMeasuredSupport(limit:limit)
  super.init()
  fusion?.maximumBlocks = Int(budget/8192)
 }
 deinit { for observer in observers { NotificationCenter.default.removeObserver(observer) } }
 func start(completion:@escaping(Result<PersonalHeadMeshCandidate,Error>)->Void) throws {
  lock.lock(); defer { lock.unlock() }
  guard phase == .idle else { throw PersonalHrtfError.message("采集不能重复启动") }
  phase = .scanning; self.completion = completion
  observers = [UIApplication.didEnterBackgroundNotification,UIApplication.didReceiveMemoryWarningNotification].map { name in
   NotificationCenter.default.addObserver(forName:name,object:nil,queue:.main) { [weak self] _ in self?.cancel() }
  }
 }
 private var isCancelled: Bool { lock.lock(); defer { lock.unlock() }; return cancelled }

 func accumulate(_ frame:PersonalHeadDepthFrame) {
  lock.lock(); defer { lock.unlock() }
  let timestamp = frame.timestamp
  guard phase == .scanning, !cancelled, timestamp.isFinite, timestamp > lastTimestamp else { return }
  lastTimestamp = timestamp
  let faces = frame.anchors.compactMap { $0 as? ARFaceAnchor }.filter(\.isTracked)
  let face = faces.count == 1 ? faces[0] : nil
  if let face {
   let pose = face.transform.inverse * frame.camera.transform
   if Self.finite(pose) { poses.append(time:timestamp,pose:pose) }
  }
  colors.append((timestamp,frame.capturedImage)); colors.removeAll { timestamp-$0.time > 0.15 }
  if colors.count > 8 { colors.removeFirst(colors.count-8) }
  let time = frame.capturedDepthDataTimestamp
  guard !busy, let depth = frame.capturedDepthData, time.isFinite, time > lastDepthTime,
        time <= timestamp+0.02, timestamp-time <= 0.15 else { return }
  lastDepthTime = time
  let width = CVPixelBufferGetWidth(depth.depthDataMap), height = CVPixelBufferGetHeight(depth.depthDataMap)
  guard (8...640).contains(width), (8...640).contains(height) else { return }
  let nearest = colors.min { abs($0.time-time) < abs($1.time-time) }
  let rgb = nearest.flatMap { abs($0.time-time) <= 0.025 ? $0.image : nil }
  let camera = frame.camera, resolution = camera.imageResolution
  guard Self.finite(camera.intrinsics), resolution.width > 0, resolution.height > 0 else { return }
  let vertices = needsVertices && face != nil ? face?.geometry.vertices : nil
  let input = FusionFrame(time:time,width:0,height:0,depth:[],calibrationK:nil,calibrationReference:nil,intrinsics:camera.intrinsics,
                          imageResolution:SIMD2(Float(resolution.width),Float(resolution.height)),arPose:poses.pose(at:time),motion:poses.motion(at:time),
                          leftEye:face?.leftEyeTransform.translation,rightEye:face?.rightEyeTransform.translation,faceVertices:vertices)
  busy = true
  worker.async { [weak self] in self?.ingest(input,depth:depth,color:rgb) }
 }
 private static func finite(_ pose:simd_float4x4) -> Bool {
  [pose.columns.0,pose.columns.1,pose.columns.2,pose.columns.3].allSatisfy { $0.x.isFinite && $0.y.isFinite && $0.z.isFinite && $0.w.isFinite }
 }
 private static func finite(_ intrinsics:simd_float3x3) -> Bool {
  let center = intrinsics.columns.2
  return intrinsics.columns.0.x.isFinite && intrinsics.columns.0.x > 0 && intrinsics.columns.1.y.isFinite && intrinsics.columns.1.y > 0 && center.x.isFinite && center.y.isFinite
 }
 private func ingest(_ input:FusionFrame,depth:AVDepthData,color:CVPixelBuffer?) {
  defer { lock.lock(); busy = false; lock.unlock() }
  guard !isCancelled, let fusion else { return }
  var frame = input
  DepthConversion.fill(&frame,from:depth)
  if let intrinsics = frame.calibrationK, let reference = frame.calibrationReference,
     !Self.finite(intrinsics) || !reference.x.isFinite || !reference.y.isFinite || reference.x <= 0 || reference.y <= 0 {
   frame.calibrationK = nil; frame.calibrationReference = nil
  }
  let old = fusion.counts, switches = fusion.diagnostics.mappingSwitches
  let independent = !depth.isDepthDataFiltered && depth.depthDataAccuracy == .absolute
  var acceptedPose: simd_float4x4?
  fusion.onMeasuredFrame = { [weak self,weak fusion] image,weights,pose in
   acceptedPose = pose
   guard independent, let self, let volume = fusion?.volume else { return }
   self.support.append(image:image,weights:weights,pose:pose,volume:volume,frame:volume.integratedFrames)
  }
  let status = fusion.process(frame)
  fusion.onMeasuredFrame = nil
  if status.diagnostics.mappingSwitches != switches {
   support = PersonalHeadMeasuredSupport(limit:pointLimit); leftSupport = false; rightSupport = false; coverage.reset()
  }
  lock.lock(); needsVertices = fusion.needsFaceVertices; lock.unlock()
  guard !isCancelled else { return }
  let left = acceptedPose != nil && fusion.counts.left > old.left, right = acceptedPose != nil && fusion.counts.right > old.right
  if independent && left { leftSupport = true }; if independent && right { rightSupport = true }
  coverage.observe(status:status,leftAdvanced:left,rightAdvanced:right,pixels:color,pose:acceptedPose)
  let guide = coverage.guide
  let statusText: String
  if guide.complete { statusText = "FaceFrame · 双耳侧面已记录" }
  else if status.tracking == .tooFast { statusText = "请慢一点转头" }
  else if status.tracking == .lost || status.tracking == .searching { statusText = "FaceFrame 正在对齐头部" }
  else { statusText = guide.status }
  lock.lock()
  guard phase == .scanning, !cancelled else { lock.unlock(); return }
  if guide.complete { phase = .captured; colors.removeAll(); poses.reset() }
  lock.unlock()
  if fusion.volume?.allocationLimitReached == true, !guide.complete {
   end(.failure(PersonalHrtfError.message("本次深度空间已达到内存上限，未生成个人档案"))); return
  }
  let measuredPoints = support.count
  DispatchQueue.main.async { [weak self] in
   guard let self, !self.isCancelled else { return }
   self.captureGuide?(guide); self.captureReadiness?(guide.complete,statusText)
   self.progress?(status.diagnostics.fusedFrames,"\(measuredPoints) 个实测点")
  }
 }

 func finish() {
  lock.lock(); guard phase == .captured, !cancelled else { lock.unlock(); return }
  phase = .processing; lock.unlock()
  worker.async { [self] in
   do { end(.success(try measuredCandidate())) } catch { end(.failure(error)) }
  }
 }
 private func measuredCandidate() throws -> PersonalHeadMeshCandidate {
  guard !isCancelled, fusion?.volume != nil, leftSupport, rightSupport, support.count >= 10000 else {
   throw PersonalHrtfError.message("已识别双耳，但无足够的原始绝对深度支持实测表面，未生成个人档案")
  }
  fusion = nil
  let available = min(budget,sda_hrtf_recommended_budget(sda_hrtf_available_memory(),ProcessInfo.processInfo.physicalMemory))
  guard available >= 64*1024*1024 else { throw PersonalHrtfError.message("当前内存不足以检查实测表面") }
  let measured = try PersonalHrtfMeshCheck.poissonMeasurement(positions:support.positions,normals:support.normals,budget:available,cancelled:{ [self] in isCancelled },operationChanged:{ [self] operation in
   lock.lock(); meshing = operation; let stop = cancelled; lock.unlock(); if stop { operation?.cancel() }
  })
  return PersonalHeadMeshCandidate(geometryScreenVersion:2,surface:measured.surface,maximumSupportDistance:measured.distance)
 }
 func cancel() {
  lock.lock(); guard phase != .finished, !cancelled else { lock.unlock(); return }; cancelled = true; let operation = meshing; lock.unlock()
  operation?.cancel()
  worker.async { [self] in end(.failure(PersonalHrtfError.message("已取消本机测量"))) }
 }
 private func end(_ result:Result<PersonalHeadMeshCandidate,Error>) {
  lock.lock(); guard phase != .finished else { lock.unlock(); return }
  phase = .finished; let callback = completion; completion = nil
  let answer: Result<PersonalHeadMeshCandidate,Error> = cancelled ? .failure(PersonalHrtfError.message("已取消本机测量")) : result
  colors.removeAll(); poses.reset(); lock.unlock()
  fusion = nil; support = PersonalHeadMeasuredSupport(limit:pointLimit)
  DispatchQueue.main.async { callback?(answer) }
 }
}
