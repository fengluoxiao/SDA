import Foundation
import AVFoundation
import CoreML
import Vision
import ImageIO
import simd
import StandardCyborgFusion

// Advisory capture evidence, not an acoustic qualification or an ear-canal detector.
// Only accepted registered frames count toward steps; at most one analysis frame is retained.
final class PersonalHrtfCaptureCoverage {
 typealias State = PersonalHeadCaptureGuide
 struct Seed { let box: CGRect; let depth: Float }
 struct Region { let center: SIMD3<Float>; let view: simd_float4x4; let projection: simd_float4x4 }
 private var region: Region?
 var captureRegion: Region? { lock.lock(); defer { lock.unlock() }; return region }
 // Seed only from a measured frontal face, never the first arbitrary camera frame.
 func prepareInitial(color: CVPixelBuffer, depth: CVPixelBuffer, orientation: CGImagePropertyOrientation, completion: @escaping (Seed?) -> Void) {
  queue.async {
   let request = VNDetectFaceRectanglesRequest()
   try? VNImageRequestHandler(cvPixelBuffer:color,orientation:orientation).perform([request])
   let faces = request.results ?? []
   guard faces.count == 1, let face = faces.first,
         face.confidence >= 0.8, abs(face.yaw?.floatValue ?? 100) < 0.3,
         face.boundingBox.width >= 0.15,
         let depthValue = Self.depthMedian(depth,in:face.boundingBox,orientation:orientation) else { completion(nil); return }
   completion(Seed(box:face.boundingBox,depth:depthValue))
  }
 }
 private let queue = DispatchQueue(label:"app.sda.capture-coverage",qos:.utility,autoreleaseFrequency:.workItem)
 private let lock = NSLock()
 private var busy = false, lastSample = -Double.infinity
 private let earModel: VNCoreMLModel
 private var state = State()
 private var anchor: SIMD3<Float>?, subjectLeft: SIMD3<Float>?
 init() throws {
  guard let bundle = SCFusionBundle.fusionBundle(),
        let url = bundle.url(forResource:"SCEarTrackingModel",withExtension:"mlmodelc") else {
   throw PersonalHrtfError.message("本机耳部识别模型缺失，请重新安装测量包")
  }
  let configuration = MLModelConfiguration(); configuration.computeUnits = .cpuAndGPU
  earModel = try VNCoreMLModel(for:MLModel(contentsOf:url,configuration:configuration))
 }
 func offer(color:CVPixelBuffer,depth:CVPixelBuffer,view:simd_float4x4,projection:simd_float4x4,
            orientation:CGImagePropertyOrientation,completion:@escaping(State)->Void) {
  let now = ProcessInfo.processInfo.systemUptime
  lock.lock()
  guard !busy, now-lastSample >= 0.3 else { lock.unlock(); return }
  busy = true; lastSample = now; lock.unlock()
  queue.async { [self] in
   defer { lock.lock(); busy = false; lock.unlock() }
   let faceRequest = VNDetectFaceRectanglesRequest()
   let ears = VNCoreMLRequest(model:earModel); ears.imageCropAndScaleOption = .scaleFill
   do {
    try VNImageRequestHandler(cvPixelBuffer:color,orientation:orientation).perform([faceRequest,ears])
    let cameraToModel = simd_inverse(view)
    func measured(_ box:CGRect, ear: Bool = false) -> SIMD3<Float>? {
     let value = ear ? Self.earDepthMedian(depth,in:box,orientation:orientation) : Self.depthMedian(depth,in:box,orientation:orientation)
     guard let d = value else { return nil }
     return Self.modelPoint(CGPoint(x:box.midX,y:box.midY),depth:d,orientation:orientation,cameraToModel:cameraToModel,projection:projection)
    }
    let camera = SIMD3(cameraToModel.columns.3.x,cameraToModel.columns.3.y,cameraToModel.columns.3.z)
    var seenHead = false
    var leftEvidence: (SIMD3<Float>,Float)?, rightEvidence: (SIMD3<Float>,Float)?
    if let face = faceRequest.results?.max(by: { $0.boundingBox.width*$0.boundingBox.height < $1.boundingBox.width*$1.boundingBox.height }),
       face.confidence >= 0.8, abs(face.yaw?.floatValue ?? 100) < 0.3,
       let center = measured(face.boundingBox) {
     seenHead = true
     if anchor == nil {
      let box = face.boundingBox
      let side = CGRect(x:box.midX+box.width*0.2,y:box.midY-box.height*0.05,width:box.width*0.1,height:box.height*0.1)
      if let d = Self.depthMedian(depth,in:box,orientation:orientation),
         let left = Self.modelPoint(CGPoint(x:side.midX,y:box.midY),depth:d,orientation:orientation,cameraToModel:cameraToModel,projection:projection),
         simd_length(left-center) > 0.01 {
       let camera = SIMD3(cameraToModel.columns.3.x,cameraToModel.columns.3.y,cameraToModel.columns.3.z)
       anchor = center + simd_normalize(center-camera)*0.05
       // An unmirrored frontal image's right is the subject's anatomical left.
       subjectLeft = simd_normalize(left-center)
      }
     }
    }
    if let anchor, let axis = subjectLeft {
     for ear in (ears.results as? [VNRecognizedObjectObservation]) ?? [] {
      guard Self.isEar(ear), let point = measured(ear.boundingBox,ear:true) else { continue }
      let side = PersonalHeadCaptureGuide.qualifiedEarSide(point:point,center:anchor,subjectLeft:axis,camera:camera)
      guard side != 0 else { continue }
      // Multiple boxes from one frame must never satisfy stable-frame evidence.
      if side > 0 {
       if ear.confidence > (leftEvidence?.1 ?? 0) { leftEvidence = (point,ear.confidence) }
      } else {
       if ear.confidence > (rightEvidence?.1 ?? 0) { rightEvidence = (point,ear.confidence) }
      }
     }
    }
    state.observe(head:seenHead,left:leftEvidence?.0,right:rightEvidence?.0,camera:camera,time:now)
    if let anchor {
     lock.lock(); if region == nil { region = Region(center:anchor,view:view,projection:projection) }; lock.unlock()
    }
   } catch { /* A failed inference never counts as measured coverage. */ }
   completion(state)
  }
 }
 // Refresh masking from EVERY accepted registration, not the throttled ML
 // callback. A stale side-view pose can cut the ear out before detection.
 func updatePose(view: simd_float4x4, projection: simd_float4x4) {
  lock.lock(); defer { lock.unlock() }
  if let previous = region { region = Region(center:previous.center,view:view,projection:projection) }
 }
 static func isEar(_ observation: VNRecognizedObjectObservation) -> Bool {
  // The pinned detector's NMS threshold is 0.4; upstream uses its results
  // directly. Do not impose a second 0.8 threshold that drops visible ears.
  observation.confidence >= 0.45 && observation.labels.contains { $0.identifier == "ear" && $0.confidence >= 0.45 }
 }
 // Ear boxes include hair and empty background. Measure the CENTRAL ear patch
 // instead of requiring 60/81 valid depths across the whole detector box.
 // This is only semantic evidence; acoustic geometry keeps its own strict gates.
 static func earDepthMedian(_ buffer:CVPixelBuffer,in box:CGRect,orientation:CGImagePropertyOrientation) -> Float? {
  guard CVPixelBufferGetPixelFormatType(buffer) == kCVPixelFormatType_DepthFloat32,
        box.width > 0.02, box.height > 0.02,
        box.minX >= 0, box.minY >= 0, box.maxX <= 1, box.maxY <= 1 else { return nil }
  let patch = box.insetBy(dx:box.width*0.2,dy:box.height*0.2)
  CVPixelBufferLockBaseAddress(buffer,.readOnly); defer { CVPixelBufferUnlockBaseAddress(buffer,.readOnly) }
  guard let base = CVPixelBufferGetBaseAddress(buffer) else { return nil }
  let width = CVPixelBufferGetWidth(buffer), height = CVPixelBufferGetHeight(buffer), stride = CVPixelBufferGetBytesPerRow(buffer)
  var values = [Float](); values.reserveCapacity(25)
  var pixels = Set<Int>()
  for y in 0..<5 { for x in 0..<5 {
   let p = rawPoint(CGPoint(x:patch.minX+patch.width*(Double(x)+0.5)/5,y:patch.minY+patch.height*(Double(y)+0.5)/5),orientation:orientation)
   let column = min(width-1,max(0,Int(p.x*Double(width)))), row = min(height-1,max(0,Int((1-p.y)*Double(height))))
   guard pixels.insert(row*width+column).inserted else { continue }
   let d = base.load(fromByteOffset:row*stride+column*4,as:Float.self)
   if d.isFinite && d >= 0.15 && d <= 0.65 { values.append(d) }
  } }
  guard values.count >= 12 else { return nil }
  values.sort(); let median = values[values.count/2]
  let support = values.filter { abs($0-median) <= 0.025 }
  guard support.count >= 12, support.count*4 >= values.count*3 else { return nil }
  return support[support.count/2]
 }
 // A fresh depth buffer is required: never modify AVFoundation-owned or retained metadata.
 // Face seed removes the room/torso before ICP; subsequent masks follow the last
 // accepted pose with a generous head-sized bound so cheek overlap remains usable.
 static func maskedDepth(_ input: CVPixelBuffer, seed: Seed?, orientation: CGImagePropertyOrientation, region: Region?) -> CVPixelBuffer? {
  guard CVPixelBufferGetPixelFormatType(input) == kCVPixelFormatType_DepthFloat32,
        seed != nil || region != nil else { return nil }
  let width = CVPixelBufferGetWidth(input), height = CVPixelBufferGetHeight(input)
  guard width <= 640, height <= 640 else { return nil }
  var optional: CVPixelBuffer?
  guard CVPixelBufferCreate(kCFAllocatorDefault,width,height,kCVPixelFormatType_DepthFloat32,nil,&optional) == kCVReturnSuccess, let output = optional else { return nil }
  CVPixelBufferLockBaseAddress(input,.readOnly); CVPixelBufferLockBaseAddress(output,[])
  defer { CVPixelBufferUnlockBaseAddress(input,.readOnly); CVPixelBufferUnlockBaseAddress(output,[]) }
  guard let source = CVPixelBufferGetBaseAddress(input), let destination = CVPixelBufferGetBaseAddress(output) else { return nil }
  let sourceStride = CVPixelBufferGetBytesPerRow(input), targetStride = CVPixelBufferGetBytesPerRow(output)
  let cameraToModel = region.map { simd_inverse($0.view) }
  let inverseProjection = region.map { simd_inverse($0.projection) }
  var seedBounds = CGRect.zero
  if let seed {
   let b = seed.box.insetBy(dx:-seed.box.width*0.3,dy:-seed.box.height*0.2)
   let corners = [CGPoint(x:b.minX,y:b.minY),CGPoint(x:b.maxX,y:b.maxY)].map { rawPoint($0,orientation:orientation) }
   seedBounds = CGRect(x:min(corners[0].x,corners[1].x),y:min(corners[0].y,corners[1].y),width:abs(corners[1].x-corners[0].x),height:abs(corners[1].y-corners[0].y))
  }
  var retained = 0
  for y in 0..<height { for x in 0..<width {
   let d = source.load(fromByteOffset:y*sourceStride+x*4,as:Float.self)
   var valid = d.isFinite && d >= 0.15 && d <= 0.65
   if valid, let region, let cameraToModel, let inverseProjection {
    let clip = SIMD4<Float>((Float(x)+0.5)/Float(width)*2-1,(Float(y)+0.5)/Float(height)*2-1,0,1)
    let v = inverseProjection*clip
    let ray = SIMD3(v.x,v.y,v.z)/v.w
    let p = cameraToModel*SIMD4(ray*(d/abs(ray.z)),1)
    let point = SIMD3(p.x,p.y,p.z)
    valid = simd_distance(point,region.center) <= 0.21
   } else if valid, let seed {
    valid = seedBounds.contains(CGPoint(x:(Double(x)+0.5)/Double(width),y:1-(Double(y)+0.5)/Double(height))) && abs(d-seed.depth) <= 0.12
   }
   destination.storeBytes(of:valid ? d : Float.nan,toByteOffset:y*targetStride+x*4,as:Float.self)
   if valid { retained += 1 }
  } }
  return retained >= 1000 ? output : nil
 }
 static func modelPoint(_ point:CGPoint,depth:Float,orientation:CGImagePropertyOrientation,cameraToModel:simd_float4x4,projection:simd_float4x4) -> SIMD3<Float>? {
  let raw = rawPoint(point,orientation:orientation)
  // The pinned camera projection uses image rows from the top (both focal diagonals negative).
  let clip = SIMD4<Float>(Float(raw.x*2-1),Float(1-raw.y*2),0,1)
  let v = simd_inverse(projection)*clip
  guard abs(v.w) > 1e-6 else { return nil }
  let ray = SIMD3(v.x,v.y,v.z)/v.w
  guard abs(ray.z) > 1e-6 else { return nil }
  let p = cameraToModel*SIMD4(ray * (depth/abs(ray.z)),1)
  let result = SIMD3(p.x,p.y,p.z)
  return result.x.isFinite && result.y.isFinite && result.z.isFinite ? result : nil
 }
 static func rawPoint(_ point:CGPoint,orientation:CGImagePropertyOrientation) -> CGPoint {
  switch orientation {
  case .left: return CGPoint(x:point.y,y:1-point.x)
  case .right: return CGPoint(x:1-point.y,y:point.x)
  case .down: return CGPoint(x:1-point.x,y:1-point.y)
  default: return point
  }
 }
 static func depthMedian(_ buffer:CVPixelBuffer,in box:CGRect,orientation:CGImagePropertyOrientation) -> Float? {
  guard CVPixelBufferGetPixelFormatType(buffer) == kCVPixelFormatType_DepthFloat32,
        box.width > 0.02, box.height > 0.02,
        box.minX >= 0, box.minY >= 0, box.maxX <= 1, box.maxY <= 1 else { return nil }
  CVPixelBufferLockBaseAddress(buffer,.readOnly); defer { CVPixelBufferUnlockBaseAddress(buffer,.readOnly) }
  guard let base = CVPixelBufferGetBaseAddress(buffer) else { return nil }
  let width = CVPixelBufferGetWidth(buffer), height = CVPixelBufferGetHeight(buffer), stride = CVPixelBufferGetBytesPerRow(buffer)
  var values = [Float](); values.reserveCapacity(81)
  for y in 0..<9 { for x in 0..<9 {
   let p = rawPoint(CGPoint(x:box.minX+box.width*(Double(x)+0.5)/9,y:box.minY+box.height*(Double(y)+0.5)/9),orientation:orientation)
   let column = min(width-1,max(0,Int(p.x*Double(width)))), row = min(height-1,max(0,Int((1-p.y)*Double(height))))
   let d = base.load(fromByteOffset:row*stride+column*4,as:Float.self)
   if d.isFinite && d >= 0.15 && d <= 0.65 { values.append(d) }
  } }
  guard values.count >= 60 else { return nil }
  values.sort()
  // Background or discontinuous depth must not qualify the semantic box.
  guard values[values.count*9/10]-values[values.count/10] <= 0.08 else { return nil }
  return values[values.count/2]
 }
}
