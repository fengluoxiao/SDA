import ExpoModulesCore
import Foundation
import AVFoundation
import MediaPlayer
import CryptoKit

public final class SdaModule: Module {
 private let player = SdaPlayer()
 private let personalHrtf = PersonalHrtfCoordinator(localPipeline:PersonalHrtfPipelineFactory.make())
 public func definition() -> ModuleDefinition {
  let player = self.player
  Name("SdaEngine")
  Function("personalHrtfStatus") { PersonalHrtfStore.status().merging(PersonalHrtfPipelineFactory.capabilities) { a,_ in a } }
  AsyncFunction("personalHrtfAction") { (action:String,promise:Promise) in
   self.personalHrtf.run(action) { result in
    switch result { case .success(let value): promise.resolve(value)
    case .failure(let error): promise.reject("ERR_PERSONAL_HRTF",error.localizedDescription) }
   }
  }.runOnQueue(.main)

  Function("sceneDisplayCapabilities") { SceneDisplayPolicy.capabilities() }
  Function("configureSceneDisplay") { (active:Bool,fps:Int,_:Bool) in SceneDisplayPolicy.configure(active:active,fps:fps) }
  #if targetEnvironment(simulator)
  Function("sceneSmokeEnabled") { ProcessInfo.processInfo.environment["SDA_IOS_SCENE_SMOKE"] == "1" }
  Function("reportSceneSmoke") { (text: String) in
   guard ProcessInfo.processInfo.environment["SDA_IOS_SCENE_SMOKE"] == "1" else { return }
   guard let data = text.data(using:.utf8) else { return }
   let directory = FileManager.default.urls(for:.documentDirectory,in:.userDomainMask)[0]
   try data.write(to:directory.appendingPathComponent("sda-ci-scene.json"),options:.atomic)
  }
  #else
  Function("sceneSmokeEnabled") { false }
  Function("reportSceneSmoke") { (_: String) in }
  #endif
  OnCreate {
   SceneDisplayPolicy.configure(active:false,fps:60)
   PlaybackStorage.startup()
   for key in player.prefs.dictionaryRepresentation().keys where key.hasPrefix("sda.loudness.") { player.prefs.removeObject(forKey:key) }
   player.prefs.removeObject(forKey:"sda.queue.selected")
   MPNowPlayingInfoCenter.default().nowPlayingInfo=nil
   MPNowPlayingInfoCenter.default().playbackState = .stopped
   player.configureDiagnostics(false)
   player.installSystemControls()
   player.locked { player.startHeadTracking() }
   #if targetEnvironment(simulator)
   if let mode = ProcessInfo.processInfo.environment["SDA_IOS_BACKGROUND_SMOKE"] {
    DispatchQueue.global(qos:.userInitiated).async { [weak player] in player?.runCIBackgroundSmoke(mode) }
   } else if ProcessInfo.processInfo.environment["SDA_IOS_SMOKE"] == "1" {
    DispatchQueue.global(qos:.userInitiated).async { [weak player] in player?.runCISmoke() }
   }
   #endif
  }
  OnDestroy { self.personalHrtf.cancel(); SceneDisplayPolicy.configure(active:false,fps:60); player.locked { player.clearPreparedStereoReferences(); player.diagnostics.configure(enabled:false) { [:] }; player.stopNative(); for o in player.observers { NotificationCenter.default.removeObserver(o) }; for (c,t) in player.remoteTargets { c.removeTarget(t) }; for session in player.imports.values { session.discard() }; player.imports.removeAll() } }
  Function("setAlacStereoUpmix") { (enabled: Bool) in player.locked { player.setAlacStereoUpmix(enabled); return true } }
  Function("setStereoSystemSpatialAudio") { (enabled: Bool) in player.locked { player.prefs.set(enabled,forKey:"sda.systemSpatialStereo"); return true } }
  Function("set360RaSystemSpatialAudio") { (enabled: Bool) in player.locked { player.prefs.set(enabled,forKey:"sda.systemSpatial360RA"); return true } }
  Function("setNowPlayingMetadata") { (hash: String, text: String) in try player.locked {
   guard let data = text.data(using:.utf8), let metadata = try JSONSerialization.jsonObject(with:data) as? [String:Any] else { throw SdaError.message("媒体元数据无效") }
   player.setMediaMetadata(hash,metadata)
  } }
  Function("renderingSettings") { try player.locked { try player.json(player.settings()) } }
  Function("hrtfStatus") { player.locked { player.hrtfState } }
  Function("playbackDiagnostics") { try player.locked { try player.json(player.playbackEvents) } }
  Function("playbackSnapshot") { (includeObjects:Bool) in try player.json(player.playbackSnapshot(includeObjects:includeObjects)) }
  Function("feedError") { player.locked { player.failure } }
  Function("feedDone") { player.locked { player.done } }
  Function("status") { try player.locked { try player.json(!player.hasPlayback ? [:] : player.command("status")) } }
  Function("objects") { try player.locked { try player.json(!player.hasPlayback ? [:] : player.command("objects")) } }
  Function("pause") { try player.locked { try player.setPaused(true) } }
  Function("resume") { try player.locked { try player.setPaused(false) } }
  Function("stop") { player.locked { player.stopNative(); return true } }
  Function("setHeadYaw") { (degrees: Double) in try player.locked { _ = try player.command("yaw",["degrees":degrees]); player.playbackYaw=degrees } }
  Function("resetHeadPose") { player.locked { player.recenterHeadTracking() } }
  Function("setHeadTracking") { (enabled: Bool) in player.locked { player.setHeadTracking(enabled) } }
  Function("headTrackingStatus") { player.locked { player.headTrackingDetail } }
  Function("configureDiagnostics") { (enabled:Bool) in player.locked { player.configureDiagnostics(enabled) } }
  Function("diagnosticsStatus") { player.diagnostics.status }
  Function("diagnosticsEnabled") { player.prefs.bool(forKey:"sda.diagnostics.enabled") }
  Function("setVolume") { (v: Double) in try player.locked { guard v.isFinite && v >= 0 && v <= 1 else { throw SdaError.message("音量无效") }; if player.hasPlayback { _ = try player.command("volume",["volume":v]) }; player.prefs.set(v,forKey:"sda.volume") } }
  Function("setListeningProfile") { (json: String) in try player.locked { try player.saveListeningProfile(json) } }
  Function("setSpatialEnhancement") { (enabled: Bool) in try player.locked { try player.saveSpatialEnhancement(enabled) } }
  Function("setVolumeBalance") { (v: Bool) in try player.locked { if player.hasPlayback { _ = try player.command("balance",["enabled":v]) }; player.prefs.set(v,forKey:"sda.balance") } }
  Function("setBinauralEnabled") { (enabled: Bool) in try player.locked {
   if player.handle != nil { _ = try player.command("binaural",["enabled":enabled]) }
   if !enabled { player.setHeadTracking(false) }
   player.prefs.set(enabled,forKey:"sda.binauralEnabled")
  } }
  Function("setObjectRendering") { (direct: Bool, directional: Bool) in try player.locked { if player.handle != nil { _ = try player.command("rendering",["direct":direct,"directional":directional]) }; player.prefs.set(direct,forKey:"sda.direct");player.prefs.set(directional,forKey:"sda.directional") } }
  Function("rooms") { try player.locked { try player.json(player.roomCatalog()) } }
  AsyncFunction("setNearField") { (enabled: Bool, scale: Double) in try player.locked { guard !enabled else { throw SdaError.message("移动端已移除近场渲染") }; guard scale.isFinite && scale >= 0.25 && scale <= 4 else { throw SdaError.message("近场距离映射无效") }; if player.handle != nil { _ = try player.command("near",["enabled":enabled,"scale":scale]) }; player.prefs.set(enabled,forKey:"sda.near"); player.prefs.set(scale,forKey:"sda.scale") } }
  AsyncFunction("setRoom") { (id: String) in try player.saveRoom(id) }
  AsyncFunction("setSpatialCueDb") { (db: Double) in try player.saveSpatialCueDb(db) }
  AsyncFunction("setRenderingPreset") { (id: String) in try player.selectRenderingPreset(id) }
  Function("configurePlaybackQueue") { (text:String,mode:String) in try player.locked {
   guard ["sequence","repeat-all","repeat-one"].contains(mode),
    let data=text.data(using:.utf8), let queue=try JSONSerialization.jsonObject(with:data) as? [[String:Any]],
    queue.allSatisfy({ ($0["hash"] as? String) != nil && ($0["name"] as? String) != nil && URL(string:$0["uri"] as? String ?? "")?.isFileURL == true })
    else { throw SdaError.message("播放队列无效") }
   // Fix protection while foreground/import still has access, not on a locked-screen EOF.
   for track in queue { if let uri=track["uri"] as? String, let url=URL(string:uri) { try CompressedInput.allowLockedPlaybackOfOwnedCopy(url) } }
   player.uncommittedRetained.subtract(queue.compactMap { $0["uri"] as? String })
   player.playbackQueue=queue; player.playbackMode=mode
   player.retainProgrammeReferences()
  } }
  Function("playbackQueueSnapshot") { try player.locked { try player.json(["queue":player.playbackQueue,"mode":player.playbackMode,"currentHash":player.trackHash]) } }
  AsyncFunction("retainQueueTrack") { (uri:String,name:String,hash:String) in
   guard let source=URL(string:uri) else { throw SdaError.message("文件地址无效") }
   return try PlaybackStorage.retain(source,name:name,hash:hash,didCreate:{ url in _ = player.locked { player.uncommittedRetained.insert(url.absoluteString) } }).absoluteString
  }
  AsyncFunction("discardPickerCopy") { (uri:String) in if let url=URL(string:uri) { PlaybackStorage.discardPicker(url) } }
  AsyncFunction("discardUnqueuedTrack") { (uri:String) in player.locked {
   guard let url=URL(string:uri),url.isFileURL else { return }
   let actual=url.resolvingSymlinksInPath().standardizedFileURL
   guard actual.deletingLastPathComponent() == PlaybackStorage.library.resolvingSymlinksInPath().standardizedFileURL,
    actual.lastPathComponent != "queue.json",player.uncommittedRetained.contains(uri), !player.playbackQueue.contains(where: { $0["uri"] as? String == uri }) else { return }
   try? FileManager.default.removeItem(at:actual)
   player.uncommittedRetained.remove(uri)
  } }
  Function("currentTrackHash") { player.locked { player.trackHash } }
  AsyncFunction("playUri") { (uri:String,name:String,yaw:Double,hash:String) in try player.play(uri,name,yaw,hash) }
  AsyncFunction("seekTo") { (positionMs:Double,hash:String) in try player.requestSeek(positionMs,expectedHash:hash) }
  AsyncFunction("durationMs") { (uri:String) in guard let u = URL(string:uri) else { return 0.0 }; return player.mediaDuration(u) }
  AsyncFunction("metadata") { (uri:String) in
   guard let u = URL(string:uri) else { throw SdaError.message("文件地址无效") }
   let scoped=u.startAccessingSecurityScopedResource();defer {if scoped {u.stopAccessingSecurityScopedResource()}}
   if u.isFileURL, let sdar=try player.decodeReply(sda_ios_sdar_metadata(u.path)) as? [String:Any] {
    if let uri=sdar["coverUri"] as? String, let comma=uri.firstIndex(of:","),let data=Data(base64Encoded:String(uri[uri.index(after:comma)...])),let color=ArtworkColor.bottomBand(data){var result=sdar;result["artworkColor"]=color;return try player.json(result)}
    return try player.json(sdar)
   }
   let asset = AVURLAsset(url:u); var result: [String:Any] = ["durationMs":player.mediaDuration(u)]
   for item in asset.commonMetadata { if item.commonKey == .commonKeyTitle { result["title"] = item.stringValue }; if item.commonKey == .commonKeyArtist { result["artist"] = item.stringValue }; if item.commonKey == .commonKeyAlbumName { result["album"] = item.stringValue }; if item.commonKey == .commonKeyArtwork, let data = item.dataValue { result["coverUri"] = "data:image/jpeg;base64,"+data.base64EncodedString(); if let color = ArtworkColor.bottomBand(data) { result["artworkColor"] = color } } }
   return try player.json(result)
  }
  AsyncFunction("prepareStereoTrack") { (uri:String,name:String,hash:String) in
   try player.prepareStereoTrack(uri,name,hash)
   return true
  }
  AsyncFunction("contentHash") { (uri:String) in
   guard let u = URL(string:uri) else { throw SdaError.message("文件地址无效") }; let scoped=u.startAccessingSecurityScopedResource();defer { if scoped { u.stopAccessingSecurityScopedResource() } }
   let file=try FileHandle(forReadingFrom:u);defer { try? file.close() };var hash=SHA256()
   while let data=try file.read(upToCount:256*1024), !data.isEmpty { hash.update(data:data) }
   return hash.finalize().map { String(format:"%02x",$0) }.joined()
  }
  Function("mp4ImportChunkBytes") { 1024*1024 }
  AsyncFunction("beginMp4Import") { (uri:String) in try player.locked {
   guard let u=URL(string:uri),u.isFileURL else { throw SdaError.message("文件地址无效") }
   let token=UUID().uuidString;let out=FileManager.default.temporaryDirectory.appendingPathComponent("sda-import-"+token+".mhas")
   let session=try Mp4ImportSession(source:u,output:out);player.imports[token]=session
   return try player.json(["token":token,"size":session.size])
  } }
  AsyncFunction("extractMp4ImportNative") { (token:String) in try player.locked {
   guard let session=player.imports[token] else {throw SdaError.message("导入任务不存在")}
   return try session.extractNative(player.decodeReply)
  } }
  AsyncFunction("readMp4Import") { (token:String,offset:Double,count:Int) in try player.locked {
   guard let session=player.imports[token] else {throw SdaError.message("导入任务不存在")}
   return try session.read(offset:offset,count:count)
  } }
  AsyncFunction("appendMp4Import") { (token:String,bytes:String) in try player.locked {
   guard let session=player.imports[token] else {throw SdaError.message("导入任务不存在")};try session.append(bytes)
  } }
  AsyncFunction("finishMp4Import") { (token:String) in try player.locked {guard let session=player.imports[token] else {throw SdaError.message("导入任务不存在")};return try session.finish()} }
  AsyncFunction("discardMp4Import") { (token:String) in player.locked {player.imports.removeValue(forKey:token)?.discard()} }
 }
}
