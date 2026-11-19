import Foundation
import simd

@main struct ReviewTest {
 static func main() throws {
  let mesh = try ReviewFixture.surface(), nose = try mesh.landmark(face:4,point:SIMD3(0.1,0,0))
  let crown = try mesh.landmark(face:5,point:SIMD3(0,0,0.1))
  let checked = try mesh.confirm(leftFaces:[1,0],rightFaces:[3,2],nose:nose,crown:crown)
  try ReviewFixture.require(checked.leftFaces == [0,1] && checked.rightFaces == [2,3],"source face IDs changed")
  try ReviewFixture.require(abs(checked.front[0]-1) < 1e-7 && abs(checked.up[2]-1) < 1e-7,"measured coordinate axes")
  let reject = ReviewFixture.reject
  try reject { _ = try mesh.confirm(leftFaces:[2,3],rightFaces:[0,1],nose:nose,crown:crown) }
  try reject { _ = try mesh.confirm(leftFaces:[0,0],rightFaces:[2,3],nose:nose,crown:crown) }
  try reject { _ = try mesh.confirm(leftFaces:[0],rightFaces:[0],nose:nose,crown:crown) }
  try reject { _ = try mesh.confirm(leftFaces:[],rightFaces:[2],nose:nose,crown:crown) }
  try reject { _ = try mesh.confirm(leftFaces:[0,4],rightFaces:[2],nose:nose,crown:crown) }
  try reject { _ = try mesh.confirm(leftFaces:[10000],rightFaces:[2],nose:nose,crown:crown) }
  try reject { _ = try mesh.confirm(leftFaces:[0],rightFaces:[2],nose:.init(face:4,point:SIMD3(0,0,0)),crown:crown) }
  try reject { _ = try mesh.landmark(face:-1,point:.zero) }
  try reject { _ = try mesh.landmark(face:4,point:SIMD3(.nan,0,0)) }
  try reject { _ = try mesh.landmark(face:4,point:SIMD3(0.1,1,1)) }
  try reject { _ = try mesh.landmark(face:4,point:SIMD3(0.11,0,0)) }
  var invalid = ReviewFixture.vertices; invalid[0] = .infinity
  try reject { _ = try PersonalHrtfReviewGeometry(vertices:invalid,triangles:ReviewFixture.triangles) }
  try reject { _ = try PersonalHrtfReviewGeometry(vertices:ReviewFixture.vertices,triangles:[0,1,999,0,1,2,0,1,2,0,1,2]) }
  var reversed = ReviewFixture.triangles; reversed.swapAt(1,2)
  let inward = try PersonalHrtfReviewGeometry(vertices:ReviewFixture.vertices,triangles:reversed)
  try reject { _ = try inward.confirm(leftFaces:[0],rightFaces:[2],nose:nose,crown:crown) }
  // Ring-shaped source patches must not be silently closed into a disk.
  var ringVertices = ReviewFixture.vertices
  for (x,z) in ([(-0.004,-0.004),(-0.004,0.004),(0.004,0.004),(0.004,-0.004),(-0.002,-0.002),(-0.002,0.002),(0.002,0.002),(0.002,-0.002)] as [(Float,Float)]) { ringVertices += [x,0.08,z] }
  var ringFaces = ReviewFixture.triangles
  for i in 0..<4 { let a = UInt32(14+i), b = UInt32(14+(i+1)%4), c = UInt32(18+(i+1)%4), d = UInt32(18+i); ringFaces += [a,b,c,a,c,d] }
  let ring = try PersonalHrtfReviewGeometry(vertices:ringVertices,triangles:ringFaces)
  try reject { _ = try ring.confirm(leftFaces:Array(6..<14).map(UInt32.init),rightFaces:[2,3],nose:nose,crown:crown) }
  // Arbitrary rigid camera-space rotation/translation must preserve metre scale
  // and return the correspondingly rotated orthonormal axes without mirroring.
  let rotation = simd_double3x3(simd_quatd(angle:0.73,axis:simd_normalize(SIMD3(1,2,3))))
  let translation = SIMD3<Double>(0.3,-0.1,0.6)
  func transform(_ p: SIMD3<Float>) -> SIMD3<Float> { SIMD3<Float>(rotation*SIMD3<Double>(p)+translation) }
  let moved = stride(from:0,to:mesh.vertices.count,by:3).flatMap { i -> [Float] in
   let p = transform(SIMD3(mesh.vertices[i],mesh.vertices[i+1],mesh.vertices[i+2])); return [p.x,p.y,p.z]
  }
  let rigid = try PersonalHrtfReviewGeometry(vertices:moved,triangles:mesh.triangles)
  let result = try rigid.confirm(leftFaces:[0,1],rightFaces:[2,3],nose:rigid.landmark(face:4,point:transform(nose.point)),crown:rigid.landmark(face:5,point:transform(crown.point)))
  let forward = SIMD3(result.front[0],result.front[1],result.front[2]), up = SIMD3(result.up[0],result.up[1],result.up[2])
  try ReviewFixture.require(simd_length(forward-rotation*SIMD3(1,0,0)) < 1e-5 && simd_length(up-rotation*SIMD3(0,0,1)) < 1e-5,"rigid frame changed")
  print("{\"passed\":true,\"rigidFrameChecked\":true,\"invalidSelectionsRejected\":true,\"holesRejected\":true,\"physicalScanTested\":false,\"acousticAccuracyValidated\":false}")
 }
}
