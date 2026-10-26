import Foundation
import Network
import AVFoundation

// Opt-in read-only server; never uploads or accepts playback commands.
final class RemoteDiagnostics {
 private let queue = DispatchQueue(label:"sda.diagnostics",qos:.utility)
 private let snapshotCache=DiagnosticSnapshotCache()
 private var listener: NWListener?
 private var clients: [UUID:NWConnection] = [:]
 private var epoch = 0
 private let statusLock = NSLock()
 private var detail = "已关闭"
 var status: String { statusLock.lock(); defer { statusLock.unlock() }; return detail }
 private func setStatus(_ text:String) { statusLock.lock(); detail=text; statusLock.unlock() }
 func configure(enabled:Bool,snapshot:@escaping () -> [String:Any]) {
  queue.async {
   self.epoch += 1; let epoch=self.epoch
   self.snapshotCache.reset()
   self.listener?.cancel(); self.listener=nil
   for client in self.clients.values { client.cancel() }; self.clients.removeAll()
   guard enabled else { self.setStatus("已关闭"); return }
   do {
    let listener=try NWListener(using:.tcp,on:8765)
    self.listener=listener; self.setStatus("正在启动")
    listener.stateUpdateHandler = { [weak self] state in
     guard let self, self.epoch == epoch else { return }
     switch state {
     case .ready: self.setStatus("服务已开启 · 端口 8765 · /diagnostics")
     case .failed: self.setStatus("监听失败 · 请关闭后重新开启"); listener.cancel()
     default: break
     }
    }
    listener.newConnectionHandler = { [weak self] connection in
     guard let self, self.epoch == epoch, self.clients.count < 4 else { connection.cancel(); return }
     let id=UUID(); self.clients[id]=connection
     connection.stateUpdateHandler = { [weak self] state in
      if case .failed = state { self?.close(id) }
      if case .cancelled = state { self?.clients.removeValue(forKey:id) }
     }
     connection.start(queue:self.queue)
     self.receive(connection,id:id,epoch:epoch,data:Data(),snapshot:snapshot)
     self.queue.asyncAfter(deadline:.now()+5) { [weak self] in self?.close(id) }
    }
    listener.start(queue:self.queue)
   } catch { self.setStatus("监听失败") }
  }
 }
 private func close(_ id:UUID) { clients.removeValue(forKey:id)?.cancel() }
 private func receive(_ connection:NWConnection,id:UUID,epoch:Int,data:Data,snapshot:@escaping () -> [String:Any]) {
  connection.receive(minimumIncompleteLength:1,maximumLength:4096) { [weak self] bytes, _, complete, error in
   guard let self, self.epoch == epoch, self.clients[id] != nil else { return }
   var data=data; if let bytes { data.append(bytes) }
   guard error == nil, data.count <= 8192 else { self.close(id); return }
   guard data.range(of:Data("\r\n\r\n".utf8)) != nil else {
    if complete { self.close(id) } else { self.receive(connection,id:id,epoch:epoch,data:data,snapshot:snapshot) }
    return
   }
   let parts=String(decoding:data,as:UTF8.self).components(separatedBy:"\r\n")[0].split(separator:" ")
   var code="400 Bad Request"; var body=Data("{}".utf8)
   if parts.count == 3 {
    if parts[0] != "GET" { code="405 Method Not Allowed" }
    else if parts[1] != "/diagnostics" && parts[1] != "/" { code="404 Not Found" }
    else if let json=try? JSONSerialization.data(withJSONObject:self.snapshotCache.read(capture:snapshot)) { code="200 OK"; body=json }
    else { code="500 Internal Server Error" }
   }
   var response=Data("HTTP/1.1 \(code)\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: \(body.count)\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n".utf8)
   response.append(body)
   connection.send(content:response,completion:.contentProcessed { [weak self] _ in self?.close(id) })
  }
 }
}

