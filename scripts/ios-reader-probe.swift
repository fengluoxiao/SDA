import Foundation
import AVFoundation
import CryptoKit

enum SdaError: LocalizedError {
 case message(String)
 var errorDescription: String? { if case let .message(s) = self { return s }; return nil }
}

@main struct ReaderProbe {
 static var phase="startup"
 static var inspected:CompressedInput?
 static func checkpoint(_ value:String) { phase=value;FileHandle.standardError.write(Data(("CHECK: " + value + "\n").utf8)) }
 static func main() throws {
  do { try run() }
  catch {
   FileHandle.standardError.write(Data(("FAILED: \(phase), input=\(inspected?.diagnosticState ?? [:]), error=\(error)\n").utf8));throw error
  }
 }
 static func run() throws {
  let fixture = URL(fileURLWithPath: CommandLine.arguments[1])
  let expected = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[2]))
  precondition(CompressedInput.isRecoverableReaderError(NSError(domain:AVFoundationErrorDomain,code:-11880)))
  precondition(CompressedInput.isRecoverableReaderError(NSError(domain:AVFoundationErrorDomain,code:-11847)))
  precondition(!CompressedInput.isRecoverableReaderError(NSError(domain:AVFoundationErrorDomain,code:-11828)))
  precondition(!CompressedInput.isRecoverableReaderError(NSError(domain:NSOSStatusErrorDomain,code:-12551)))
   precondition(CompressedInput.requiresSequentialRecovery(NSError(domain:AVFoundationErrorDomain,code:-11880)))
   precondition(!CompressedInput.requiresSequentialRecovery(NSError(domain:AVFoundationErrorDomain,code:-11847)))
  checkpoint("normal-full-read")
  let input = try CompressedInput(url: fixture, name: fixture.lastPathComponent)
  inspected=input
  precondition(input.nativeCodec == "eac3","MP4 must keep its declared codec")
  var actual = Data(); var packets = 0
  while let packet = try input.next() { actual.append(packet); packets += 1 }
  guard actual == expected else { throw SdaError.message("Compressed MP4 changed bytes: \(actual.count) vs \(expected.count)") }
  checkpoint("ranged-resume")
  let resumed = try CompressedInput(url: fixture, name: fixture.lastPathComponent)
  inspected=resumed
  var recovered = Data()
  for _ in 0..<3 { if let packet = try resumed.next() { recovered.append(packet) } }
  try resumed.reopenAfterInterruption()
  while let packet = try resumed.next() { recovered.append(packet) }
  guard recovered == expected else { throw SdaError.message("Resumed compressed cursor dropped or duplicated packets") }
  checkpoint("sequential-resume")
   let sequential = try CompressedInput(url: fixture, name: fixture.lastPathComponent)
  inspected=sequential
   var sequentialData = Data()
   for _ in 0..<3 { if let packet = try sequential.next() { sequentialData.append(packet) } }
   try sequential.reopenAfterInterruption(forceSequential:true)
   while let packet = try sequential.next() { sequentialData.append(packet) }
   guard sequentialData == expected else { throw SdaError.message("Sequential recovery dropped or duplicated compressed packets") }
  checkpoint("explicit-indexed-resume")
  let indexed = try CompressedInput(url:fixture,name:fixture.lastPathComponent)
  inspected=indexed
  var indexedData=Data()
  for _ in 0..<3 { if let packet=try indexed.next() { indexedData.append(packet) } }
  if indexed.diagnosticState["backend"] as? String != "indexed-mp4" {
   guard try indexed.recoverInvalidSampleCursor() else { throw SdaError.message("Indexed continuation unavailable") }
  }
  while let packet=try indexed.next() { indexedData.append(packet) }
  guard indexedData == expected else { throw SdaError.message("Indexed continuation dropped or duplicated packets") }
  checkpoint("early-completion")
  let early = try CompressedInput(url:fixture,name:fixture.lastPathComponent)
  inspected=early
  var earlyBytes=Data()
  for _ in 0..<3 { if let packet=try early.next() { earlyBytes.append(packet) } }
  if let packet=try early.nextAfterReaderCompletion() { earlyBytes.append(packet) }
  while let packet=try early.next() { earlyBytes.append(packet) }
  guard earlyBytes == expected else { throw SdaError.message("Early reader completion dropped the unread tail") }
  if CommandLine.arguments.count > 3 { try cursorFixtures(URL(fileURLWithPath:CommandLine.arguments[3])) }
  print("PASS: \(packets) compressed packets, \(actual.count) bytes exactly match raw E-AC-3; no Apple downmix")
 }
 static func cursorFixtures(_ root:URL) throws {
  let expected=try Data(contentsOf:root.appendingPathComponent("expected.eac3"))
  let manifest=try JSONSerialization.jsonObject(with:Data(contentsOf:root.appendingPathComponent("manifest.json"))) as! [String:Any]
  let sizes=manifest["sizes"] as! [Int]
  var checks:[[String:Any]]=[]
  for name in ["cursor32.m4a","cursor64.m4a","cursor-clipped-tail.m4a"] {
   for start in [0,2] {
    checkpoint("manual-index \(name) start=\(start)")
    guard let reader=try IndexedCompressedInput.open(root.appendingPathComponent(name)) else { throw SdaError.message("Synthetic EC3 index not found") }
    let submitted=9224; let firstByte=sizes.prefix(start).reduce(0,+); let endByte=sizes.prefix(start+submitted).reduce(0,+)
    let prefix=expected.subdata(in:firstByte..<endByte)
    try reader.verifyContinuation(firstPTS:CMTime(value:Int64(start*1536-3072),timescale:48000),lastEnd:CMTime(value:Int64((start+submitted)*1536-3072),timescale:48000),submittedBytes:UInt64(prefix.count),digest:Data(SHA256.hash(data:prefix)))
    var tail=Data();while let packet=try reader.next(chunkBytes:24576) { tail.append(packet) }
    guard tail == expected.subdata(in:endByte..<expected.count),reader.verifiedPrefixBytes==UInt64(prefix.count),reader.sampleCount==9270 else { throw SdaError.message("Invalid-cursor tail lost/repeated bytes") }
    checks.append(["container":name,"firstSample":start,"verifiedPrefixBytes":prefix.count,"tailBytes":tail.count,"fullTailByteIdentical":true])
   }
  }
  checkpoint("partial-preroll-with-edited-timestamp")
  guard let partial=try IndexedCompressedInput.open(root.appendingPathComponent("cursor32.m4a")) else { throw SdaError.message("Missing preroll fixture") }
  let partialStart=sizes[0], partialEnd=sizes.prefix(9225).reduce(0,+)
  let partialPrefix=expected.subdata(in:partialStart..<partialEnd)
  try partial.verifyContinuation(firstPTS:CMTime(value:0,timescale:48000),lastEnd:CMTime(value:Int64(9224*1536),timescale:48000),submittedBytes:UInt64(partialPrefix.count),digest:Data(SHA256.hash(data:partialPrefix)))
  var partialTail=Data();while let packet=try partial.next(chunkBytes:24576) { partialTail.append(packet) }
  guard partial.recoveryStartSample == 1, partialTail == expected.suffix(from:partialEnd) else { throw SdaError.message("Partial preroll skipped or repeated compressed packets") }
  // A valid edit clips the last CMSampleBuffer duration; unique byte hashes
  // still prove the exact continuation even when end-time equality fails.
  checkpoint("clamped-timestamp")
  guard let clipped=try IndexedCompressedInput.open(root.appendingPathComponent("cursor32.m4a")) else { throw SdaError.message("Missing clip fixture") }
  let prefixCount=sizes.prefix(9224).reduce(0,+)
  try clipped.verifyContinuation(firstPTS:CMTime(value:-3072,timescale:48000),lastEnd:CMTime(value:Int64(9224*1536-3072-766),timescale:48000),submittedBytes:UInt64(prefixCount),digest:Data(SHA256.hash(data:expected.prefix(prefixCount))))
  var clippedTail=Data();while let packet=try clipped.next(chunkBytes:24576) { clippedTail.append(packet) }
  guard clippedTail == expected.suffix(from:prefixCount) else { throw SdaError.message("Clamped timestamp dropped or repeated the tail") }
  for name in ["cursor32.m4a","cursor64.m4a","cursor-clipped-tail.m4a"] {
   checkpoint("automatic-index \(name)")
   let input=try CompressedInput(url:root.appendingPathComponent(name),name:name)
   inspected=input
   var bytes=Data();while let packet=try input.next() { bytes.append(packet) }
   let start=input.diagnosticState["recoveryStartSample"] as? Int ?? -1
   guard input.diagnosticState["indexedPrepared"] as? Bool == true, start >= 0,
    bytes == expected.suffix(from:sizes.prefix(start).reduce(0,+)) else { throw SdaError.message("Prepared indexed playback changed the full compressed stream") }
   checks.append(["container":name,"preparedBeforeTail":true,"firstSample":start,"fullTrackBytes":bytes.count])
  }
  guard let ambiguous=try IndexedCompressedInput.open(root.appendingPathComponent("ambiguous.m4a")) else { throw SdaError.message("Missing ambiguity fixture") }
  var ambiguityRejected=false
  do { try ambiguous.verifyContinuation(firstPTS:CMTime(value:0,timescale:48000),lastEnd:nil,submittedBytes:UInt64(sizes[0]),digest:Data(SHA256.hash(data:Data(repeating:0,count:sizes[0])))) } catch { ambiguityRejected=true }
  guard ambiguityRejected else { throw SdaError.message("Ambiguous byte prefix was accepted") }
  guard let bad=try IndexedCompressedInput.open(root.appendingPathComponent("cursor32.m4a")) else { throw SdaError.message("Missing negative fixture") }
  var rejected=false
  do { try bad.verifyContinuation(firstPTS:CMTime(value:-3072,timescale:48000),lastEnd:nil,submittedBytes:UInt64(sizes[0]),digest:Data(repeating:0,count:32)) } catch { rejected=true }
  guard rejected else { throw SdaError.message("Mismatched prefix was accepted") }
  rejected=false
  do { _ = try IndexedCompressedInput.open(root.appendingPathComponent("truncated.m4a")) } catch { rejected=true }
  guard rejected else { throw SdaError.message("Truncated media was treated as EOF") }
  let report:[String:Any] = ["ok":true,"cases":checks,"hashMismatchRejected":true,"truncationRejected":true,"partialPrerollVerified":true,"clampedTimestampTailVerified":true,"ambiguityRejected":true,"earlyCompletionTailVerified":true,"syntheticOnly":true]
  print(String(decoding:try JSONSerialization.data(withJSONObject:report,options:[.sortedKeys]),as:UTF8.self))
 }

}
