import Foundation
import simd

// Immutable measured coordinates in metres. Landmark review does not detect
// ears, repair missing anatomy or establish acoustic accuracy.
struct PersonalHrtfReviewGeometry {
 let vertices: [Float], triangles: [UInt32]
 private let identity = UUID()
 struct Failure: LocalizedError {
  let message: String
  var errorDescription: String? { message }
 }
 struct Landmark {
  let face: Int
  let point: SIMD3<Float>
 }
 struct Confirmation {
  fileprivate let surfaceID: UUID
  let leftFaces: [UInt32], rightFaces: [UInt32]
  let nose: Landmark, crown: Landmark
  let front: [Double], up: [Double]
 }
 init(vertices: [Float], triangles: [UInt32]) throws {
  guard vertices.count % 3 == 0, (12...3000000).contains(vertices.count),
        triangles.count % 3 == 0, (12...6000000).contains(triangles.count),
        vertices.allSatisfy({ $0.isFinite && abs($0) < 100 }),
        triangles.allSatisfy({ Int($0) < vertices.count / 3 }) else {
   throw Failure(message:"测量模型尺寸或坐标无效")
  }
  self.vertices = vertices; self.triangles = triangles
 }
 func vertex(_ i: UInt32) -> SIMD3<Double> {
  let j = Int(i) * 3
  return SIMD3(Double(vertices[j]),Double(vertices[j+1]),Double(vertices[j+2]))
 }
 func landmark(face: Int, point: SIMD3<Float>) throws -> Landmark {
  guard (0..<triangles.count/3).contains(face), point.x.isFinite, point.y.isFinite, point.z.isFinite else {
   throw Failure(message:"请在测量表面上选择位置")
  }
  let a = vertex(triangles[3*face]), b = vertex(triangles[3*face+1]), c = vertex(triangles[3*face+2])
  let p = SIMD3<Double>(point), u = b-a, v = c-a, w = p-a, n = simd_cross(u,v)
  let area2 = simd_length(n)
  guard area2 > 1e-12, abs(simd_dot(w,n)/area2) <= 0.00005 else { throw Failure(message:"选择位置不在有效测量面上") }
  let uu = simd_dot(u,u), uv = simd_dot(u,v), vv = simd_dot(v,v), denominator = uu*vv-uv*uv
  guard denominator > 0 else { throw Failure(message:"测量三角面退化") }
  let s = (simd_dot(w,u)*vv-simd_dot(w,v)*uv)/denominator
  let t = (simd_dot(w,v)*uu-simd_dot(w,u)*uv)/denominator
  guard s >= -0.0001, t >= -0.0001, s+t <= 1.0001 else { throw Failure(message:"选择位置超出测量三角面") }
  return Landmark(face:face,point:point)
 }
 private struct Edge: Hashable {
  let a: UInt32, b: UInt32
  init(_ x:UInt32,_ y:UInt32) { a = min(x,y); b = max(x,y) }
 }
 private func patch(_ faces: [UInt32]) throws -> (center: SIMD3<Double>, normal: SIMD3<Double>) {
  guard !faces.isEmpty, faces.count <= 1024, Set(faces).count == faces.count,
        faces.allSatisfy({ Int($0) < triangles.count/3 }) else { throw Failure(message:"左右耳分别需要选择不重复且有界的测量面") }
  var edges: [Edge:[Int]] = [:], points = Set<UInt32>()
  var area = 0.0, center = SIMD3<Double>.zero, normal = SIMD3<Double>.zero
  for (index,face) in faces.enumerated() {
   let offset = 3*Int(face), ids = Array(triangles[offset..<offset+3])
   let a = vertex(ids[0]), b = vertex(ids[1]), c = vertex(ids[2]), n = simd_cross(b-a,c-a)
   let size = simd_length(n)/2
   guard size > 1e-12 else { throw Failure(message:"耳入口选择包含退化面") }
   area += size; center += (a+b+c)*(size/3); normal += n/2
   for j in 0..<3 { points.insert(ids[j]); edges[Edge(ids[j],ids[(j+1)%3]),default:[]].append(index) }
  }
  guard (1e-9...0.001).contains(area) else { throw Failure(message:"耳入口选择的面积无效或过大") }
  var neighbors = [[Int]](repeating:[],count:faces.count), boundary: [UInt32:[UInt32]] = [:]
  for (edge,adjacent) in edges {
   if adjacent.count == 2 { neighbors[adjacent[0]].append(adjacent[1]); neighbors[adjacent[1]].append(adjacent[0]) }
   else if adjacent.count == 1 { boundary[edge.a,default:[]].append(edge.b); boundary[edge.b,default:[]].append(edge.a) }
   else { throw Failure(message:"耳入口选择包含非流形边") }
  }
  var queue = [0], seen: Set<Int> = [0], index = 0
  while index < queue.count { for next in neighbors[queue[index]] where seen.insert(next).inserted { queue.append(next) }; index += 1 }
  guard seen.count == faces.count, !boundary.isEmpty, boundary.values.allSatisfy({ $0.count == 2 }),
        points.count+faces.count == edges.count+1 else { throw Failure(message:"耳入口需为连续、无孔的单片测量面，请重新选择") }
  var loop = [boundary.keys.first!], reached = Set<UInt32>(); reached.insert(loop[0]); index = 0
  while index < loop.count { for next in boundary[loop[index]]! where reached.insert(next).inserted { loop.append(next) }; index += 1 }
  guard reached.count == boundary.count else { throw Failure(message:"耳入口选择含多条边界") }
  return (center/area,normal/area)
 }
 func revalidate(_ confirmation: Confirmation) throws -> Confirmation {
  guard confirmation.surfaceID == identity else { throw Failure(message:"确认位置与当前扫描模型不匹配，请重新确认") }
  return try confirm(leftFaces:confirmation.leftFaces,rightFaces:confirmation.rightFaces,nose:confirmation.nose,crown:confirmation.crown)
 }
 func confirm(leftFaces: [UInt32], rightFaces: [UInt32], nose: Landmark, crown: Landmark) throws -> Confirmation {
  _ = try landmark(face:nose.face,point:nose.point); _ = try landmark(face:crown.face,point:crown.point)
  guard Set(leftFaces).isDisjoint(with:rightFaces) else { throw Failure(message:"左右耳测量面不能重叠") }
  let l = try patch(leftFaces), r = try patch(rightFaces)
  let separation = l.center-r.center, origin = (l.center+r.center)/2
  guard simd_length(separation) > 0.01 else { throw Failure(message:"左右耳入口距离过近，请检查左右耳选择") }
  let left = simd_normalize(separation), top = SIMD3<Double>(crown.point)-origin
  let vertical = top-left*simd_dot(top,left)
  guard simd_length(vertical) > 0.01 else { throw Failure(message:"请在头顶选择向上的测量位置") }
  let up = simd_normalize(vertical), front = simd_cross(left,up)
  let forward = SIMD3<Double>(nose.point)-origin
  guard simd_length(forward) > 0.01, simd_dot(simd_normalize(forward),front) > 0.5 else {
   throw Failure(message:"左右耳、鼻尖与头顶方向不一致；请按本人左右重新选择")
  }
  guard simd_dot(l.normal,left) > 0.2, simd_dot(r.normal,left) < -0.2 else {
   throw Failure(message:"耳入口测量面需分别朝向左右外侧，不能选择耳后或相反表面")
  }
  return Confirmation(surfaceID:identity,leftFaces:leftFaces.sorted(),rightFaces:rightFaces.sorted(),nose:nose,crown:crown,
   front:[front.x,front.y,front.z],up:[up.x,up.y,up.z])
 }
}