extension SdaPlayer {
 func configureDiagnostics(_ enabled:Bool) {
  prefs.set(enabled,forKey:"sda.diagnostics.enabled")
  diagnostics.configure(enabled:enabled) { [weak self] in
   guard let self else { return [:] }
   return self.locked {
    let status=(self.hasPlayback ? (try? self.command("status")) as? [String:Any] : nil) ?? [:]
    var playback:[String:Any]=[:]
    for key in ["positionMs","fifoFrames","decodedSamplePos","consumedSamplePos","sourceCodec","outputChannels","outputLayout","underruns","callbackMissingFrames","renderBlocks","renderTotalMicros","renderMaxMicros","renderWorstStagesMicros","rendererRevision","spatialBalanceAlgorithm","liveLoudnessDucking","rendererConfiguration","stereoBassPreservationActive","stereoBassPreservationAlgorithm","fftBackend","accelerators","programmeBalancePolicy", "volumeBalanceEnabled","volumeBalanceEligible","programGainDb","renderedProgramGainDb","rendererProgramEnabled","spatialEnhancementEnabled","auxiliaryBalanceEnabled","auxiliaryBalanceLevels","temporalDetailEnabled","temporalDetailLevels"] { playback[key]=status[key] }
    return ["schema":1,"time":Date().timeIntervalSince1970,"playback":playback,
     "appVersion":Bundle.main.object(forInfoDictionaryKey:"CFBundleShortVersionString") ?? "",
     "appBuild":Bundle.main.object(forInfoDictionaryKey:"CFBundleVersion") ?? "",
     "volumeBalancePreference":self.prefs.bool(forKey:"sda.balance"),
     "systemSpatialPreference":self.prefs.bool(forKey:"sda.systemSpatial360RA"),
     "playing":self.hasPlayback && !self.done,"paused":self.isPaused,"background":self.inBackground,
     "generation":self.generation,"route":self.systemSpatial == nil ? "SDA-HRTFHQ" : "Apple",
     "thermalState":ProcessInfo.processInfo.thermalState.rawValue,"engineRunning":self.audio?.isRunning ?? false,
     "tracking":self.headTrackingDetail,"motion":self.diagnosticMotion,
     "backgroundTrackingEnabled":self.prefs.bool(forKey:"sda.headTracking"),
     "error":self.failure == nil ? "" : "playback-error",
     "errorDetails":self.failureDiagnostic,"input":self.compressedInputDiagnostic,
     "events":self.playbackEvents.suffix(16).map { ["time":$0["time"] ?? 0,"event":$0["event"] ?? "","generation":$0["generation"] ?? 0] }]
   }
  }
 }
}

// Expose stable categories and numeric codes, never error descriptions or URLs.
extension SdaPlayer {
 static func safePlaybackFailure(_ error: Error) -> [String:Any] {
  var errors=[NSError]();var current:NSError?=error as NSError
  for _ in 0..<4 { guard let value=current else { break };errors.append(value);current=value.userInfo[NSUnderlyingErrorKey] as? NSError }
  let text=String(describing:error);var category="playback-feed"
  if errors.contains(where: { $0.domain == AVFoundationErrorDomain && $0.code == -11880 }) { category="invalid-sample-cursor" }
  else if errors.contains(where: { $0.domain == AVFoundationErrorDomain && $0.code == -11847 }) { category="asset-reader-interrupted" }
  else if text.contains("no decoded audio") || text.contains("没有解码出任何音频") { category="no-decoded-audio" }
  else if text.contains("停止消耗数据") { category="audio-output-no-progress" }
  else if text.contains("PCM frame acknowledgement timed out") { category="native-pcm-ack-timeout" }
  else if text.contains("command queue full") { category="native-command-queue-full" }
  else if text.contains("object metadata timeline full") { category="native-object-timeline-full" }
  else if text.contains("MP4 压缩包索引无效") { category="container-continuation-unverified" }
  let allowed=[AVFoundationErrorDomain,NSOSStatusErrorDomain,NSCocoaErrorDomain,NSPOSIXErrorDomain]
  let codes=errors.map { value in ["domain":allowed.contains(value.domain) ? value.domain : "app","code":value.code] as [String:Any] }
  return ["category":category,"errors":codes,"time":Date().timeIntervalSince1970]
 }
}
