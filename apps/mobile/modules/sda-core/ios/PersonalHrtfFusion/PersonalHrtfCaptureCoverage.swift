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

