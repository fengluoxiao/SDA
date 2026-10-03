import Foundation
import AVFoundation
import AudioToolbox
import CoreMedia

/// No Apple decoder/downmix: compressed access units are passed to SDA Rust.
final class CompressedInput {
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
  } else { throw SdaError.message("支持 E-AC-3/Atmos M4A/MP4、MHAS 和 MP3") }
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
     return CMFormatDescriptionGetMediaSubType(format) == kAudioFormatEnhancedAC3
    }
   }) else { throw SdaError.message("文件中没有未加密的 E-AC-3/Atmos 音轨") }
   let r = try AVAssetReader(asset: asset)
   let output = AVAssetReaderTrackOutput(track: track, outputSettings: nil)
   output.alwaysCopiesSampleData = false
   guard r.canAdd(output) else { throw SdaError.message("无法读取压缩音轨") }
   r.add(output)
   resumedRange = CMTimeCompare(nextTime, .zero) > 0
   if resumedRange { r.timeRange = CMTimeRange(start:nextTime,duration:.positiveInfinity) }
   guard r.startReading() else { throw r.error ?? SdaError.message("音轨读取失败") }
   reader = r; trackOutput = output
 }
 func next(chunkBytes: Int = 24 * 1024) throws -> Data? {
  if let f = file { let d = try f.read(upToCount: chunkBytes); return d?.isEmpty == false ? d : nil }
  guard let output = trackOutput, let r = reader else { return nil }
  while true {
  guard let sample = output.copyNextSampleBuffer() else {
   if r.status == .failed {
    if let error = r.error as NSError?, error.domain == AVFoundationErrorDomain,
       error.code == -11847 /* AVErrorOperationInterrupted; not exposed by the iOS SDK enum */, retries < 3 {
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
  let count = CMBlockBufferGetDataLength(block)
  guard count > 0 && count <= 1024 * 1024 else { throw SdaError.message("音频包长度无效") }
  var data = Data(count: count)
  let status = data.withUnsafeMutableBytes { CMBlockBufferCopyDataBytes(block, atOffset: 0, dataLength: count, destination: $0.baseAddress!) }
  guard status == kCMBlockBufferNoErr else { throw SdaError.message("音频包复制失败") }
  let pts = CMSampleBufferGetPresentationTimeStamp(sample)
  let duration = CMSampleBufferGetDuration(sample)
  // A seek can expose preceding sync samples; don't feed them to the decoder twice.
  if resumedRange && pts.isNumeric && CMTimeCompare(pts, nextTime) < 0 { continue }
  resumedRange = false
  resumeTimeValid = pts.isNumeric && duration.isNumeric && CMTimeCompare(duration,.zero) > 0
  if resumeTimeValid { nextTime = CMTimeAdd(pts,duration) }
  retries = 0
  return data
  }
 }
 deinit { reader?.cancelReading(); try? file?.close(); if scoped { url.stopAccessingSecurityScopedResource() } }
}
