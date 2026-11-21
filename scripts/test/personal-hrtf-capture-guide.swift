import Foundation
import simd

enum ReplayFailure: Error { case invalid(String) }

@main struct FaceFrameDepthReplay {
 static let width = 144, height = 120
 static let focal: Float = 194, center = Vec3(0,-0.006,-0.06)
 static let shapes: [(Vec3,Vec3)] = [
  (center,Vec3(0.082,0.108,0.092)),(Vec3(0.002,-0.017,0.028),Vec3(0.022,0.031,0.027)),
  (Vec3(0.081,-0.006,-0.062),Vec3(0.014,0.029,0.017)),(Vec3(-0.081,-0.003,-0.062),Vec3(0.013,0.027,0.018))
 ]
 static func pose(_ degrees:Int) -> simd_float4x4 {
  let radians = Float(degrees)*Float.pi/180
  var matrix = simd_float4x4(simd_quatf(angle:radians,axis:Vec3(0,1,0)))
  matrix.columns.3 = SIMD4(center+matrix.transformDirection(Vec3(0,0,0.36)),1)
  return matrix
 }
 static func fixedPhoneChecks() throws {
  // World axes at neutral: +X is the person's left, +Z is their nose
  // (ARFaceAnchor). Hold the phone fixed; rotate the FACE in world space.
  var phone = matrix_identity_float4x4; phone.columns.3.z = 0.36
  let eyes = frontSlots
  // Freeze the 967 device report as a regression: the same positive projection
  // on its named eye-slot vector is a physical RIGHT ear, not a left step.
  let previousSlots = (left:Vec3(0.032,0,0),right:Vec3(-0.032,0,0))
  var guide = PersonalHeadCaptureGuide(); guide.show(front:1,measuredEar:nil)
  let wrong = PersonalHeadEarView.side(advanced:true,pose:pose(65),eyes:previousSlots)
  guide.show(front:1,measuredEar:wrong)
  guard wrong == .right, guide.targetEar == .left else { throw ReplayFailure.invalid("967 wrong ear advanced the left prompt") }
  guide.show(front:1,measuredEar:PersonalHeadEarView.side(advanced:true,pose:pose(-65),eyes:previousSlots))
  guard guide.targetEar == .right else { throw ReplayFailure.invalid("Physical left ear did not advance") }
  func rotation(_ r:Float,_ axis:Vec3) -> simd_float4x4 { simd_float4x4(simd_quatf(angle:r * .pi / 180,axis:axis)) }
  for sign:Float in [-1,1] {
   let face = rotation(sign*65,Vec3(0,1,0)), expected:PersonalHeadCaptureGuide.EarSide = sign < 0 ? .left : .right
   let heading = face.transformDirection(Vec3(0,0,1))
   guard heading.x * sign > 0.8 else { throw ReplayFailure.invalid("Invalid fixture nose direction") }
   for roll:Float in [0,90,180,-90] {
    let view = face.inverse * phone * rotation(roll,Vec3(0,0,1))
    guard PersonalHeadEarView.side(advanced:true,pose:view,eyes:eyes) == expected,
          PersonalHeadEarView.side(advanced:false,pose:view,eyes:eyes) == nil,
          PersonalHeadEarView.side(advanced:true,pose:view,eyes:nil) == nil,
          PersonalHeadEarView.side(advanced:true,pose:view,eyes:(eyes.right,eyes.left)) != expected,
          PersonalHeadEarView.side(advanced:true,pose:view,eyes:(eyes.left,eyes.left)) == nil else { throw ReplayFailure.invalid("Wrong anatomical side on fixed-phone head turn") }
   }
   var shifted = matrix_identity_float4x4; shifted.columns.3.x = sign * 0.6
   let translation = shifted.inverse * phone
   guard PersonalHeadEarView.side(advanced:true,pose:translation,eyes:eyes) == nil else { throw ReplayFailure.invalid("Translation credited an ear") }
  }
 }
 // Synthetic front camera slot ordering; not a recording of the user's ARFrame.
 static let frontSlots = (left:Vec3(-0.032,0,0),right:Vec3(0.032,0,0))
 static func image(_ pose:simd_float4x4) -> [Float] {
  var depths = [Float](repeating:0,count:width*height)
  for row in 0..<height { for column in 0..<width {
   let ray = pose.transformDirection(Vec3((Float(column)-Float(width)/2)/focal,-(Float(row)-Float(height)/2)/focal,-1))
   var nearest = Float.infinity
   for (origin,radius) in shapes {
    let offset = (pose.translation-origin)/radius, direction = ray/radius
    let quadratic = simd_dot(direction,direction), linear = simd_dot(offset,direction)
    let discriminant = linear*linear-quadratic*(simd_dot(offset,offset)-1)
    if discriminant >= 0 { let hit = (-linear-sqrt(discriminant))/quadratic; if hit > 0 { nearest = min(nearest,hit) } }
   }
   if nearest.isFinite { depths[row*width+column] = nearest }
  } }
  return depths
 }
 static func main() throws {
  try fixedPhoneChecks()
  let directory = URL(fileURLWithPath:CommandLine.arguments[1],isDirectory:true)
  let engine = FusionEngine(), front = pose(0), frontalDepth = image(front)
  let intrinsic = simd_float3x3(columns:(SIMD3(focal,0,0),SIMD3(0,focal,0),SIMD3(Float(width)/2,Float(height)/2,1)))
  let frontalImage = DepthImage(width:width,height:height,values:frontalDepth,fx:focal,fy:focal,cx:Float(width)/2,cy:Float(height)/2)
  var vertices = [Vec3]()
  for row in stride(from:4,to:height-4,by:3) { for column in stride(from:4,to:width-4,by:3) {
   let depth = frontalDepth[row*width+column]; if depth > 0 { vertices.append(front.transformPoint(frontalImage.cameraPoint(x:column,y:row,depth:depth))) }
  } }
  let angles = Array(repeating:0,count:36) + Array(stride(from:2,through:70,by:2)) + Array(repeating:70,count:4) + Array(stride(from:68,through:-70,by:-2)) + Array(repeating:-70,count:4)
  var trace = [[String:Any]](), guide = PersonalHeadCaptureGuide(), acceptedPose:simd_float4x4?, observedLeft = false, observedRight = false, geometryRight = 0
  var support = PersonalHeadMeasuredSupport(limit:500000)
  engine.onMeasuredFrame = { [weak engine] image,weights,pose in
   acceptedPose = pose
   if let volume = engine?.volume { support.append(image:image,weights:weights,pose:pose,volume:volume,frame:volume.integratedFrames) }
  }
  for (index,angle) in angles.enumerated() {
   let camera = pose(angle), invalid = index == 33 || index == angles.count-3
   let depths = invalid ? [Float](repeating:0,count:width*height) : (angle == 0 ? frontalDepth : image(camera))
   let faceLost = angle == -64 || angle == -66
   let frame = FusionFrame(time:Double(index)/15,width:width,height:height,depth:depths,calibrationK:intrinsic,calibrationReference:SIMD2(Float(width),Float(height)),intrinsics:intrinsic,imageResolution:SIMD2(Float(width),Float(height)),arPose:faceLost ? nil : camera,motion:index == 0 ? 0 : Float(abs(angle-angles[index-1]))*15,leftEye:faceLost ? nil : Self.frontSlots.left,rightEye:faceLost ? nil : Self.frontSlots.right,faceVertices:engine.needsFaceVertices && !faceLost ? vertices : nil)
   let before = engine.counts; acceptedPose = nil
   let status = engine.process(frame)
   let side = PersonalHeadEarView.side(advanced:engine.counts.left > before.left || engine.counts.right > before.right,pose:acceptedPose,eyes:engine.eyes)
   let left = side == .left, right = side == .right
   guard !(left && right), !invalid || (acceptedPose == nil && !left && !right), !left || angle > 40, !right || angle < -40 else { throw ReplayFailure.invalid("Unexpected side on depth frame \(index), yaw=\(angle)") }
   observedLeft = observedLeft || left; observedRight = observedRight || right
   if faceLost && right && status.tracking == .geometry { geometryRight += 1 }
   guide.show(front:status.coverage.front,measuredEar:left ? .left : (right ? .right : nil))
   if angle == 0 && index < 36 && (guide.stage == .rightEar || guide.complete) { throw ReplayFailure.invalid("Frontal frame credited an ear") }
   let matrix = acceptedPose.map { pose in [pose.columns.0,pose.columns.1,pose.columns.2,pose.columns.3].flatMap { [$0.x,$0.y,$0.z,$0.w] } } ?? []
   trace.append(["yaw":angle,"front":status.coverage.front,"left":left,"right":right,"pose":matrix,"invalidDepth":invalid])
  }
  guard observedLeft, observedRight, guide.complete, geometryRight > 0, support.count >= 10000 else { throw ReplayFailure.invalid("FaceFrame did not register both sides: \(engine.counts), geometry right frames=\(geometryRight)") }
  try support.positions.withUnsafeBytes { try Data($0).write(to:directory.appendingPathComponent("replay-points.f32")) }
  try support.normals.withUnsafeBytes { try Data($0).write(to:directory.appendingPathComponent("replay-normals.f32")) }
  try JSONSerialization.data(withJSONObject:trace).write(to:directory.appendingPathComponent("faceframe-sweep.json"))
  let result: [String:Any] = ["frames":angles.count,"frontalOnlyNoEar":true,"fixedPhoneNoseTurnsPassed":true,"translationOnlyRejected":true,"rollInvariant":true,"leftFrames":trace.filter { $0["left"] as? Bool == true }.count,"rightFrames":trace.filter { $0["right"] as? Bool == true }.count,"rightFramesWithoutFaceAnchor":geometryRight,"missingDepthRejected":true,"supportPoints":support.count,"syntheticDepth":true,"physicalScanTested":false]
  print(String(decoding:try JSONSerialization.data(withJSONObject:result),as:UTF8.self))
 }
}
