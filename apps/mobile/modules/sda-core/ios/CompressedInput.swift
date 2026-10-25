import Foundation
import AVFoundation
import AudioToolbox
import CoreMedia
import CryptoKit

/// Atmos stays compressed for SDA Rust. Only stereo ALAC is decoded to 48 kHz PCM.
final class CompressedInput {
 private(set) var nativeCodec: String?
 private(set) var isAlac = false
 private(set) var isAv3a = false
 private(set) var av3aSpeakerBed = false
 private var file: FileHandle?
 private var indexed: IndexedCompressedInput?
 private var preflightIndex: IndexedCompressedInput?
 private var indexPreflightAttempted = false
 private var submittedHash = SHA256()
 private var submittedCompressedBytes: UInt64 = 0
 private var firstCompressedPTS: CMTime?
 private(set) var indexedRecoveryCount = 0
 private var indexedRecoveryMillis: Double = 0
 private var indexedRecoveryAttempted = false
 private var indexedPreparationAttempted = false
 private var indexedPrepared = false
 private var indexedRecoveryFailure: String?
 private var lastReaderErrorCode: Int?
 private var selectedTrackID: CMPersistentTrackID?
 private var reader: AVAssetReader?
 private var trackOutput: AVAssetReaderTrackOutput?
 private var nextTime = CMTime.zero
 private var retries = 0
 private var resumeTimeValid = true
 private var resumedRange = false
 // Invalid-cursor recovery must not recreate the same failing ranged cursor.
 // Restart container iteration and discard already-submitted timestamps.
 private var sequentialRecovery = false
 private let url: URL
 private let scoped: Bool
 init(url: URL, name: String) throws {
  self.url = url
  scoped = url.startAccessingSecurityScopedResource()
  try Self.allowLockedPlaybackOfOwnedCopy(url)
  let ext = (name as NSString).pathExtension.lowercased()
  isAv3a = ext == "av3a"
  if ["eac3","ec3"].contains(ext) { nativeCodec="eac3" }
  else if ["ac4","av3a"].contains(ext) { nativeCodec=ext }
  if ["eac3", "ec3", "ac4", "mhas", "mp3", "av3a"].contains(ext) {
   let input = try FileHandle(forReadingFrom: url)
   file = input
   if isAv3a {
    av3aSpeakerBed = Self.av3aSpeakerBedHeader(try input.read(upToCount:9) ?? Data())
    try input.seek(toOffset:0)
   }
  } else if ["m4a", "mp4"].contains(ext) {
   try reopenAfterInterruption()
  } else { throw SdaError.message("支持立体声 ALAC、E-AC-3/Atmos M4A/MP4、MHAS、AV3A 和 MP3") }
 }
 // The Apple speaker route accepts the verified channel-based 7.1.4 profile.
 // Mix/HOA transport counts do not identify speaker layouts; keep their scene
 // metadata on the shared native path. Full framing/CRC validation stays in Rust.
 static func av3aSpeakerBedHeader(_ prefix: Data) -> Bool {
  let b = Array(prefix)
  guard b.count >= 7, b[0] == 0xff, b[1] == 0xf2,
   (b[2] & 0x80) == 0, ((b[2] >> 4) & 7) <= 1,
   ((b[2] >> 1) & 7) == 0 else { return false }
  let config = Int(b[4] & 0x1f) * 4 + Int(b[5] >> 6)
  return config == 10
 }
 // Only change protection on SDA-owned imported copies, never a provider/source file.
 // copyItem can preserve a source file's complete/while-open protection class.
 static func allowLockedPlaybackOfOwnedCopy(_ url: URL) throws {
  #if os(iOS)
  let path = url.resolvingSymlinksInPath().standardizedFileURL.path
  let fm = FileManager.default
  let roots = [fm.urls(for:.cachesDirectory,in:.userDomainMask).first!,
   fm.urls(for:.documentDirectory,in:.userDomainMask).first!, fm.temporaryDirectory]
  guard roots.contains(where: { path.hasPrefix($0.resolvingSymlinksInPath().standardizedFileURL.path + "/") }) else { return }
  let value = try fm.attributesOfItem(atPath:path)[.protectionKey]
  let protection = (value as? FileProtectionType) ?? (value as? String).map { FileProtectionType(rawValue:$0) }
  if let desired = lockedPlaybackProtection(protection), desired != protection {
   try fm.setAttributes([.protectionKey:desired],ofItemAtPath:path)
  }
  #endif
 }
 #if os(iOS)
 static func lockedPlaybackProtection(_ protection: FileProtectionType?) -> FileProtectionType? {
  if protection == .complete || protection == .completeUnlessOpen { return .completeUntilFirstUserAuthentication }
  return protection
 }
 #endif
 // Resume the compressed reader only; the Rust decoder and playback clock stay intact.
 func reopenAfterInterruption(forceSequential: Bool = false) throws {
  if indexed != nil { return }
  if forceSequential { sequentialRecovery = true }
  guard resumeTimeValid else { throw SdaError.message("音轨缺少恢复时间戳，不能安全重读") }
  reader?.cancelReading()
   let asset = AVURLAsset(url: url)
   guard !asset.hasProtectedContent else { throw SdaError.message("不支持 DRM 加密音轨") }
   guard let track = asset.tracks(withMediaType: .audio).first(where: { track in
    if let selectedTrackID, track.trackID != selectedTrackID { return false }
    return track.formatDescriptions.contains { description in
     let format = description as! CMFormatDescription
     return [kAudioFormatEnhancedAC3, kAudioFormatAppleLossless].contains(CMFormatDescriptionGetMediaSubType(format))
    }
   }) else { throw SdaError.message("文件中没有未加密的 ALAC 或 E-AC-3/Atmos 音轨") }
   selectedTrackID=track.trackID
   guard let description = track.formatDescriptions.first else { throw SdaError.message("音轨格式缺失") }
   let sourceFormat = description as! CMAudioFormatDescription
   let formatID=CMFormatDescriptionGetMediaSubType(sourceFormat)
   guard [kAudioFormatEnhancedAC3,kAudioFormatAppleLossless].contains(formatID) else { throw SdaError.message("不支持混合编码音轨") }
   isAlac = formatID == kAudioFormatAppleLossless
   nativeCodec = isAlac ? nil : "eac3"
   if isAlac {
    guard let asbd = CMAudioFormatDescriptionGetStreamBasicDescription(sourceFormat), asbd.pointee.mChannelsPerFrame == 2 else {
     throw SdaError.message("当前 ALAC 路径仅支持立体声，不会下混多声道母版")
    }
   }
   if !isAlac { _ = try validatedIndex() }
   let r = try AVAssetReader(asset: asset)
   let settings: [String:Any]? = isAlac ? [AVFormatIDKey:kAudioFormatLinearPCM, AVSampleRateKey:48000,
    AVNumberOfChannelsKey:2, AVLinearPCMBitDepthKey:32, AVLinearPCMIsFloatKey:true,
    AVLinearPCMIsBigEndianKey:false, AVLinearPCMIsNonInterleaved:false] : nil
   let output = AVAssetReaderTrackOutput(track: track, outputSettings: settings)
   output.alwaysCopiesSampleData = false
   guard r.canAdd(output) else { throw SdaError.message("无法读取压缩音轨") }
   r.add(output)
   resumedRange = CMTimeCompare(nextTime, .zero) > 0
   if resumedRange && !sequentialRecovery {
    let remaining = CMTimeSubtract(track.timeRange.end, nextTime)
    if remaining.isNumeric && CMTimeCompare(remaining, .zero) > 0 {
     r.timeRange = CMTimeRange(start:nextTime,duration:remaining)
    }
    // Some OS versions expose an invalid/empty compressed track timeRange.
    // In that case read from the beginning and discard already-submitted PTS
    // below. Do not guess an end time, drop packets, or reset the decoder.
   }
   guard r.startReading() else { throw r.error ?? SdaError.message("音轨读取失败") }
   reader = r; trackOutput = output
 }
 // Recreate a failed cursor at the first unsubmitted packet, never skip audio.
 static func isRecoverableReaderError(_ error: NSError) -> Bool {
  error.domain == AVFoundationErrorDomain && [-11847, -11880].contains(error.code)
 }
 static func requiresSequentialRecovery(_ error: NSError) -> Bool {
  error.domain == AVFoundationErrorDomain && error.code == -11880
 }
 var diagnosticState: [String:Any] {
  var value:[String:Any] = ["backend":indexed != nil ? "indexed-mp4" : (file != nil ? "raw-file" : "avassetreader"),"indexedRecoveries":indexedRecoveryCount,"submittedCompressedBytes":submittedCompressedBytes,"readerRetries":retries,"indexedRecoveryMillis":indexedRecoveryMillis,"indexedPrepared":indexedPrepared,"indexPreflight":indexPreflightAttempted]
  if let nativeCodec { value["declaredCodec"]=nativeCodec }
  if let id=selectedTrackID { value["trackId"]=id }
  if let code=lastReaderErrorCode { value["lastReaderErrorCode"]=code }
  if let reason=indexedRecoveryFailure { value["indexedRecoveryFailure"]=reason }
  if let indexed { value["indexedSamples"]=indexed.sampleCount;value["indexedSamplePosition"]=indexed.samplePosition;value["verifiedPrefixBytes"]=indexed.verifiedPrefixBytes;value["recoveryStartSample"]=indexed.recoveryStartSample }
  return value
 }
 // Switch before reaching the fragile tail cursor. Prefix verification costs
 // at most the initial 64 KiB plus one AVFoundation buffer; no PCM cache.
 private func prepareIndexedContinuation() {
  guard !isAlac, indexed == nil, !indexedPreparationAttempted, submittedCompressedBytes >= 65536 else { return }
  indexedPreparationAttempted=true
  do { indexedPrepared=try adoptVerifiedIndex(recovery:false) }
  catch { indexedRecoveryFailure="initial-prefix-or-index-unverified" }
 }
 // Inspect the physical sample bounds before playback, not at the fragile
 // final reader cursor. Reuse the metadata index for later hash verification.
 private func validatedIndex() throws -> IndexedCompressedInput? {
  if !indexPreflightAttempted {
   indexPreflightAttempted=true
   preflightIndex=try IndexedCompressedInput.open(url,trackID:selectedTrackID)
  }
  return preflightIndex
 }
 private func adoptVerifiedIndex(recovery: Bool) throws -> Bool {
  guard !isAlac, indexed == nil, submittedCompressedBytes > 0,
   let firstCompressedPTS, firstCompressedPTS.isNumeric, let selectedTrackID, selectedTrackID > 0 else { return false }
  let started=Date()
  guard let replacement=try validatedIndex() else { indexedRecoveryFailure="unsupported-container";return false }
  var expected=submittedHash
  try replacement.verifyContinuation(firstPTS:firstCompressedPTS,lastEnd:resumeTimeValid && nextTime.isNumeric ? nextTime : nil,submittedBytes:submittedCompressedBytes,digest:Data(expected.finalize()))
  reader?.cancelReading();reader=nil;trackOutput=nil;indexed=replacement;preflightIndex=nil
  if recovery { indexedRecoveryCount += 1 }
  indexedRecoveryFailure=nil
  indexedRecoveryMillis=Date().timeIntervalSince(started)*1000
  return true
 }
 func recoverInvalidSampleCursor() throws -> Bool {
  guard indexed == nil, !indexedRecoveryAttempted else { return false }
  indexedRecoveryAttempted=true
  return try adoptVerifiedIndex(recovery:true)
 }
 // A completed AVAssetReader is not proof that its compressed cursor visited
 // every physical sample. Continue any verified tail before finishing decode.
 func nextAfterReaderCompletion(chunkBytes: Int = 24 * 1024) throws -> Data? {
  if indexed != nil { return try next(chunkBytes:chunkBytes) }
  if !isAlac && submittedCompressedBytes == 0 { throw SdaError.message("压缩音轨没有返回任何音频包") }
  do {
   if try adoptVerifiedIndex(recovery:true) { return try next(chunkBytes:chunkBytes) }
  } catch {
   indexedRecoveryFailure="end-prefix-or-index-unverified"
   throw error
  }
  return nil
 }
 func next(chunkBytes: Int = 24 * 1024) throws -> Data? {
  if let indexed {
   let data=try indexed.next(chunkBytes:chunkBytes)
   if let data { submittedHash.update(data:data);submittedCompressedBytes += UInt64(data.count) }
   return data
  }
  if let f = file { let d = try f.read(upToCount: chunkBytes); return d?.isEmpty == false ? d : nil }
  guard let output = trackOutput, let r = reader else { return nil }
  while true {
  guard let sample = output.copyNextSampleBuffer() else {
   if r.status == .failed {
    if let error=r.error as NSError? {
     lastReaderErrorCode=error.code
     if Self.requiresSequentialRecovery(error), !isAlac {
      var recovered=false
      do { recovered=try recoverInvalidSampleCursor() }
      catch { indexedRecoveryFailure="prefix-or-index-unverified" }
      if recovered { return try next(chunkBytes:chunkBytes) }
     }
    }
    if let error = r.error as NSError?, error.domain == AVFoundationErrorDomain,
       Self.isRecoverableReaderError(error), resumeTimeValid, retries < 3 {
     retries += 1
     if Self.requiresSequentialRecovery(error) { sequentialRecovery = true }
     try reopenAfterInterruption(); return try next(chunkBytes:chunkBytes)
    }
    throw r.error ?? SdaError.message("音轨读取失败")
   }
   guard r.status == .completed else { throw r.error ?? SdaError.message("音轨读取意外停止") }
   return try nextAfterReaderCompletion(chunkBytes:chunkBytes)
  }
  // AVAssetReader may emit empty marker samples or deferred compressed data.
  if CMSampleBufferGetNumSamples(sample) == 0 { continue }
  if !CMSampleBufferDataIsReady(sample) {
   let ready = CMSampleBufferMakeDataReady(sample)
   guard ready == noErr else { throw SdaError.message("压缩样本加载失败: \(ready)") }
  }
  guard let block = CMSampleBufferGetDataBuffer(sample) else {
   throw SdaError.message("音频包没有压缩数据: samples=\(CMSampleBufferGetNumSamples(sample)), bytes=\(CMSampleBufferGetTotalSampleSize(sample)), ready=\(CMSampleBufferDataIsReady(sample)), format=\(String(describing: CMSampleBufferGetFormatDescription(sample)))")
  }
  if isAlac {
   guard let desc = CMSampleBufferGetFormatDescription(sample), let asbd = CMAudioFormatDescriptionGetStreamBasicDescription(desc),
    asbd.pointee.mFormatID == kAudioFormatLinearPCM, asbd.pointee.mSampleRate == 48000,
    asbd.pointee.mChannelsPerFrame == 2, asbd.pointee.mBitsPerChannel == 32, asbd.pointee.mBytesPerFrame == 8,
    asbd.pointee.mFormatFlags & kAudioFormatFlagIsFloat != 0,
    asbd.pointee.mFormatFlags & (kAudioFormatFlagIsNonInterleaved | kAudioFormatFlagIsBigEndian) == 0 else {
     throw SdaError.message("ALAC 解码输出不是 48 kHz 交错浮点立体声")
   }
  }
  let count = CMBlockBufferGetDataLength(block)
  guard count > 0 && count <= 1024 * 1024 else { throw SdaError.message("音频包长度无效") }
  if isAlac && (count % 8 != 0 || count > 65536*4) { throw SdaError.message("ALAC PCM 块长度无效") }
  var data = Data(count: count)
  let status = data.withUnsafeMutableBytes { CMBlockBufferCopyDataBytes(block, atOffset: 0, dataLength: count, destination: $0.baseAddress!) }
  guard status == kCMBlockBufferNoErr else { throw SdaError.message("音频包复制失败") }
  let pts = CMSampleBufferGetPresentationTimeStamp(sample)
  let duration = CMSampleBufferGetDuration(sample)
  // A seek can expose preceding sync samples; don't feed them to the decoder twice.
  if resumedRange && pts.isNumeric && CMTimeCompare(pts, nextTime) < 0 {
   if !isAlac { continue }
   // PCM seeks can overlap a partial decoded packet. Trim only the submitted
   // samples, not the whole packet (which would drop the first new samples).
   let overlap = max(0, Int((CMTimeGetSeconds(CMTimeSubtract(nextTime,pts))*48000).rounded()))
   if overlap >= count/8 { continue }
   data.removeFirst(overlap*8)
  }
  resumedRange = false
  resumeTimeValid = pts.isNumeric && duration.isNumeric && CMTimeCompare(duration,.zero) > 0
  if resumeTimeValid { nextTime = CMTimeAdd(pts,duration) }
  if !isAlac {
   if firstCompressedPTS == nil { firstCompressedPTS=pts }
   submittedHash.update(data:data);submittedCompressedBytes += UInt64(data.count)
  }
  retries = 0
  prepareIndexedContinuation()
  return data
  }
 }
 deinit { reader?.cancelReading(); try? file?.close(); if scoped { url.stopAccessingSecurityScopedResource() } }
}
