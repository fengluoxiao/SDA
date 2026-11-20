import Foundation
import Metal
import CoreVideo
import CoreML
import Vision
import ImageIO
import simd
import StandardCyborgFusion

enum PersonalHrtfError: Error { case message(String) } // test-only bridge for the capture helper

@_silgen_name("sdaInstallMeshCrashTrace")
func installMeshCrashTrace()

@_silgen_name("sdaTestTrackingFusion")
func testTrackingFusion() -> UInt32

@main
struct FusionResourcesTest {
 enum Failure: Error { case invalid(String) }
 // Real upstream PLY export and Poisson operation, not a stubbed geometry check.
 static func meshRoundTrip(partial:Bool = false) throws -> [String:Any] {
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
   let phi = Double(i) * 2.399963229728653
   let z = partial ? (Double(i)+0.5)/Double(count) : sin(phi)*sqrt(1-y*y)
   let x = partial ? cos(phi)*sqrt(1-z*z) : cos(phi)*sqrt(1-y*y)
   let measuredY = partial ? sin(phi)*sqrt(1-z*z) : y
   text += String(format:"%.8g %.8g %.8g %.8g %.8g %.8g 128 128 128 0.001\n",locale:Locale(identifier:"en_US_POSIX"),x*0.09,measuredY*0.12,z*0.10,x*0.001/0.09,measuredY*0.001/0.12,z*0.001/0.10)
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
  operation.parameters.closed = !partial
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
  guard let mesh = SCMesh(plyPath:output.path,jpegPath:""), mesh.vertexCount > 80000, mesh.faceCount > 160000 else { throw Failure.invalid("Actual Poisson surface is empty") }
  print("SDA Poisson mesh readback: vertices=\(mesh.vertexCount), faces=\(mesh.faceCount)")
  var reviewFaces = 0
  if partial {
   // os_proc_available_memory reports zero in this standalone simulator process.
   // Do not weaken the device budget gate to accommodate the test environment.
   // Decode this bounded synthetic fixture, then run the real measurement path.
   guard mesh.positionData.count == mesh.vertexCount*16,
         mesh.facesData.count == mesh.faceCount*12 else { throw Failure.invalid("Fixture mesh buffer layout changed") }
   let vertices = mesh.positionData.withUnsafeBytes { raw in
    (0..<mesh.vertexCount*3).map { raw.loadUnaligned(fromByteOffset:($0/3)*16+($0%3)*4,as:Float.self) }
   }
   let faces = mesh.facesData.withUnsafeBytes { raw in
    (0..<mesh.faceCount*3).map { raw.loadUnaligned(fromByteOffset:$0*4,as:UInt32.self) }
   }
   let stride = Int(SCPointCloud.pointStride()), position = Int(SCPointCloud.positionOffset()), normal = Int(SCPointCloud.normalOffset())
   guard (24...128).contains(stride), position >= 0, normal >= 0,
         position+12 <= stride, normal+12 <= stride, cloud.pointsData.count == count*stride else { throw Failure.invalid("Fixture support buffer layout changed") }
   let positions = cloud.pointsData.withUnsafeBytes { raw in
    (0..<count*3).map { raw.loadUnaligned(fromByteOffset:($0/3)*stride+position+($0%3)*4,as:Float.self) }
   }
   let normals = cloud.pointsData.withUnsafeBytes { raw in
    (0..<count*3).map { raw.loadUnaligned(fromByteOffset:($0/3)*stride+normal+($0%3)*4,as:Float.self) }
   }
   let loaded = PersonalHrtfMeshCheck.Loaded(surface:try PersonalHrtfReviewGeometry(vertices:vertices,triangles:faces),support:positions,normals:normals)
   let measured = try PersonalHrtfMeshCheck.measurement(loaded,cancelled:{ false })
   reviewFaces = measured.surface.triangles.count/3
   guard reviewFaces > 1000, reviewFaces <= mesh.faceCount, measured.distance <= 0.0015,
         Swift.stride(from:2,to:measured.surface.vertices.count,by:3).allSatisfy({ measured.surface.vertices[$0] >= -0.0015 }) else {
    throw Failure.invalid("No-rear Poisson measurement did not retain supported front surface")
   }
   var acousticRejected = false
   do { _ = try PersonalHrtfMeshCheck.screen(PersonalHrtfMeshCheck.Loaded(surface:measured.surface,support:loaded.support,normals:loaded.normals),cancelled:{ false }) }
   catch { acousticRejected = true }
   guard acousticRejected else { throw Failure.invalid("Partial surface passed acoustic closed-mesh screen") }
  }
  guard mesh.texCoordData.isEmpty else { throw Failure.invalid("Absent UVs fabricated attribute bytes") }
  print("SDA testing absent optional attributes")
  let minimal = folder.appendingPathComponent("no-optional-attributes.ply")
  let minimalHeader = "ply\nformat ascii 1.0\nelement vertex 4\nproperty float x\nproperty float y\nproperty float z\nelement face 4\nproperty list uchar int vertex_indices\nend_header\n"
  try (minimalHeader+"0 0 0\n1 0 0\n0 1 0\n0 0 1\n3 0 2 1\n3 0 1 3\n3 0 3 2\n3 1 2 3\n").write(to:minimal,atomically:true,encoding:.ascii)
  guard let sparse = SCMesh(plyPath:minimal.path,jpegPath:""), sparse.vertexCount == 4, sparse.faceCount == 4,
        sparse.normalData.isEmpty, sparse.texCoordData.isEmpty else { throw Failure.invalid("Absent optional mesh attributes were read out of bounds") }
  guard SCMesh(plyPath:folder.appendingPathComponent("missing.ply").path,jpegPath:"") == nil else { throw Failure.invalid("Missing PLY was accepted") }
  print("SDA testing truncated PLY rejection")
  let truncated = folder.appendingPathComponent("truncated.ply")
  try minimalHeader.write(to:truncated,atomically:true,encoding:.ascii)
  guard SCMesh(plyPath:truncated.path,jpegPath:"") == nil else { throw Failure.invalid("Truncated PLY was accepted") }
  print("SDA testing non-triangle rejection")
  let badFace = folder.appendingPathComponent("two-index-face.ply")
  try (minimalHeader+"0 0 0\n1 0 0\n0 1 0\n0 0 1\n2 0 2\n3 0 1 3\n3 0 3 2\n3 1 2 3\n").write(to:badFace,atomically:true,encoding:.ascii)
  guard SCMesh(plyPath:badFace.path,jpegPath:"") == nil else { throw Failure.invalid("Non-triangle PLY was accepted") }
  return ["deviceMemoryPreflightTested":false,"noRearInput":partial,"partialMeasurementFaces":reviewFaces,"missingOptionalAttributesSafe":true,"malformedMeshRejected":true,"syntheticOnly":true,"cancelledOutputAbsent":true,"smallNormalsPreserved":true,"binaryMeshExport":true,"inputPoints":count,"outputBytes":size,"vertices":mesh.vertexCount,"faces":mesh.faceCount]
 }
 static func coverageResources() throws -> [String:Any] {
  _ = try PersonalHrtfCaptureCoverage() // actually load the compiled offline model
  print("SDA loading offline ear detector")
  guard let bundle = SCFusionBundle.fusionBundle(), let url = bundle.url(forResource:"SCEarTrackingModel",withExtension:"mlmodelc") else { throw Failure.invalid("Compiled ear detector absent") }
  let configuration = MLModelConfiguration(); configuration.computeUnits = .all
  let visionModel = try VNCoreMLModel(for:MLModel(contentsOf:url,configuration:configuration))
  var colorOptional: CVPixelBuffer?
  guard CVPixelBufferCreate(kCFAllocatorDefault,448,448,kCVPixelFormatType_32BGRA,nil,&colorOptional) == kCVReturnSuccess,
        let color = colorOptional else { throw Failure.invalid("Neutral inference buffer unavailable") }
  CVPixelBufferLockBaseAddress(color,[])
  memset(CVPixelBufferGetBaseAddress(color)!,0,CVPixelBufferGetDataSize(color))
  CVPixelBufferUnlockBaseAddress(color,[])
  let request = VNCoreMLRequest(model:visionModel); request.imageCropAndScaleOption = .scaleFill
  try VNImageRequestHandler(cvPixelBuffer:color,orientation:.up).perform([request])
  print("SDA offline ear detector inference completed on neutral input")
  var state = PersonalHrtfCaptureCoverage.State()
  let leftEarPoint = SIMD3<Float>(0.08,0,0), rightEarPoint = SIMD3<Float>(-0.08,0,0)
  state.observe(head:true,left:leftEarPoint,right:nil)
  guard state.head == 1, state.leftEar == 1, state.rightEar == 0 else { throw Failure.invalid("A single frame qualified unobserved regions") }
  state.observe(head:false,left:SIMD3<Float>(0.12,0,0),right:nil)
  guard state.leftEar == 1 else { throw Failure.invalid("Unstable ear observations accumulated") }
  for _ in 0..<3 { state.observe(head:true,left:leftEarPoint,right:nil) }
  guard state.head == 3, state.leftEar == 3, state.rightEar == 0 else { throw Failure.invalid("Left-ear evidence counted as both ears") }
  state.observe(head:false,left:SIMD3<Float>(0.15,0,0),right:rightEarPoint)
  guard state.leftEar == 3, state.rightEar == 1 else { throw Failure.invalid("Completed region was lost while scanning the other ear") }
  for _ in 0..<2 { state.observe(head:false,left:nil,right:rightEarPoint) }
  guard state.rightEar == 3 else { throw Failure.invalid("Stable right-ear evidence was lost") }
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
  return ["neutralVisionInferencePassed":true,"stableRegionEvidencePassed":true,"earModelLoaded":true,"invalidDepthRejected":true,"orientationDepthPassed":true,"realEarDetectionValidated":false]
 }
 static func main() {
  do { try run() }
  catch { print("SDA verification failure: \(error)"); exit(1) }
 }
 static func run() throws {
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
   print("SDA resource fixture: finalizing empty manager")
   model.finalize { lock.lock(); completed = true; lock.unlock() }
   let deadline = Date().addingTimeInterval(20)
   while true {
    lock.lock(); let done = completed; lock.unlock()
    if done { break }
    guard Date() < deadline else { throw Failure.invalid("Empty reconstruction failed to finalize") }
    RunLoop.current.run(until:Date().addingTimeInterval(0.02))
   }
   lock.lock(); completed = false; lock.unlock()
   print("SDA resource fixture: resetting manager")
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
  let trackingCode = testTrackingFusion()
  guard trackingCode == 0 else { throw Failure.invalid("Real tracking recovery regression failed: \(trackingCode)") }
  let roundTrip = try meshRoundTrip()
  let partialRoundTrip = try meshRoundTrip(partial:true)
  let coverage = try coverageResources()
  let result:[String:Any] = ["realMetalTrackingRecoveryPassed":true,"trackingLossSeconds":10,"blankFramesDidNotChangeMeasuredSurfels":true,"partialNoRearRoundTrip":partialRoundTrip,"captureCoverage":coverage,"poissonRoundTrip":roundTrip,"bundleLocated":true,"versionMetadataValid":true,"metalFunctions":functions,"reconstructionInitialized":initialized,"atomicPointCountSnapshotValid":initialized,"resetCompletionPassed":initialized,"scanBudgetConfiguredAndClamped":initialized,"metalGPUAvailable":initialized,"physicalScanTested":false,"earAccuracyValidated":false]
  print(String(data:try JSONSerialization.data(withJSONObject:result,options:[.sortedKeys]),encoding:.utf8)!)
 }
}
