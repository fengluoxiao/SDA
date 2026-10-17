import Foundation
import AVFoundation

enum SdaError: LocalizedError {
 case message(String)
 var errorDescription: String? { if case let .message(s) = self { return s }; return nil }
}

@main struct AlacProbe {
 static func fixture(_ url: URL, rate: Double, channels: AVAudioChannelCount) throws {
  let settings: [String:Any] = [AVFormatIDKey:kAudioFormatAppleLossless,
   AVSampleRateKey:rate, AVNumberOfChannelsKey:channels, AVEncoderBitDepthHintKey:24]
  let file = try AVAudioFile(forWriting:url,settings:settings)
  let count = AVAudioFrameCount(rate)
  let buffer = AVAudioPCMBuffer(pcmFormat:file.processingFormat,frameCapacity:count)!
  buffer.frameLength = count
  for ch in 0..<Int(channels) {
   for i in 0..<Int(count) {
    buffer.floatChannelData![ch][i] = Float(sin(2 * Double.pi * 440 * Double(i)/rate)) * (ch == 0 ? 0.25 : 0.0625)
   }
  }
  try file.write(from:buffer)
 }
 static func decode(_ url: URL, resume: Bool) throws -> [Float] {
  let input = try CompressedInput(url:url,name:url.lastPathComponent)
  precondition(input.isAlac)
  var bytes = Data(); var blocks = 0
  while let packet = try input.next() {
   bytes.append(packet); blocks += 1
   if resume && blocks == 3 { try input.reopenAfterInterruption() }
  }
  return bytes.withUnsafeBytes { raw in
   stride(from:0,to:raw.count,by:4).map { Float(bitPattern:UInt32(littleEndian:raw.loadUnaligned(fromByteOffset:$0,as:UInt32.self))) }
  }
 }
 static func main() throws {
  let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
  try FileManager.default.createDirectory(at:dir,withIntermediateDirectories:true)
  defer { try? FileManager.default.removeItem(at:dir) }
  for rate in [44100.0,48000.0,96000.0] {
   let url = dir.appendingPathComponent("stereo-\(Int(rate)).m4a")
   try fixture(url,rate:rate,channels:2)
   let pcm = try decode(url,resume:false)
   let resumed = try decode(url,resume:true)
   let source = try AVAudioFile(forReading:url)
   let track = AVURLAsset(url:url).tracks(withMediaType:.audio).first!
   let diagnostic = "rate=\(rate) sourceFrames=\(source.length) trackDuration=\(CMTimeGetSeconds(track.timeRange.duration)) decodedFrames=\(pcm.count/2) resumedFrames=\(resumed.count/2)"
   FileHandle.standardError.write(Data((diagnostic + "\n").utf8))
   // AVAssetReader's sample-rate converter is not frame-count exact at EOF:
   // macOS 26.6 returns 47,983 frames for a 44,100-frame / 1-second source.
   // Keep native-rate decoding exact and bound SRC endpoint loss to <= 1 ms.
   // Do not pad the application audio or weaken recovery/channel checks.
   precondition(source.length == Int64(rate), "Fixture encoding changed duration")
   let durationTolerance = rate == 48000 ? 0 : 48
   guard abs(pcm.count/2 - 48000) <= durationTolerance else { throw SdaError.message("Incorrect resampled duration: " + diagnostic) }
   precondition(resumed.count == pcm.count, "Recovery dropped or duplicated samples")
   precondition(pcm.allSatisfy { $0.isFinite } && resumed.allSatisfy { $0.isFinite })
   for i in stride(from:1000,to:pcm.count-1000,by:2) {
    precondition(abs(pcm[i] - 4*pcm[i+1]) < 0.0001, "Stereo identity changed")
   }
   // At the native rate recovery must be sample-exact. Resampling can restart filter history.
   if rate == 48000 { precondition(zip(pcm,resumed).allSatisfy { abs($0.0-$0.1) < 0.000001 }) }
   print("PASS ALAC \(rate): 48 kHz stereo, channel identity, duration and recovery")
  }
  let mono = dir.appendingPathComponent("mono.m4a")
  try fixture(mono,rate:48000,channels:1)
  var rejected = false
  do { _ = try CompressedInput(url:mono,name:mono.lastPathComponent) } catch { rejected = true }
  precondition(rejected,"Non-stereo ALAC must not silently enter stereo path")
 }
}
