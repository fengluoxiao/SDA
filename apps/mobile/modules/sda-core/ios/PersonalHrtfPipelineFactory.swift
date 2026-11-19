import Foundation

// This build exposes real front-depth measurement and landmark review. Acoustic
// qualification is deliberately separate: no unqualified mesh becomes an HRTF.
final class PersonalHrtfPipelineFactory {
 static func make() -> PersonalHrtfLocalPipeline? {
  #if canImport(StandardCyborgFusion)
  return PersonalHrtfMeasurementPipeline()
  #else
  return nil
  #endif
 }
 static var capabilities: [String:Any] {
  #if canImport(StandardCyborgFusion)
  return ["scanSupported":PersonalHeadScanController.supported(),"measurementOnly":true,"localReconstruction":true,"personalGenerationReady":false,"cameraPolicy":"front-truedepth-only"]
  #else
  return ["scanSupported":false,"measurementOnly":true,"localReconstruction":false,"personalGenerationReady":false,"cameraPolicy":"front-truedepth-only"]
  #endif
 }
}
