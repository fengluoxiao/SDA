import Foundation
import AVFoundation
import CoreML
import Vision
import ImageIO
import simd
import StandardCyborgFusion

// Advisory capture evidence, not an acoustic qualification or an ear-canal detector.
// Only successfully registered frames enter this queue; at most one frame is retained.
final class PersonalHrtfCaptureCoverage {
 struct State {
  var head = 0, leftEar = 0, rightEar = 0
  var status: String {
   func mark(_ n:Int) -> String { n >= 3 ? "区域已采集" : (n > 0 ? "继续缓慢扫描" : "待采集") }
   return "头部：\(mark(head)) · 左耳：\(mark(leftEar)) · 右耳：\(mark(rightEar))"
  }
 }
 private let queue = DispatchQueue(label:"app.sda.capture-coverage",qos:.utility,autoreleaseFrequency:.workItem)
 private let lock = NSLock()
 private var busy = false, lastSample = -Double.infinity
 private let earModel: VNCoreMLModel
 private var state = State()
 private var anchor: SIMD3<Float>?, subjectLeft: SIMD3<Float>?
 private var lastLeft: SIMD3<Float>?, lastRight: SIMD3<Float>?
 init() throws {
  guard let bundle = SCFusionBundle.fusionBundle(),
        let url = bundle.url(forResource:"SCEarTrackingModel",withExtension:"mlmodelc") else {
   throw PersonalHrtfError.message("本机耳部识别模型缺失，请重新安装测量包")
  }
  let configuration = MLModelConfiguration(); configuration.computeUnits = .all
  earModel = try VNCoreMLModel(for:MLModel(contentsOf:url,configuration:configuration))
 }
 func offer(color:CVPixelBuffer,depth:CVPixelBuffer,view:simd_float4x4,projection:simd_float4x4,
            orientation:CGImagePropertyOrientation,completion:@escaping(State)->Void) {
  let now = ProcessInfo.processInfo.systemUptime
  lock.lock()
  guard !busy, now-lastSample >= 0.5 else { lock.unlock(); return }
  busy = true; lastSample = now; lock.unlock()
  queue.async { [self] in
   defer { lock.lock(); busy = false; lock.unlock() }
   let faceRequest = VNDetectFaceRectanglesRequest()
   let ears = VNCoreMLRequest(model:earModel); ears.imageCropAndScaleOption = .scaleFill
   do {
    try VNImageRequestHandler(cvPixelBuffer:color,orientation:orientation).perform([faceRequest,ears])
    let cameraToModel = simd_inverse(view)
    func measured(_ box:CGRect) -> SIMD3<Float>? {
     guard let d = Self.depthMedian(depth,in:box,orientation:orientation) else { return nil }
     return Self.modelPoint(CGPoint(x:box.midX,y:box.midY),depth:d,orientation:orientation,cameraToModel:cameraToModel,projection:projection)
    }
    if let face = faceRequest.results?.max(by: { $0.boundingBox.width*$0.boundingBox.height < $1.boundingBox.width*$1.boundingBox.height }),
       face.confidence >= 0.8, abs(face.yaw?.floatValue ?? 100) < 0.3,
       let center = measured(face.boundingBox) {
     state.head = min(3,state.head+1)
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
      guard ear.confidence >= 0.8, ear.labels.contains(where: { $0.identifier == "ear" && $0.confidence >= 0.8 }),
            let point = measured(ear.boundingBox) else { continue }
      let offset = point-anchor, side = simd_dot(offset,axis), distance = simd_length(offset)
      // Require physically separate ear-side observations in the registered model.
      // Merely turning the phone does not count as a captured ear.
      guard distance >= 0.055, distance <= 0.19, abs(side) >= 0.045 else { continue }
      if side > 0 {
       if let previous = lastLeft, simd_distance(previous,point) > 0.025 { state.leftEar = 0 }
       lastLeft = point; state.leftEar = min(3,state.leftEar+1)
      } else {
       if let previous = lastRight, simd_distance(previous,point) > 0.025 { state.rightEar = 0 }
       lastRight = point; state.rightEar = min(3,state.rightEar+1)
      }
     }
    }
   } catch { /* A failed inference never counts as measured coverage. */ }
   completion(state)
  }
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
