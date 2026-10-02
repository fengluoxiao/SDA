import Foundation
import AVFoundation
import AudioToolbox
import CoreMedia

/// No Apple decoder/downmix: compressed access units are passed to SDA Rust.
final class CompressedInput {
 private var file: FileHandle?
 private var reader: AVAssetReader?
 private var trackOutput: AVAssetReaderTrackOutput?
 private let url: URL
 private let scoped: Bool
 init(url: URL, name: String) throws {
  self.url = url
  scoped = url.startAccessingSecurityScopedResource()
  let ext = (name as NSString).pathExtension.lowercased()
  if ["eac3", "ec3", "mhas", "mp3"].contains(ext) {
   file = try FileHandle(forReadingFrom: url)
  } else if ["m4a", "mp4"].contains(ext) {
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
   guard r.startReading() else { throw r.error ?? SdaError.message("音轨读取失败") }
   reader = r; trackOutput = output
  } else { throw SdaError.message("支持 E-AC-3/Atmos M4A/MP4、MHAS 和 MP3") }
 }
 func next() throws -> Data? {
  if let f = file { let d = try f.read(upToCount: 24 * 1024); return d?.isEmpty == false ? d : nil }
  guard let output = trackOutput, let r = reader else { return nil }
  while true {
  guard let sample = output.copyNextSampleBuffer() else {
   if r.status == .failed { throw r.error ?? SdaError.message("音轨读取失败") }
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
  return data
  }
 }
 deinit { reader?.cancelReading(); try? file?.close(); if scoped { url.stopAccessingSecurityScopedResource() } }
}
