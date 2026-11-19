import Foundation
import StandardCyborgFusion

// A conservative geometric screen, never a claim that measured ear detail is
// acoustically sufficient. Ear landmarks/convergence are a separate later gate.
enum PersonalHrtfMeshCheck {
 private final class Cancellation {
  let check: () -> Bool
  init(_ check: @escaping () -> Bool) { self.check = check }
 }
 struct Loaded {
  let surface: PersonalHrtfReviewGeometry
  let support: [Float], normals: [Float]
 }
 static func screen(mesh meshURL: URL, support supportURL: URL, cancelled: @escaping () -> Bool) throws -> Double {
  try screen(try load(mesh:meshURL,support:supportURL,cancelled:cancelled),cancelled:cancelled)
 }
 static func load(mesh meshURL: URL, support supportURL: URL, cancelled: @escaping () -> Bool) throws -> Loaded {
  if cancelled() { throw PersonalHrtfError.message("已取消本机网格检查") }
  let header = try preflight(meshURL,maximumVertices:1000000,maximumFaces:2000000)
  let supportHeader = try preflight(supportURL,maximumVertices:Int(SDA_HRTF_MAXIMUM_SUPPORT_POINTS),maximumFaces:0)
  // Bound allocations from actual headers before invoking the upstream readers.
  // Estimate includes geometry, topology/BVH, support search and copied buffers.
  let needed = UInt64(header.vertices)*160 + UInt64(header.faces)*240 + UInt64(supportHeader.vertices)*64
  let budget = sda_hrtf_recommended_budget(sda_hrtf_available_memory(),ProcessInfo.processInfo.physicalMemory)
  guard needed <= budget else { throw PersonalHrtfError.message("当前候选表面检查预计需要 \(needed/1024/1024) MiB，本机本次可用预算为 \(budget/1024/1024) MiB；请关闭其他应用后重试") }
  let mesh = SCMesh(plyPath:meshURL.path,jpegPath:"")
  guard let support = SCPointCloud(plyPath:supportURL.path),
        (4...1000000).contains(mesh.vertexCount), (4...2000000).contains(mesh.faceCount),
        (4...Int(SDA_HRTF_MAXIMUM_SUPPORT_POINTS)).contains(support.pointCount) else {
   throw PersonalHrtfError.message("候选网格或测量点数超出本机处理范围")
  }
  // Pinned upstream Vec3 is aligned/padded to 16 bytes; Face3 is three Int32.
  // Reading it as packed xyz would silently distort geometry and ear positions.
  guard mesh.positionData.count == mesh.vertexCount * 16,
        mesh.facesData.count == mesh.faceCount * 12 else {
   throw PersonalHrtfError.message("本机网格布局与固定版本重建器不一致")
  }
  let pointStride = Int(SCPointCloud.pointStride())
  let position = Int(SCPointCloud.positionOffset()), normal = Int(SCPointCloud.normalOffset())
  guard pointStride >= 24, pointStride <= 128,
        position >= 0, normal >= 0, position + 12 <= pointStride, normal + 12 <= pointStride,
        SCPointCloud.positionComponentCount() == 3, SCPointCloud.positionComponentSize() == 4,
        SCPointCloud.normalComponentCount() == 3, SCPointCloud.normalComponentSize() == 4,
        support.pointsData.count == support.pointCount * pointStride else {
   throw PersonalHrtfError.message("本机测量点布局不一致")
  }
  let vertices = try unpack(mesh.positionData,count:mesh.vertexCount,stride:16,offset:0,cancelled:cancelled)
  let positions = try unpack(support.pointsData,count:support.pointCount,stride:pointStride,offset:position,cancelled:cancelled)
  let normals = try unpack(support.pointsData,count:support.pointCount,stride:pointStride,offset:normal,cancelled:cancelled)
  let faces = mesh.facesData.withUnsafeBytes { raw in
   (0..<mesh.faceCount * 3).map { raw.loadUnaligned(fromByteOffset:$0 * 4,as:UInt32.self) }
  }
  return Loaded(surface:try PersonalHrtfReviewGeometry(vertices:vertices,triangles:faces),support:positions,normals:normals)
 }
 static func screen(_ loaded: Loaded, cancelled: @escaping () -> Bool) throws -> Double {
  let token = Cancellation(cancelled)
  let vertices = loaded.surface.vertices, faces = loaded.surface.triangles
  let positions = loaded.support, normals = loaded.normals
  var result = vertices.withUnsafeBufferPointer { v in
   faces.withUnsafeBufferPointer { f in
    positions.withUnsafeBufferPointer { p in
     normals.withUnsafeBufferPointer { n in
      let input = SdaHrtfMeshView(vertices:v.baseAddress,vertex_count:vertices.count/3,triangles:f.baseAddress,triangle_count:faces.count/3,support:p.baseAddress,support_normals:n.baseAddress,support_count:positions.count/3)
      return sda_hrtf_screen_head_mesh(input,{ pointer in
       guard let pointer else { return 1 }
       return Unmanaged<Cancellation>.fromOpaque(pointer).takeUnretainedValue().check() ? 1 : 0
      },Unmanaged.passUnretained(token).toOpaque())
     }
    }
   }
  }
  guard result.code == 0 else {
   if result.code == 2 { throw PersonalHrtfError.message("已取消本机网格检查") }
   let detail = withUnsafePointer(to:&result.message) { p in
    p.withMemoryRebound(to:CChar.self,capacity:256) { String(cString:$0) }
   }
   throw PersonalHrtfError.message("头耳表面未通过本机几何检查：\(detail)")
  }
  return result.support_distance
 }
 // Reject oversized count declarations before the upstream PLY reader can
 // reserve a buffer. This also bounds failures if a private job is incomplete.
 private static func preflight(_ url:URL,maximumVertices:Int,maximumFaces:Int) throws -> (vertices:Int,faces:Int) {
  let size = try url.resourceValues(forKeys:[.fileSizeKey]).fileSize ?? 0
  guard (128...256 * 1024 * 1024).contains(size) else { throw PersonalHrtfError.message("本机测量文件尺寸无效") }
  let handle = try FileHandle(forReadingFrom:url); defer { try? handle.close() }
  let prefix = try handle.read(upToCount:16384) ?? Data()
  let marker = prefix.range(of:Data("end_header\n".utf8)) ?? prefix.range(of:Data("end_header\r\n".utf8))
  guard let marker, let header = String(data:prefix[..<marker.upperBound],encoding:.ascii) else {
   throw PersonalHrtfError.message("本机测量文件头不完整")
  }
  var vertices:Int?, faces:Int?
  for line in header.split(whereSeparator: { $0.isNewline }) {
   let parts = line.split(whereSeparator: { $0 == " " || $0 == "\t" })
   if parts.first == "element" {
    guard parts.count == 3, let count = Int(parts[2]), count >= 0 else { throw PersonalHrtfError.message("测量文件元素声明无效") }
    if parts[1] == "vertex", vertices == nil, count <= maximumVertices { vertices = count }
    else if parts[1] == "face", faces == nil, count <= maximumFaces { faces = count }
    else { throw PersonalHrtfError.message("测量文件声明超出本机处理范围") }
   }
  }
  guard let vertices, vertices >= 4, (maximumFaces == 0 || (faces ?? 0) >= 4) else {
   throw PersonalHrtfError.message("测量文件缺少有效表面")
  }
  return (vertices,faces ?? 0)
 }
 private static func unpack(_ data: Data,count:Int,stride:Int,offset:Int,cancelled:() -> Bool) throws -> [Float] {
  try data.withUnsafeBytes { raw in
   var values = [Float](); values.reserveCapacity(count * 3)
   for i in 0..<count {
    if i % 4096 == 0 && cancelled() { throw PersonalHrtfError.message("已取消本机网格检查") }
    for j in 0..<3 { values.append(raw.loadUnaligned(fromByteOffset:i * stride + offset + j * 4,as:Float.self)) }
   }
   return values
  }
 }
}
