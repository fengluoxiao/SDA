import Foundation
import simd

struct PersonalHeadMeasuredSupport {
 let limit: Int
 private var occupied = Set<UInt64>()
 private(set) var positions = [Float](), normals = [Float]()
 var count: Int { positions.count / 3 }
 init(limit:Int) { self.limit = limit }

 static func writePLY(to url:URL,positions:[Float],normals:[Float],cancelled:()->Bool) throws {
  guard positions.count == normals.count, positions.count.isMultiple(of:3) else { throw CocoaError(.fileWriteUnknown) }
  let header = "ply\nformat binary_little_endian 1.0\nelement vertex \(positions.count/3)\nproperty float x\nproperty float y\nproperty float z\nproperty float nx\nproperty float ny\nproperty float nz\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nend_header\n"
  try Data(header.utf8).write(to:url)
  let handle = try FileHandle(forWritingTo:url); defer { try? handle.close() }; try handle.seekToEnd()
  var data = Data(); data.reserveCapacity(8192*27)
  func append(_ value:Float) {
   var bits = value.bitPattern.littleEndian
   withUnsafeBytes(of:&bits) { data.append(contentsOf:$0) }
  }
  for start in stride(from:0,to:positions.count,by:3) {
   if start.isMultiple(of:8192*3) {
    if cancelled() { throw CancellationError() }
    try handle.write(contentsOf:data); data.removeAll(keepingCapacity:true)
   }
   for axis in 0..<3 { append(positions[start+axis]) }
   for axis in 0..<3 { append(normals[start+axis]) }
   data.append(contentsOf:[128,128,128])
  }
  try handle.write(contentsOf:data)
 }

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
