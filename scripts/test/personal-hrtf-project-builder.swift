import Foundation
import simd

@main struct ProjectBuilderTest {
 static func require(_ yes: Bool,_ s: String) throws { if !yes { throw NSError(domain:s,code:1) } }
 static func rows(_ url: URL) throws -> [[Substring]] {
  try String(contentsOf:url,encoding:.utf8).split(whereSeparator:{$0.isNewline}).dropFirst().map { $0.split(whereSeparator:{$0 == " " || $0 == "\t"}) }
 }
 static func reject(_ operation: () throws -> Void) throws {
  do { try operation() } catch { return }; throw NSError(domain:"Expected rejection",code:1)
 }
 static func main() throws {
  let args = CommandLine.arguments
  try require(args.count == 3,"Pass the native synthetic project test root and a fresh Swift test root")
  let source = URL(fileURLWithPath:args[1]).appendingPathComponent("AcousticProject",isDirectory:true)
  let root = URL(fileURLWithPath:args[2],isDirectory:true)
  try require(!FileManager.default.fileExists(atPath:root.path),"Swift fixture root must be fresh")
  try FileManager.default.createDirectory(at:root,withIntermediateDirectories:false)
  let vertices: [Float] = try rows(source.appendingPathComponent("ObjectMeshes/Reference/Nodes.txt")).flatMap { row in
   try row[1...3].map { value in guard let n = Float(value) else { throw NSError(domain:"node",code:1) }; return n }
  }
  let faces: [UInt32] = try rows(source.appendingPathComponent("ObjectMeshes/Reference/Elements.txt")).flatMap { row in
   try row[1...3].map { value in guard let n = UInt32(value) else { throw NSError(domain:"triangle",code:1) }; return n }
  }
  // Test-only measured support on a synthetic cube, never real scan evidence.
  func vertex(_ id: UInt32) -> SIMD3<Float> { let i = Int(id)*3; return SIMD3(vertices[i],vertices[i+1],vertices[i+2]) }
  var support = [Float](), normals = [Float]()
  for f in stride(from:0,to:faces.count,by:3) {
   let a = vertex(faces[f]), b = vertex(faces[f+1]), c = vertex(faces[f+2])
   let n = simd_normalize(simd_cross(b-a,c-a))
   for i in 0...3 { for j in 0...(3-i) {
    let p = a + (b-a)*Float(i)/3 + (c-a)*Float(j)/3
    support.append(contentsOf:[p.x,p.y,p.z]);normals.append(contentsOf:[n.x,n.y,n.z])
   } }
  }
  let measurement = PersonalHrtfProjectBuilder.Measurement(vertices:vertices,triangles:faces,support:support,normals:normals,leftFaces:[24,25],rightFaces:[16],front:[1,0,0],up:[0,0,1])
  // The reviewed IDs/coordinates pass through the production bridge without
  // replacing the surface, scaling it or reusing another scan's confirmation.
  let reviewSurface = try ReviewFixture.surface()
  let landmarks = try reviewSurface.confirm(leftFaces:[0,1],rightFaces:[2,3],nose:reviewSurface.landmark(face:4,point:SIMD3(0.1,0,0)),crown:reviewSurface.landmark(face:5,point:SIMD3(0,0,0.1)))
  let bridged = try PersonalHrtfProjectBuilder.measurement(surface:reviewSurface,support:support,normals:normals,landmarks:landmarks)
  try require(bridged.vertices == reviewSurface.vertices && bridged.triangles == reviewSurface.triangles && bridged.leftFaces == [0,1] && bridged.rightFaces == [2,3],"reviewed source IDs or scale changed")
  try require(bridged.support == support && bridged.normals == normals && bridged.front == [1,0,0] && bridged.up == [0,0,1],"review bridge changed measured inputs")
  try reject { _ = try PersonalHrtfProjectBuilder.measurement(surface:ReviewFixture.surface(),support:support,normals:normals,landmarks:landmarks) }
  let builder = PersonalHrtfProjectBuilder()
  let prepared = try builder.prepare(root:root,measurement:measurement,bins:16,azimuthSteps:16,elevationSteps:4,radius:1.2,commonShift:8)
  let spec = prepared.specification
  try require(spec.directions.count == 50 && spec.bins == 16,"Swift direction dimensions differ")
  try require(abs(spec.sourceAreas[0]-2*spec.sourceAreas[1])<1e-12,"Swift normalized source areas")
  let nodes = try rows(prepared.project.appendingPathComponent("EvaluationGrids/FullSphere/Nodes.txt"))
  for (d,row) in zip(spec.directions,nodes) {
   try require(Int32(row[0]) == d.nodeID,"Swift direction IDs differ")
   let a = d.azimuth*Double.pi/180, e = d.elevation*Double.pi/180
   let expected = [1.2*cos(e)*cos(a),1.2*cos(e)*sin(a),1.2*sin(e)]
   for j in 0..<3 { try require(abs((Double(row[j+1]) ?? .infinity)-expected[j])<1e-12,"Swift direction angles differ from native geometry") }
  }
  try reject { _ = try builder.prepare(root:root,measurement:measurement,bins:16,azimuthSteps:16,elevationSteps:4,radius:1.2,commonShift:8) }
  let cancelled = PersonalHrtfProjectBuilder();cancelled.cancel()
  try reject { _ = try cancelled.prepare(root:root,measurement:measurement,bins:16,azimuthSteps:16,elevationSteps:4,radius:1.2,commonShift:8) }
  try reject { _ = try PersonalHrtfProjectBuilder().prepare(root:root,measurement:measurement,bins:16,azimuthSteps:Int.max,elevationSteps:4,radius:1.2,commonShift:8) }
  try reject { _ = try PersonalHrtfProjectBuilder().prepare(root:root,measurement:measurement,bins:16,azimuthSteps:16,elevationSteps:4,radius:1.2,commonShift:32) }
  // Consume the C++ test's actual solver outputs with the same specification
  // returned through Swift. Synthetic geometry is NEVER installed or selected.
  let archive = try PersonalHrtfResponseBuilder().build(project:source,specification:spec,numericalBudget:32*1024*1024)
  let size = try archive.resourceValues(forKeys:[.fileSizeKey]).fileSize ?? 0
  try require(size > 0 && size <= 16*1024*1024,"Actual solved response archive missing")
  let result: [String:Any] = ["passed":true,"reviewMeasurementBridgeChecked":true,"swiftNativeDirectionMatch":true,"actualSolvedResponseArchived":true,"directions":50,"cancellation":true,"overwriteRejected":true,"physicalScanTested":false,"acousticAccuracyValidated":false]
  print(String(data:try JSONSerialization.data(withJSONObject:result,options:[.sortedKeys]),encoding:.utf8)!)
 }
}
