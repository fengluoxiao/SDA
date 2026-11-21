import Foundation

enum ScanExportTest {
 static func run() throws {
  let p:[Float] = [0,0,0,0.1,0,0,0,0.1,0,0,0,0.1], f:[UInt32] = [0,2,1,0,1,3,1,2,3,2,0,3]
  let n:[Float] = [1,0,0,0,1,0,0,0,1,0,1,0]
  let surface = try PersonalHrtfReviewGeometry(vertices:p,triangles:f)
  for item in [PersonalHeadExport.mesh(surface,verified:true),.mesh(surface,verified:false),.points(p,n)] {
   var file:PersonalHeadExport.File? = try item.makeFile(); let url=file!.url
   let data = try Data(contentsOf:url), marker = Data("end_header\n".utf8)
   guard let end = data.range(of:marker)?.upperBound,let header = String(data:data[..<end],encoding:.ascii) else { throw NSError(domain:"PLY header",code:1) }
   let points = url.lastPathComponent == "SDA-measured-points.ply", width = points ? 24 : 12
   try ReviewFixture.require(header.contains("element vertex 4\n") && header.contains("property float nx\n") == points && header.contains("element face 4\n") == !points,"PLY schema")
   try ReviewFixture.require(data.count == end+4*width+(points ? 0 : 4*13),"PLY length")
   try data.withUnsafeBytes { raw in
    func u32(_ offset:Int)->UInt32 { UInt32(littleEndian:raw.loadUnaligned(fromByteOffset:offset,as:UInt32.self)) }
    for i in p.indices {
     let offset=end+i/3*width+i%3*4
     try ReviewFixture.require(Float(bitPattern:u32(offset)) == p[i],"Changed measured coordinate")
     if points { try ReviewFixture.require(Float(bitPattern:u32(offset+12)) == n[i],"Changed measured normal") }
     else { let face=end+48+i/3*13; try ReviewFixture.require(raw[face] == 3 && u32(face+1+i%3*4) == f[i],"Changed mesh indices") }
    }
   }
   if case .mesh(_,let verified) = item { try ReviewFixture.require(header.contains(verified ? "SDA measured_surface" : "SDA unverified_candidate"),"incorrect validity label") }
   file = nil; try ReviewFixture.require(!FileManager.default.fileExists(atPath:url.path),"Abandoned share retained a PLY")
  }
  let log = PersonalHeadScanLog(); log.stage="extract-surface"; log.leftDepthFrames=3; log.rightDepthFrames=4
  let underlying = NSError(domain:"SDA.Poisson",code:5,userInfo:[NSLocalizedDescriptionKey:"/private/person/secret.ply XYZ", "SDAPoissonLine":1648])
  log.record(6,underlying); log.record(5,nil); log.finish(underlying,applicationReason:nil)
  let file = try PersonalHeadExport.log(log).makeFile(), data = try Data(contentsOf:file.url)
  let object = try JSONSerialization.jsonObject(with:data) as? [String:Any], attempts = object?["attempts"] as? [[String:Any]]
  try ReviewFixture.require(file.url.pathExtension == "json" && object?["stage"] as? String == "extract-surface" && attempts?.count == 2 && attempts?.first?["reason"] as? String == "level-set-topology" && attempts?.first?["sourceLine"] as? Int == 1648,"Incomplete failure diagnostics")
  try ReviewFixture.require(data.range(of:Data("secret.ply".utf8)) == nil && object?["vertices"] == nil && object?["normals"] == nil,"Private geometry or path in diagnostic log")
  file.discard(); try ReviewFixture.require(!FileManager.default.fileExists(atPath:file.url.path),"Share completion retained a log")
  let acoustic=PersonalHeadScanLog();acoustic.stage="prepare-acoustic-input";acoustic.boundaryLoops=2;acoustic.depthBackedFaces=0
  acoustic.boundaryIssue="insufficient-interior-depth";acoustic.boundaryRegion="right-ear"
  let report=PersonalHeadReconstructionFailure(message:"合成测试",log:acoustic,geometry:.mesh(surface,verified:true),independentPoints:.points(p,n))
  let diagnostic=try report.exports[0].makeFile();defer { diagnostic.discard() }
  let metadata=try JSONSerialization.jsonObject(with:Data(contentsOf:diagnostic.url)) as? [String:Any]
  try ReviewFixture.require(metadata?["boundaryIssue"] as? String == acoustic.boundaryIssue && metadata?["boundaryRegion"] as? String == "right-ear" && metadata?["boundaryLoops"] as? Int == 2 && metadata?["depthBackedFaces"] as? Int == 0 && metadata?["support"] == nil && metadata?["vertices"] == nil,"Unexpected acoustic diagnostic payload")
 }
}
