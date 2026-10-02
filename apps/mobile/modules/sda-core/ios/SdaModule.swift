import ExpoModulesCore
import Foundation
import AVFoundation
import MediaPlayer
import CryptoKit
import AudioToolbox
import SdaNative

enum SdaError: LocalizedError {
 case message(String)
 var errorDescription: String? { if case let .message(text) = self { return text }; return nil }
}

public final class SdaModule: Module {
 private let lock = NSRecursiveLock()
 private let feeder = DispatchQueue(label: "sda.ios.decode", qos: .userInitiated)
 private var handle: UnsafeMutableRawPointer?
 private var audio: AVAudioEngine?
 private var source: AVAudioSourceNode?
 private var generation = 0
 private var isPaused = false
 private var done = false
 private var failure: String?
 private var hrtfState = "KU100 · 等待播放加载"
 private var layout = "7.1.4"
 private var imports: [String: (URL, URL)] = [:]
 private var observers: [NSObjectProtocol] = []
 private var remoteTargets: [(MPRemoteCommand, Any)] = []
 private var title = "SDA"
 private var duration: Double = 0
 private var interruptionWasPlaying = false
 private let prefs = UserDefaults.standard
 private func locked<T>(_ work: () throws -> T) rethrows -> T { lock.lock(); defer { lock.unlock() }; return try work() }
 private func json(_ value: Any) throws -> String { String(decoding: try JSONSerialization.data(withJSONObject: value, options: [.fragmentsAllowed]), as: UTF8.self) }
 private func decodeReply(_ p: UnsafeMutablePointer<CChar>?) throws -> Any {
  guard let p else { throw SdaError.message("原生返回为空") }
  defer { sda_ios_string_free(p) }
  let data = Data(String(cString: p).utf8)
  guard let result = try JSONSerialization.jsonObject(with: data) as? [String: Any] else { throw SdaError.message("原生返回无效") }
  guard result["ok"] as? Bool == true else { throw SdaError.message(result["error"] as? String ?? "原生引擎错误") }
  return result["value"] ?? NSNull()
 }
 private func command(_ op: String, _ args: [String: Any] = [:]) throws -> Any {
  guard let h = handle else { throw SdaError.message("引擎未启动") }
  return try decodeReply(sda_ios_command(h, op, try json(args)))
 }
 private func assetRoot() throws -> URL {
  let moduleBundle = Bundle(for: SdaModule.self)
  guard let url = moduleBundle.url(forResource: "SdaCoreAssets", withExtension: "bundle") ?? Bundle.main.url(forResource: "SdaCoreAssets", withExtension: "bundle"),
        let bundle = Bundle(url: url), let root = bundle.resourceURL else { throw SdaError.message("iOS KU100 资源包缺失") }
  return root
 }
 private func settings() -> [String: Any] {
  return ["layout": layout, "hrtfSet": prefs.string(forKey: "sda.hrtfSet") ?? "dense",
   "hrtfWetWeight": prefs.object(forKey: "sda.wet") ?? 0.04,
   "direct": prefs.object(forKey: "sda.direct") ?? true, "directional": prefs.object(forKey: "sda.directional") ?? true,
   "nearField": prefs.bool(forKey: "sda.near"), "metresPerUnit": prefs.object(forKey: "sda.scale") ?? 1.0,
   "roomId": prefs.string(forKey: "sda.room") ?? "", "volumeBalanceEnabled": prefs.bool(forKey: "sda.balance")]
 }
 private func hrtfPath(_ set: String) throws -> String {
  let directory = set == "standard" ? "hrtf" : set == "dense-raw" ? "hrtf-dense-raw" : "hrtf-dense"
  let path = try assetRoot().appendingPathComponent(directory).appendingPathComponent("hrtf-set.json")
  guard FileManager.default.fileExists(atPath: path.path) else { throw SdaError.message("KU100 测量资源缺失") }
  return path.path
 }
 private func stopNative() {
  generation += 1
  // Stop callbacks before freeing the C handle. Do not reset on preset changes.
  audio?.stop()
  if let node = source { audio?.detach(node) }
  audio = nil; source = nil
  if let h = handle { sda_ios_close(h); handle = nil }
  isPaused = false; done = false
  MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
 }
 private func startNative() throws {
  let session = AVAudioSession.sharedInstance()
  try session.setCategory(.playback, mode: .default, options: [])
  try session.setPreferredSampleRate(48000)
  try session.setPreferredIOBufferDuration(0.01)
  try session.setActive(true)
  let s = settings()
  let config = try json(["sampleRate":48000,"outputChannels":2,"layout":layout,
   "directObjectHrtf":s["direct"]!,"directionalHrtf":s["directional"]!,"hrtfWetWeight":s["hrtfWetWeight"]!])
  var error: UnsafeMutablePointer<CChar>?
  guard let h = sda_ios_create(config, try hrtfPath(s["hrtfSet"] as! String), &error) else {
   _ = try decodeReply(error); throw SdaError.message("引擎初始化失败")
  }
  handle = h
  do {
   let engine = AVAudioEngine()
   let format = AVAudioFormat(standardFormatWithSampleRate: 48000, channels: 2)!
   let node = AVAudioSourceNode(format: format) { _, _, frames, list -> OSStatus in
    let buffers = UnsafeMutableAudioBufferListPointer(list)
    guard buffers.count == 2, let l = buffers[0].mData, let r = buffers[1].mData else { return kAudio_ParamError }
    sda_ios_render(h, l.assumingMemoryBound(to: Float.self), r.assumingMemoryBound(to: Float.self), Int(frames))
    return noErr
   }
   engine.attach(node); engine.connect(node, to: engine.mainMixerNode, format: format)
   audio = engine; source = node
   engine.prepare(); try engine.start()
   _ = try command("near", ["enabled":s["nearField"]!,"scale":s["metresPerUnit"]!])
   _ = try command("balance", ["enabled":s["volumeBalanceEnabled"]!])
   _ = try command("volume", ["volume":prefs.object(forKey:"sda.volume") ?? 1.0])
   let room = s["roomId"] as! String
   if !room.isEmpty { _ = try command("room",["path":try roomPath(room)]) }
   hrtfState = "KU100 · 完整 HRTF · 实际方向 · iOS 原生输出"
  } catch { stopNative(); throw error }
 }
 private func setPaused(_ paused: Bool) throws -> Bool {
  guard handle != nil else { return false }
  if !paused { try AVAudioSession.sharedInstance().setActive(true); if audio?.isRunning == false { try audio?.start() } }
  _ = try command("pause",["paused":paused]); isPaused = paused; updateNowPlaying(); return true
 }
 private func updateNowPlaying() {
  guard handle != nil else { return }
  let s = (try? command("status")) as? [String: Any] ?? [:]
  MPNowPlayingInfoCenter.default().nowPlayingInfo = [MPMediaItemPropertyTitle:title,
   MPMediaItemPropertyPlaybackDuration:duration/1000,
   MPNowPlayingInfoPropertyElapsedPlaybackTime:(s["positionMs"] as? Double ?? 0)/1000,
   MPNowPlayingInfoPropertyPlaybackRate:isPaused ? 0.0 : 1.0]
 }
 private func installSystemControls() {
  let center = MPRemoteCommandCenter.shared()
  remoteTargets.append((center.pauseCommand, center.pauseCommand.addTarget { [weak self] _ in
   guard let self else { return .commandFailed }; return self.locked { (try? self.setPaused(true)) == true ? .success : .commandFailed }
  }))
  remoteTargets.append((center.playCommand, center.playCommand.addTarget { [weak self] _ in
   guard let self else { return .commandFailed }; return self.locked { (try? self.setPaused(false)) == true ? .success : .commandFailed }
  }))
  remoteTargets.append((center.togglePlayPauseCommand, center.togglePlayPauseCommand.addTarget { [weak self] _ in
   guard let self else { return .commandFailed }; return self.locked { (try? self.setPaused(!self.isPaused)) == true ? .success : .commandFailed }
  }))
  observers.append(NotificationCenter.default.addObserver(forName: AVAudioSession.interruptionNotification, object:nil, queue:nil) { [weak self] note in
   guard let self, let raw = note.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt,
         let type = AVAudioSession.InterruptionType(rawValue:raw) else { return }
   self.locked {
    if type == .began { self.interruptionWasPlaying = self.handle != nil && !self.isPaused; _ = try? self.setPaused(true) }
    else if self.interruptionWasPlaying,
      let options = note.userInfo?[AVAudioSessionInterruptionOptionKey] as? UInt,
      AVAudioSession.InterruptionOptions(rawValue:options).contains(.shouldResume) { _ = try? self.setPaused(false) }
   }
  })
  observers.append(NotificationCenter.default.addObserver(forName: AVAudioSession.routeChangeNotification, object:nil, queue:nil) { [weak self] note in
   guard let self else { return }
   self.locked {
    if (note.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt) == AVAudioSession.RouteChangeReason.oldDeviceUnavailable.rawValue { _ = try? self.setPaused(true) }
    else if self.handle != nil && !self.isPaused && self.audio?.isRunning == false { do { try self.audio?.start() } catch { self.failure = String(describing:error) } }
   }
  })
  observers.append(NotificationCenter.default.addObserver(forName: .AVAudioEngineConfigurationChange, object:nil, queue:nil) { [weak self] _ in
   guard let self else { return }
   self.locked { if self.handle != nil && !self.isPaused && self.audio?.isRunning == false { do { try self.audio?.start() } catch { self.failure = String(describing:error) } } }
  })
 }
 private func play(_ uri: String, _ name: String, _ yaw: Double, _ hash: String) throws -> String {
  try locked {
   stopNative(); failure = nil
   guard let url = URL(string:uri), url.isFileURL else { throw SdaError.message("请选择本地音频文件") }
   let ext = (name as NSString).pathExtension.lowercased()
   layout = ext == "mhas" ? "360RA-13" : "7.1.4"
   let input = try CompressedInput(url:url,name:name)
   do {
    try startNative(); title = name; duration = mediaDuration(url)
    if ext == "mp3" { _ = try command("mp3",["path":url.path]) }
    if ext == "mhas" { _ = try command("mpegh") }
    _ = try command("yaw",["degrees":yaw])
    if let cached = prefs.string(forKey:"sda.loudness."+hash) { _ = try? command("measured",["json":cached]) }
    let token = generation; updateNowPlaying()
    feeder.async { [weak self] in self?.feed(input:input, mp3:ext == "mp3", token:token, hash:hash) }
    return try json(["layout":layout])
   } catch { stopNative(); throw error }
  }
 }
 private func feed(input: CompressedInput, mp3: Bool, token: Int, hash: String) {
  var finished = false
  var lastClock: UInt64 = 0
  var lastProgress = Date()
  var lastInfo = Date.distantPast
  do {
   while true {
    let shouldWait: Bool? = try locked {
     guard generation == token, handle != nil else { return nil }
     if isPaused { lastProgress = Date(); return true }
     let status = try command("status") as! [String:Any]
     let decoded = (status["decodedSamplePos"] as? NSNumber)?.uint64Value ?? 0
     let consumed = (status["consumedSamplePos"] as? NSNumber)?.uint64Value ?? 0
     if consumed != lastClock { lastClock = consumed; lastProgress = Date() }
     if decoded > 0 && Date().timeIntervalSince(lastProgress) > 15 { throw SdaError.message("iOS 音频输出停止消耗数据") }
     if Date().timeIntervalSince(lastInfo) > 1 { updateNowPlaying(); lastInfo = Date() }
     if finished { if consumed >= decoded && (status["fifoFrames"] as? Int ?? 0) == 0 { done = true; isPaused = true; updateNowPlaying(); return nil }; return true }
     return decoded > consumed + 4*48000 || (status["fifoFrames"] as? Int ?? 0) > 4*48000
    }
    guard let wait = shouldWait else { return }
    if wait { Thread.sleep(forTimeInterval:0.02); continue }
    let data = mp3 ? nil : try input.next()
    try locked {
     guard generation == token, let h = handle else { return }
     var eof = false
     if mp3 { let result = try command("pullMp3") as! [String:Any]; eof = result["eof"] as? Bool == true }
     else if let data { let result = try data.withUnsafeBytes { try decodeReply(sda_ios_feed(h, $0.bindMemory(to: UInt8.self).baseAddress, data.count)) } as? [String:Any]; if let errors = result?["errors"] as? [String], !errors.isEmpty { throw SdaError.message(errors.joined(separator:"; ")) } }
     else { eof = true }
     if eof {
      let result = try command("finish") as? [String:Any]
      if let errors = result?["errors"] as? [String], !errors.isEmpty { throw SdaError.message(errors.joined(separator:"; ")) }
      if let measurement = try command("loudness") as? String, !measurement.isEmpty && measurement != "null" { prefs.set(measurement,forKey:"sda.loudness."+hash) }
      finished = true
     }
    }
   }
  } catch { locked { if generation == token { failure = String(describing:error); _ = try? setPaused(true) } } }
 }
 private func mediaDuration(_ url: URL) -> Double { let seconds = CMTimeGetSeconds(AVURLAsset(url:url).duration); return seconds.isFinite ? max(0,seconds*1000) : 0 }
 private func roomCatalog() throws -> [[String:Any]] {
  let d = try Data(contentsOf:assetRoot().appendingPathComponent("rooms/catalog.json"))
  return (try JSONSerialization.jsonObject(with:d) as? [String:Any])?["profiles"] as? [[String:Any]] ?? []
 }
 private func roomPath(_ id: String) throws -> String {
  guard try roomCatalog().contains(where:{ ($0["summary"] as? [String:Any])?["id"] as? String == id }) else { throw SdaError.message("未知房间资源") }
  return try assetRoot().appendingPathComponent("rooms/"+id+".json").path
 }
 public func definition() -> ModuleDefinition {
  Name("SdaEngine")
  OnCreate { self.installSystemControls() }
  OnDestroy { self.locked { self.stopNative(); for o in self.observers { NotificationCenter.default.removeObserver(o) }; for (c,t) in self.remoteTargets { c.removeTarget(t) }; for pair in self.imports.values { try? FileManager.default.removeItem(at:pair.1) }; self.imports.removeAll() } }
  Function("renderingSettings") { try self.locked { try self.json(self.settings()) } }
  Function("hrtfStatus") { self.locked { self.hrtfState } }
  Function("feedError") { self.locked { self.failure } }
  Function("feedDone") { self.locked { self.done } }
  Function("status") { try self.locked { try self.json(self.handle == nil ? [:] : self.command("status")) } }
  Function("objects") { try self.locked { try self.json(self.handle == nil ? [:] : self.command("objects")) } }
  Function("pause") { try self.locked { try self.setPaused(true) } }
  Function("resume") { try self.locked { try self.setPaused(false) } }
  Function("stop") { self.locked { self.stopNative(); return true } }
  Function("setHeadYaw") { (degrees: Double) in try self.locked { _ = try self.command("yaw",["degrees":degrees]) } }
  Function("resetHeadPose") { try self.locked { _ = try self.command("resetPose") } }
  Function("setVolume") { (v: Double) in try self.locked { guard v.isFinite && v >= 0 && v <= 1 else { throw SdaError.message("音量无效") }; if self.handle != nil { _ = try self.command("volume",["volume":v]) }; self.prefs.set(v,forKey:"sda.volume") } }
  Function("setVolumeBalance") { (v: Bool) in try self.locked { if self.handle != nil { _ = try self.command("balance",["enabled":v]) }; self.prefs.set(v,forKey:"sda.balance") } }
  Function("setObjectRendering") { (direct: Bool, directional: Bool) in try self.locked { if self.handle != nil { _ = try self.command("rendering",["direct":direct,"directional":directional]) }; self.prefs.set(direct,forKey:"sda.direct");self.prefs.set(directional,forKey:"sda.directional") } }
  Function("rooms") { try self.json(self.roomCatalog().compactMap { $0["summary"] }) }
  AsyncFunction("setNearField") { (enabled: Bool, scale: Double) in try self.locked { guard scale.isFinite && scale >= 0.25 && scale <= 4 else { throw SdaError.message("近场距离映射无效") }; if self.handle != nil { _ = try self.command("near",["enabled":enabled,"scale":scale]) }; self.prefs.set(enabled,forKey:"sda.near"); self.prefs.set(scale,forKey:"sda.scale") } }
  AsyncFunction("setRoom") { (id: String) in try self.locked { let path = id.isEmpty ? "" : try self.roomPath(id); if self.handle != nil { _ = try self.command("room",["path":path]) }; self.prefs.set(id,forKey:"sda.room") } }
  AsyncFunction("setRenderingPreset") { (id: String) in try self.locked {
   let data = try Data(contentsOf:self.assetRoot().appendingPathComponent("rendering-presets.json"))
   let presets = try JSONSerialization.jsonObject(with:data) as! [[String:Any]]
   guard let p = presets.first(where:{$0["id"] as? String == id}) else { throw SdaError.message("未知渲染预设") }
   if self.handle != nil { _ = try self.command("preset",["path":try self.hrtfPath(p["hrtfSet"] as! String),"wet":p["hrtfWetWeight"]!,"direct":p["direct"]!,"directional":p["directional"]!]); _ = try self.command("near",["enabled":false,"scale":1.0]); _ = try self.command("room",["path":""]) }
   for (k,v) in [("sda.hrtfSet",p["hrtfSet"]!),("sda.wet",p["hrtfWetWeight"]!),("sda.direct",p["direct"]!),("sda.directional",p["directional"]!),("sda.near",false),("sda.room","")] as [(String,Any)] { self.prefs.set(v,forKey:k) }
   self.hrtfState = (p["label"] as? String ?? "KU100")+" · 完整 HRTF"
  } }
  AsyncFunction("playUri") { (uri:String,name:String,yaw:Double,hash:String) in try self.play(uri,name,yaw,hash) }
  AsyncFunction("durationMs") { (uri:String) in guard let u = URL(string:uri) else { return 0.0 }; return self.mediaDuration(u) }
  AsyncFunction("metadata") { (uri:String) in
   guard let u = URL(string:uri) else { throw SdaError.message("文件地址无效") }
   let asset = AVURLAsset(url:u); var result: [String:Any] = ["durationMs":self.mediaDuration(u)]
   for item in asset.commonMetadata { if item.commonKey == .commonKeyTitle { result["title"] = item.stringValue }; if item.commonKey == .commonKeyArtist { result["artist"] = item.stringValue }; if item.commonKey == .commonKeyAlbumName { result["album"] = item.stringValue }; if item.commonKey == .commonKeyArtwork, let data = item.dataValue { result["coverUri"] = "data:image/jpeg;base64,"+data.base64EncodedString() } }
   return try self.json(result)
  }
  AsyncFunction("contentHash") { (uri:String) in
   guard let u = URL(string:uri) else { throw SdaError.message("文件地址无效") }; let scoped=u.startAccessingSecurityScopedResource();defer { if scoped { u.stopAccessingSecurityScopedResource() } }
   let file=try FileHandle(forReadingFrom:u);defer { try? file.close() };var hash=SHA256()
   while let data=try file.read(upToCount:256*1024), !data.isEmpty { hash.update(data:data) }
   return hash.finalize().map { String(format:"%02x",$0) }.joined()
  }
  AsyncFunction("beginMp4Import") { (uri:String) in try self.locked {
   guard let u=URL(string:uri),u.isFileURL else { throw SdaError.message("文件地址无效") }
   let token=UUID().uuidString;let out=FileManager.default.temporaryDirectory.appendingPathComponent(token+".mhas")
   FileManager.default.createFile(atPath:out.path,contents:Data());self.imports[token]=(u,out)
   let size=(try FileManager.default.attributesOfItem(atPath:u.path)[.size] as? NSNumber)?.int64Value ?? 0
   return try self.json(["token":token,"size":size])
  } }
  AsyncFunction("readMp4Import") { (token:String,offset:Double,count:Int) in try self.locked {
   guard let pair=self.imports[token], offset.isFinite && offset >= 0 && count > 0 && count <= 256*1024 else { throw SdaError.message("导入参数无效") }
   let f=try FileHandle(forReadingFrom:pair.0);defer { try? f.close() };try f.seek(toOffset:UInt64(offset));return try f.read(upToCount:count)?.base64EncodedString() ?? ""
  } }
  AsyncFunction("appendMp4Import") { (token:String,bytes:String) in try self.locked {
   guard let pair=self.imports[token],let data=Data(base64Encoded:bytes),data.count <= 256*1024 else { throw SdaError.message("导入参数无效") }
   let f=try FileHandle(forWritingTo:pair.1);defer { try? f.close() };try f.seekToEnd();try f.write(contentsOf:data)
  } }
  AsyncFunction("finishMp4Import") { (token:String) in try self.locked { guard let pair=self.imports[token] else { throw SdaError.message("导入任务不存在") };return pair.1.absoluteString } }
  AsyncFunction("discardMp4Import") { (token:String) in self.locked { if let pair=self.imports.removeValue(forKey:token) { try? FileManager.default.removeItem(at:pair.1) } } }
 }
}
