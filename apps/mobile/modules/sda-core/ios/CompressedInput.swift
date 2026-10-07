import Foundation
import AVFoundation
import AudioToolbox
import CoreMedia

/// Atmos stays compressed for SDA Rust. Only stereo ALAC is decoded to 48 kHz PCM.
final class CompressedInput {
 private(set) var isAlac = false
 private var file: FileHandle?
 private var reader: AVAssetReader?
 private var trackOutput: AVAssetReaderTrackOutput?
 private var nextTime = CMTime.zero
 private var retries = 0
 private var resumeTimeValid = true
 private var resumedRange = false
 private let url: URL
 private let scoped: Bool
 init(url: URL, name: String) throws {
  self.url = url
  scoped = url.startAccessingSecurityScopedResource()
  try Self.allowLockedPlaybackOfOwnedCopy(url)
  let ext = (name as NSString).pathExtension.lowercased()
  if ["eac3", "ec3", "mhas", "mp3"].contains(ext) {
   file = try FileHandle(forReadingFrom: url)
  } else if ["m4a", "mp4"].contains(ext) {
   try reopenAfterInterruption()
  } else { throw SdaError.message("支持立体声 ALAC、E-AC-3/Atmos M4A/MP4、MHAS 和 MP3") }
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
 func reopenAfterInterruption() throws {
  guard resumeTimeValid else { throw SdaError.message("音轨缺少恢复时间戳，不能安全重读") }
  reader?.cancelReading()
   let asset = AVURLAsset(url: url)
   guard !asset.hasProtectedContent else { throw SdaError.message("不支持 DRM 加密音轨") }
   guard let track = asset.tracks(withMediaType: .audio).first(where: { track in
    track.formatDescriptions.contains { description in
     let format = description as! CMFormatDescription
     return [kAudioFormatEnhancedAC3, kAudioFormatAppleLossless].contains(CMFormatDescriptionGetMediaSubType(format))
    }
   }) else { throw SdaError.message("文件中没有未加密的 ALAC 或 E-AC-3/Atmos 音轨") }
   guard let description = track.formatDescriptions.first else { throw SdaError.message("音轨格式缺失") }
   let sourceFormat = description as! CMAudioFormatDescription
   isAlac = CMFormatDescriptionGetMediaSubType(sourceFormat) == kAudioFormatAppleLossless
   if isAlac {
    guard let asbd = CMAudioFormatDescriptionGetStreamBasicDescription(sourceFormat), asbd.pointee.mChannelsPerFrame == 2 else {
     throw SdaError.message("当前 ALAC 路径仅支持立体声，不会下混多声道母版")
    }
   }
   let r = try AVAssetReader(asset: asset)
   let settings: [String:Any]? = isAlac ? [AVFormatIDKey:kAudioFormatLinearPCM, AVSampleRateKey:48000,
    AVNumberOfChannelsKey:2, AVLinearPCMBitDepthKey:32, AVLinearPCMIsFloatKey:true,
    AVLinearPCMIsBigEndianKey:false, AVLinearPCMIsNonInterleaved:false] : nil
   let output = AVAssetReaderTrackOutput(track: track, outputSettings: settings)
   output.alwaysCopiesSampleData = false
   guard r.canAdd(output) else { throw SdaError.message("无法读取压缩音轨") }
   r.add(output)
   resumedRange = CMTimeCompare(nextTime, .zero) > 0
   if resumedRange {
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
 func next(chunkBytes: Int = 24 * 1024) throws -> Data? {
  if let f = file { let d = try f.read(upToCount: chunkBytes); return d?.isEmpty == false ? d : nil }
  guard let output = trackOutput, let r = reader else { return nil }
  while true {
  guard let sample = output.copyNextSampleBuffer() else {
   if r.status == .failed {
    if let error = r.error as NSError?, error.domain == AVFoundationErrorDomain,
       Self.isRecoverableReaderError(error), resumeTimeValid, retries < 3 {
     retries += 1; try reopenAfterInterruption(); return try next(chunkBytes:chunkBytes)
    }
    throw r.error ?? SdaError.message("音轨读取失败")
   }
   guard r.status == .completed else { throw r.error ?? SdaError.message("音轨读取意外停止") }
   return nil
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
  retries = 0
  return data
  }
 }
 deinit { reader?.cancelReading(); try? file?.close(); if scoped { url.stopAccessingSecurityScopedResource() } }
}
