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
  let checked = try surface.confirm(leftFaces:landmarks.leftFaces,rightFaces:landmarks.rightFaces,nose:landmarks.nose,crown:landmarks.crown)
  return Measurement(vertices:surface.vertices,triangles:surface.triangles,support:support,normals:normals,
   leftFaces:checked.leftFaces,rightFaces:checked.rightFaces,front:checked.front,up:checked.up)
 }
 struct PreparedInput {
  let project: URL
  let specification: PersonalHrtfResponseBuilder.Specification
  let origin: [Double]
 }
 enum Failure: LocalizedError {
  case message(String)
  var errorDescription: String? { if case .message(let s) = self { return s }; return nil }
 }
 private let lock = NSLock()
 private var cancelled = false, running = false
 func cancel() { lock.lock(); cancelled = true; lock.unlock() }
 private func stopped() -> Bool { lock.lock(); defer { lock.unlock() }; return cancelled }
 private func require(_ test: Bool,_ message: String) throws { if !test { throw Failure.message(message) } }
 func prepare(root: URL, measurement m: Measurement, bins: Int,
              azimuthSteps: Int, elevationSteps: Int, radius: Double,
              commonShift: Int) throws -> PreparedInput {
  lock.lock()
  guard !running && !cancelled else { lock.unlock(); throw Failure.message("本机输入准备已取消或正在进行") }
  running = true; lock.unlock()
  defer { lock.lock(); running = false; lock.unlock() }
  try require(m.vertices.count % 3 == 0 && (12...60000).contains(m.vertices.count) &&
              m.triangles.count % 3 == 0 && (12...60000).contains(m.triangles.count) &&
              m.support.count % 3 == 0 && (12...1500000).contains(m.support.count) &&
              m.normals.count == m.support.count,"测量数组尺寸无效或超过手机本机求解范围")
  try require(m.front.count == 3 && m.up.count == 3 && !m.leftFaces.isEmpty && !m.rightFaces.isEmpty && m.leftFaces.count <= 1024 && m.rightFaces.count <= 1024,"需要在测量表面上确认左右耳声源面和头部方向")
  try require((16...2048).contains(bins) && bins & (bins-1) == 0 &&
              (8...64).contains(azimuthSteps) && azimuthSteps % 8 == 0 &&
              (4...32).contains(elevationSteps) && elevationSteps % 2 == 0 &&
              (0..<2*bins).contains(commonShift),"本机响应网格、频率或共同延迟设置无效")
  let available = sda_hrtf_available_memory()
  #if os(iOS) && !targetEnvironment(simulator)
  try require(available > 0,"无法确认 iPhone 的可用内存余量")
  #endif
  try require(available == 0 || available >= 256*1024*1024,"iPhone 可用内存不足以准备头耳声学输入")
  let token = Unmanaged.passUnretained(self).toOpaque()
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
         sda_hrtf_prepare_project(path,input,{ context in
          guard let context else { return 1 }
          return Unmanaged<PersonalHrtfProjectBuilder>.fromOpaque(context).takeUnretainedValue().stopped() ? 1 : 0
         },token)
        }
       }
      }
     }
    }
   }
  }
  guard reply.code == 0 else {
   let text = withUnsafePointer(to:&reply.message) { p in p.withMemoryRebound(to:CChar.self,capacity:256) { String(cString:$0) } }
   throw Failure.message("本机声学输入准备失败：\(text)")
  }
  // Identical ordering to native generation: south pole, rings in
  // SOFA's positive-left azimuth, north pole. No device-screen coordinates.
  var directions = [PersonalHrtfResponseBuilder.Direction(nodeID:400000,azimuth:0,elevation:-90)]
  for ring in 1..<elevationSteps {
   for azimuth in 0..<azimuthSteps {
    directions.append(.init(nodeID:400000 + Int32(directions.count),
      azimuth:Double(azimuth)*360/Double(azimuthSteps),elevation:-90+Double(ring)*180/Double(elevationSteps)))
   }
  }
  directions.append(.init(nodeID:400000 + Int32(directions.count),azimuth:0,elevation:90))
  // Native checks all bounds before publishing a project. These values are
  // measured areas and shared delay only; no left/right gain normalization.
  return PreparedInput(project:root.appendingPathComponent("AcousticProject",isDirectory:true),
    specification:.init(directions:directions,bins:bins,
      sourceAreas:[reply.source_areas.0,reply.source_areas.1],referenceRadius:radius,commonShift:commonShift),
    origin:[reply.origin.0,reply.origin.1,reply.origin.2])
 }
}
