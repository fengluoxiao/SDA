import Foundation
import simd

struct PersonalHeadMeasuredSupport {
 let limit: Int
 private var occupied = Set<UInt64>()
 private(set) var positions = [Float](), normals = [Float]()
 var count: Int { positions.count / 3 }
 init(limit:Int) { self.limit = limit }

 mutating func append(image:DepthImage,weights:[UInt8],pose:simd_float4x4,volume:TSDFVolume,frame:Int) {
  guard weights.count == image.width*image.height, image.width > 4, image.height > 4, count < limit else { return }
  let high = volume.origin + Vec3(volume.dims)*volume.voxelSize
  for row in stride(from:1+(frame/3)%3,to:image.height-1,by:3) {
   for column in stride(from:1+frame%3,to:image.width-1,by:3) {
    let index = row*image.width+column
    guard weights[index] > 0 else { continue }
    let point = image.cameraPoint(x:column,y:row,depth:image.values[index])
    let head = pose.transformPoint(point)
    guard head.x >= volume.origin.x, head.y >= volume.origin.y, head.z >= volume.origin.z,
          head.x < high.x, head.y < high.y, head.z < high.z else { continue }
    let cell = (head-volume.origin)*1000
    let key = UInt64(cell.x) | (UInt64(cell.y)<<16) | (UInt64(cell.z)<<32)
    guard !occupied.contains(key) else { continue }
    let dx = image.cameraPoint(x:column+1,y:row,depth:image.values[index+1])-image.cameraPoint(x:column-1,y:row,depth:image.values[index-1])
    let dy = image.cameraPoint(x:column,y:row+1,depth:image.values[index+image.width])-image.cameraPoint(x:column,y:row-1,depth:image.values[index-image.width])
    var normal = simd_cross(dx,dy)
    guard simd_length_squared(normal) > 1e-20 else { continue }
    if simd_dot(normal,point) > 0 { normal = -normal }
    normal = simd_normalize(pose.transformDirection(normal))
    occupied.insert(key)
    positions.append(contentsOf:[head.x,head.y,head.z]); normals.append(contentsOf:[normal.x,normal.y,normal.z])
    if count == limit { return }
   }
  }
 }
}
