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
  struct Frame: Decodable { let yaw:Int, front:Float, left:Bool, right:Bool, pose:[Float], invalidDepth:Bool }
  let folder = URL(fileURLWithPath:CommandLine.arguments[1],isDirectory:true)
  let trace = try JSONDecoder().decode([Frame].self,from:Data(contentsOf:folder.appendingPathComponent("faceframe-sweep.json")))
  guard trace.count >= 100 else { throw Failure.invalid("No registered FaceFrame sweep") }
  var optional:CVPixelBuffer?
  guard CVPixelBufferCreate(kCFAllocatorDefault,448,448,kCVPixelFormatType_32BGRA,[kCVPixelBufferIOSurfacePropertiesKey:[:]] as CFDictionary,&optional) == kCVReturnSuccess, let color = optional else { throw Failure.invalid("No test image buffer") }
  let context = CIContext(options:[.useSoftwareRenderer:false])
  func render(_ name:String) throws {
   guard let image = CIImage(contentsOf:folder.appendingPathComponent(name),options:[.applyOrientationProperty:true]) else { throw Failure.invalid(name) }
   let scaled = image.transformed(by:CGAffineTransform(translationX:-image.extent.minX,y:-image.extent.minY)).transformed(by:CGAffineTransform(scaleX:448/image.extent.width,y:448/image.extent.height))
   context.render(scaled,to:color)
  }
  for prefix in ["asian","caucasian"] {
   let coverage = try PersonalHrtfCaptureCoverage()
   var photo = "", blankLeft = false, missingRight = false
   for (index,frame) in trace.enumerated() {
    let name = index < 28 ? "frontal-control.png" : prefix+(frame.yaw >= 0 ? "-left_medium.jpg" : "-right_medium.jpg")
    if name != photo { try render(name); photo = name }
    var pose:simd_float4x4?
    if frame.pose.count == 16 {
     func column(_ index:Int) -> SIMD4<Float> { SIMD4(frame.pose[index],frame.pose[index+1],frame.pose[index+2],frame.pose[index+3]) }
     pose = simd_float4x4(columns:(column(0),column(4),column(8),column(12)))
    }
    var status = ScanStatus(); status.coverage.front = frame.front
    let before = coverage.guide.stage
    if frame.left && !blankLeft {
     context.render(CIImage(color:.black).cropped(to:CGRect(x:0,y:0,width:448,height:448)),to:color)
     coverage.observe(status:status,leftAdvanced:true,rightAdvanced:false,pixels:color,pose:pose)
     guard coverage.guide.stage == before else { throw Failure.invalid("Side pose alone credited an invisible ear") }
     blankLeft = true; photo = ""; continue
    }
    if frame.right && !missingRight {
     coverage.observe(status:status,leftAdvanced:false,rightAdvanced:true,pixels:nil,pose:pose)
     guard coverage.guide.stage == before else { throw Failure.invalid("Reused stale image on opposite side") }
     missingRight = true; continue
    }
    coverage.observe(status:status,leftAdvanced:frame.left,rightAdvanced:frame.right,pixels:color,pose:pose)
    if before == .leftEar, before != coverage.guide.stage, frame.yaw < 40 { throw Failure.invalid("Frontal view credited left ear") }
    if before == .rightEar, coverage.guide.complete, frame.yaw > -40 { throw Failure.invalid("Wrong side credited right ear") }
    if frame.invalidDepth && before != coverage.guide.stage { throw Failure.invalid("Empty depth changed coverage") }
   }
   guard blankLeft, missingRight, coverage.guide.complete else { throw Failure.invalid("FaceFrame sweep did not finish both ears: \(prefix)") }
  }
  func samples(_ name:String) throws -> [Float] {
   let bytes = try Data(contentsOf:folder.appendingPathComponent(name))
   guard bytes.count.isMultiple(of:12), (120000...6000000).contains(bytes.count) else { throw Failure.invalid("Invalid replay support file") }
   return bytes.withUnsafeBytes { raw in (0..<bytes.count/4).map { raw.loadUnaligned(fromByteOffset:4*$0,as:Float.self) } }
  }
  let positions = try samples("replay-points.f32"), normals = try samples("replay-normals.f32")
  let measured = try PersonalHrtfMeshCheck.poissonMeasurement(positions:positions,normals:normals,budget:512*1024*1024,cancelled:{false},operationChanged:{_ in})
  guard measured.surface.triangles.count > 3000, measured.distance <= 0.0015 else { throw Failure.invalid("FaceFrame scan did not reach strict measured review") }
  return ["earModelLoaded":true,"frontalPositiveIgnored":true,"bothAnatomicalSidesCompleted":true,"blankEarAndMissingImageRejected":true,"positiveEarImageInference":try positiveEarResources(),"measuredReviewFaces":measured.surface.triangles.count/3,"maximumSupportDistance":measured.distance,"physicalEarCaptureValidated":false]
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
