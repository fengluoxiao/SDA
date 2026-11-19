import Foundation
import CryptoKit
import zlib

// Converts a COMPLETED, qualified local acoustic project. This is not a mesh
// qualifier or ear detector, and never selects/installs its output by itself.
final class PersonalHrtfResponseBuilder {
 struct Direction {
  let nodeID: Int32
  let azimuth: Double // SOFA: positive left
  let elevation: Double // positive up
 }
 struct Specification {
  let directions: [Direction]
  let bins: Int
  let sourceAreas: [Double] // left, right; measured acoustic source patches
  let referenceRadius: Double
  let commonShift: Int
 }
 enum Failure: LocalizedError {
  case message(String)
  var errorDescription: String? { if case .message(let text) = self { return text }; return nil }
 }
 private let lock = NSLock()
 private var running = false, cancelled = false
 private var handle: OpaquePointer?
 private func require(_ test: Bool, _ text: String) throws { if !test { throw Failure.message(text) } }
 func cancel() {
  lock.lock(); defer { lock.unlock() }; cancelled = true
  if let handle { sda_hrtf_solver_cancel(handle) }
 }
 private func checkpoint() throws {
  lock.lock(); let stopped = cancelled; lock.unlock()
  try require(!stopped,"已取消个人响应构建，未替换现有档案")
  let available = sda_hrtf_available_memory()
  #if os(iOS) && !targetEnvironment(simulator)
  try require(available > 0,"iPhone 当前没有可用的响应处理内存余量")
  #endif
  try require(available == 0 || available >= 128 * 1024 * 1024,"iPhone 当前可用内存不足以继续构建个人响应")
 }
 private func checked(_ result: SdaHrtfSolverResult) throws {
  if result.code == 0 { return }
  var reply = result
  let detail = withUnsafePointer(to:&reply.message) { pointer in
   pointer.withMemoryRebound(to:CChar.self,capacity:512) { String(cString:$0) }
  }
  throw Failure.message("个人响应构建未完成：\(detail)")
 }
 // Synchronous on the owner's worker. Owner retains builder until return and
 // keeps the private project alive; it deletes all solver outputs afterward.
 func build(project: URL, specification s: Specification, numericalBudget: UInt64) throws -> URL {
  lock.lock()
  guard !running, !cancelled else { lock.unlock(); throw Failure.message("个人响应任务已取消或正在进行") }
  running = true; lock.unlock()
  defer {
   lock.lock(); let job = handle; handle = nil; running = false; lock.unlock()
   if let job { sda_hrtf_solver_destroy(job) }
  }
  try checkpoint()
  let count = s.directions.count
  try require((32...2048).contains(count) && (16...2048).contains(s.bins) && s.bins & (s.bins-1) == 0,"个人响应方向数或频率数量无效")
  try require(s.sourceAreas.count == 2 && s.sourceAreas.allSatisfy { $0.isFinite && (1e-9...0.001).contains($0) },"左右耳声源面面积无效")
  try require(s.referenceRadius.isFinite && (0.25...10).contains(s.referenceRadius) && (0..<2*s.bins).contains(s.commonShift),"响应参考距离或共同延迟无效")
  let bytes = count * 4 * s.bins * MemoryLayout<Double>.size
  let assetBytes = count * 4 * s.bins * MemoryLayout<Float>.size
  try require(bytes <= 64 * 1024 * 1024 && assetBytes <= 16 * 1024 * 1024 && numericalBudget >= UInt64(bytes + 4*1024*1024) && numericalBudget <= 768*1024*1024,"个人响应超过本机内存或档案尺寸上限")
  var ids = Set<Int32>(), directions = Set<String>(), sectors = Set<Int>(), low = false, high = false
  let locale = Locale(identifier:"en_US_POSIX")
  var names: [String] = [], positions: [[String:Any]] = []
  for d in s.directions {
   try require(d.nodeID >= 0 && ids.insert(d.nodeID).inserted && d.azimuth.isFinite && d.elevation.isFinite && abs(d.azimuth) <= 360 && abs(d.elevation) <= 90,"个人响应方向或节点编号无效")
   let az = (d.azimuth.truncatingRemainder(dividingBy:360) + 360).truncatingRemainder(dividingBy:360)
   // Names follow the existing .phrtf importer grammar. Stable integer indices
   // avoid collisions for distinct non-integer azimuth/elevation positions.
   let name = "az\(names.count)_el0_dry.f32"
   try require(directions.insert(String(format:"%.4f/%.4f",locale:locale,az,d.elevation)).inserted,"个人响应包含重复方向")
   if abs(d.elevation) <= 30 { sectors.insert(Int((az+22.5).truncatingRemainder(dividingBy:360)/45)) }
   low = low || d.elevation <= -60; high = high || d.elevation >= 60
   names.append(name); positions.append(["azimuth":d.azimuth,"elevation":d.elevation,"dry":name,"wet":name])
  }
  try require(sectors.count == 8 && low && high,"个人响应需要完整前后、上方和下方覆盖")
  let available = sda_hrtf_available_memory()
  // Account for pressure, response payload/base64, JSON and compression buffers,
  // separately from numerical arrays; reserve room for playback/application.
  let working = UInt64(bytes + assetBytes * 6 + 128*1024*1024)
  try require(available == 0 || available >= working,"iPhone 当前可用内存不足以构建完整响应档案")
  guard let job = project.path.withCString({ sda_hrtf_solver_create($0,numericalBudget) }) else { throw Failure.message("无法创建本机响应读取任务") }
  lock.lock(); handle = job; if cancelled { sda_hrtf_solver_cancel(job) }; lock.unlock()
  var digest = SHA256()
  let identity = try JSONSerialization.data(withJSONObject:["positions":positions,"bins":s.bins,"sourceAreas":s.sourceAreas,"radius":s.referenceRadius,"commonShift":s.commonShift],options:[.sortedKeys])
  digest.update(data:identity)
  // The pressure bank is released before materializing the archive JSON.
  let files: [[String:Any]] = try {
   var pressure = [Double](repeating:0,count:count*4*s.bins)
   var nodeIDs = s.directions.map { $0.nodeID }
   try pressure.withUnsafeMutableBufferPointer { bank in
    try nodeIDs.withUnsafeMutableBufferPointer { nodes in
     try checked(sda_hrtf_read_pressure(job,nodes.baseAddress,count,s.bins,bank.baseAddress,bank.count))
    }
   }
   var files: [[String:Any]] = []
   var samples = [Float](repeating:0,count:4*s.bins)
   var areas = s.sourceAreas
   for d in 0..<count {
    try checkpoint()
    try pressure.withUnsafeBufferPointer { bank in
     try areas.withUnsafeMutableBufferPointer { area in
      try samples.withUnsafeMutableBufferPointer { output in
       try checked(sda_hrtf_make_hrir(job,bank.baseAddress!.advanced(by:d*4*s.bins),4*s.bins,s.bins,area.baseAddress,s.referenceRadius,Int32(s.commonShift),output.baseAddress,output.count))
      }
     }
    }
    var energy = [0.0,0.0], data = Data(capacity:samples.count*4)
    for i in samples.indices {
     let x = samples[i]; try require(x.isFinite && abs(x) <= 100,"个人响应采样无效")
     energy[i < 2*s.bins ? 0 : 1] += Double(x)*Double(x)
     var word = x.bitPattern.littleEndian; withUnsafeBytes(of:&word) { data.append(contentsOf:$0) }
    }
    try require(energy.allSatisfy { $0 > 1e-16 },"个人响应不能包含静音耳朵")
    digest.update(data:data)
    files.append(["name":names[d],"data":data.base64EncodedString(),"sha256":Self.hex(SHA256.hash(data:data))])
   }
   return files
  }()
  try checkpoint()
  let id = "personal-" + Self.hex(digest.finalize())
  let manifest: [String:Any] = ["schemaVersion":2,"calibrationVersion":0,"personalSofaVersion":1,"sampleRate":48000,"completeSubject":true,"subjectId":id,
   "source":["name":"个人头耳 · 本机计算","method":"front-truedepth-local-mesh2hrtf-experimental","convention":"SimpleFreeFieldHRIR","cameraPolicy":"front-truedepth-only","mesh2hrtfVersion":"1.3.0","physicalAccuracyValidated":false],
   "azimuthConvention":"positive left, degrees; elevation positive up",
   "processing":["calibrated":false,"preserveMeasurements":true,"commonDelaySamples":s.commonShift,"delayApplied":true,"normalization":false,"roomResponse":false],"positions":positions]
  let json = try JSONSerialization.data(withJSONObject:["format":"sda-phrtf","version":1,"name":"个人头耳 · 本机计算","assessment":NSNull(),"manifest":manifest,"files":files],options:[.sortedKeys])
  try require(json.count <= 24*1024*1024,"个人响应档案展开尺寸超出上限")
  let archive = try gzip(json)
  let target = project.appendingPathComponent("local-personal.phrtf")
  try require(!FileManager.default.fileExists(atPath:target.path),"本机响应输出已存在，请使用新的独立任务目录")
  var complete = false
  defer { if !complete { try? FileManager.default.removeItem(at:target) } }
  try checkpoint(); try archive.write(to:target,options:.atomic); try checkpoint()
  complete = true; return target
 }
 private static func hex<D: Sequence>(_ bytes: D) -> String where D.Element == UInt8 { bytes.map { String(format:"%02x",$0) }.joined() }
 private func gzip(_ input: Data) throws -> Data {
  var stream = z_stream()
  try require(deflateInit2_(&stream,Z_DEFAULT_COMPRESSION,Z_DEFLATED,15+16,8,Z_DEFAULT_STRATEGY,ZLIB_VERSION,Int32(MemoryLayout<z_stream>.size)) == Z_OK,"无法创建个人响应档案")
  defer { deflateEnd(&stream) }
  var output = Data(), buffer = [UInt8](repeating:0,count:32768)
  try input.withUnsafeBytes { bytes in
   stream.next_in = UnsafeMutablePointer<Bytef>(mutating:bytes.bindMemory(to:UInt8.self).baseAddress!)
   stream.avail_in = uInt(input.count)
   while true {
    try checkpoint()
    let result = buffer.withUnsafeMutableBufferPointer { block -> Int32 in
     stream.next_out = block.baseAddress; stream.avail_out = uInt(block.count)
     return deflate(&stream,Z_FINISH)
    }
    output.append(contentsOf:buffer.prefix(buffer.count-Int(stream.avail_out)))
    try require(output.count <= 16*1024*1024,"个人响应压缩档案超过 16 MiB")
    if result == Z_STREAM_END { break }
    try require(result == Z_OK,"个人响应压缩失败")
   }
  }
  return output
 }
}
