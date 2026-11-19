import Foundation
import CryptoKit
import zlib

// A bounded, desktop-compatible archive. Imported IR samples are never equalized,
// symmetrized, peak-normalized or rewritten. The native loader validates again.
enum PersonalHrtfError: LocalizedError {
 case message(String)
 var errorDescription: String? { if case .message(let text) = self { return text }; return nil }
}
enum PersonalHrtfStore {
 private static let lock = NSRecursiveLock()
 private static func serialized<T>(_ operation: () throws -> T) rethrows -> T {
  lock.lock(); defer { lock.unlock() }; return try operation()
 }
 private static let launchCleanup: Void = {
  PersonalHeadScanController.discardAbandonedScans()
  guard let directory = try? root() else { return }
  let selected = UserDefaults.standard.string(forKey:key)
  for item in (try? FileManager.default.contentsOfDirectory(at:directory,includingPropertiesForKeys:nil)) ?? [] {
   if item.lastPathComponent != selected { try? FileManager.default.removeItem(at:item) }
  }
 }()
 static func prepareForLaunch() { serialized { _ = launchCleanup } }
 static let key = "sda.personalHrtf"
 static let maxArchive = 16 * 1024 * 1024
 static let maxJSON = 24 * 1024 * 1024
 static let maxSamples = 16 * 1024 * 1024
 static func require(_ ok: Bool, _ text: String) throws { if !ok { throw PersonalHrtfError.message(text) } }
 static func root() throws -> URL {
  let url = FileManager.default.urls(for:.applicationSupportDirectory,in:.userDomainMask)[0].appendingPathComponent("PersonalHrtf",isDirectory:true)
  try FileManager.default.createDirectory(at:url,withIntermediateDirectories:true)
  var values = URLResourceValues(); values.isExcludedFromBackup = true; var mutable = url
  try mutable.setResourceValues(values)
  return url
 }
 static func selectedPath() throws -> String? { try serialized {
  _ = launchCleanup
  guard let id = UserDefaults.standard.string(forKey:key) else { return nil }
  try require(id.range(of:"^personal-[a-f0-9]{64}$",options:.regularExpression) != nil,"个人档案 ID 无效，请恢复默认")
  let path = try root().appendingPathComponent(id).appendingPathComponent("hrtf-set.json")
  try require(FileManager.default.fileExists(atPath:path.path),"个人档案缺失，请重新导入或恢复默认")
  return path.path
 } }
 static func status() -> [String:Any] { serialized {
  do {
   guard let path = try selectedPath() else { return ["selected":false,"name":"SDA Spatial · 默认档案"] }
   let m = try JSONSerialization.jsonObject(with:Data(contentsOf:URL(fileURLWithPath:path))) as? [String:Any]
   let source = m?["source"] as? [String:Any]
   return ["selected":true,"name":source?["name"] as? String ?? "个人 HRTF","directions":(m?["positions"] as? [Any])?.count ?? 0]
  } catch { return ["selected":true,"name":"个人档案不可用","error":error.localizedDescription] }
 } }
 static func useDefault() { serialized { _ = launchCleanup; UserDefaults.standard.removeObject(forKey:key) } }
 static func remove() throws {
  // Active native references may still be loading an immutable file. Unselect
  // now, reclaim at the next process launch, before any player reads it.
  useDefault()
 }
 static func inflateArchive(_ data: Data) throws -> Data {
  try require(!data.isEmpty && data.count <= maxArchive,"档案需小于 16 MiB")
  var stream = z_stream()
  try require(inflateInit2_(&stream, 15 + 16, ZLIB_VERSION, Int32(MemoryLayout<z_stream>.size)) == Z_OK,"无法初始化档案读取")
  defer { inflateEnd(&stream) }
  var output = Data(); let size = 32768
  let buffer = UnsafeMutablePointer<UInt8>.allocate(capacity:size); defer { buffer.deallocate() }
  try data.withUnsafeBytes { raw in
   stream.next_in = UnsafeMutablePointer<Bytef>(mutating:raw.bindMemory(to:UInt8.self).baseAddress!)
   stream.avail_in = uInt(data.count)
   while true {
    stream.next_out = buffer; stream.avail_out = uInt(size)
    let result = inflate(&stream, Z_NO_FLUSH); let count = size - Int(stream.avail_out)
    try require(output.count + count <= maxJSON,"展开后的档案过大")
    output.append(buffer,count:count)
    if result == Z_STREAM_END { try require(stream.avail_in == 0,"档案含额外数据"); break }
    try require(result == Z_OK && (count > 0 || stream.avail_in > 0),"档案损坏或被截断")
   }
  }
  return output
 }
 static func install(_ url: URL) throws -> [String:Any] { try serialized {
  _ = launchCleanup
  let length = try url.resourceValues(forKeys:[.fileSizeKey]).fileSize ?? (maxArchive + 1)
  try require(length <= maxArchive,"档案需小于 16 MiB")
  let unpacked = try inflateArchive(Data(contentsOf:url))
  guard let p = try JSONSerialization.jsonObject(with:unpacked) as? [String:Any],
   p["format"] as? String == "sda-phrtf", p["version"] as? Int == 1,
   let m = p["manifest"] as? [String:Any], let id = m["subjectId"] as? String,
   id.range(of:"^personal-[a-f0-9]{64}$",options:.regularExpression) != nil,
   m["sampleRate"] as? Int == 48000, m["completeSubject"] as? Bool == true,
   m["personalSofaVersion"] as? Int == 1,
   let processing = m["processing"] as? [String:Any], processing["preserveMeasurements"] as? Bool == true,
   let rows = m["positions"] as? [[String:Any]], (32...2048).contains(rows.count),
   let files = p["files"] as? [[String:Any]], files.count <= 4096 else {
   throw PersonalHrtfError.message("请选择完整的 48 kHz SOFA 转换档案；不接受参数猜测档案")
  }
  var assets: [String:Data] = [:]; var total = 0
  for f in files {
   guard let name = f["name"] as? String, name.range(of:"^azm?[0-9]+_elm?[0-9]+_(dry|wet)\\.f32$",options:.regularExpression) != nil,
    assets[name] == nil, let encoded = f["data"] as? String, encoded.count <= 174764,
    let bytes = Data(base64Encoded:encoded), bytes.base64EncodedString() == encoded,
    bytes.count >= 32, bytes.count % 8 == 0, bytes.count <= 131072,
    let digest = f["sha256"] as? String, SHA256.hash(data:bytes).map({String(format:"%02x",$0)}).joined() == digest else {
    throw PersonalHrtfError.message("响应路径、尺寸或校验失败")
   }
   total += bytes.count; try require(total <= maxSamples,"响应总量超过 16 MiB")
   var energy = [0.0,0.0]
   for i in stride(from:0,to:bytes.count,by:4) {
    let word = bytes[i..<(i+4)].enumerated().reduce(UInt32(0)) { $0 | (UInt32($1.element) << (8 * $1.offset)) }
    let x = Float(bitPattern:word); try require(x.isFinite && abs(x) <= 100,"响应包含无效采样"); energy[i < bytes.count / 2 ? 0 : 1] += Double(x*x)
   }
   try require(energy.allSatisfy { $0 > 1e-16 },"响应不能静音"); assets[name] = bytes
  }
  var used = Set<String>(), directions = Set<String>(), sectors = Set<Int>(); var low = false, high = false
  for row in rows {
   guard let az = row["azimuth"] as? Double, let el = row["elevation"] as? Double,
    az.isFinite, el.isFinite, abs(az) <= 360, abs(el) <= 90,
    let dry = row["dry"] as? String, let wet = row["wet"] as? String,
    assets[dry] != nil, assets[wet] != nil else { throw PersonalHrtfError.message("方向或响应文件缺失") }
   let wrapped = (az.truncatingRemainder(dividingBy:360) + 360).truncatingRemainder(dividingBy:360)
   try require(directions.insert(String(format:"%.4f/%.4f",wrapped,el)).inserted,"档案包含重复方向")
   if abs(el) <= 30 { sectors.insert(Int((wrapped + 22.5).truncatingRemainder(dividingBy:360) / 45)) }
   low = low || el <= -60; high = high || el >= 60; used.insert(dry); used.insert(wet)
  }
  try require(used.count == assets.count && sectors.count == 8 && low && high,"个人档案需要完整前后、上方及下方覆盖")
  let root = try root(), target = root.appendingPathComponent(id,isDirectory:true)
  let manifest = try JSONSerialization.data(withJSONObject:m,options:[.sortedKeys])
  if FileManager.default.fileExists(atPath:target.path) {
   try require(try Data(contentsOf:target.appendingPathComponent("hrtf-set.json")) == manifest,"同 ID 档案内容不同")
   for (name,bytes) in assets { try require(try Data(contentsOf:target.appendingPathComponent(name)) == bytes,"同 ID 响应不同") }
  } else {
   let enumerator = FileManager.default.enumerator(at:root,includingPropertiesForKeys:[.fileSizeKey])
   var stored = 0
   while let item = enumerator?.nextObject() as? URL { stored += (try item.resourceValues(forKeys:[.fileSizeKey]).fileSize) ?? 0 }
   try require(stored + total + manifest.count <= 32 * 1024 * 1024,"个人档案总量已达上限，请恢复默认并重启应用释放旧档案后导入")
   let stage = root.appendingPathComponent("import-" + UUID().uuidString,isDirectory:true)
   try FileManager.default.createDirectory(at:stage,withIntermediateDirectories:false)
   defer { try? FileManager.default.removeItem(at:stage) }
   for (name,bytes) in assets { try bytes.write(to:stage.appendingPathComponent(name),options:.atomic) }
   try manifest.write(to:stage.appendingPathComponent("hrtf-set.json"),options:.atomic)
   try validateNative(stage.appendingPathComponent("hrtf-set.json").path)
   try FileManager.default.moveItem(at:stage,to:target)
  }
  try validateNative(target.appendingPathComponent("hrtf-set.json").path)
  UserDefaults.standard.set(id,forKey:key)
  return status()
 } }
 private static func validateNative(_ path: String) throws {
  guard let result = sda_ios_validate_hrtf(path) else { throw PersonalHrtfError.message("原生响应校验没有返回结果") }
  defer { sda_ios_string_free(result) }
  let reply = try JSONSerialization.jsonObject(with:Data(String(cString:result).utf8)) as? [String:Any]
  try require(reply?["ok"] as? Bool == true,reply?["error"] as? String ?? "原生渲染器不接受此档案")
 }
}
