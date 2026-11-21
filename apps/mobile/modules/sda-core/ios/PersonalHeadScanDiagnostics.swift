import Foundation

// One scan, written only on the fusion queue; immutable by the time it is shared.
// No arbitrary NSError userInfo, camera frames, paths, poses or vertex coordinates in JSON.
final class PersonalHeadScanLog: Encodable {
 let schema = "sda.truedepth.diagnostics.v1"
 let version = Bundle.main.object(forInfoDictionaryKey:"CFBundleShortVersionString") as? String ?? "test"
 let build = Bundle.main.object(forInfoDictionaryKey:"CFBundleVersion") as? String ?? "test"
 let osVersion = ProcessInfo.processInfo.operatingSystemVersionString
 var stage = "capture", failure = "", registeredFrames = 0, points = 0
 var leftDepthFrames = 0, rightDepthFrames = 0, leftEarConfirmed = false, rightEarConfirmed = false
 var budgetMiB:UInt64 = 0, candidateVertices = 0, candidateTriangles = 0
 struct Attempt: Encodable { let resolution:Int32, code:Int, reason:String, sourceLine:Int?, source:String?, detail:String? }
 var attempts = [Attempt]()
 func record(_ resolution:Int32,_ error:NSError?) {
  let names = [1:"native-exception",2:"unknown-native-exception",3:"no-output",4:"missing-edge",5:"level-set-topology",6:"no-surface",7:"mesh-capacity",8:"allocation"]
  let poisson = error?.domain == "SDA.Poisson"
  let line = poisson ? error?.userInfo["SDAPoissonLine"] as? Int : nil
  let code = error?.code ?? 0, detail = error?.userInfo["SDAContourIssue"] as? String
  let site = poisson && (error?.userInfo["SDAExtractorSite"] as? String) == "FEMTree.LevelSet.3D.inl" ? "FEMTree.LevelSet.3D.inl" : nil
  attempts.append(Attempt(resolution:resolution,code:code,reason:error == nil ? "ok" : (poisson ? names[code] ?? "other-poisson" : "system-error"),sourceLine:line.flatMap { (1...99999).contains($0) ? $0 : nil },source:site,detail:poisson ? detail.flatMap { ["open-contour","edge-map","zero-crossing","triangulation"].contains($0) ? $0 : nil } : nil))
 }
 func finish(_ error:Error,applicationReason:String?) {
  // The caller supplies only SDA-authored validation messages, never a file/OS description.
  if let applicationReason { failure = applicationReason }
  else { let e = error as NSError; failure = "\(e.domain == NSCocoaErrorDomain ? "Cocoa" : (e.domain == NSPOSIXErrorDomain ? "POSIX" : "system")):\(e.code)" }
 }
}

struct PersonalHeadReconstructionFailure: LocalizedError {
 let message:String, log:PersonalHeadScanLog, geometry:PersonalHeadExport?
 var errorDescription:String? { message }
}

enum PersonalHeadExport {
 case log(PersonalHeadScanLog), mesh(PersonalHrtfReviewGeometry, verified:Bool), points([Float],[Float])
 var title:String {
  switch self { case .log:return "导出日志"; case .mesh(_,true):return "导出实测模型"; case .mesh(_,false):return "导出候选模型（未通过检查）"; case .points:return "导出实测点云（无网格）" }
 }
 private var name:String {
  switch self { case .log:return "SDA-scan-log.json"; case .mesh(_,true):return "SDA-measured-surface.ply"; case .mesh(_,false):return "SDA-unverified-candidate.ply"; case .points:return "SDA-measured-points.ply" }
 }
 // Give the share sheet a real file. Never leave a second copy in Documents or a cache.
 final class File {
  let url:URL
  fileprivate init(name:String) throws {
   let directory = FileManager.default.temporaryDirectory.appendingPathComponent("SDAHeadScan",isDirectory:true).appendingPathComponent(UUID().uuidString,isDirectory:true)
   url = directory.appendingPathComponent(name)
   try FileManager.default.createDirectory(at:directory,withIntermediateDirectories:true,attributes:[.protectionKey:FileProtectionType.complete])
  }
  func discard() { try? FileManager.default.removeItem(at:url.deletingLastPathComponent()) }
  deinit { discard() }
 }
 func makeFile() throws -> File {
  let file = try File(name:name)
  if case .log(let log) = self {
   let encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted,.sortedKeys]
   try encoder.encode(log).write(to:file.url,options:[.atomic,.completeFileProtection]); return file
  }
  let xyz:[Float], normals:[Float], faces:[UInt32], kind:String
  switch self {
  case .mesh(let g,let verified): (xyz,normals,faces,kind) = (g.vertices,[],g.triangles,verified ? "measured_surface" : "unverified_candidate")
  case .points(let p,let n):
   guard !p.isEmpty, p.count == n.count else { throw CocoaError(.fileWriteUnknown) }
   (xyz,normals,faces,kind) = (p,n,[],"measured_points")
  case .log: preconditionFailure()
  }
  guard xyz.count.isMultiple(of:3), faces.count.isMultiple(of:3), xyz.count <= 4500000, faces.count <= 6000000 else { throw CocoaError(.fileWriteUnknown) }
  var header = "ply\nformat binary_little_endian 1.0\ncomment SDA \(kind)\ncomment units metres; frame FaceFrame_head\nelement vertex \(xyz.count/3)\nproperty float x\nproperty float y\nproperty float z\n"
  if !normals.isEmpty { header += "property float nx\nproperty float ny\nproperty float nz\n" }
  if !faces.isEmpty { header += "element face \(faces.count/3)\nproperty list uchar uint vertex_indices\n" }
  try Data((header+"end_header\n").utf8).write(to:file.url,options:.completeFileProtection)
  let out = try FileHandle(forWritingTo:file.url); defer { try? out.close() }; try out.seekToEnd()
  var chunk = Data(); chunk.reserveCapacity(65536+24)
  func append(_ value:UInt32) { var le=value.littleEndian; withUnsafeBytes(of:&le){chunk.append(contentsOf:$0)} }
  func flush() throws { try out.write(contentsOf:chunk); chunk.removeAll(keepingCapacity:true) }
  for i in stride(from:0,to:xyz.count,by:3) {
   for j in i..<i+3 { guard xyz[j].isFinite else { throw CocoaError(.fileWriteUnknown) }; append(xyz[j].bitPattern) }
   if !normals.isEmpty { for j in i..<i+3 { guard normals[j].isFinite else { throw CocoaError(.fileWriteUnknown) }; append(normals[j].bitPattern) } }
   if chunk.count >= 65536 { try flush() }
  }
  for i in stride(from:0,to:faces.count,by:3) {
   chunk.append(3); for j in i..<i+3 { guard faces[j] < UInt32(xyz.count/3) else { throw CocoaError(.fileWriteUnknown) }; append(faces[j]) }
   if chunk.count >= 65536 { try flush() }
  }
  try flush(); return file
 }
}
