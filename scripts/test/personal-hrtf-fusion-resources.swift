import Foundation
import Metal
import StandardCyborgFusion

@main
struct FusionResourcesTest {
 enum Failure: Error { case invalid(String) }
 static func main() throws {
  let framework = Bundle(for:SCReconstructionManager.self)
  guard framework.bundleIdentifier == "com.standardcyborg.StandardCyborgFusion",
        framework.object(forInfoDictionaryKey:"CFBundleShortVersionString") as? String == "1.0.0",
        let bundle = SCFusionBundle.fusionBundle(),
        bundle.url(forResource:"default",withExtension:"metallib") != nil else {
   throw Failure.invalid("Embedded reconstruction identity/Metal bundle is missing")
  }
  var initialized = false, functions = 0
  if let device = MTLCreateSystemDefaultDevice() {
   let library = try device.makeDefaultLibrary(bundle:bundle)
   functions = library.functionNames.count
   guard functions >= 8, let queue = device.makeCommandQueue() else {
    throw Failure.invalid("Reconstruction Metal functions/queue unavailable")
   }
   let model = SCReconstructionManager(device:device,commandQueue:queue,maxThreadCount:2)
   model.sdaMaximumSurfelCount = 1234567
   guard model.sdaMaximumSurfelCount == 1234567, !model.sdaReachedSurfelLimit else {
    throw Failure.invalid("Configured scan budget did not reach native reconstruction")
   }
   model.sdaMaximumSurfelCount = UInt.max
   guard model.sdaMaximumSurfelCount == 1500000 else { throw Failure.invalid("Native scan budget did not clamp") }
   model.sdaMaximumSurfelCount = 0
   guard model.sdaMaximumSurfelCount == 10000 else { throw Failure.invalid("Native minimum budget did not clamp") }
   model.sdaMaximumSurfelCount = 1234567
   guard model.sdaSurfelCount == 0 else { throw Failure.invalid("New model has stale point count") }
   let lock = NSLock(); var completed = false
   model.finalize { lock.lock(); completed = true; lock.unlock() }
   let deadline = Date().addingTimeInterval(20)
   while true {
    lock.lock(); let done = completed; lock.unlock()
    if done { break }
    guard Date() < deadline else { throw Failure.invalid("Empty reconstruction failed to finalize") }
    RunLoop.current.run(until:Date().addingTimeInterval(0.02))
   }
   lock.lock(); completed = false; lock.unlock()
   model.sdaReset { lock.lock(); completed = true; lock.unlock() }
   let resetDeadline = Date().addingTimeInterval(20)
   while true {
    lock.lock(); let done = completed; lock.unlock()
    if done { break }
    guard Date() < resetDeadline else { throw Failure.invalid("Model release completion timed out") }
    RunLoop.current.run(until:Date().addingTimeInterval(0.02))
   }
   guard model.sdaSurfelCount == 0 else { throw Failure.invalid("Released model has stale point count") }
   initialized = true
  }
  let result:[String:Any] = ["bundleLocated":true,"versionMetadataValid":true,"metalFunctions":functions,"reconstructionInitialized":initialized,"atomicPointCountSnapshotValid":initialized,"resetCompletionPassed":initialized,"scanBudgetConfiguredAndClamped":initialized,"metalGPUAvailable":initialized,"physicalScanTested":false,"earAccuracyValidated":false]
  print(String(data:try JSONSerialization.data(withJSONObject:result,options:[.sortedKeys]),encoding:.utf8)!)
 }
}
