import Foundation
#if canImport(StandardCyborgFusion)
import StandardCyborgFusion
#endif

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
 #if canImport(StandardCyborgFusion)
 static func poissonMeasurement(positions:[Float],normals:[Float],budget:UInt64,currentBudget:(()->UInt64)? = nil,cancelled:@escaping()->Bool,operationChanged:(SCMeshingOperation?)->Void,startOperation:(SCMeshingOperation)throws->Void = { $0.start() }) throws -> (surface:PersonalHrtfReviewGeometry,distance:Double) {
  guard positions.count == normals.count, positions.count.isMultiple(of:3), (10000...Int(SDA_HRTF_MAXIMUM_SUPPORT_POINTS)).contains(positions.count/3) else { throw PersonalHrtfError.message("原始测量点不足或超出本机预算") }
  let pointBytes = UInt64(positions.count/3)*64
  func available() -> UInt64 { min(budget,currentBudget?() ?? budget) }
  func requireBudget() throws {
   guard available() >= max(pointBytes,64*1024*1024) else { throw PersonalHrtfError.message("当前内存不足以检查实测表面") }
  }
  if cancelled() { throw CancellationError() }; try requireBudget()
  let directory = FileManager.default.temporaryDirectory.appendingPathComponent("SDAHeadScan",isDirectory:true).appendingPathComponent(UUID().uuidString,isDirectory:true)
  try FileManager.default.createDirectory(at:directory,withIntermediateDirectories:true)
  defer { try? FileManager.default.removeItem(at:directory) }
  let input = directory.appendingPathComponent("measured.ply")
  try PersonalHeadMeasuredSupport.writePLY(to:input,positions:positions,normals:normals,cancelled:cancelled)
  let meshURL = try extractSurface(input:input,cancelled:cancelled,requireBudget:requireBudget,operationChanged:operationChanged,start:startOperation)
  let header = try preflight(meshURL,maximumVertices:1000000,maximumFaces:2000000)
  // Bound allocations from actual headers before invoking the upstream readers.
  // Estimate includes geometry, topology/BVH, support search and copied buffers.
  let needed = UInt64(header.vertices)*192 + UInt64(header.faces)*256 + pointBytes, remaining = available()
  guard needed <= remaining else { throw PersonalHrtfError.message("当前候选表面检查预计需要 \(needed/1024/1024) MiB，本机本次可用预算为 \(remaining/1024/1024) MiB；请关闭其他应用后重试") }
  guard let mesh = SCMesh(plyPath:meshURL.path,jpegPath:"") else {
   throw PersonalHrtfError.message("候选网格无法读取，重建文件可能不完整或格式无效，请重新采集")
  }
  guard mesh.vertexCount == header.vertices, mesh.faceCount == header.faces else {
   throw PersonalHrtfError.message("候选网格或测量点数超出本机处理范围")
  }
  // Pinned upstream Vec3 is aligned/padded to 16 bytes; Face3 is three Int32.
  // Reading it as packed xyz would silently distort geometry and ear positions.
  guard mesh.positionData.count == mesh.vertexCount * 16,
        mesh.facesData.count == mesh.faceCount * 12 else {
   throw PersonalHrtfError.message("本机网格布局与固定版本重建器不一致")
  }
  let vertices = try unpack(mesh.positionData,count:mesh.vertexCount,stride:16,offset:0,cancelled:cancelled)
  let faces = mesh.facesData.withUnsafeBytes { raw in
   (0..<mesh.faceCount * 3).map { raw.loadUnaligned(fromByteOffset:$0 * 4,as:UInt32.self) }
  }
  return try measurement(Loaded(surface:try PersonalHrtfReviewGeometry(vertices:vertices,triangles:faces),support:positions,normals:normals),cancelled:cancelled)
 }
 private static func extractSurface(input:URL,cancelled:()->Bool,requireBudget:()throws->Void,operationChanged:(SCMeshingOperation?)->Void,start:(SCMeshingOperation)throws->Void) throws -> URL {
  // Retry only the pinned extractor's missing-edge error. Keep the same
  // absolute-depth points, independent output files, and the same final screen.
  for resolution:Int32 in [6,5] {
   if cancelled() { throw CancellationError() }; try requireBudget()
   let output = input.deletingLastPathComponent().appendingPathComponent("surface-\(resolution).ply")
   do {
    try autoreleasepool {
     let operation = SCMeshingOperation(inputPLYPath:input.path,outputPLYPath:output.path)
     operation.parameters.resolution = resolution; operation.parameters.smoothness = resolution == 6 ? 1 : 2
     operation.parameters.closed = false; operation.parameters.surfaceTrimmingAmount = 0
     operationChanged(operation); defer { operationChanged(nil) }
     if cancelled() { throw CancellationError() }
     try start(operation)
     if cancelled() || operation.isCancelled { throw CancellationError() }
     if let error = operation.sdaError { throw error }
    }
    return output
   } catch {
    if cancelled() || error is CancellationError { throw CancellationError() }
    let failure = error as NSError
    guard failure.domain == "SDA.Poisson" else { throw error }
    guard failure.code == 4 else { throw PersonalHrtfError.message("本次头耳测量未能重建出可检查的表面。") }
   }
  }
  throw PersonalHrtfError.message("双耳已记录，但两次重建都未能连成完整的表面边缘。本次没有生成个人档案，请重新扫描。")
 }
 #endif
 // The measurement build reviews only measured triangles. Poisson's inferred
 // closures never appear in review or satisfy the measurement gate.
 static func measurement(_ loaded: Loaded, cancelled: @escaping () -> Bool) throws -> (surface: PersonalHrtfReviewGeometry, distance: Double) {
  let token = Cancellation(cancelled)
  let vertices = loaded.surface.vertices, faces = loaded.surface.triangles
  var mask = [UInt8](repeating:0,count:faces.count/3)
  let selected = vertices.withUnsafeBufferPointer { v in
   faces.withUnsafeBufferPointer { f in
    loaded.support.withUnsafeBufferPointer { p in
     loaded.normals.withUnsafeBufferPointer { n in
      mask.withUnsafeMutableBufferPointer { m in
       sda_hrtf_select_measured_faces(SdaHrtfMeshView(vertices:v.baseAddress,vertex_count:vertices.count/3,triangles:f.baseAddress,triangle_count:faces.count/3,support:p.baseAddress,support_normals:n.baseAddress,support_count:loaded.support.count/3),m.baseAddress,m.count,{ pointer in
        guard let pointer else { return 1 }
        return Unmanaged<Cancellation>.fromOpaque(pointer).takeUnretainedValue().check() ? 1 : 0
       },Unmanaged.passUnretained(token).toOpaque())
      }
     }
    }
   }
  }
  try requirePassed(selected)
  // Compact used vertices so discarded closures are not retained in the review
  // surface, and every displayed face uses the original measured coordinates.
  var remap = [UInt32](repeating:UInt32.max,count:vertices.count/3)
  var retainedVertices = [Float](), retainedFaces = [UInt32]()
  for face in mask.indices where mask[face] != 0 {
   if face % 1024 == 0 && cancelled() { throw PersonalHrtfError.message("已取消测量表面检查") }
   for j in 0..<3 {
    let old = Int(faces[3*face+j])
    if remap[old] == UInt32.max {
     remap[old] = UInt32(retainedVertices.count/3)
     retainedVertices.append(contentsOf:vertices[3*old..<3*old+3])
    }
    retainedFaces.append(remap[old])
   }
  }
  if cancelled() { throw PersonalHrtfError.message("已取消测量表面检查") }
  let surface = try PersonalHrtfReviewGeometry(vertices:retainedVertices,triangles:retainedFaces)
  let distance = try screen(Loaded(surface:surface,support:loaded.support,normals:loaded.normals),measurementOnly:true,cancelled:cancelled)
  return (surface,distance)
 }
 static func screen(_ loaded: Loaded, cancelled: @escaping () -> Bool) throws -> Double {
  try screen(loaded,measurementOnly:false,cancelled:cancelled)
 }
 private static func screen(_ loaded: Loaded, measurementOnly:Bool, cancelled: @escaping () -> Bool) throws -> Double {
  let token = Cancellation(cancelled)
  let vertices = loaded.surface.vertices, faces = loaded.surface.triangles
  let positions = loaded.support, normals = loaded.normals
  var result = vertices.withUnsafeBufferPointer { v in
   faces.withUnsafeBufferPointer { f in
    positions.withUnsafeBufferPointer { p in
     normals.withUnsafeBufferPointer { n in
      let input = SdaHrtfMeshView(vertices:v.baseAddress,vertex_count:vertices.count/3,triangles:f.baseAddress,triangle_count:faces.count/3,support:p.baseAddress,support_normals:n.baseAddress,support_count:positions.count/3)
      let screen = measurementOnly ? sda_hrtf_screen_measured_surface : sda_hrtf_screen_head_mesh
      return screen(input,{ pointer in
       guard let pointer else { return 1 }
       return Unmanaged<Cancellation>.fromOpaque(pointer).takeUnretainedValue().check() ? 1 : 0
      },Unmanaged.passUnretained(token).toOpaque())
     }
    }
   }
  }
  try requirePassed(result)
  return result.support_distance
 }
 private static func requirePassed(_ value:SdaHrtfMeshResult) throws {
  var result = value
  guard result.code == 0 else {
   if result.code == 2 { throw PersonalHrtfError.message("已取消本机网格检查") }
   let detail = withUnsafePointer(to:&result.message) { p in
    p.withMemoryRebound(to:CChar.self,capacity:256) { String(cString:$0) }
   }
   throw PersonalHrtfError.message("头耳表面未通过本机几何检查：\(detail)")
  }
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
