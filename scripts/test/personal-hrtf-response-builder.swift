import Foundation
import CryptoKit
import zlib

@main struct ResponseBuilderTest {
 static func require(_ yes: Bool, _ text: String) throws { if !yes { throw NSError(domain:text,code:1) } }
 static func inflate(_ input: Data) throws -> Data {
  var stream = z_stream()
  try require(inflateInit2_(&stream,31,ZLIB_VERSION,Int32(MemoryLayout<z_stream>.size)) == Z_OK,"inflate init")
  defer { inflateEnd(&stream) }
  var output = Data(), buffer = [UInt8](repeating:0,count:32768)
  try input.withUnsafeBytes { bytes in
   stream.next_in = UnsafeMutablePointer<Bytef>(mutating:bytes.bindMemory(to:UInt8.self).baseAddress!); stream.avail_in = uInt(input.count)
   while true {
    let status = buffer.withUnsafeMutableBufferPointer { b -> Int32 in stream.next_out = b.baseAddress; stream.avail_out = uInt(b.count); return zlib.inflate(&stream,Z_NO_FLUSH) }
    output.append(contentsOf:buffer.prefix(buffer.count-Int(stream.avail_out)))
    if status == Z_STREAM_END { break }; try require(status == Z_OK && output.count <= 24*1024*1024,"inflate")
   }
  }
  return output
 }
 static func main() throws {
  let args = CommandLine.arguments
  try require(args.count == 4,"Pass generated pressure project, FFT fixtures, and a fresh temporary directory")
  let source = URL(fileURLWithPath:args[1]), fixture = URL(fileURLWithPath:args[2]), root = URL(fileURLWithPath:args[3])
  try FileManager.default.createDirectory(at:root,withIntermediateDirectories:true)
  let bins = 256, count = 34
  for ear in 1...2 {
   let base = root.appendingPathComponent("NumCalc/source_\(ear)")
   try FileManager.default.createDirectory(at:base,withIntermediateDirectories:true)
   try FileManager.default.copyItem(at:source.appendingPathComponent("NumCalc/source_\(ear)/NC.inp"),to:base.appendingPathComponent("NC.inp"))
   for k in 1...bins {
    let original = try String(contentsOf:source.appendingPathComponent("NumCalc/source_\(ear)/be.out/be.\(k)/pEvalGrid"),encoding:.utf8)
    var values: [Int:String] = [:]
    for row in original.split(separator:"\n").dropFirst(3) { let parts = row.split(whereSeparator: { $0.isWhitespace }); values[Int(parts[0])!-300000] = "\(parts[1]) \(parts[2])" }
    var rows = "Mesh2HRTF 1.0.0\n1\n1 \(count)\n"
    for d in 0..<count { rows += "\(400000+d) \(values[d%3]!)\n" }
    let path = base.appendingPathComponent("be.out/be.\(k)/pEvalGrid")
    try FileManager.default.createDirectory(at:path.deletingLastPathComponent(),withIntermediateDirectories:true)
    try rows.write(to:path,atomically:true,encoding:.utf8)
   }
  }
  var directions: [PersonalHrtfResponseBuilder.Direction] = []
  for d in 0..<32 { directions.append(.init(nodeID:Int32(400000+d),azimuth:Double(d)*11.25,elevation:0)) }
  directions.append(.init(nodeID:400032,azimuth:0,elevation:-90)); directions.append(.init(nodeID:400033,azimuth:0,elevation:90))
  let spec = PersonalHrtfResponseBuilder.Specification(directions:directions,bins:bins,sourceAreas:[2.3e-5,3.1e-5],referenceRadius:1.2,commonShift:32)
  let builder = PersonalHrtfResponseBuilder(), archive = try builder.build(project:root,specification:spec,numericalBudget:64*1024*1024)
  let archiveBytes = try Data(contentsOf:archive)
  let json = try inflate(archiveBytes)
  let payload = try JSONSerialization.jsonObject(with:json) as! [String:Any]
  let manifest = payload["manifest"] as! [String:Any], files = payload["files"] as! [[String:Any]], positions = manifest["positions"] as! [[String:Any]]
  try require(payload["name"] as? String == "个人头耳 · 本机计算" && payload["assessment"] is NSNull,"Archive library metadata")
  try require(payload["format"] as? String == "sda-phrtf" && files.count == count && positions.count == count,"Archive structure")
  let processing = manifest["processing"] as! [String:Any]
  try require(processing["normalization"] as? Bool == false && processing["preserveMeasurements"] as? Bool == true && processing["commonDelaySamples"] as? Int == 32,"Processing must preserve ear response")
  let expected = try Data(contentsOf:fixture.appendingPathComponent("fft-expected-f32.bin")), length = bins*4*4
  for d in 0..<count {
   let bytes = Data(base64Encoded:files[d]["data"] as! String)!
   try require(bytes.count == length && positions[d]["dry"] as? String == positions[d]["wet"] as? String,"Planar ears and zero room residual")
   try require(SHA256.hash(data:bytes).map { String(format:"%02x",$0) }.joined() == files[d]["sha256"] as? String,"Response digest")
   for k in stride(from:0,to:length,by:4) {
    let actualWord = bytes[k..<(k+4)].enumerated().reduce(UInt32(0)) { $0 | UInt32($1.element) << (8*$1.offset) }
    let offset = (d%3)*length+k
    let referenceWord = expected[offset..<(offset+4)].enumerated().reduce(UInt32(0)) { $0 | UInt32($1.element) << (8*$1.offset) }
    try require(abs(Float(bitPattern:actualWord)-Float(bitPattern:referenceWord)) < 2e-6,"Archive sample differs from upstream")
   }
  }
  // A second run must not overwrite a completed immutable output.
  var overwriteRejected = false
  do { _ = try builder.build(project:root,specification:spec,numericalBudget:64*1024*1024) } catch { overwriteRejected = true }
  let afterRejectedOverwrite = try Data(contentsOf:archive)
  try require(overwriteRejected && afterRejectedOverwrite == archiveBytes,"Output overwrite")
  try FileManager.default.removeItem(at:archive)
  let cancelled = PersonalHrtfResponseBuilder(); cancelled.cancel()
  var cancelRejected = false
  do { _ = try cancelled.build(project:root,specification:spec,numericalBudget:64*1024*1024) } catch { cancelRejected = true }
  try require(cancelRejected && !FileManager.default.fileExists(atPath:archive.path),"Cancelled build wrote archive")
  let broken = root.appendingPathComponent("NumCalc/source_2/be.out/be.256/pEvalGrid")
  try FileManager.default.removeItem(at:broken)
  var partialRejected = false
  do { _ = try PersonalHrtfResponseBuilder().build(project:root,specification:spec,numericalBudget:64*1024*1024) } catch { partialRejected = true }
  try require(partialRejected && !FileManager.default.fileExists(atPath:archive.path),"Partial solve wrote archive")
  print("{\"passed\":true,\"upstreamImpulseMatch\":true,\"directions\":34,\"archiveHashChecked\":true,\"overwriteRejected\":true,\"cancellation\":true,\"partialSolveRejected\":true,\"physicalScanTested\":false}")
 }
}
