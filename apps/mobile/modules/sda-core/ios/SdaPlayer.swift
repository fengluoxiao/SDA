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
 var alacActive = false
 var alacUpmixActive = false
 var systemSpatial: SystemSpatial360?
 var hasPlayback: Bool { handle != nil || systemSpatial != nil }
 var audio: AVAudioEngine?
 var source: AVAudioSourceNode?
 var playbackQueue: [[String:Any]] = []
 var playbackMode = "sequence"
 var playbackYaw: Double = 0
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
 var sessionInterrupted = false
 var inBackground = false
 var backgroundTransitions = 0
 var recoveringOutput = false
 var preparationLease: PlaybackPreparationLease?
 var preparationSerial = 0
 var playbackEvents: [[String:Any]] = []
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
   // Status is queried by the feeder every ~20 ms. Do not serialize the
   // entire object graph here; explicit objects + the native 1 s tick drain
   // consumed events even when JS is suspended in the background.
   case "status": return system.status()
   case "objects": return try system.objects(decodeReply)
   case "balance": system.setBalance(args["enabled"] as? Bool ?? false); return true
   case "pause": system.setPaused(args["paused"] as? Bool ?? false); return true
   case "volume": system.renderer.volume = (args["volume"] as? NSNumber)?.floatValue ?? 1; return true
   case "finish": try system.feed(nil,finish:true,decodeReply:decodeReply); return ["errors":[]]
   case "loudness": return try json(decodeReply(sda_ios_speakers_measurement(system.decoder)))
   case "measured": try system.setMeasurement(args["json"] as? String ?? "",decodeReply:decodeReply); return true
   // Saved SDA preferences never enter the system-rendered path.
   case "yaw", "resetPose", "near", "room", "preset", "rendering": return ["bypassed":true]
   default: throw SdaError.message("系统空间音频不支持该命令: \(op)")
   }
  }
  guard let h = handle else { throw SdaError.message("引擎未启动") }
  let result = try decodeReply(sda_ios_command(h, op, try json(args)))
  if op == "status", var status = result as? [String:Any] {
   status["sourceCodec"] = alacActive ? "alac" : layout == "360RA-13" ? "mpegh" : ""
   status["alacUpmixActive"] = alacUpmixActive
   status["outputChannels"] = 2
   status["outputLayout"] = layout
   return status
  }
  return result
 }
 func assetRoot() throws -> URL {
  let moduleBundle = Bundle(for: SdaPlayer.self)
  guard let url = moduleBundle.url(forResource: "SdaCoreAssets", withExtension: "bundle") ?? Bundle.main.url(forResource: "SdaCoreAssets", withExtension: "bundle"),
        let bundle = Bundle(url: url), let root = bundle.resourceURL else { throw SdaError.message("iOS KU100 资源包缺失") }
  return root
 }
 func spatialCueDb() -> Double {
  let value = (prefs.object(forKey:"sda.spatialCueDb") as? NSNumber)?.doubleValue ?? -6
  return [0.0, -3, -6, -9, -12].contains(value) ? value : -6
 }
 func saveSpatialCueDb(_ db: Double) throws {
  guard [0.0, -3, -6, -9, -12].contains(db) else { throw SdaError.message("空间线索强度无效") }
  let snapshot: (Int, String, String)? = try locked {
   guard handle != nil && systemSpatial == nil else { prefs.set(db, forKey:"sda.spatialCueDb"); return nil }
   return (generation, layout, try hrtfPath("dense"))
  }
  guard let snapshot else { return }
  // Do not hold the playback/decoder lock during disk IO or FFT preparation.
  var error: UnsafeMutablePointer<CChar>?
  guard let prepared = sda_ios_prepare_cues(snapshot.2, snapshot.1, Float(pow(10.0, db / 20.0)), &error) else {
   _ = try decodeReply(error); throw SdaError.message("空间线索准备失败")
  }
  defer { sda_ios_free_cues(prepared) }
  try locked {
   guard generation == snapshot.0, layout == snapshot.1, let h = handle, systemSpatial == nil else {
    throw SdaError.message("播放曲目已切换，请重新选择空间线索强度")
   }
   _ = try decodeReply(sda_ios_apply_cues(h, prepared))
   prefs.set(db, forKey:"sda.spatialCueDb")
  }
 }
 func saveSpatialEnhancement(_ enabled: Bool) throws {
  if handle != nil && systemSpatial == nil { _ = try command("spatialEnhancement", ["enabled":enabled]) }
  prefs.set(enabled, forKey:"sda.spatialEnhancement")
 }
 func settings() -> [String: Any] {
  if prefs.integer(forKey:"sda.mobileDirectVersion") < 1 {
   for (k,v) in [("sda.hrtfSet","dense"),("sda.wet",0.0),("sda.direct",true),("sda.directional",true),("sda.near",false),("sda.room","")] as [(String,Any)] { prefs.set(v,forKey:k) }
   prefs.set(1,forKey:"sda.mobileDirectVersion")
  }
  return ["alacStereoUpmix":prefs.bool(forKey:"sda.alacStereoUpmix"), "systemSpatialStereo":prefs.bool(forKey:"sda.systemSpatialStereo"), "systemSpatial360RA":prefs.bool(forKey:"sda.systemSpatial360RA"), "systemSpatial360RAActive":systemSpatial != nil, "layout": layout, "hrtfSet": "dense",
   "hrtfWetWeight": 0.0, "spatialCueDb": spatialCueDb(),
   "spatialEnhancementEnabled": prefs.bool(forKey:"sda.spatialEnhancement"),
   "direct": prefs.object(forKey: "sda.direct") ?? true, "directional": prefs.object(forKey: "sda.directional") ?? true,
   "nearField": false, "metresPerUnit": prefs.object(forKey: "sda.scale") ?? 1.0,
   "roomId": "", "volumeBalanceEnabled": prefs.bool(forKey: "sda.balance")]
 }
 func hrtfPath(_ set: String) throws -> String {
  let directory = "hrtf-restored/hrtf-dense"
  let path = try assetRoot().appendingPathComponent(directory).appendingPathComponent("hrtf-set.json")
  guard FileManager.default.fileExists(atPath: path.path) else { throw SdaError.message("KU100 测量资源缺失") }
  return path.path
 }
 func stopNative() {
  generation += 1
  finishPreparation(); interruptionWasPlaying = false; sessionInterrupted = false
  systemSpatial?.close(); systemSpatial = nil
  alacActive = false; alacUpmixActive = false
  // Stop callbacks before freeing the C handle. Do not reset on preset changes.
  audio?.stop()
  if let node = source { audio?.detach(node) }
  audio = nil; source = nil
  if let h = handle { sda_ios_close(h); handle = nil }
  isPaused = false; done = false
  MPNowPlayingInfoCenter.default().playbackState = .stopped
  MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
 }
 // Serialized with the feeder; no decoder, clock, loudness or audio reset.
 func setAlacStereoUpmix(_ enabled: Bool) {
  prefs.set(enabled,forKey:"sda.alacStereoUpmix")
  guard alacActive, hasPlayback else { return }
  alacUpmixActive = enabled
  layout = enabled ? "7.1.4" : "2.0"
  systemSpatial?.setAlacUpmix(enabled)
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
   // Bundled kernels already contain the approved -6 dB cues.
   if spatialCueDb() != -6 { try saveSpatialCueDb(spatialCueDb()) }
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
   _ = try command("spatialEnhancement", ["enabled":s["spatialEnhancementEnabled"]!])
   let room = s["roomId"] as! String
   if !room.isEmpty { _ = try command("room",["path":try roomPath(room)]) }
   hrtfState = "KU100 · 61 方向 · 历史兼容插值 · 前后/上下空间线索 · iOS 原生输出"
  } catch { stopNative(); throw error }
 }
 func setPaused(_ paused: Bool) throws -> Bool {
  guard hasPlayback, paused || !done else { return false }
  if paused {
   finishPreparation()
   _ = try command("pause",["paused":true])
   // The worker command is asynchronous. Freeze the Apple consumer as well so
   // queued DSP work cannot advance the audible clock after pause returns.
   audio?.pause()
  } else {
   if failure != nil { throw SdaError.message("音频输出发生错误，请重新播放") }
   if systemSpatial?.started == false { beginPreparation(generation) }
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
 func beginPreparation(_ token: Int) {
  finishPreparation()
  let serial = preparationSerial
  preparationLease = PlaybackPreparationLease { [weak self] in
   guard let self else { return }
   self.locked {
    guard self.generation == token, self.preparationSerial == serial, self.preparationLease != nil else { return }
    self.recordPlaybackEvent("preparationExpired")
    self.stopNative(); self.failure = "后台音频准备时间已到，请回到前台重新播放"
   }
  }
 }
 func finishPreparation() { guard preparationLease != nil else { return }; preparationSerial += 1; preparationLease?.end(); preparationLease = nil }
 func recordPlaybackEvent(_ event: String, detail: String = "") {
  let status = hasPlayback ? (try? command("status")) as? [String:Any] ?? [:] : [:]
  playbackEvents.append(["time":Date().timeIntervalSince1970,"event":event,"detail":detail,
   "generation":generation,"background":inBackground,"paused":isPaused,"done":done,
   "sessionInterrupted":sessionInterrupted,"engineRunning":audio?.isRunning ?? false,
   "route":systemSpatial == nil ? "KU100" : "system714",
   "decoded":status["decodedSamplePos"] ?? 0,"consumed":status["consumedSamplePos"] ?? 0])
  if playbackEvents.count > 96 { playbackEvents.removeFirst(playbackEvents.count-96) }
  let url = FileManager.default.urls(for:.cachesDirectory,in:.userDomainMask)[0].appendingPathComponent("sda-playback-diagnostics.json")
  if let text = try? json(playbackEvents) { try? Data(text.utf8).write(to:url,options:.atomic) }
 }
 func recoverOutputIfNeeded(_ reason: String) throws {
  guard hasPlayback, !isPaused, !done, !sessionInterrupted, !recoveringOutput else { return }
  recoveringOutput = true; defer { recoveringOutput = false }
  if let audio, !audio.isRunning {
   try AVAudioSession.sharedInstance().setActive(true)
   try audio.start(); recordPlaybackEvent("engineRestart",detail:reason)
  }
  if let system = systemSpatial { try system.pump() }
 }
 func installSystemControls() {
  for notification in [UIApplication.didEnterBackgroundNotification, UIApplication.didBecomeActiveNotification] {
   observers.append(NotificationCenter.default.addObserver(forName: notification, object:nil, queue:nil) { [weak self] _ in
    guard let self else { return }; self.locked {
     self.inBackground = notification == UIApplication.didEnterBackgroundNotification
     if self.inBackground { self.backgroundTransitions += 1 }
     self.recordPlaybackEvent(self.inBackground ? "background" : "foreground")
     do { try self.recoverOutputIfNeeded("lifecycle") } catch { self.recordPlaybackEvent("outputRecoveryError",detail:String(describing:error)) }
     self.updateNowPlaying()
    }
   })
  }
  for notification in [UIApplication.protectedDataWillBecomeUnavailableNotification, UIApplication.protectedDataDidBecomeAvailableNotification] {
   observers.append(NotificationCenter.default.addObserver(forName:notification,object:nil,queue:nil) { [weak self] _ in
    guard let self else { return }; self.locked { self.recordPlaybackEvent("protectedData",detail:notification.rawValue) }
   })
  }
  observers.append(NotificationCenter.default.addObserver(forName:UIApplication.didReceiveMemoryWarningNotification,object:nil,queue:nil) { [weak self] _ in
   guard let self else { return }; self.locked { self.recordPlaybackEvent("memoryWarning") }
  })
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
    self.recordPlaybackEvent("interruption",detail:String(describing:note.userInfo ?? [:]))
    if type == .began {
     self.sessionInterrupted = true; self.interruptionWasPlaying = self.hasPlayback && !self.isPaused
     _ = try? self.setPaused(true)
    } else {
     self.sessionInterrupted = false
     let shouldResume = self.interruptionWasPlaying && AVAudioSession.InterruptionOptions(rawValue:note.userInfo?[AVAudioSessionInterruptionOptionKey] as? UInt ?? 0).contains(.shouldResume)
     self.interruptionWasPlaying = false
     if shouldResume { _ = try? self.setPaused(false) }
    }
   }
  })
  observers.append(NotificationCenter.default.addObserver(forName: AVAudioSession.routeChangeNotification, object:nil, queue:nil) { [weak self] note in
   guard let self else { return }
   self.locked {
    self.recordPlaybackEvent("routeChange",detail:String(describing:note.userInfo ?? [:]))
    if (note.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt) == AVAudioSession.RouteChangeReason.oldDeviceUnavailable.rawValue { _ = try? self.setPaused(true) }
    else { do { try self.recoverOutputIfNeeded("routeChange") } catch { self.failure = String(describing:error); self.recordPlaybackEvent("outputRecoveryError",detail:String(describing:error)) } }
   }
  })
  observers.append(NotificationCenter.default.addObserver(forName: .AVAudioEngineConfigurationChange, object:nil, queue:nil) { [weak self] _ in
   guard let self else { return }
   self.locked { do { try self.recoverOutputIfNeeded("configurationChange") } catch { self.failure = String(describing:error); self.recordPlaybackEvent("outputRecoveryError",detail:String(describing:error)) } }
  })
 }
 func play(_ uri: String, _ name: String, _ yaw: Double, _ hash: String) throws -> String {
  try locked {
   stopNative(); failure = nil
   guard let url = URL(string:uri), url.isFileURL else { throw SdaError.message("请选择本地音频文件") }
   let ext = (name as NSString).pathExtension.lowercased()
   let input = try CompressedInput(url:url,name:name)
   alacActive = input.isAlac
   alacUpmixActive = input.isAlac && prefs.bool(forKey:"sda.alacStereoUpmix")
   layout = input.isAlac ? (alacUpmixActive ? "7.1.4" : "2.0") : (ext == "mhas" ? "360RA-13" : "7.1.4")
   do {
    if (ext == "mhas" && prefs.bool(forKey:"sda.systemSpatial360RA")) || (input.isAlac && prefs.bool(forKey:"sda.systemSpatialStereo")) {
     let session = AVAudioSession.sharedInstance()
     try session.setCategory(.playback, mode:.default, options:[])
     try session.setPreferredSampleRate(48000); try session.setActive(true)
     systemSpatial = try SystemSpatial360(decodeReply:decodeReply,volume:(prefs.object(forKey:"sda.volume") as? NSNumber)?.floatValue ?? 1, alacUpmix:input.isAlac ? alacUpmixActive : nil)
     systemSpatial?.setBalance(prefs.bool(forKey:"sda.balance"))
     hrtfState = input.isAlac ? "ALAC 立体声 → \(alacUpmixActive ? "7.1.4 上混" : "2.0") · 苹果系统输出 · KU100/房间已旁路" : "360RA → 7.1.4 · 苹果系统空间音频请求 · KU100/房间已旁路"
    } else { try startNative() }
    playbackYaw = yaw
    title = name; trackHash = hash; mediaInfo = [MPNowPlayingInfoPropertyAssetURL:url]; duration = mediaDuration(url)
    if ext == "mp3" { _ = try command("mp3",["path":url.path]) }
    if ext == "mhas" && systemSpatial == nil { _ = try command("mpegh") }
    // Apply persisted balance after the new route/source is fully selected.
    _ = try command("balance",["enabled":prefs.bool(forKey:"sda.balance")])
    _ = try command("yaw",["degrees":yaw])
    // The CICP19 speaker measurement is not interchangeable with the KU100 reference.
    let loudnessKey = (systemSpatial?.loudnessPrefix ?? (input.isAlac ? "sda.loudness.alac.stereo.v1." : "sda.loudness.")) + hash
    if !hash.isEmpty, let cached = prefs.string(forKey:loudnessKey) { _ = try? command("measured",["json":cached]) }
    // Both descriptors must be opened before JS may unlink an extracted MHAS copy.
    let analysisInput: CompressedInput?
    if systemSpatial != nil { analysisInput = try CompressedInput(url:url,name:name) } else { analysisInput = nil }
    let token = generation; beginPreparation(token); recordPlaybackEvent("play"); updateNowPlaying()
    feeder.async { [weak self] in self?.feed(input:input, mp3:ext == "mp3", token:token, hash:hash, analysisInput:analysisInput) }
    return try json(["layout":layout])
   } catch { stopNative(); throw error }
  }
 }
 // The bridge owns a single MPEG-H instance. Analyze using that instance,
 // then recreate it for playback under the same lock. Input has its own reader.
 // Each chunk is bounded; cancellation cannot free an in-use decoder pointer.
 func prepareSystemBalance(input: CompressedInput?, token: Int, hash: String) throws -> Bool {
  let needed = locked { () -> Bool in
   guard generation == token, let system = systemSpatial,
     system.balanceEnabled, !system.started, !system.measurementReady else { return false }
   sda_ios_speakers_measure_only(system.decoder); return true
  }
  guard needed else { return locked { generation == token } }
  guard let analysis = input else { throw SdaError.message("360RA 响度分析输入未打开") }
  while true {
   guard locked({ generation == token && hasPlayback }) else { return false }
   let more: Bool? = try autoreleasepool {
    let data = try analysis.next(chunkBytes:1024)
    return try locked { () throws -> Bool? in
     guard generation == token, let system = systemSpatial else { return nil }
     if let data {
      _ = try system.decodeInput(data,finish:false,decodeReply:decodeReply); return true
     }
     _ = try system.decodeInput(nil,finish:true,decodeReply:decodeReply); return false
    }
   }
   guard let more else { return false }
   if !more { break }
  }
  return try locked {
   guard generation == token, let system = systemSpatial else { return false }
   let measurement = try json(decodeReply(sda_ios_speakers_measurement(system.decoder)))
   try system.restartAfterMeasurement(measurement,decodeReply:decodeReply)
   if !hash.isEmpty { prefs.set(measurement,forKey:system.loudnessPrefix+hash) }
   return true
  }
 }
 func feed(input: CompressedInput, mp3: Bool, token: Int, hash: String, analysisInput: CompressedInput?) {
  var finished = false
  var lastClock: UInt64 = 0
  var lastProgress = Date()
  var lastInfo = Date.distantPast
  do {
   let systemMode = locked { systemSpatial != nil && generation == token }
   if systemMode { if !(try prepareSystemBalance(input:analysisInput,token:token,hash:hash)) { return }; lastProgress = Date() }
   while true {
    // A long-lived GCD loop must drain Cocoa temporaries per iteration.
    let keepFeeding = try autoreleasepool { () throws -> Bool in
    let shouldWait: Bool? = try locked {
     guard generation == token, hasPlayback else { return nil }
     if isPaused { lastProgress = Date(); return true }
     try recoverOutputIfNeeded("decode")
     let status = try command("status") as! [String:Any]
     let decoded = (status["decodedSamplePos"] as? NSNumber)?.uint64Value ?? 0
     let consumed = (status["consumedSamplePos"] as? NSNumber)?.uint64Value ?? 0
     if consumed > 0 { finishPreparation() }
     if consumed != lastClock { lastClock = consumed; lastProgress = Date() }
     if decoded > 0 && Date().timeIntervalSince(lastProgress) > 15 { throw SdaError.message("iOS 音频输出停止消耗数据: decoded=\(decoded), consumed=\(consumed), engineRunning=\(audio?.isRunning == true), system=\(systemMode)") }
     if Date().timeIntervalSince(lastInfo) > 1 {
      // Advance metadata even while React polling is suspended. Keep future
      // events, coalesce consumed events to the current object snapshot.
      _ = try command("objects"); updateNowPlaying(); lastInfo = Date()
     }
     if finished { if consumed >= decoded && (status["fifoFrames"] as? Int ?? 0) == 0 {
      if try advanceNativeQueue() { return nil }
      done = true
      // End the actual Apple consumer too; an engine pumping silence can
      // leave system media activity visible even after its rate becomes zero.
      _ = try setPaused(true)
      recordPlaybackEvent("ended"); updateNowPlaying(); return nil }; return true }
     if status["preparingAudio"] as? Bool == true { return false }
     // Bound ALAC lookahead so a live switch does not sit behind four seconds.
     let lead = alacActive ? 24000 : 4*48000
     return decoded > consumed + UInt64(lead) || (status["fifoFrames"] as? Int ?? 0) > lead
    }
    guard let wait = shouldWait else { return false }
    if wait { Thread.sleep(forTimeInterval:0.02); return true }
    if systemMode && locked({systemSpatial?.preparingAudio == true}) {
     if !(try prepareSystemBalance(input:analysisInput,token:token,hash:hash)) { return false }; lastProgress = Date()
    }
    let data = mp3 ? nil : try input.next(chunkBytes:systemMode ? 1024 : 24*1024)
    try locked {
     guard generation == token, hasPlayback else { return }
     var eof = false
     if mp3 { let result = try command("pullMp3") as! [String:Any]; eof = result["eof"] as? Bool == true }
     else if let data, let system = systemSpatial { try system.feed(data,finish:false,decodeReply:decodeReply) }
     else if let data, input.isAlac, let h = handle {
      _ = try data.withUnsafeBytes { try decodeReply(sda_ios_alac_feed(h,$0.bindMemory(to:UInt8.self).baseAddress,data.count,alacUpmixActive)) }
     }
     else if let data, let h = handle { let result = try data.withUnsafeBytes { try decodeReply(sda_ios_feed(h, $0.bindMemory(to: UInt8.self).baseAddress, data.count)) } as? [String:Any]; if let errors = result?["errors"] as? [String], !errors.isEmpty { throw SdaError.message(errors.joined(separator:"; ")) } }
     else { eof = true }
     if eof {
      let result = try command("finish") as? [String:Any]
      if let errors = result?["errors"] as? [String], !errors.isEmpty { throw SdaError.message(errors.joined(separator:"; ")) }
      if !hash.isEmpty, let measurement = try command("loudness") as? String, !measurement.isEmpty && measurement != "null" { prefs.set(measurement,forKey:(systemSpatial?.loudnessPrefix ?? (input.isAlac ? "sda.loudness.alac.stereo.v1." : "sda.loudness."))+hash) }
      let finalStatus = try command("status") as! [String:Any]
      if let samples = finalStatus["decodedSamplePos"] as? NSNumber, samples.doubleValue > 0 { duration = samples.doubleValue / 48 }
      finished = true
     }
    }
    return true
    }
    if !keepFeeding { return }
   }
  } catch { locked { if generation == token { finishPreparation(); failure = String(describing:error); recordPlaybackEvent("feedError",detail:String(describing:error)); _ = try? setPaused(true) } } }
 }
 // Called only by the serialized native feeder under the player lock, before
 // stopping the old consumer. No React timer/import/bridge round trip at EOF.
 func advanceNativeQueue() throws -> Bool {
  guard let index = playbackQueue.firstIndex(where: { $0["hash"] as? String == trackHash }) else { return false }
  let next = playbackMode == "repeat-one" ? index : index + 1
  guard next < playbackQueue.count || playbackMode == "repeat-all" else { return false }
  let track = playbackQueue[next % playbackQueue.count]
  guard let uri = track["uri"] as? String, let name = track["name"] as? String,
   let hash = track["hash"] as? String else { return false }
  let lease = PlaybackPreparationLease(expired: {})
  defer { lease.end() }
  do {
   _ = try play(uri,name,playbackYaw,hash)
   setMediaMetadata(hash,track["metadata"] as? [String:Any] ?? [:])
   recordPlaybackEvent("nativeQueueAdvance",detail:hash)
  } catch {
   // play() changes generation even on failure; the outgoing feeder must not
   // swallow this error through its stale-generation catch guard.
   stopNative(); done = true; failure = String(describing:error)
   recordPlaybackEvent("nativeQueueError",detail:failure ?? "")
  }
  return true
 }
 func mediaDuration(_ url: URL) -> Double { let seconds = CMTimeGetSeconds(AVURLAsset(url:url).duration); return seconds.isFinite ? max(0,seconds*1000) : 0 }
 // Compatibility bridge: mobile builds have no room assets or selectable rooms.
 func roomCatalog() throws -> [[String:Any]] { [] }
 func savedRoom() -> String { "" }
 func saveRoom(_ id: String) throws {
  guard id.isEmpty else { throw SdaError.message("移动端已移除房间仿真") }
  if handle != nil { _ = try command("room",["path":""]) }
 }
 func roomPath(_ id: String) throws -> String {
  guard id.isEmpty else { throw SdaError.message("移动端已移除房间仿真") }
  return ""
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
   report["system360RACached"] = try smokeSystem360(useCache:true)
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
  guard layout == "360RA-13" else { throw SdaError.message("360RA 源布局缺失") }
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
     guard isPaused, audio?.isRunning == false,
       MPNowPlayingInfoCenter.default().playbackState == .stopped,
       MPNowPlayingInfoCenter.default().nowPlayingInfo?[MPNowPlayingInfoPropertyPlaybackRate] as? Double == 0 else { throw SdaError.message("KU100 ended output/media state is still active") }
     guard displayedObjects == 2, decoded > 48000, consumed >= decoded else { throw SdaError.message("360RA KU100 未完成播放") }
     guard status["volumeBalanceEnabled"] as? Bool == prefs.bool(forKey:"sda.balance"),
       status["volumeBalanceEligible"] as? Bool == true else { throw SdaError.message("KU100 音量平衡偏好未恢复") }
     guard status["spatialCuesEnabled"] as? Bool == true,
       status["roomEnabled"] as? Bool == false,
       status["nearFieldEnabled"] as? Bool == false,
       status["directObjectHrtf"] as? Bool == true,
       status["directionalHrtf"] as? Bool == true else { throw SdaError.message("KU100 空间线索或逐对象实际方向未启用") }
     return ["ok":true,"status":status,"route":"KU100","decodeQueue":"sda.ios.decode","displayedObjects":displayedObjects]
    }
    return nil
   }
   if let complete { return complete }
   Thread.sleep(forTimeInterval:0.01)
  }
  throw SdaError.message("360RA KU100 冒烟测试超时")
 }

 func runCIBackgroundSmoke(_ mode: String) {
  let directory = FileManager.default.urls(for:.documentDirectory,in:.userDomainMask)[0]
  let reportURL = directory.appendingPathComponent("sda-ci-background.json")
  let readyURL = directory.appendingPathComponent("sda-ci-background-ready.json")
  let clip = directory.appendingPathComponent("ci-background.mhas")
  var report: [String:Any] = ["ok":false,"mode":mode,"physicalLockVerified":false]
  let oldSystem = prefs.bool(forKey:"sda.systemSpatial360RA"), oldBalance = prefs.bool(forKey:"sda.balance")
  defer {
   locked { playbackQueue = []; playbackMode = "sequence"; stopNative() }
   prefs.set(oldSystem,forKey:"sda.systemSpatial360RA"); prefs.set(oldBalance,forKey:"sda.balance")
   try? FileManager.default.removeItem(at:clip)
   if let text = try? json(report) { try? Data(text.utf8).write(to:reportURL,options:.atomic) }
  }
  do {
   let fixture = try Data(contentsOf:assetRoot().appendingPathComponent("ci-360ra.mhas"))
   var repeated = Data(); for _ in 0..<12 { repeated.append(fixture) }
   try repeated.write(to:clip)
   try FileManager.default.setAttributes([.protectionKey:FileProtectionType.complete],ofItemAtPath:clip.path)
   let beforeValue = try FileManager.default.attributesOfItem(atPath:clip.path)[.protectionKey]
   let beforeProtection = (beforeValue as? FileProtectionType) ?? (beforeValue as? String).map { FileProtectionType(rawValue:$0) }
   // CoreSimulator can accept setAttributes without implementing data protection.
   // Verify the policy separately, never report physical protection as proven.
   guard CompressedInput.lockedPlaybackProtection(.complete) == .completeUntilFirstUserAuthentication,
     CompressedInput.lockedPlaybackProtection(.completeUnlessOpen) == .completeUntilFirstUserAuthentication,
     CompressedInput.lockedPlaybackProtection(FileProtectionType.none) == FileProtectionType.none,
     CompressedInput.lockedPlaybackProtection(.completeUntilFirstUserAuthentication) == .completeUntilFirstUserAuthentication else {
    throw SdaError.message("锁屏副本保护策略不正确")
   }
   prefs.set(mode == "system",forKey:"sda.systemSpatial360RA"); prefs.set(mode == "system",forKey:"sda.balance")
   prefs.removeObject(forKey:"sda.loudness.system714.v1.ci-background-"+mode)
   _ = try play(clip.absoluteString,"ci-background.mhas",0,"ci-background-"+mode)
   let value = try FileManager.default.attributesOfItem(atPath:clip.path)[.protectionKey]
   let protection = (value as? FileProtectionType) ?? (value as? String).map { FileProtectionType(rawValue:$0) }
   let protectionAvailable = beforeProtection == .complete
   let protectionVerified = protectionAvailable && protection == .completeUntilFirstUserAuthentication
   #if targetEnvironment(simulator)
   if protectionAvailable && !protectionVerified { throw SdaError.message("导入副本未调整为锁屏可读保护级别") }
   #else
   guard protectionVerified else { throw SdaError.message("导入副本未调整为锁屏可读保护级别") }
   #endif
   // Mirrors JS discarding an extracted MHAS after playUri returns: both open
   // descriptors must remain valid for playback and the independent full scan.
   try FileManager.default.removeItem(at:clip)
   let startTransitions = locked { backgroundTransitions }
   let deadline = Date().addingTimeInterval(60)
   var ready = false
   var backgroundClock: UInt64?
   var sustainedBackground = false
   var sustainedEnd: UInt64 = 0
   var nextGeneration: Int?
   while Date() < deadline {
    let state: (UInt64, Bool, Bool) = try locked {
     if let failure { throw SdaError.message(failure) }
     let status = try command("status") as! [String:Any]
     return ((status["consumedSamplePos"] as? NSNumber)?.uint64Value ?? 0, inBackground && backgroundTransitions > startTransitions, preparationLease == nil)
    }
    if !ready && state.0 > 12000 && state.2 {
     ready = true
     try Data("{\"ready\":true}".utf8).write(to:readyURL,options:.atomic)
    }
    if state.1 && backgroundClock == nil { backgroundClock = state.0 }
    if let clock = backgroundClock, state.1, !sustainedBackground, state.0 > clock + 8*48000 {
     sustainedBackground = true
     sustainedEnd = state.0
     let nextURI = try assetRoot().appendingPathComponent("ci-360ra.mhas").absoluteString
     locked {
      playbackQueue = [["hash":trackHash,"uri":clip.absoluteString,"name":"ci-background.mhas"],
       ["hash":"ci-background-next-"+mode,"uri":nextURI,"name":"ci-360ra.mhas"]]
      playbackMode = "sequence"
     }
    }
    if sustainedBackground && state.1 {
     let progress = locked { () -> (Bool, Int) in
      let next = trackHash == "ci-background-next-"+mode
      if next { playbackMode = "repeat-one" }
      return (next,generation)
     }
     if progress.0 && nextGeneration == nil { nextGeneration = progress.1 }
     if let first = nextGeneration, progress.0, progress.1 > first, state.0 > 12000, state.2 {
     report = ["ok":true,"mode":mode,"backgroundNotificationObserved":true,
      "backgroundStart":backgroundClock ?? 0,"backgroundEnd":sustainedEnd,"preparationAssertionReleased":state.2,
      "nativeNextTrackVerified":true,"nativeRepeatOneVerified":true,"javascriptHandoffRequired":false,
      "ownedCopyProtectionVerified":protectionVerified,"ownedCopyProtectionPolicyVerified":true,
      "simulatorFileProtectionAvailable":protectionAvailable,"unlinkedAnalysisInputVerified":true,"physicalLockVerified":false,
      "events":locked { playbackEvents }]
     return
     }
    }
    Thread.sleep(forTimeInterval:0.02)
   }
   throw SdaError.message("真实后台播放未持续推进音频时钟")
  } catch { report["error"] = String(describing:error); report["events"] = locked { playbackEvents } }
 }

 func smokeSystem360(useCache: Bool = false) throws -> [String:Any] {
  prefs.set(true,forKey:"sda.systemSpatial360RA"); prefs.set(true,forKey:"sda.balance")
  if !useCache { prefs.removeObject(forKey:"sda.loudness.system714.v1.ci-360ra") }
  let uri = try assetRoot().appendingPathComponent("ci-360ra.mhas").absoluteString
  _ = try play(uri,"ci-360ra.mhas",0,"ci-360ra")
  if useCache && !locked({systemSpatial?.measurementReady == true}) { throw SdaError.message("系统空间音频响度缓存未在起播前恢复") }
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
       s["outputChannels"] as? Int == 12, system.measurementReady else { throw SdaError.message("360RA 系统输出未完成") }
     guard MPNowPlayingInfoCenter.default().playbackState == .stopped,
       duration > 0 else { throw SdaError.message("曲终媒体状态未停止") }
     return ["ok":true,"status":s,"endedStateVerified":true,"pauseClockStable":true,"togglePreservesCurrentRoute":true,
      "allowedMultichannel":system.renderer.allowedAudioSpatializationFormats.contains(.multichannel),
      "physicalSpatialListeningVerified":false,"displayedObjects":displayedObjects,"nowPlayingMetadataVerified":true,"balanceToggleVerified":true,"fullTrackBalancePrepared":system.measurementReady,"firstSubmissionBalanced":system.firstSubmissionBalanced,"cachedMeasurementRestored":useCache]
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
