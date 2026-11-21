import Foundation

// Worker-only bridge from measured arrays + explicitly confirmed ear patches.
// PreparedInput is NOT an acoustic qualification; the pipeline must establish
// ear accuracy and frequency convergence before calling the production solver.
final class PersonalHrtfProjectBuilder {
 struct Measurement {
  let vertices: [Float], triangles: [UInt32], support: [Float], normals: [Float]
  let leftFaces: [UInt32], rightFaces: [UInt32]
  let front: [Double], up: [Double]
 }
 // Recheck the same immutable surface used by the review controller.
 static func measurement(surface: PersonalHrtfReviewGeometry, support: [Float], normals: [Float],
                         landmarks: PersonalHrtfReviewGeometry.Confirmation) throws -> Measurement {
  let checked = try surface.revalidate(landmarks)
  return Measurement(vertices:surface.vertices,triangles:surface.triangles,support:support,normals:normals,
   leftFaces:checked.leftFaces,rightFaces:checked.rightFaces,front:checked.front,up:checked.up)
 }
 struct PreparedInput {
  let project: URL
  let specification: PersonalHrtfResponseBuilder.Specification
  let origin: [Double]
 }
 static func samplingPlan(vertices:Int, triangles:Int, headroom:UInt64 = sda_hrtf_available_memory(),
                          physicalMemory:UInt64 = ProcessInfo.processInfo.physicalMemory) -> SdaHrtfSamplingPlan {
  sda_hrtf_sampling_plan(headroom,physicalMemory,vertices,triangles)
 }
 // Freeze the grid before writing either ear's project. Both ears and the
 // response builder must use exactly the same node IDs and FFT bins.
 func prepare(root:URL, measurement:Measurement,
              headroom:UInt64 = sda_hrtf_available_memory(),
              physicalMemory:UInt64 = ProcessInfo.processInfo.physicalMemory) throws -> PreparedInput {
  let plan = Self.samplingPlan(vertices:measurement.vertices.count/3,triangles:measurement.triangles.count/3,
                               headroom:headroom,physicalMemory:physicalMemory)
  guard plan.tier != 0 else { throw Failure.message("当前内存余量或声学网格规模不适合本机求解，未更改现有档案") }
  return try prepare(root:root,measurement:measurement,bins:Int(plan.bins),azimuthSteps:Int(plan.azimuth_steps),
                     elevationSteps:Int(plan.elevation_steps),radius:1.2,commonShift:Int(plan.bins)/2)
 }
 enum Failure: LocalizedError {
  case message(String)
  var errorDescription: String? { if case .message(let s) = self { return s }; return nil }
 }
 struct ReviewFailure: LocalizedError {
  let message:String, issue:String, region:String?, loops:Int, depthBackedFaces:Int
  var errorDescription:String? { message }
  init(_ result:SdaHrtfReviewProjectResult,detail:String){
   loops=Int(result.review_loops);depthBackedFaces=Int(result.retriangulated_faces)
   region=result.boundary_region.map { String(cString:$0) }
   let area=["left-ear":"左耳周围","right-ear":"右耳周围","both-ears":"双耳周围","posterior":"后方","front-or-side":"正脸或侧面"][region ?? ""] ?? "实测表面"
   let code=result.boundary_issue.map { String(cString:$0) }
   let why:String
   switch code {
   case "insufficient-interior-depth": why="\(area)的小孔内部缺少位置和方向一致的实测深度。"
   case "confirmed-source-gap": why="\(area)的缺口靠近已确认的耳道声源，不能代替实测。"
   case "open-anatomical-contour": why="\(area)有超出局部修补范围的开口，不能当作后方计算边界。"
   case "ambiguous-posterior-contours": why="无法从这些开口确定唯一的后方计算边界。"
   case "too-many-contours": why="实测表面有过多分离的开口，超过本机局部修补范围。"
   case "gap-outside-repair-limit","nonplanar-gap": why="\(area)的缺口范围或形状超出了可验证的修补范围。"
   default:
    if detail.contains("measured depth support") || detail.contains("ear fold") || detail.contains("Measured folds"){why="简化后的头耳表面与实测深度不够一致。"}
    else if detail.contains("Posterior contour"){why="后方轮廓无法形成有效的计算边界。"}
    else if result.project.code == 3 {why="声学输入准备达到本机资源上限。"}
    else {why="声学输入未通过检查：\(detail)。"}
   }
   issue=code ?? (result.project.code == 3 ? "resource-bound" : "project-validation")
   message=why+"现有档案未更改，无需扫描后脑。"
  }
 }
 private let lock = NSLock()
 private var cancelled = false, running = false
 func cancel() { lock.lock(); cancelled = true; lock.unlock() }
 private func stopped() -> Bool { lock.lock(); defer { lock.unlock() }; return cancelled }
 private func require(_ test: Bool,_ message: String) throws { if !test { throw Failure.message(message) } }
 func prepare(root: URL, measurement m: Measurement, bins: Int,
              azimuthSteps: Int, elevationSteps: Int, radius: Double,
              commonShift: Int, reviewResources:(headroom:UInt64,physical:UInt64)? = nil) throws -> PreparedInput {
  lock.lock()
  guard !running && !cancelled else { lock.unlock(); throw Failure.message("本机输入准备已取消或正在进行") }
  running = true; lock.unlock()
  defer { lock.lock(); running = false; lock.unlock() }
  try require(m.vertices.count % 3 == 0 && (12...(reviewResources == nil ? 60000 : 3000000)).contains(m.vertices.count) &&
              m.triangles.count % 3 == 0 && (12...(reviewResources == nil ? 60000 : 6000000)).contains(m.triangles.count) &&
              m.support.count % 3 == 0 && (4...Int(SDA_HRTF_MAXIMUM_SUPPORT_POINTS)).contains(m.support.count/3) &&
              m.normals.count == m.support.count,"测量数组尺寸无效或超过手机本机求解范围")
  try require(m.front.count == 3 && m.up.count == 3 && !m.leftFaces.isEmpty && !m.rightFaces.isEmpty && m.leftFaces.count <= 1024 && m.rightFaces.count <= 1024,"需要在测量表面上确认左右耳声源面和头部方向")
  try require((16...2048).contains(bins) && bins & (bins-1) == 0 &&
              (8...64).contains(azimuthSteps) && azimuthSteps % 8 == 0 &&
              (4...32).contains(elevationSteps) && elevationSteps % 2 == 0 &&
              (0..<2*bins).contains(commonShift),"本机响应网格、频率或共同延迟设置无效")
  let available = sda_hrtf_available_memory()
  #if os(iOS) && !targetEnvironment(simulator)
  try require(available > 0,"无法确认本机的可用内存余量")
  #endif
  try require(available == 0 || available >= 256*1024*1024,"本机可用内存不足以准备头耳声学输入")
  let token = Unmanaged.passUnretained(self).toOpaque()
  var selected:SdaHrtfReviewProjectResult?
  var reply = m.vertices.withUnsafeBufferPointer { vertices in
   m.triangles.withUnsafeBufferPointer { triangles in
    m.support.withUnsafeBufferPointer { support in
     m.normals.withUnsafeBufferPointer { normals in
      m.leftFaces.withUnsafeBufferPointer { left in
       m.rightFaces.withUnsafeBufferPointer { right in
        let mesh = SdaHrtfMeshView(vertices:vertices.baseAddress,vertex_count:m.vertices.count/3,
          triangles:triangles.baseAddress,triangle_count:m.triangles.count/3,
          support:support.baseAddress,support_normals:normals.baseAddress,support_count:m.support.count/3)
        let input = SdaHrtfProjectInput(mesh:mesh,left_faces:left.baseAddress,left_face_count:left.count,
          right_faces:right.baseAddress,right_face_count:right.count,
          front:(m.front[0],m.front[1],m.front[2]),up:(m.up[0],m.up[1],m.up[2]),
          bins:bins,azimuth_steps:Int32(azimuthSteps),elevation_steps:Int32(elevationSteps),reference_radius:radius)
        return root.path.withCString { path in
         let stop:SdaHrtfMeshCancelled = { context in
          guard let context else { return 1 }
          return Unmanaged<PersonalHrtfProjectBuilder>.fromOpaque(context).takeUnretainedValue().stopped() ? 1 : 0
         }
         if let resources = reviewResources {
          let output = sda_hrtf_prepare_review_project(path,input,resources.headroom,resources.physical,stop,token)
          selected = output;return output.project
         }
         return sda_hrtf_prepare_project(path,input,stop,token)
        }
       }
      }
     }
    }
   }
  }
  guard reply.code == 0 else {
   let text = withUnsafePointer(to:&reply.message) { p in p.withMemoryRebound(to:CChar.self,capacity:256) { String(cString:$0) } }
   if reply.code == 2 {throw CancellationError()}
   if let selected {throw ReviewFailure(selected,detail:text)}
   throw Failure.message("本机声学输入准备失败：\(text)")
  }
  // Identical ordering to native generation: south pole, rings in
  // SOFA's positive-left azimuth, north pole. No device-screen coordinates.
  let az = selected.map { Int($0.plan.azimuth_steps) } ?? azimuthSteps
  let el = selected.map { Int($0.plan.elevation_steps) } ?? elevationSteps
  let frequencies = selected.map { Int($0.plan.bins) } ?? bins
  var directions = [PersonalHrtfResponseBuilder.Direction(nodeID:400000,azimuth:0,elevation:-90)]
  for ring in 1..<el {
   for azimuth in 0..<az {
    directions.append(.init(nodeID:400000 + Int32(directions.count),
      azimuth:Double(azimuth)*360/Double(az),elevation:-90+Double(ring)*180/Double(el)))
   }
  }
  directions.append(.init(nodeID:400000 + Int32(directions.count),azimuth:0,elevation:90))
  // Native checks all bounds before publishing a project. These values are
  // measured areas and shared delay only; no left/right gain normalization.
  return PreparedInput(project:root.appendingPathComponent("AcousticProject",isDirectory:true),
    specification:.init(directions:directions,bins:frequencies,
      sourceAreas:[reply.source_areas.0,reply.source_areas.1],referenceRadius:radius,commonShift:selected == nil ? commonShift : frequencies/2,
      reviewedGeometry:selected.map { .init(measuredFaces:Int($0.measured_faces),boundaryFaces:Int($0.boundary_faces),reviewDeviation:$0.review_deviation,depthBackedFaces:Int($0.retriangulated_faces)) }),
    origin:[reply.origin.0,reply.origin.1,reply.origin.2])
 }
 func prepareReview(root:URL,measurement:Measurement,headroom:UInt64 = sda_hrtf_available_memory(),physical:UInt64 = ProcessInfo.processInfo.physicalMemory) throws -> PreparedInput {
  // The final grid is chosen natively after bounded simplification and closure.
  try prepare(root:root,measurement:measurement,bins:256,azimuthSteps:16,elevationSteps:8,radius:1.2,commonShift:128,reviewResources:(headroom,physical))
 }
}
