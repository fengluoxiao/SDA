import ExpoModulesCore
import Foundation
import AVFoundation
import CryptoKit

public final class SdaModule: Module {
 private let player = SdaPlayer()
 public func definition() -> ModuleDefinition {
  let player = self.player
  Name("SdaEngine")
  OnCreate {
   player.installSystemControls()
   if ProcessInfo.processInfo.environment["SDA_IOS_SMOKE"] == "1" {
    DispatchQueue.global(qos:.userInitiated).async { [weak player] in player?.runCISmoke() }
   }
  }
  OnDestroy { player.locked { player.stopNative(); for o in player.observers { NotificationCenter.default.removeObserver(o) }; for (c,t) in player.remoteTargets { c.removeTarget(t) }; for pair in player.imports.values { try? FileManager.default.removeItem(at:pair.1) }; player.imports.removeAll() } }
  Function("set360RaSystemSpatialAudio") { (enabled: Bool) in player.locked { player.prefs.set(enabled,forKey:"sda.systemSpatial360RA"); return true } }
  Function("setNowPlayingMetadata") { (hash: String, text: String) in try player.locked {
   guard let data = text.data(using:.utf8), let metadata = try JSONSerialization.jsonObject(with:data) as? [String:Any] else { throw SdaError.message("媒体元数据无效") }
   player.setMediaMetadata(hash,metadata)
  } }
  Function("renderingSettings") { try player.locked { try player.json(player.settings()) } }
  Function("hrtfStatus") { player.locked { player.hrtfState } }
  Function("feedError") { player.locked { player.failure } }
  Function("feedDone") { player.locked { player.done } }
  Function("status") { try player.locked { try player.json(!player.hasPlayback ? [:] : player.command("status")) } }
  Function("objects") { try player.locked { try player.json(!player.hasPlayback ? [:] : player.command("objects")) } }
  Function("pause") { try player.locked { try player.setPaused(true) } }
  Function("resume") { try player.locked { try player.setPaused(false) } }
  Function("stop") { player.locked { player.stopNative(); return true } }
  Function("setHeadYaw") { (degrees: Double) in try player.locked { _ = try player.command("yaw",["degrees":degrees]) } }
  Function("resetHeadPose") { try player.locked { _ = try player.command("resetPose") } }
  Function("setVolume") { (v: Double) in try player.locked { guard v.isFinite && v >= 0 && v <= 1 else { throw SdaError.message("音量无效") }; if player.hasPlayback { _ = try player.command("volume",["volume":v]) }; player.prefs.set(v,forKey:"sda.volume") } }
  Function("setVolumeBalance") { (v: Bool) in try player.locked { if player.hasPlayback { _ = try player.command("balance",["enabled":v]) }; player.prefs.set(v,forKey:"sda.balance") } }
  Function("setObjectRendering") { (direct: Bool, directional: Bool) in try player.locked { if player.handle != nil { _ = try player.command("rendering",["direct":direct,"directional":directional]) }; player.prefs.set(direct,forKey:"sda.direct");player.prefs.set(directional,forKey:"sda.directional") } }
  Function("rooms") { try player.json(player.roomCatalog().compactMap { $0["summary"] }) }
  AsyncFunction("setNearField") { (enabled: Bool, scale: Double) in try player.locked { guard scale.isFinite && scale >= 0.25 && scale <= 4 else { throw SdaError.message("近场距离映射无效") }; if player.handle != nil { _ = try player.command("near",["enabled":enabled,"scale":scale]) }; player.prefs.set(enabled,forKey:"sda.near"); player.prefs.set(scale,forKey:"sda.scale") } }
  AsyncFunction("setRoom") { (id: String) in try player.locked { try player.saveRoom(id) } }
  AsyncFunction("setRenderingPreset") { (id: String) in try player.locked {
   let data = try Data(contentsOf:player.assetRoot().appendingPathComponent("rendering-presets.json"))
   let presets = try JSONSerialization.jsonObject(with:data) as! [[String:Any]]
   guard let p = presets.first(where:{$0["id"] as? String == id}) else { throw SdaError.message("未知渲染预设") }
   if player.handle != nil { _ = try player.command("preset",["path":try player.hrtfPath(p["hrtfSet"] as! String),"wet":p["hrtfWetWeight"]!,"direct":p["direct"]!,"directional":p["directional"]!]); _ = try player.command("near",["enabled":false,"scale":1.0]); _ = try player.command("room",["path":""]) }
   for (k,v) in [("sda.hrtfSet",p["hrtfSet"]!),("sda.wet",p["hrtfWetWeight"]!),("sda.direct",p["direct"]!),("sda.directional",p["directional"]!),("sda.near",false),("sda.room","")] as [(String,Any)] { player.prefs.set(v,forKey:k) }
   player.prefs.set("",forKey:"sda.room."+player.layout)
   if player.systemSpatial == nil { player.hrtfState = (p["label"] as? String ?? "KU100")+" · 完整 HRTF" }
  } }
  AsyncFunction("playUri") { (uri:String,name:String,yaw:Double,hash:String) in try player.play(uri,name,yaw,hash) }
  AsyncFunction("durationMs") { (uri:String) in guard let u = URL(string:uri) else { return 0.0 }; return player.mediaDuration(u) }
  AsyncFunction("metadata") { (uri:String) in
   guard let u = URL(string:uri) else { throw SdaError.message("文件地址无效") }
   let asset = AVURLAsset(url:u); var result: [String:Any] = ["durationMs":player.mediaDuration(u)]
   for item in asset.commonMetadata { if item.commonKey == .commonKeyTitle { result["title"] = item.stringValue }; if item.commonKey == .commonKeyArtist { result["artist"] = item.stringValue }; if item.commonKey == .commonKeyAlbumName { result["album"] = item.stringValue }; if item.commonKey == .commonKeyArtwork, let data = item.dataValue { result["coverUri"] = "data:image/jpeg;base64,"+data.base64EncodedString() } }
   return try player.json(result)
  }
  AsyncFunction("contentHash") { (uri:String) in
   guard let u = URL(string:uri) else { throw SdaError.message("文件地址无效") }; let scoped=u.startAccessingSecurityScopedResource();defer { if scoped { u.stopAccessingSecurityScopedResource() } }
   let file=try FileHandle(forReadingFrom:u);defer { try? file.close() };var hash=SHA256()
   while let data=try file.read(upToCount:256*1024), !data.isEmpty { hash.update(data:data) }
   return hash.finalize().map { String(format:"%02x",$0) }.joined()
  }
  AsyncFunction("beginMp4Import") { (uri:String) in try player.locked {
   guard let u=URL(string:uri),u.isFileURL else { throw SdaError.message("文件地址无效") }
   let token=UUID().uuidString;let out=FileManager.default.temporaryDirectory.appendingPathComponent(token+".mhas")
   FileManager.default.createFile(atPath:out.path,contents:Data());player.imports[token]=(u,out)
   let size=(try FileManager.default.attributesOfItem(atPath:u.path)[.size] as? NSNumber)?.int64Value ?? 0
   return try player.json(["token":token,"size":size])
  } }
  AsyncFunction("readMp4Import") { (token:String,offset:Double,count:Int) in try player.locked {
   guard let pair=player.imports[token], offset.isFinite && offset >= 0 && count > 0 && count <= 256*1024 else { throw SdaError.message("导入参数无效") }
   let f=try FileHandle(forReadingFrom:pair.0);defer { try? f.close() };try f.seek(toOffset:UInt64(offset));return try f.read(upToCount:count)?.base64EncodedString() ?? ""
  } }
  AsyncFunction("appendMp4Import") { (token:String,bytes:String) in try player.locked {
   guard let pair=player.imports[token],let data=Data(base64Encoded:bytes),data.count <= 256*1024 else { throw SdaError.message("导入参数无效") }
   let f=try FileHandle(forWritingTo:pair.1);defer { try? f.close() };try f.seekToEnd();try f.write(contentsOf:data)
  } }
  AsyncFunction("finishMp4Import") { (token:String) in try player.locked { guard let pair=player.imports[token] else { throw SdaError.message("导入任务不存在") };return pair.1.absoluteString } }
  AsyncFunction("discardMp4Import") { (token:String) in player.locked { if let pair=player.imports.removeValue(forKey:token) { try? FileManager.default.removeItem(at:pair.1) } } }
 }
}
