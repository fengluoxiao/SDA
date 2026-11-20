import CoreML
import Foundation
import ImageIO
import StandardCyborgFusion
import Vision
import simd

final class PersonalHrtfCaptureCoverage {
 private let model: VNCoreMLModel
 private(set) var guide = PersonalHeadCaptureGuide()
 init() throws {
  guard let url = SCFusionBundle.fusionBundle()?.url(forResource:"SCEarTrackingModel",withExtension:"mlmodelc") else { throw PersonalHrtfError.message("离线耳部识别模型缺失") }
  let config = MLModelConfiguration(); config.computeUnits = .cpuAndGPU
  model = try VNCoreMLModel(for:MLModel(contentsOf:url,configuration:config))
 }
 static func isEar(_ observation:VNRecognizedObjectObservation) -> Bool {
  observation.labels.first?.identifier == "ear"
 }
 func seesEar(_ pixels:CVPixelBuffer,orientation:CGImagePropertyOrientation) -> Bool {
  let request = VNCoreMLRequest(model:model); request.imageCropAndScaleOption = .scaleFill
  do { try VNImageRequestHandler(cvPixelBuffer:pixels,orientation:orientation).perform([request]) }
  catch { return false }
  return (request.results as? [VNRecognizedObjectObservation] ?? []).contains(where:Self.isEar)
 }
 func reset() { guide = PersonalHeadCaptureGuide() }
 func observe(status:ScanStatus,leftAdvanced:Bool,rightAdvanced:Bool,pixels:CVPixelBuffer?,pose:simd_float4x4?) {
  let side: PersonalHeadCaptureGuide.EarSide? = leftAdvanced != rightAdvanced ? (leftAdvanced ? .left : .right) : nil
  var ear: PersonalHeadCaptureGuide.EarSide?
  if let side, side == guide.targetEar, let pixels, let pose {
   let rotations: [CGImagePropertyOrientation] = [.up,.right,.down,.left]
   if seesEar(pixels,orientation:rotations[FaceFrameUpright.quarterTurns(pose)]) { ear = side }
  }
  guide.show(front:status.coverage.front,measuredEar:ear)
 }
}

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
