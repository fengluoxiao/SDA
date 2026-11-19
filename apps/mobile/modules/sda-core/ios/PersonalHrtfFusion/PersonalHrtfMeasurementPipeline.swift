import Foundation

// A usable on-device measurement build, not an assertion of acoustic accuracy.
// No scan files persist after the coordinator receives this completion.
final class PersonalHrtfMeasurementPipeline: PersonalHrtfLocalPipeline {
 private let lock = NSLock()
 private var cancelled = false
 func makeReconstruction() throws -> PersonalHeadReconstructor {
  lock.lock(); cancelled = false; lock.unlock()
  return try PersonalHrtfReconstruction(frontTrueDepth:())
 }
 func cancel() { lock.lock(); cancelled = true; lock.unlock() }
 func process(_ candidate: PersonalHeadMeshCandidate,
              landmarks: PersonalHrtfReviewGeometry.Confirmation,
              completion: @escaping (Result<[String:Any],Error>) -> Void) {
  DispatchQueue.global(qos:.utility).async { [self] in
   let result: Result<[String:Any],Error> = Result {
    let checked = try candidate.surface.revalidate(landmarks)
    lock.lock(); let stopped = cancelled; lock.unlock()
    guard !stopped else { throw PersonalHrtfError.message("已取消测量验证") }
    guard candidate.geometryScreenVersion == 1, candidate.maximumSupportDistance.isFinite,
          candidate.maximumSupportDistance <= 0.0015 else {
     throw PersonalHrtfError.message("测量表面没有通过几何支持检查")
    }
    return ["measurementOnly":true,"installed":false,
     "vertices":candidate.surface.vertices.count/3,"triangles":candidate.surface.triangles.count/3,
     "leftFaces":checked.leftFaces.count,"rightFaces":checked.rightFaces.count,
     "message":"前置 TrueDepth 采集、本机重建和位置确认已完成。临时扫描数据已删除。此测试版用于验证实机测量，尚未生成或启用个人 HRTF：耳部精度与声学网格收敛仍需验证。现有 SDA Spatial 档案继续使用。"]
   }
   completion(result)
  }
 }
}
