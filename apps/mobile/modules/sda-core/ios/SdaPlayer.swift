import Foundation
import AVFoundation
import MediaPlayer
import CryptoKit
import AudioToolbox
import UIKit

enum SdaError: LocalizedError {
 case message(String)
 var errorDescription: String? { if case let .message(text) = self { return text }; return nil }

}

final class SdaPlayer {
 let lock = NSRecursiveLock()
 let feeder = DispatchQueue(label: "sda.ios.decode", qos: .userInitiated)
 var handle: UnsafeMutableRawPointer?
 var systemSpatial: SystemSpatial360?
 var hasPlayback: Bool { handle != nil || systemSpatial != nil }
 var audio: AVAudioEngine?
 var source: AVAudioSourceNode?
 var generation = 0
 var isPaused = false
 var done = false
 var failure: String?
 var hrtfState = "KU100 · 等待播放加载"
 var layout = "7.1.4"
 var imports: [String: (URL, URL)] = [:]
 var observers: [NSObjectProtocol] = []
 var remoteTargets: [(MPRemoteCommand, Any)] = []
 var title = "SDA"
 var trackHash = ""
 var mediaInfo: [String:Any] = [:]
 var duration: Double = 0
 var interruptionWasPlaying = false
 let prefs = UserDefaults.standard
 func locked<T>(_ work: () throws -> T) rethrows -> T { lock.lock(); defer { lock.unlock() }; return try work() }
 func json(_ value: Any) throws -> String { String(decoding: try JSONSerialization.data(withJSONObject: value, options: [.fragmentsAllowed]), as: UTF8.self) }
 func decodeReply(_ p: UnsafeMutablePointer<CChar>?) throws -> Any {
  guard let p else { throw SdaError.message("原生返回为空") }
  defer { sda_ios_string_free(p) }
  let data = Data(String(cString: p).utf8)
  guard let result = try JSONSerialization.jsonObject(with: data) as? [String: Any] else { throw SdaError.message("原生返回无效") }
  guard result["ok"] as? Bool == true else { throw SdaError.message(result["error"] as? String ?? "原生引擎错误") }
  return result["value"] ?? NSNull()
 }
 func command(_ op: String, _ args: [String: Any] = [:]) throws -> Any {
  if let system = systemSpatial {
   switch op {
   case "status": _ = try system.objects(decodeReply); return system.status()
   case "objects": return try system.objects(decodeReply)
   case "balance": system.setBalance(args["enabled"] as? Bool ?? false); return true
   case "pause": system.setPaused(args["paused"] as? Bool ?? false); return true
   case "volume": system.renderer.volume = (args["volume"] as? NSNumber)?.floatValue ?? 1; return true
   case "finish": try system.feed(nil,finish:true,decodeReply:decodeReply); return ["errors":[]]
   case "loudness": return "null"
   // Saved SDA preferences never enter the system-rendered path.
   case "yaw", "resetPose", "measured", "near", "room", "preset", "rendering": return ["bypassed":true]
   default: throw SdaError.message("系统空间音频不支持该命令: \(op)")
   }
  }
  guard let h = handle else { throw SdaError.message("引擎未启动") }
  return try decodeReply(sda_ios_command(h, op, try json(args)))
 }
 func assetRoot() throws -> URL {
  let moduleBundle = Bundle(for: SdaPlayer.self)
  guard let url = moduleBundle.url(forResource: "SdaCoreAssets", withExtension: "bundle") ?? Bundle.main.url(forResource: "SdaCoreAssets", withExtension: "bundle"),
        let bundle = Bundle(url: url), let root = bundle.resourceURL else { throw SdaError.message("iOS KU100 资源包缺失") }
  return root
 }
 func settings() -> [String: Any] {
  return ["systemSpatial360RA":prefs.bool(forKey:"sda.systemSpatial360RA"), "systemSpatial360RAActive":systemSpatial != nil, "layout": layout, "hrtfSet": prefs.string(forKey: "sda.hrtfSet") ?? "dense",
   "hrtfWetWeight": prefs.object(forKey: "sda.wet") ?? 0.04,
   "direct": prefs.object(forKey: "sda.direct") ?? true, "directional": prefs.object(forKey: "sda.directional") ?? true,
   "nearField": prefs.bool(forKey: "sda.near"), "metresPerUnit": prefs.object(forKey: "sda.scale") ?? 1.0,
   "roomId": savedRoom(), "volumeBalanceEnabled": prefs.bool(forKey: "sda.balance")]
 }
 func hrtfPath(_ set: String) throws -> String {
  let directory = set == "standard" ? "hrtf" : set == "dense-raw" ? "hrtf-dense-raw" : "hrtf-dense"
  let path = try assetRoot().appendingPathComponent(directory).appendingPathComponent("hrtf-set.json")
  guard FileManager.default.fileExists(atPath: path.path) else { throw SdaError.message("KU100 测量资源缺失") }
  return path.path
 }
 func stopNative() {
  generation += 1
  systemSpatial?.close(); systemSpatial = nil
  // Stop callbacks before freeing the C handle. Do not reset on preset changes.
  audio?.stop()
  if let node = source { audio?.detach(node) }
  audio = nil; source = nil
  if let h = handle { sda_ios_close(h); handle = nil }
  isPaused = false; done = false
  MPNowPlayingInfoCenter.default().playbackState = .stopped
  MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
 }
 func startNative() throws {
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
 func setPaused(_ paused: Bool) throws -> Bool {
  guard hasPlayback, paused || !done else { return false }
  if paused {
   _ = try command("pause",["paused":true])
   // The worker command is asynchronous. Freeze the Apple consumer as well so
   // queued DSP work cannot advance the audible clock after pause returns.
   audio?.pause()
  } else {
   try AVAudioSession.sharedInstance().setActive(true)
   _ = try command("pause",["paused":false])
   if audio?.isRunning == false { try audio?.start() }
  }
  isPaused = paused; updateNowPlaying(); return true
 }
 func updateNowPlaying() {
  guard hasPlayback else { return }
  let s = (try? command("status")) as? [String: Any] ?? [:]
  let endedPosition = done ? ((s["decodedSamplePos"] as? NSNumber)?.doubleValue ?? 0)/48 : nil
  var info = mediaInfo
  info.merge([MPMediaItemPropertyTitle:title,
   MPMediaItemPropertyPlaybackDuration:duration/1000,
   MPNowPlayingInfoPropertyElapsedPlaybackTime:(endedPosition ?? (s["positionMs"] as? Double ?? 0))/1000,
   MPNowPlayingInfoPropertyPlaybackRate:(isPaused || done || s["preparingAudio"] as? Bool == true) ? 0.0 : 1.0,
   MPNowPlayingInfoPropertyDefaultPlaybackRate:1.0,
   MPNowPlayingInfoPropertyIsLiveStream:false,
   MPNowPlayingInfoPropertyExternalContentIdentifier:trackHash,
   MPNowPlayingInfoPropertyMediaType:MPNowPlayingInfoMediaType.audio.rawValue]) { _, new in new }
  MPNowPlayingInfoCenter.default().nowPlayingInfo = info
  MPNowPlayingInfoCenter.default().playbackState = done ? .stopped : isPaused ? .paused : .playing
 }
 func setMediaMetadata(_ hash: String, _ metadata: [String:Any]) {
  guard hasPlayback, hash == trackHash else { return }
  if let value = metadata["title"] as? String, !value.isEmpty { title = value }
  if let value = metadata["artist"] as? String { mediaInfo[MPMediaItemPropertyArtist] = value }
  if let value = metadata["album"] as? String { mediaInfo[MPMediaItemPropertyAlbumTitle] = value }
  if let value = metadata["albumArtist"] as? String { mediaInfo[MPMediaItemPropertyAlbumArtist] = value }
  if let value = metadata["durationMs"] as? Double, value.isFinite, value > 0 { duration = value }
  if let uri = metadata["coverUri"] as? String, uri.hasPrefix("data:image/"),
     let comma = uri.firstIndex(of:","), uri.count <= 16*1024*1024,
     let data = Data(base64Encoded:String(uri[uri.index(after:comma)...])), let image = UIImage(data:data) {
   mediaInfo[MPMediaItemPropertyArtwork] = MPMediaItemArtwork(boundsSize:image.size) { _ in image }
  }
  updateNowPlaying()
 }
 func installSystemControls() {
  for notification in [UIApplication.didEnterBackgroundNotification, UIApplication.didBecomeActiveNotification] {
   observers.append(NotificationCenter.default.addObserver(forName: notification, object:nil, queue:nil) { [weak self] _ in
    guard let self else { return }; self.locked { self.updateNowPlaying() }
   })
  }
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
    if type == .began { self.interruptionWasPlaying = self.hasPlayback && !self.isPaused; _ = try? self.setPaused(true) }
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
 func play(_ uri: String, _ name: String, _ yaw: Double, _ hash: String) throws -> String {
  try locked {
   stopNative(); failure = nil
   guard let url = URL(string:uri), url.isFileURL else { throw SdaError.message("请选择本地音频文件") }
   let ext = (name as NSString).pathExtension.lowercased()
   layout = ext == "mhas" ? "360RA-13" : "7.1.4"
   let input = try CompressedInput(url:url,name:name)
   do {
    if ext == "mhas" && prefs.bool(forKey:"sda.systemSpatial360RA") {
     let session = AVAudioSession.sharedInstance()
     try session.setCategory(.playback, mode:.default, options:[])
     try session.setPreferredSampleRate(48000); try session.setActive(true)
     systemSpatial = try SystemSpatial360(decodeReply:decodeReply,volume:(prefs.object(forKey:"sda.volume") as? NSNumber)?.floatValue ?? 1)
     systemSpatial?.setBalance(prefs.bool(forKey:"sda.balance"))
     hrtfState = "360RA → 7.1.4 · 苹果系统空间音频请求 · KU100/房间已旁路"
    } else { try startNative() }
    title = name; trackHash = hash; mediaInfo = [MPNowPlayingInfoPropertyAssetURL:url]; duration = mediaDuration(url)
    if ext == "mp3" { _ = try command("mp3",["path":url.path]) }
    if ext == "mhas" && systemSpatial == nil { _ = try command("mpegh") }
    // Apply persisted balance after the new route/source is fully selected.
    _ = try command("balance",["enabled":prefs.bool(forKey:"sda.balance")])
    _ = try command("yaw",["degrees":yaw])
    if let cached = prefs.string(forKey:"sda.loudness."+hash) { _ = try? command("measured",["json":cached]) }
    let token = generation; updateNowPlaying()
    feeder.async { [weak self] in self?.feed(input:input, mp3:ext == "mp3", token:token, hash:hash) }
    return try json(["layout":layout])
   } catch { stopNative(); throw error }
  }
 }
 func feed(input: CompressedInput, mp3: Bool, token: Int, hash: String) {
  var finished = false
  var lastClock: UInt64 = 0
  var lastProgress = Date()
  var lastInfo = Date.distantPast
  do {
   let systemMode = locked { systemSpatial != nil && generation == token }
   while true {
    let shouldWait: Bool? = try locked {
     guard generation == token, hasPlayback else { return nil }
     if isPaused { lastProgress = Date(); return true }
     try systemSpatial?.pump()
     let status = try command("status") as! [String:Any]
     let decoded = (status["decodedSamplePos"] as? NSNumber)?.uint64Value ?? 0
     let consumed = (status["consumedSamplePos"] as? NSNumber)?.uint64Value ?? 0
     if consumed != lastClock { lastClock = consumed; lastProgress = Date() }
     if decoded > 0 && Date().timeIntervalSince(lastProgress) > 15 { throw SdaError.message("iOS 音频输出停止消耗数据: decoded=\(decoded), consumed=\(consumed), engineRunning=\(audio?.isRunning == true), system=\(systemMode)") }
     if Date().timeIntervalSince(lastInfo) > 1 { updateNowPlaying(); lastInfo = Date() }
     if finished { if consumed >= decoded && (status["fifoFrames"] as? Int ?? 0) == 0 { done = true; isPaused = true; updateNowPlaying(); return nil }; return true }
     if status["preparingAudio"] as? Bool == true { return false }
     return decoded > consumed + 4*48000 || (status["fifoFrames"] as? Int ?? 0) > 4*48000
    }
    guard let wait = shouldWait else { return }
    if wait { Thread.sleep(forTimeInterval:0.02); continue }
    let data = mp3 ? nil : try input.next(chunkBytes:systemMode ? 1024 : 24*1024)
    try locked {
     guard generation == token, hasPlayback else { return }
     var eof = false
     if mp3 { let result = try command("pullMp3") as! [String:Any]; eof = result["eof"] as? Bool == true }
     else if let data, let system = systemSpatial { try system.feed(data,finish:false,decodeReply:decodeReply) }
     else if let data, let h = handle { let result = try data.withUnsafeBytes { try decodeReply(sda_ios_feed(h, $0.bindMemory(to: UInt8.self).baseAddress, data.count)) } as? [String:Any]; if let errors = result?["errors"] as? [String], !errors.isEmpty { throw SdaError.message(errors.joined(separator:"; ")) } }
     else { eof = true }
     if eof {
      let result = try command("finish") as? [String:Any]
      if let errors = result?["errors"] as? [String], !errors.isEmpty { throw SdaError.message(errors.joined(separator:"; ")) }
      if let measurement = try command("loudness") as? String, !measurement.isEmpty && measurement != "null" { prefs.set(measurement,forKey:"sda.loudness."+hash) }
      let finalStatus = try command("status") as! [String:Any]
      if let samples = finalStatus["decodedSamplePos"] as? NSNumber, samples.doubleValue > 0 { duration = samples.doubleValue / 48 }
      finished = true
     }
    }
   }
  } catch { locked { if generation == token { failure = String(describing:error); _ = try? setPaused(true) } } }
 }
 func mediaDuration(_ url: URL) -> Double { let seconds = CMTimeGetSeconds(AVURLAsset(url:url).duration); return seconds.isFinite ? max(0,seconds*1000) : 0 }
 func roomCatalog() throws -> [[String:Any]] {
  let d = try Data(contentsOf:assetRoot().appendingPathComponent("rooms/catalog.json"))
  return (try JSONSerialization.jsonObject(with:d) as? [String:Any])?["profiles"] as? [[String:Any]] ?? []
 }
 // Room preferences belong to the SOURCE layout, not the Apple output layout.
 func savedRoom() -> String {
  if let id = prefs.string(forKey:"sda.room."+layout) { return id }
  let legacy = prefs.string(forKey:"sda.room") ?? ""
  return (try? roomCatalog().contains(where:{ item in
   guard let summary = item["summary"] as? [String:Any] else { return false }
   return summary["id"] as? String == legacy && summary["layout"] as? String == layout
  })) == true ? legacy : ""
 }
 func saveRoom(_ id: String) throws {
  if !id.isEmpty {
   guard try roomCatalog().contains(where:{ item in
    guard let s = item["summary"] as? [String:Any] else { return false }
    return s["id"] as? String == id && s["layout"] as? String == layout
   }) else { throw SdaError.message("房间布局与当前音源不匹配") }
  }
  if handle != nil { _ = try command("room",["path":id.isEmpty ? "" : try roomPath(id)]) }
  prefs.set(id,forKey:"sda.room."+layout)
 }
 func roomPath(_ id: String) throws -> String {
  guard try roomCatalog().contains(where:{ ($0["summary"] as? [String:Any])?["id"] as? String == id }) else { throw SdaError.message("未知房间资源") }
  return try assetRoot().appendingPathComponent("rooms/"+id+".json").path
 }

 /// Only enabled by the explicit simulator CI environment; never on ordinary launch.
 func runCISmoke() {
  var report: [String: Any] = ["ok": false]
  let savedBalance = prefs.bool(forKey:"sda.balance")
  let savedSystemPreference = prefs.bool(forKey:"sda.systemSpatial360RA")
  prefs.set(false,forKey:"sda.systemSpatial360RA")
  defer { prefs.set(savedSystemPreference,forKey:"sda.systemSpatial360RA"); prefs.set(savedBalance,forKey:"sda.balance") }
  do {
   let uri = try assetRoot().appendingPathComponent("ci-stereo-tones.m4a").absoluteString
   _ = try play(uri, "ci-stereo-tones.m4a", 0, "ci-generated-tone")
   let deadline = Date().addingTimeInterval(30)
   var changed = false
   while Date() < deadline {
    let complete = try locked { () throws -> Bool in
     if let failure { throw SdaError.message(failure) }
     let s = try command("status") as! [String: Any]
     let clock = (s["consumedSamplePos"] as? NSNumber)?.uint64Value ?? 0
     if !changed && clock >= 4096 && !done {
      let token = generation
      prefs.set(true,forKey:"sda.systemSpatial360RA")
      guard systemSpatial == nil else { throw SdaError.message("开关打断现有 SDA 播放") }
      report["togglePreservesCurrentRoute"] = true
      _ = try command("preset", ["path":try hrtfPath("dense"),"wet":0.0,"direct":true,"directional":true])
      let after = try command("status") as! [String: Any]
      let afterClock = (after["consumedSamplePos"] as? NSNumber)?.uint64Value ?? 0
      guard generation == token, afterClock >= clock else { throw SdaError.message("预设切换重置播放时钟") }
      report["presetClockBefore"] = clock
      report["presetClockAfter"] = after["consumedSamplePos"]
      _ = try setPaused(true)
      changed = true
     }
     if done {
      guard changed && clock >= 46080 else { throw SdaError.message("压缩音轨未完成实际播放") }
      report["ok"] = true; report["status"] = s
      report["hardwareSampleRate"] = AVAudioSession.sharedInstance().sampleRate
      return true
     }
     return false
    }
    if complete { break }
    if locked({isPaused && !done}) {
     Thread.sleep(forTimeInterval: 0.15)
     let before = try locked { try command("status") as! [String: Any] }
     Thread.sleep(forTimeInterval: 0.15)
     try locked {
      let after = try command("status") as! [String: Any]
      guard before["consumedSamplePos"] as? NSNumber == after["consumedSamplePos"] as? NSNumber else { throw SdaError.message("暂停仍消耗音频") }
      report["pauseClockStable"] = true
      _ = try setPaused(false)
     }
    }
    Thread.sleep(forTimeInterval: 0.02)
   }
   if report["ok"] as? Bool != true { throw SdaError.message("模拟器音频冒烟测试超时") }
   report["compressedReaderRecovery"] = try smokeReaderRecovery()
   report["native360RA"] = try smokeNative360()
   prefs.set(true,forKey:"sda.systemSpatial360RA")
   report["system360RA"] = try smokeSystem360()
   prefs.set(true,forKey:"sda.balance")
   report["native360RAAfterSystem"] = try smokeNative360()
   locked { stopNative() }
   if #available(iOS 26.0, *) {
    let output = FileManager.default.urls(for:.documentDirectory,in:.userDomainMask).first!
    report["phase360RA"] = try Phase360Prototype.run(try assetRoot().appendingPathComponent("ci-360ra.mhas"),output:output,decodeReply:decodeReply)
   } else { throw SdaError.message("PHASE 原型需要 iOS 26 模拟器") }
  } catch { report["ok"] = false; report["error"] = error.localizedDescription }
  locked { stopNative() }
  if let dir = FileManager.default.urls(for:.documentDirectory,in:.userDomainMask).first,
     let data = try? JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys]) {
   try? data.write(to:dir.appendingPathComponent("sda-ci-smoke.json"),options:.atomic)
  }
 }

 func smokeReaderRecovery() throws -> [String:Any] {
  let url = try assetRoot().appendingPathComponent("ci-stereo-tones.m4a")
  let baseline = try CompressedInput(url:url,name:"ci-stereo-tones.m4a")
  var expected = Data()
  while let packet = try baseline.next() { expected.append(packet) }
  let resumed = try CompressedInput(url:url,name:"ci-stereo-tones.m4a")
  var actual = Data()
  for _ in 0..<3 { if let packet = try resumed.next() { actual.append(packet) } }
  try resumed.reopenAfterInterruption()
  while let packet = try resumed.next() { actual.append(packet) }
  guard actual == expected, !actual.isEmpty else { throw SdaError.message("压缩音轨恢复丢包或重复") }
  return ["ok":true,"bytes":actual.count,"byteIdentical":true]
 }

 // Use the real default KU100 route and sda.ios.decode dispatch queue.
 func smokeNative360() throws -> [String:Any] {
  prefs.set(false,forKey:"sda.systemSpatial360RA")
  let uri = try assetRoot().appendingPathComponent("ci-360ra.mhas").absoluteString
  _ = try play(uri,"ci-360ra.mhas",0,"ci-native-360ra")
  guard layout == "360RA-13", try roomCatalog().contains(where:{ ($0["summary"] as? [String:Any])?["layout"] as? String == "360RA-13" }) else { throw SdaError.message("360RA 源房间目录缺失") }
  let deadline = Date().addingTimeInterval(30)
  var displayedObjects = 0
  while Date() < deadline {
   let complete: [String:Any]? = try locked {
    if let failure { throw SdaError.message(failure) }
    guard handle != nil, systemSpatial == nil else { throw SdaError.message("360RA 默认 KU100 路由缺失") }
    let status = try command("status") as! [String:Any]
    displayedObjects = max(displayedObjects,(try command("objects") as? [String:Any])?.count ?? 0)
    if done {
     let decoded = (status["decodedSamplePos"] as? NSNumber)?.uint64Value ?? 0
     let consumed = (status["consumedSamplePos"] as? NSNumber)?.uint64Value ?? 0
     guard displayedObjects == 2, decoded > 48000, consumed >= decoded else { throw SdaError.message("360RA KU100 未完成播放") }
     guard status["volumeBalanceEnabled"] as? Bool == prefs.bool(forKey:"sda.balance"),
       status["volumeBalanceEligible"] as? Bool == true else { throw SdaError.message("KU100 音量平衡偏好未恢复") }
     return ["ok":true,"status":status,"route":"KU100","decodeQueue":"sda.ios.decode","displayedObjects":displayedObjects]
    }
    return nil
   }
   if let complete { return complete }
   Thread.sleep(forTimeInterval:0.01)
  }
  throw SdaError.message("360RA KU100 冒烟测试超时")
 }

 func smokeSystem360() throws -> [String:Any] {
  let uri = try assetRoot().appendingPathComponent("ci-360ra.mhas").absoluteString
  _ = try play(uri,"ci-360ra.mhas",0,"ci-360ra")
  let deadline = Date().addingTimeInterval(30)
  var checkedPause = false
  var displayedObjects = 0
  try locked {
   let cover = UIGraphicsImageRenderer(size:CGSize(width:16,height:16)).image { context in
    UIColor.red.setFill(); context.fill(CGRect(x:0,y:0,width:16,height:16))
   }.pngData()!
   setMediaMetadata("ci-360ra",["title":"360RA test","artist":"SDA CI","album":"Spatial test","durationMs":3000.0,"coverUri":"data:image/png;base64,"+cover.base64EncodedString()])
   setMediaMetadata("stale-track",["title":"wrong track"])
   updateNowPlaying()
   let info = MPNowPlayingInfoCenter.default().nowPlayingInfo ?? [:]
   guard info[MPMediaItemPropertyArtist] as? String == "SDA CI", info[MPMediaItemPropertyTitle] as? String == "360RA test",
     info[MPMediaItemPropertyAlbumTitle] as? String == "Spatial test", info[MPMediaItemPropertyArtwork] is MPMediaItemArtwork else { throw SdaError.message("系统路径媒体元数据未发送") }
   _ = try command("balance",["enabled":true])
  }
  while Date() < deadline {
   let complete: [String:Any]? = try locked {
    if let failure { throw SdaError.message(failure) }
    guard let system = systemSpatial else { throw SdaError.message("360RA 未进入系统输出路径") }
    let s = system.status()
    displayedObjects = max(displayedObjects, (try command("objects") as? [String:Any])?.count ?? 0)
    if !checkedPause && system.enqueued > 0 && !done {
     _ = try setPaused(true)
     prefs.set(false,forKey:"sda.systemSpatial360RA")
     return nil
    }
    if done {
     guard checkedPause, displayedObjects == 2, system.enqueued > 0, system.queued == 0,
       s["outputChannels"] as? Int == 12 else { throw SdaError.message("360RA 系统输出未完成") }
     guard MPNowPlayingInfoCenter.default().playbackState == .stopped,
       duration > 0 else { throw SdaError.message("曲终媒体状态未停止") }
     return ["ok":true,"status":s,"endedStateVerified":true,"pauseClockStable":true,"togglePreservesCurrentRoute":true,
      "allowedMultichannel":system.renderer.allowedAudioSpatializationFormats.contains(.multichannel),
      "physicalSpatialListeningVerified":false,"displayedObjects":displayedObjects,"nowPlayingMetadataVerified":true,"balanceToggleVerified":true]
    }
    return nil
   }
   if let complete { return complete }
   if locked({isPaused && !done}) {
    Thread.sleep(forTimeInterval:0.15)
    let before = try locked { try command("status") as! [String:Any] }
    Thread.sleep(forTimeInterval:0.15)
    try locked {
     let after = try command("status") as! [String:Any]
     guard before["consumedSamplePos"] as? NSNumber == after["consumedSamplePos"] as? NSNumber,
       systemSpatial != nil else { throw SdaError.message("360RA 暂停或开关重置当前路由") }
     systemSpatial?.restorePendingSamples()
     checkedPause = true; _ = try setPaused(false)
    }
   }
   Thread.sleep(forTimeInterval:0.01)
  }
  throw SdaError.message("360RA 系统空间音频冒烟测试超时")
 }
}
