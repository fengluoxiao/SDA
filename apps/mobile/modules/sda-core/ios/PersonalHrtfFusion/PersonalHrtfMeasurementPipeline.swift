import Foundation

// A usable on-device measurement build, not an assertion of acoustic accuracy.
// No scan files persist after the coordinator receives this completion.
final class PersonalHrtfMeasurementPipeline: PersonalHrtfLocalPipeline {
 private let lock = NSLock()
 private var cancelled = false
 private var builder:PersonalHrtfProjectBuilder?,solver:PersonalHrtfOnDeviceSolver?,response:PersonalHrtfResponseBuilder?
 func makeReconstruction() throws -> PersonalHeadReconstructor {
  lock.lock(); cancelled = false; lock.unlock()
  return try PersonalHrtfReconstruction(frontTrueDepth:())
 }
 func cancel() { lock.lock();cancelled=true;let b=builder,s=solver,r=response;lock.unlock();b?.cancel();s?.cancel();r?.cancel() }
 private func checkpoint() throws {
  lock.lock();let stopped=cancelled;lock.unlock()
  if stopped {throw CancellationError()}
  guard ProcessInfo.processInfo.thermalState.rawValue < ProcessInfo.ThermalState.serious.rawValue else {throw PersonalHrtfError.message("设备温度过高，已停止本机求解")}
 }
 func generate(_ candidate:PersonalHeadMeshCandidate,landmarks:PersonalHrtfReviewGeometry.Confirmation,root:URL,
               progress:@escaping(String)->Void,completion:@escaping(Result<[String:Any],Error>)->Void){
  let b=PersonalHrtfProjectBuilder(),s=PersonalHrtfOnDeviceSolver(),r=PersonalHrtfResponseBuilder()
  lock.lock();builder=b;solver=s;response=r;lock.unlock()
  let finish:(Result<[String:Any],Error>)->Void = { [self] result in
   lock.lock();builder=nil;solver=nil;response=nil;let stopped=cancelled;lock.unlock()
   completion(stopped ? .failure(CancellationError()) : result)
  }
  DispatchQueue.global(qos:.utility).async { [self] in
   do {
    try checkpoint()
    let p:PersonalHrtfProjectBuilder.PreparedInput
    do {
     guard candidate.geometryScreenVersion==2,candidate.maximumSupportDistance.isFinite,candidate.maximumSupportDistance<=0.0015 else {throw PersonalHrtfError.message("实测表面未通过深度验证")}
     let m=try PersonalHrtfProjectBuilder.measurement(surface:candidate.surface,support:candidate.support,normals:candidate.supportNormals,landmarks:landmarks)
     p=try b.prepareReview(root:root,measurement:m)
    } catch {
     try checkpoint();if error is CancellationError {throw error}
     let log=candidate.scanLog ?? PersonalHeadScanLog();log.stage="prepare-acoustic-input";log.points=candidate.support.count/3
     log.retainedTriangles=candidate.surface.triangles.count/3
     if let detail=error as? PersonalHrtfProjectBuilder.ReviewFailure {
      log.boundaryIssue=detail.issue;log.boundaryRegion=detail.region;log.boundaryLoops=detail.loops;log.depthBackedFaces=detail.depthBackedFaces
     }
     log.finish(error,applicationReason:log.boundaryIssue ?? "input-validation")
     throw PersonalHeadReconstructionFailure(message:error.localizedDescription,log:log,geometry:.mesh(candidate.surface,verified:true),independentPoints:.points(candidate.support,candidate.supportNormals))
    }
    try checkpoint()
    DispatchQueue.main.async { [self] in
     do {try checkpoint()} catch {finish(.failure(error));return}
     progress("正在检查两耳频率和内存预算")
     s.run(qualifiedProject:p.project,frequencies:p.specification.bins,progress:{progress("双耳声学计算 \(Int(100*$0))%")}) { outcome in
      switch outcome {
      case .failure(let error): finish(.failure(error))
      case .success(let solved):
       progress("正在校验双耳响应并生成档案")
       DispatchQueue.global(qos:.utility).async { [self] in
        finish(Result { try checkpoint();let archive = try r.build(project:p.project,specification:p.specification,numericalBudget:solved.numericalBudgetBytes);try checkpoint()
         try FileManager.default.moveItem(at:archive,to:root.appendingPathComponent("response.phrtf"));try FileManager.default.removeItem(at:p.project)
         return ["directions":p.specification.directions.count,"boundaryFaces":p.specification.reviewedGeometry?.boundaryFaces ?? 0,"installed":false] })
       }
      }
     }
    }
   } catch {finish(.failure(error))}
  }
 }
 func process(_ candidate: PersonalHeadMeshCandidate,
              landmarks: PersonalHrtfReviewGeometry.Confirmation,
              completion: @escaping (Result<[String:Any],Error>) -> Void) {
  DispatchQueue.global(qos:.utility).async { [self] in
   let result: Result<[String:Any],Error> = Result {
    let checked = try candidate.surface.revalidate(landmarks)
    lock.lock(); let stopped = cancelled; lock.unlock()
    guard !stopped else { throw PersonalHrtfError.message("已取消测量验证") }
    guard candidate.geometryScreenVersion == 2, candidate.maximumSupportDistance.isFinite,
          candidate.maximumSupportDistance <= 0.0015 else {
     throw PersonalHrtfError.message("测量表面没有通过几何支持检查")
    }
    let vertices = candidate.surface.vertices.count/3, triangles = candidate.surface.triangles.count/3
    // Resource preflight only: an open review surface is not solver input.
    // Return counts, never coordinates or a reusable/cached device plan.
    let plan = PersonalHrtfProjectBuilder.samplingPlan(vertices:vertices,triangles:triangles)
    let sampling: [String:Any] = ["available":plan.tier != 0,"azimuthSteps":Int(plan.azimuth_steps),
     "elevationSteps":Int(plan.elevation_steps),"bins":Int(plan.bins)]
    return ["measurementOnly":true,"installed":false,"partialMeasuredSurface":true,
     "vertices":vertices,"triangles":triangles,"samplingPreflight":sampling,
     "leftFaces":checked.leftFaces.count,"rightFaces":checked.rightFaces.count,
     "message":"头部与双耳实测表面的采集、本机重建和位置确认已完成，无需扫描后脑。临时扫描数据已删除。此测试版用于验证实机测量，尚未生成或启用个人 HRTF：耳部精度与声学网格收敛仍需验证。现有 SDA Spatial 档案继续使用。"]
   }
   completion(result)
  }
 }
}
