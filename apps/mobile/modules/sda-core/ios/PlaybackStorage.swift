import Foundation
import CryptoKit

/// File ownership, not file age, decides what may be removed. Original and legacy library audio is never swept; new working copies are session-owned.
final class PlaybackStorage {
 static let fm = FileManager.default
 static let startupLock = NSLock()
 static var swept = false
 static let sessionName="sda-session-"+String(ProcessInfo.processInfo.processIdentifier)+"-"+UUID().uuidString.lowercased()
 static var legacyLibrary:URL {fm.urls(for:.applicationSupportDirectory,in:.userDomainMask)[0].appendingPathComponent("playback-library",isDirectory:true)}
 static var sessions:URL {fm.urls(for:.applicationSupportDirectory,in:.userDomainMask)[0].appendingPathComponent("playback-sessions",isDirectory:true)}
 static var library:URL {sessions.appendingPathComponent(sessionName,isDirectory:true)}
 static var queueFile: URL { legacyLibrary.appendingPathComponent("queue.json") }
 // Read legacy rows only to protect owned songs during migration, never to resume UI.
 static func retireLegacyQueue(at file:URL) -> [String:Any] {
  defer { try? fm.removeItem(at:file) }
  guard let data=try? Data(contentsOf:file), let value=(try? JSONSerialization.jsonObject(with:data)) as? [String:Any] else { return [:] }
  return value
 }
 static func ownedPicker(_ url:URL) -> Bool {
  guard url.isFileURL else { return false }
  let picker=fm.urls(for:.cachesDirectory,in:.userDomainMask)[0].appendingPathComponent("DocumentPicker").resolvingSymlinksInPath().standardizedFileURL
  return url.resolvingSymlinksInPath().standardizedFileURL.deletingLastPathComponent() == picker
 }
 static func discardPicker(_ url:URL) {
  if ownedPicker(url) { try? fm.removeItem(at:url) }
 }
 static func retain(_ source:URL, name:String, hash:String, didCreate:((URL)->Void)? = nil) throws -> URL {
  guard source.isFileURL, hash.range(of:"^[0-9a-f]{64}$",options:.regularExpression) != nil else { throw SdaError.message("音频文件标识无效") }
  try fm.createDirectory(at:library,withIntermediateDirectories:true)
  let ext=String((name as NSString).pathExtension.lowercased().filter { $0.isASCII && ($0.isLetter || $0.isNumber) }.prefix(8))
  let target=library.appendingPathComponent(hash+"."+(ext.isEmpty ? "audio" : ext))
  let actual=source.resolvingSymlinksInPath().standardizedFileURL
  if actual == target.resolvingSymlinksInPath().standardizedFileURL { return target }
  let scratch=fm.temporaryDirectory.resolvingSymlinksInPath().standardizedFileURL
  let owned=ownedPicker(source) || (actual.deletingLastPathComponent() == scratch && actual.lastPathComponent.hasPrefix("sda-import-"))
  // Keep the source until the destination has been installed successfully.
  if !fm.fileExists(atPath:target.path) {
   let pending=library.appendingPathComponent("pending-"+UUID().uuidString)
   defer { try? fm.removeItem(at:pending) }
   try fm.copyItem(at:source,to:pending)
   #if os(iOS)
   try fm.setAttributes([.protectionKey:FileProtectionType.completeUntilFirstUserAuthentication],ofItemAtPath:pending.path)
   #endif
   try fm.moveItem(at:pending,to:target)
   didCreate?(target)
  }
  if owned { try? fm.removeItem(at:source) }
  return target
 }
 static func startup() {
  startupLock.lock(); defer { startupLock.unlock() }
  guard !swept else { return }; swept=true
  let rows=retireLegacyQueue(at:queueFile)["queue"] as? [[String:Any]] ?? []
  let protected=rows.compactMap { ($0["uri"] as? String).flatMap(URL.init(string:)) }.map { $0.resolvingSymlinksInPath().standardizedFileURL.path }
  cleanPreviousSessions(sessions,keep:library)
  cleanAbandoned([fm.temporaryDirectory,fm.urls(for:.cachesDirectory,in:.userDomainMask)[0]],protected:protected)
  for directory in [legacyLibrary,library] {
   for url in (try? fm.contentsOfDirectory(at:directory,includingPropertiesForKeys:nil)) ?? [] where url.lastPathComponent.hasPrefix("pending-") {
    if (try? url.resourceValues(forKeys:[.isRegularFileKey,.isSymbolicLinkKey]).isRegularFile)==true,
     (try? url.resourceValues(forKeys:[.isSymbolicLinkKey]).isSymbolicLink) != true {try? fm.removeItem(at:url)}
   }
  }
  // Old iOS imports had no ownership table. Merge only byte-identical picker
  // copies once; preserve every unique song and every saved queue URI.
  let key="sda.storage.legacyPickerDedup.v1"
  if !UserDefaults.standard.bool(forKey:key) {
   let picker=fm.urls(for:.cachesDirectory,in:.userDomainMask)[0].appendingPathComponent("DocumentPicker",isDirectory:true)
   let candidates=(try? fm.contentsOfDirectory(at:picker,includingPropertiesForKeys:nil)) ?? []
   DispatchQueue.global(qos:.utility).async {
    if deduplicatePicker(picker,candidates:candidates,protected:protected) { UserDefaults.standard.set(true,forKey:key) }
   }
  }
 }
 /// Only our UUID session folders are disposable; originals and legacy library
 /// songs remain outside this boundary. Module reloads do not run startup again.
 static func cleanPreviousSessions(_ base:URL,keep:URL?) {
  guard (try? base.resourceValues(forKeys:[.isSymbolicLinkKey]).isSymbolicLink) != true else {return}
  let root=base.resolvingSymlinksInPath().standardizedFileURL
  let kept=keep?.resolvingSymlinksInPath().standardizedFileURL.path
  for url in (try? fm.contentsOfDirectory(at:base,includingPropertiesForKeys:[.isDirectoryKey,.isSymbolicLinkKey])) ?? [] {
   guard url.lastPathComponent.range(of:"^sda-session-[0-9]+-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",options:.regularExpression) != nil,
    (try? url.resourceValues(forKeys:[.isDirectoryKey]).isDirectory)==true,
    (try? url.resourceValues(forKeys:[.isSymbolicLinkKey]).isSymbolicLink) != true else {continue}
   let actual=url.resolvingSymlinksInPath().standardizedFileURL
   guard actual.deletingLastPathComponent()==root,actual.path != kept else {continue}
   var linked=false
   if let entries=fm.enumerator(at:url,includingPropertiesForKeys:[.isSymbolicLinkKey]){for case let child as URL in entries{if (try? child.resourceValues(forKeys:[.isSymbolicLinkKey]).isSymbolicLink)==true{linked=true;break}}}
   if !linked {try? fm.removeItem(at:url)}
  }
 }
 static func cleanAbandoned(_ bases:[URL],protected:[String]) {
  func remove(_ url:URL, base:URL) {
   guard let values=try? url.resourceValues(forKeys:[.isSymbolicLinkKey]),values.isSymbolicLink != true else { return }
   let path=url.resolvingSymlinksInPath().standardizedFileURL.path
   guard path.hasPrefix(base.resolvingSymlinksInPath().standardizedFileURL.path+"/"), !protected.contains(where: { $0 == path || $0.hasPrefix(path+"/") }) else { return }
   // Refuse trees containing links instead of following them during recursive removal.
   if let entries=fm.enumerator(at:url,includingPropertiesForKeys:[.isSymbolicLinkKey]) {
    for case let child as URL in entries { if (try? child.resourceValues(forKeys:[.isSymbolicLinkKey]).isSymbolicLink) == true { return } }
   }
   try? fm.removeItem(at:url)
  }
  let patterns=["^sda-format-[0-9]+-[0-9]+$","^sda-import-[0-9A-Fa-f-]{36}\\.mhas$","^[0-9A-Fa-f]{8}(?:-[0-9A-Fa-f]{4}){3}-[0-9A-Fa-f]{12}\\.mhas$"]
  for base in bases {
   for url in (try? fm.contentsOfDirectory(at:base,includingPropertiesForKeys:nil)) ?? [] {
    if patterns.contains(where: { url.lastPathComponent.range(of:$0,options:.regularExpression) != nil }) { remove(url,base:base) }
   }
  }
 }
 /// Migration only: full streaming hashes, never names/dates alone. The caller
 /// supplies a startup snapshot so new imports cannot join this sweep.
 static func deduplicatePicker(_ directory:URL,candidates:[URL],protected:[String]) -> Bool {
  let parent=directory.resolvingSymlinksInPath().standardizedFileURL
  guard (try? directory.resourceValues(forKeys:[.isSymbolicLinkKey]).isSymbolicLink) != true else { return false }
  let keep=Set(protected);var seen=[String:URL]();var complete=true
  let ordered=candidates.sorted { a,b in
   let x=keep.contains(a.resolvingSymlinksInPath().standardizedFileURL.path)
   let y=keep.contains(b.resolvingSymlinksInPath().standardizedFileURL.path)
   return x != y ? x : a.path < b.path
  }
  for url in ordered {
   do {
    guard let values=try? url.resourceValues(forKeys:[.isRegularFileKey,.isSymbolicLinkKey]),values.isRegularFile == true,values.isSymbolicLink != true,
     url.resolvingSymlinksInPath().standardizedFileURL.deletingLastPathComponent() == parent else { continue }
    let before=try fm.attributesOfItem(atPath:url.path)
    let handle=try FileHandle(forReadingFrom:url);defer { try? handle.close() }
    var digest=SHA256()
    while let bytes=try handle.read(upToCount:256*1024),!bytes.isEmpty { digest.update(data:bytes) }
    let after=try fm.attributesOfItem(atPath:url.path)
    guard (before[.size] as? NSNumber) == (after[.size] as? NSNumber),
     (before[.modificationDate] as? Date) == (after[.modificationDate] as? Date) else { complete=false;continue }
    let hash=digest.finalize().map { String(format:"%02x",$0) }.joined()
    let path=url.resolvingSymlinksInPath().standardizedFileURL.path
    if let survivor=seen[hash],fm.fileExists(atPath:survivor.path),!keep.contains(path) {
     // Verify path ownership again immediately before unlinking.
     guard (try? url.resourceValues(forKeys:[.isSymbolicLinkKey]).isSymbolicLink) != true,
      url.resolvingSymlinksInPath().standardizedFileURL.deletingLastPathComponent() == parent else { continue }
     try fm.removeItem(at:url)
    } else { seen[hash]=url }
   } catch { complete=false }
  }
  return complete
 }

}
