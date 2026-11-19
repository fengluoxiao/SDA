import Foundation
import Metal
import CoreVideo
import ImageIO
import simd
import StandardCyborgFusion

enum PersonalHrtfError: Error { case message(String) } // test-only bridge for the capture helper

@_silgen_name("sdaInstallMeshCrashTrace")
func installMeshCrashTrace()

@main
struct FusionResourcesTest {
 enum Failure: Error { case invalid(String) }
 // Real upstream PLY export and Poisson operation, not a stubbed geometry check.
 static func meshRoundTrip() throws -> [String:Any] {
  let folder = FileManager.default.temporaryDirectory.appendingPathComponent("sda-mesh-probe-" + UUID().uuidString)
  try FileManager.default.createDirectory(at:folder,withIntermediateDirectories:true)
  defer { try? FileManager.default.removeItem(at:folder) }
  let input = folder.appendingPathComponent("sphere.ply")
  let support = folder.appendingPathComponent("support.ply")
  let output = folder.appendingPathComponent("candidate.ply")
  let count = 100000
  var text = "ply\nformat ascii 1.0\ncomment StandardCyborgFusionVersion 1.0.0\nelement vertex \(count)\nproperty float x\nproperty float y\nproperty float z\nproperty float nx\nproperty float ny\nproperty float nz\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nproperty float surfel_radius\nelement face 0\nproperty list uchar int vertex_indices\nend_header\n"
  for i in 0..<count {
   let y = 1 - 2 * (Double(i) + 0.5) / Double(count)
   let r = sqrt(1 - y*y), phi = Double(i) * 2.399963229728653
   let x = cos(phi)*r, z = sin(phi)*r
   text += String(format:"%.8g %.8g %.8g %.8g %.8g %.8g 128 128 128 0.001\n",locale:Locale(identifier:"en_US_POSIX"),x*0.09,y*0.12,z*0.10,x*0.001/0.09,y*0.001/0.12,z*0.001/0.10)
  }
  try text.write(to:input,atomically:true,encoding:.utf8)
  guard let cloud = SCPointCloud(plyPath:input.path), cloud.pointCount == count,
        cloud.writeToPLY(atPath:support.path) else { throw Failure.invalid("Actual point-cloud PLY round trip failed") }
  let tiny = folder.appendingPathComponent("tiny.ply"), tinyExport = folder.appendingPathComponent("tiny-export.ply")
  try text.replacingOccurrences(of:"128 128 128 0.001",with:"128 128 128 0.000001").write(to:tiny,atomically:true,encoding:.utf8)
  guard let small = SCPointCloud(plyPath:tiny.path), small.writeToPLY(atPath:tinyExport.path),
        let reloaded = SCPointCloud(plyPath:tinyExport.path), reloaded.pointCount == count else { throw Failure.invalid("Small measured normals were lost in PLY export") }
  let operation = SCMeshingOperation(inputPLYPath:support.path,outputPLYPath:output.path)
  operation.parameters.resolution = 6
  operation.parameters.smoothness = 1
  operation.parameters.surfaceTrimmingAmount = 0
  operation.parameters.closed = true
  operation.progressHandler = { print("SDA real meshing progress: \($0)") }
  let queue = OperationQueue(); queue.maxConcurrentOperationCount = 1
  queue.addOperation(operation); queue.waitUntilAllOperationsAreFinished()
  let size = (try? output.resourceValues(forKeys:[.fileSizeKey]).fileSize) ?? 0
  guard operation.sdaError == nil, !operation.isCancelled, size >= 128 else { throw Failure.invalid("Actual Poisson operation produced no mesh; bytes=\(size), error=\(String(describing:operation.sdaError))") }
  let cancelled = SCMeshingOperation(inputPLYPath:support.path,outputPLYPath:folder.appendingPathComponent("cancelled.ply").path)
  cancelled.cancel(); queue.addOperation(cancelled); queue.waitUntilAllOperationsAreFinished()
  guard cancelled.isCancelled, !FileManager.default.fileExists(atPath:folder.appendingPathComponent("cancelled.ply").path) else { throw Failure.invalid("Cancelled reconstruction wrote an output") }
  let file = try FileHandle(forReadingFrom:output); defer { try? file.close() }
  let prefix = try file.read(upToCount:512) ?? Data()
  guard String(data:prefix.prefix(64),encoding:.ascii)?.contains("format binary_little_endian 1.0") == true else { throw Failure.invalid("Poisson mesh lost binary Float32 export") }
  let mesh = SCMesh(plyPath:output.path,jpegPath:"")
  guard mesh.vertexCount > 80000, mesh.faceCount > 160000 else { throw Failure.invalid("Actual Poisson surface is empty") }
  return ["syntheticOnly":true,"cancelledOutputAbsent":true,"smallNormalsPreserved":true,"binaryMeshExport":true,"inputPoints":count,"outputBytes":size,"vertices":mesh.vertexCount,"faces":mesh.faceCount]
 }
 static func coverageResources() throws -> [String:Any] {
  _ = try PersonalHrtfCaptureCoverage() // actually load the compiled offline model
  var optional: CVPixelBuffer?
  guard CVPixelBufferCreate(kCFAllocatorDefault,32,32,kCVPixelFormatType_DepthFloat32,nil,&optional) == kCVReturnSuccess,
        let buffer = optional else { throw Failure.invalid("Synthetic depth allocation failed") }
  func fill(_ value:Float) {
   CVPixelBufferLockBaseAddress(buffer,[]); defer { CVPixelBufferUnlockBaseAddress(buffer,[]) }
   let base = CVPixelBufferGetBaseAddress(buffer)!, stride = CVPixelBufferGetBytesPerRow(buffer)
   for y in 0..<32 { for x in 0..<32 { base.storeBytes(of:value,toByteOffset:y*stride+x*4,as:Float.self) } }
  }
  let box = CGRect(x:0.2,y:0.2,width:0.6,height:0.6)
  for bad in [Float.nan,0,0.1,0.8] {
   fill(bad)
   guard PersonalHrtfCaptureCoverage.depthMedian(buffer,in:box,orientation:.up) == nil else { throw Failure.invalid("Invalid/background depth counted as captured") }
  }
  fill(0.3)
  for orientation in [CGImagePropertyOrientation.up,.down,.left,.right] {
   guard PersonalHrtfCaptureCoverage.depthMedian(buffer,in:box,orientation:orientation) == 0.3 else { throw Failure.invalid("Valid oriented depth evidence lost") }
  }
  let left = PersonalHrtfCaptureCoverage.rawPoint(CGPoint(x:0.2,y:0.7),orientation:.left)
  guard abs(left.x-0.7)<1e-6, abs(left.y-0.8)<1e-6 else { throw Failure.invalid("Portrait ROI did not map to raw depth") }
  let projection = simd_float4x4(columns:(SIMD4(-2,0,0,0),SIMD4(0,-2,0,0),SIMD4(0,0,-1.02,-1),SIMD4(0,0,-0.202,0)))
  guard let world = PersonalHrtfCaptureCoverage.modelPoint(CGPoint(x:0.8,y:0.7),depth:0.3,orientation:.up,cameraToModel:matrix_identity_float4x4,projection:projection),
        abs(world.x + 0.09)<1e-5, abs(world.y-0.06)<1e-5, abs(world.z+0.3)<1e-5 else { throw Failure.invalid("Pinned image-row projection mapped the capture region incorrectly") }
  return ["earModelLoaded":true,"invalidDepthRejected":true,"orientationDepthPassed":true,"realEarDetectionValidated":false]
 }
 static func main() throws {
  installMeshCrashTrace()
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
  let roundTrip = try meshRoundTrip()
  let coverage = try coverageResources()
  let result:[String:Any] = ["captureCoverage":coverage,"poissonRoundTrip":roundTrip,"bundleLocated":true,"versionMetadataValid":true,"metalFunctions":functions,"reconstructionInitialized":initialized,"atomicPointCountSnapshotValid":initialized,"resetCompletionPassed":initialized,"scanBudgetConfiguredAndClamped":initialized,"metalGPUAvailable":initialized,"physicalScanTested":false,"earAccuracyValidated":false]
  print(String(data:try JSONSerialization.data(withJSONObject:result,options:[.sortedKeys]),encoding:.utf8)!)
 }
}
