import Foundation
import Metal
import CoreVideo
import CoreImage
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
 static func positiveEarResources() throws -> [[String:Any]] {
  guard CommandLine.arguments.count == 2 else { throw Failure.invalid("Positive ear image folder required") }
  let folder = URL(fileURLWithPath:CommandLine.arguments[1],isDirectory:true)
  guard let bundle = SCFusionBundle.fusionBundle(), let url = bundle.url(forResource:"SCEarTrackingModel",withExtension:"mlmodelc") else { throw Failure.invalid("Ear model absent") }
  let config = MLModelConfiguration(); config.computeUnits = .cpuAndGPU
  let model = try VNCoreMLModel(for:MLModel(contentsOf:url,configuration:config))
  var results = [[String:Any]]()
  for name in ["caucasian-left_medium.jpg","caucasian-right_medium.jpg","asian-left_medium.jpg","asian-right_medium.jpg"] {
   guard let image = CIImage(contentsOf:folder.appendingPathComponent(name),options:[.applyOrientationProperty:true]) else { throw Failure.invalid("Positive ear fixture could not load: \(name)") }
   let orientations: [(CGImagePropertyOrientation,CGImagePropertyOrientation)] = [(.up,.up),(.down,.down),(.right,.left),(.left,.right)]
   for pair in orientations {
    let request = VNCoreMLRequest(model:model); request.imageCropAndScaleOption = .scaleFill
    try VNImageRequestHandler(ciImage:image.oriented(pair.0),orientation:pair.1).perform([request])
    let ears = (request.results as? [VNRecognizedObjectObservation] ?? []).filter { PersonalHrtfCaptureCoverage.isEar($0) }
    guard let ear = ears.max(by: { $0.confidence < $1.confidence }) else { throw Failure.invalid("Real ear not detected: \(name), orientation=\(pair.1.rawValue), results=\(String(describing:request.results))") }
    results.append(["image":name,"orientation":pair.1.rawValue,"confidence":ear.confidence,"label":ear.labels.first?.identifier ?? "", "acceptedByEarFilter":true])
   }
  }
  return results
 }
 static func coverageResources() throws -> [String:Any] {
  _ = try PersonalHrtfCaptureCoverage() // actually load the compiled offline model
  print("SDA loading offline ear detector")
  guard let bundle = SCFusionBundle.fusionBundle(), let url = bundle.url(forResource:"SCEarTrackingModel",withExtension:"mlmodelc") else { throw Failure.invalid("Compiled ear detector absent") }
  let configuration = MLModelConfiguration(); configuration.computeUnits = .cpuAndGPU
  let visionModel = try VNCoreMLModel(for:MLModel(contentsOf:url,configuration:configuration))
  var colorOptional: CVPixelBuffer?
  guard CVPixelBufferCreate(kCFAllocatorDefault,448,448,kCVPixelFormatType_32BGRA,nil,&colorOptional) == kCVReturnSuccess,
        let color = colorOptional else { throw Failure.invalid("Neutral inference buffer unavailable") }
  CVPixelBufferLockBaseAddress(color,[])
  memset(CVPixelBufferGetBaseAddress(color)!,0,CVPixelBufferGetDataSize(color))
  CVPixelBufferUnlockBaseAddress(color,[])
  let request = VNCoreMLRequest(model:visionModel); request.imageCropAndScaleOption = .scaleFill
  try VNImageRequestHandler(cvPixelBuffer:color,orientation:.up).perform([request])
  guard ((request.results as? [VNRecognizedObjectObservation]) ?? []).allSatisfy({ !PersonalHrtfCaptureCoverage.isEar($0) }) else { throw Failure.invalid("Neutral image produced a qualifying ear") }
  print("SDA offline ear detector inference completed on neutral input")
  var state = PersonalHeadCaptureGuide()
  let leftEarPoint = SIMD3<Float>(0.08,0,0), rightEarPoint = SIMD3<Float>(-0.08,0,0)
  var clock = 1.0
  func observe(_ head:Bool = false,_ left:SIMD3<Float>? = nil,_ right:SIMD3<Float>? = nil,_ camera:SIMD3<Float> = .zero) {
   state.observe(head:head,left:left,right:right,camera:camera,time:clock); clock += 0.31
  }
  for _ in 0..<10 { observe(false,leftEarPoint,rightEarPoint) }
  guard state.stage == .face, state.head == 0, state.leftEar == 0, state.rightEar == 0 else { throw Failure.invalid("Ears skipped the required face setup") }
  for _ in 0..<4 { observe(true,leftEarPoint,rightEarPoint) }
  guard state.stage == .leftEar, state.head == 4, state.leftEar == 0 else { throw Failure.invalid("Face did not advance alone") }
  for _ in 0..<10 { observe(true,nil,rightEarPoint) }
  guard state.stage == .leftEar, state.leftEar == 0, state.rightEar == 0 else { throw Failure.invalid("Wrong ear advanced the current step") }
  for _ in 0..<10 { observe(false,leftEarPoint) }
  guard state.stage == .leftEar, !state.complete else { throw Failure.invalid("One frozen ear view qualified multi-view capture") }
  observe(false,SIMD3<Float>(0.15,0,0))
  guard state.leftEar == 1 else { throw Failure.invalid("Unstable ear evidence accumulated") }
  for i in 0..<7 { observe(false,leftEarPoint,nil,SIMD3<Float>(Float(i)*0.002,0,0)) }
  guard state.stage == .rightEar, state.leftEar == 6 else { throw Failure.invalid("Measured left-ear views did not advance") }
  for _ in 0..<20 { observe(true,leftEarPoint,nil) }
  guard state.stage == .rightEar, state.head == 4, state.leftEar == 6, state.rightEar == 0 else { throw Failure.invalid("Journey through the face reset a completed step") }
  observe(false,nil,rightEarPoint)
  state.observe(head:false,left:nil,right:rightEarPoint,camera:SIMD3<Float>(0.1,0,0),time:clock-0.31)
  guard state.rightEar == 1 else { throw Failure.invalid("Duplicate callback counted as evidence") }
  for _ in 0..<10 { observe() }
  guard state.rightEar == 0 else { throw Failure.invalid("Occlusion did not clear incomplete step evidence") }
  for i in 0..<7 { observe(false,nil,rightEarPoint,SIMD3<Float>(Float(i)*0.002,0,0)) }
  guard state.complete, state.head == 4, state.leftEar == 6, state.rightEar == 6 else { throw Failure.invalid("Valid sequential capture failed to complete") }
  for _ in 0..<2000 { observe() }
  guard state.complete else { throw Failure.invalid("Completed capture was lost") }
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
  // Exercise the same foreground buffer path used before native ICP. Do not
  // accept torso/background depth or mutate borrowed AVFoundation buffers.
  var largeOptional: CVPixelBuffer?
  guard CVPixelBufferCreate(kCFAllocatorDefault,128,128,kCVPixelFormatType_DepthFloat32,nil,&largeOptional) == kCVReturnSuccess, let large = largeOptional else { throw Failure.invalid("Mask test allocation failed") }
  CVPixelBufferLockBaseAddress(large,[])
  let source = CVPixelBufferGetBaseAddress(large)!, sourceStride = CVPixelBufferGetBytesPerRow(large)
  for y in 0..<128 { for x in 0..<128 { source.storeBytes(of:Float(0.3),toByteOffset:y*sourceStride+x*4,as:Float.self) } }
  CVPixelBufferUnlockBaseAddress(large,[])
  func validCount(_ b:CVPixelBuffer) -> Int {
   CVPixelBufferLockBaseAddress(b,.readOnly); defer { CVPixelBufferUnlockBaseAddress(b,.readOnly) }
   let p = CVPixelBufferGetBaseAddress(b)!, stride = CVPixelBufferGetBytesPerRow(b)
   var count = 0
   for y in 0..<128 { for x in 0..<128 { if p.load(fromByteOffset:y*stride+x*4,as:Float.self).isFinite { count += 1 } } }
   return count
  }
  // Reproduce detector boxes with missing hair/background depth: the old
  // full-box requirement rejected these even with a measured ear centre.
  let earBox = CGRect(x:0.2,y:0.2,width:0.6,height:0.6)
  for orientation in [CGImagePropertyOrientation.up,.down,.left,.right] {
   CVPixelBufferLockBaseAddress(large,[])
   for y in 0..<128 { for x in 0..<128 {
    let valid = x >= 42 && x <= 85 && y >= 42 && y <= 85
    source.storeBytes(of:valid ? Float(0.3) : Float.nan,toByteOffset:y*sourceStride+x*4,as:Float.self)
   } }
   CVPixelBufferUnlockBaseAddress(large,[])
   guard PersonalHrtfCaptureCoverage.depthMedian(large,in:earBox,orientation:orientation) == nil,
         PersonalHrtfCaptureCoverage.earDepthMedian(large,in:earBox,orientation:orientation) == 0.3 else { throw Failure.invalid("Measured ear centre still rejected by surrounding missing depth") }
  }
  CVPixelBufferLockBaseAddress(large,[])
  for y in 0..<128 { for x in 0..<128 { source.storeBytes(of:Float(x)*0.004+0.15,toByteOffset:y*sourceStride+x*4,as:Float.self) } }
  CVPixelBufferUnlockBaseAddress(large,[])
  guard PersonalHrtfCaptureCoverage.earDepthMedian(large,in:earBox,orientation:.up) == nil else { throw Failure.invalid("Discontinuous ear depth qualified") }
  for bad in [Float.nan,0,0.1,0.8] {
   fill(bad)
   guard PersonalHrtfCaptureCoverage.earDepthMedian(buffer,in:box,orientation:.up) == nil else { throw Failure.invalid("Missing/background ear depth qualified") }
  }
  CVPixelBufferLockBaseAddress(large,[])
  for y in 0..<128 { for x in 0..<128 { source.storeBytes(of:Float(0.3),toByteOffset:y*sourceStride+x*4,as:Float.self) } }
  CVPixelBufferUnlockBaseAddress(large,[])
  for orientation in [CGImagePropertyOrientation.up,.down,.left,.right] {
   let seed = PersonalHrtfCaptureCoverage.Seed(box:CGRect(x:0.35,y:0.3,width:0.3,height:0.4),depth:0.3)
   guard let masked = PersonalHrtfCaptureCoverage.maskedDepth(large,seed:seed,orientation:orientation,region:nil), validCount(masked) >= 1000, validCount(masked) < 8000, validCount(large) == 128*128 else { throw Failure.invalid("Frontal seed retained background or modified borrowed depth") }
  }
  let foreground = PersonalHrtfCaptureCoverage.Region(center:SIMD3<Float>(0,0,-0.3),view:matrix_identity_float4x4,projection:projection)
  guard let masked = PersonalHrtfCaptureCoverage.maskedDepth(large,seed:nil,orientation:.up,region:foreground), validCount(masked) > 1000 else { throw Failure.invalid("Head-sized mask lost measured overlap") }
  let background = PersonalHrtfCaptureCoverage.Region(center:SIMD3<Float>(0,0,-0.6),view:matrix_identity_float4x4,projection:projection)
  guard PersonalHrtfCaptureCoverage.maskedDepth(large,seed:nil,orientation:.up,region:background) == nil else { throw Failure.invalid("Unrelated depth admitted to head model") }
  let initial = try PersonalHrtfCaptureCoverage(), semaphore = DispatchSemaphore(value:0)
  var neutralSeedRejected = false
  initial.prepareInitial(color:color,depth:large,orientation:.up) { seed in neutralSeedRejected = seed == nil; semaphore.signal() }
  guard semaphore.wait(timeout:.now()+10) == .success, neutralSeedRejected else { throw Failure.invalid("Neutral/background image bootstrapped reconstruction") }
  return ["neutralVisionInferencePassed":true,"stableRegionEvidencePassed":true,"completedCoverageSurvivesMissingDetections":true,"earModelLoaded":true,"invalidDepthRejected":true,"orientationDepthPassed":true,"sequentialTargetGatePassed":true,"wrongEarAndDuplicateRejected":true,"multiViewEarRequired":true,"foregroundMaskPassed":true,"backgroundSeedRejected":true,"partialEarDepthRegressionPassed":true,"positiveEarImageInference":try positiveEarResources(),"physicalEarCaptureValidated":false]
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
   guard !model.sdaIsAligning, !model.sdaNeedsTrackingRecovery else { throw Failure.invalid("New manager has stale alignment guidance") }
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
   guard !model.sdaIsAligning, !model.sdaNeedsTrackingRecovery else { throw Failure.invalid("New manager has stale alignment guidance") }
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
