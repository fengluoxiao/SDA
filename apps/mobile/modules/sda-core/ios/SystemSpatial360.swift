import Foundation
import AVFoundation
import AudioToolbox

/// MPEG-H 7.1.4 or ALAC stereo/upmix PCM -> Apple renderer. No SDA HRTF/room.
/// Access only under SdaPlayer.lock, never from an audio realtime callback.
final class SystemSpatial360 {
 let alacUpmix: Bool?
 let channels: Int
 var decoder: UnsafeMutableRawPointer
 let renderer = AVSampleBufferAudioRenderer()
 let synchronizer = AVSampleBufferRenderSynchronizer()
 let format: CMAudioFormatDescription
 var enqueued: UInt64 = 0
 var queued: UInt64 = 0
 var started = false
 var paused = false
 var buffering = false
 var underruns = 0
 var closed = false
 var balanceEnabled = false
 var inputFinished = false
 var measurementReady = false
 var firstSubmissionBalanced = false
 var pendingSamples: [(sample: CMSampleBuffer, end: UInt64)] = []
 var interruptionRecoveries = 0
 var pcm = [Float](repeating: 0, count: 1024*12)
 init(decodeReply: (UnsafeMutablePointer<CChar>?) throws -> Any, volume: Float, alacUpmix: Bool? = nil) throws {
  self.alacUpmix = alacUpmix
  channels = alacUpmix == false ? 2 : 12
  // Explicit labels match CICP19, whose rear channels precede its side channels.
  let allLabels: [AudioChannelLabel] = [kAudioChannelLabel_Left, kAudioChannelLabel_Right,
   kAudioChannelLabel_Center, kAudioChannelLabel_LFEScreen,
   kAudioChannelLabel_RearSurroundLeft, kAudioChannelLabel_RearSurroundRight,
   kAudioChannelLabel_LeftSurround, kAudioChannelLabel_RightSurround,
   kAudioChannelLabel_LeftTopFront, kAudioChannelLabel_RightTopFront,
   kAudioChannelLabel_LeftTopRear, kAudioChannelLabel_RightTopRear]
  let labels = Array(allLabels.prefix(channels))
  let size = MemoryLayout<AudioChannelLayout>.size + (labels.count-1)*MemoryLayout<AudioChannelDescription>.stride
  let memory = UnsafeMutableRawPointer.allocate(byteCount:size, alignment:MemoryLayout<AudioChannelLayout>.alignment)
  defer { memory.deallocate() }
  memory.initializeMemory(as:UInt8.self, repeating:0, count:size)
  let layout = memory.assumingMemoryBound(to:AudioChannelLayout.self)
  layout.pointee.mChannelLayoutTag = kAudioChannelLayoutTag_UseChannelDescriptions
  layout.pointee.mNumberChannelDescriptions = UInt32(labels.count)
  let offset = MemoryLayout<AudioChannelLayout>.offset(of: \AudioChannelLayout.mChannelDescriptions)!
  let descriptions = memory.advanced(by:offset).assumingMemoryBound(to:AudioChannelDescription.self)
  for i in labels.indices { descriptions[i].mChannelLabel = labels[i] }
  var asbd = AudioStreamBasicDescription(mSampleRate:48000, mFormatID:kAudioFormatLinearPCM,
   mFormatFlags:kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked, mBytesPerPacket:UInt32(channels*4),
   mFramesPerPacket:1, mBytesPerFrame:UInt32(channels*4), mChannelsPerFrame:UInt32(channels), mBitsPerChannel:32, mReserved:0)
  var description: CMAudioFormatDescription?
  let result = CMAudioFormatDescriptionCreate(allocator:kCFAllocatorDefault, asbd:&asbd,
   layoutSize:size, layout:layout, magicCookieSize:0, magicCookie:nil, extensions:nil, formatDescriptionOut:&description)
  guard result == noErr, let description else { throw SdaError.message("系统音频格式创建失败: \(result)") }
  format = description
  var error: UnsafeMutablePointer<CChar>?
  guard let h = alacUpmix.map({ sda_ios_alac_speakers_create($0) }) ?? sda_ios_speakers_create(&error) else { _ = try decodeReply(error); throw SdaError.message("系统音频输出初始化失败") }
  decoder = h
  renderer.allowedAudioSpatializationFormats = channels == 2 ? .monoAndStereo : .multichannel
  renderer.volume = volume
  synchronizer.delaysRateChangeUntilHasSufficientMediaData = false
  synchronizer.addRenderer(renderer)
 }
 func close() {
  guard !closed else { return }; closed = true
  synchronizer.setRate(0, time:synchronizer.currentTime())
  renderer.stopRequestingMediaData(); renderer.flush(); pendingSamples.removeAll()
  sda_ios_speakers_close(decoder)
 }
 deinit { close() }
 func feed(_ data: Data?, finish: Bool, decodeReply: (UnsafeMutablePointer<CChar>?) throws -> Any) throws {
  let value = try decodeInput(data, finish:finish, decodeReply:decodeReply)
  queued = ((value as? [String:Any])?["queuedFrames"] as? NSNumber)?.uint64Value ?? 0
  if finish { inputFinished = true }
  try pump()
 }
 func decodeInput(_ data: Data?, finish: Bool, decodeReply: (UnsafeMutablePointer<CChar>?) throws -> Any) throws -> Any {
  let feed = alacUpmix == nil ? sda_ios_speakers_feed : sda_ios_alac_speakers_feed
  if let data { return try data.withUnsafeBytes { try decodeReply(feed(decoder,$0.bindMemory(to:UInt8.self).baseAddress,data.count,finish)) } }
  return try decodeReply(feed(decoder,nil,0,finish))
 }
 var loudnessPrefix: String { alacUpmix == nil ? "sda.loudness.system714.v1." : "sda.loudness.alac.stereo.v1." }
 func pump() throws {
  if paused { return }
  if renderer.status == .failed {
   guard let error = renderer.error as NSError?, error.domain == AVFoundationErrorDomain,
     error.code == -11847 /* AVErrorOperationInterrupted; not exposed by the iOS SDK enum */, interruptionRecoveries < 3 else {
    throw SdaError.message(renderer.error?.localizedDescription ?? "系统多声道输出失败")
   }
   interruptionRecoveries += 1; restorePendingSamples()
  }
  pendingSamples.removeAll { $0.end <= consumed }
  // Freeze the presentation clock at the end of submitted PCM on underrun.
  // Otherwise newly decoded buffers would carry timestamps already in the past.
  if started && !paused && !buffering && consumed >= enqueued {
   synchronizer.setRate(0,time:CMTime(value:Int64(enqueued),timescale:48000)); buffering = true; underruns += 1
  }
  // Never submit unbalanced startup PCM while the full-track analysis is pending.
  if preparingAudio { return }
  while !paused && queued > 0 && renderer.isReadyForMoreMediaData {
   // Bound system queue as well as decoder queue, including when renderer stays ready.
   if enqueued > consumed + 48000 { break }
   let frames = pcm.withUnsafeMutableBufferPointer { sda_ios_speakers_read(decoder,$0.baseAddress,1024) }
   guard frames > 0 else { throw SdaError.message("7.1.4 PCM 队列不一致") }
   var block: CMBlockBuffer?
   var result = CMBlockBufferCreateWithMemoryBlock(allocator:kCFAllocatorDefault, memoryBlock:nil,
    blockLength:frames*channels*4, blockAllocator:kCFAllocatorDefault, customBlockSource:nil,
    offsetToData:0, dataLength:frames*channels*4, flags:0, blockBufferOut:&block)
   guard result == noErr, let block else { throw SdaError.message("多声道缓冲创建失败") }
   result = pcm.withUnsafeBytes { CMBlockBufferReplaceDataBytes(with:$0.baseAddress!, blockBuffer:block, offsetIntoDestination:0, dataLength:frames*channels*4) }
   guard result == noErr else { throw SdaError.message("多声道 PCM 拷贝失败") }
   var timing = CMSampleTimingInfo(duration:CMTime(value:1,timescale:48000), presentationTimeStamp:CMTime(value:Int64(enqueued),timescale:48000), decodeTimeStamp:.invalid)
   var sampleSize = channels*4
   var sample: CMSampleBuffer?
   result = CMSampleBufferCreateReady(allocator:kCFAllocatorDefault, dataBuffer:block, formatDescription:format,
    sampleCount:frames, sampleTimingEntryCount:1, sampleTimingArray:&timing,
    sampleSizeEntryCount:1, sampleSizeArray:&sampleSize, sampleBufferOut:&sample)
   guard result == noErr, let sample else { throw SdaError.message("7.1.4 音频样本创建失败: \(result)") }
   pendingSamples.append((sample:sample,end:enqueued + UInt64(frames)))
   if !started { firstSubmissionBalanced = balanceEnabled && measurementReady }
   renderer.enqueue(sample)
   enqueued += UInt64(frames); queued -= UInt64(frames)
  }
  // Never restart on a single 21 ms packet after underrun. Short tracks / EOF
  // must drain even if they cannot meet the reserve, including pause/resume.
  if (!started || buffering) && SystemAudioBufferPolicy.shouldStart(
    enqueued:enqueued, consumed:consumed, queued:queued,
    inputFinished:inputFinished, rebuffering:buffering, paused:paused) {
   synchronizer.setRate(1, time:started ? synchronizer.currentTime() : .zero)
   started = true; buffering = false
  }
 }
 // Replay only the bounded, not-yet-consumed Apple queue after a recoverable interruption.
 // The compressed decoder, object clock and balance meter are not restarted.
 func restorePendingSamples() {
  let clock = consumed
  synchronizer.setRate(0,time:CMTime(value:Int64(clock),timescale:48000))
  renderer.flush()
  pendingSamples.removeAll { $0.end <= clock }
  for pending in pendingSamples { renderer.enqueue(pending.sample) }
  if started && !paused && !buffering { synchronizer.setRate(1,time:CMTime(value:Int64(clock),timescale:48000)) }
 }
 var consumed: UInt64 {
  guard started else { return 0 }
  let seconds = CMTimeGetSeconds(synchronizer.currentTime())
  guard seconds.isFinite, seconds > 0 else { return 0 }
  return min(enqueued, UInt64(seconds*48000))
 }
 func setPaused(_ value: Bool) {
  paused = value
  if started { synchronizer.setRate(value || buffering ? 0 : 1, time:synchronizer.currentTime()) }
 }
 func objects(_ decodeReply: (UnsafeMutablePointer<CChar>?) throws -> Any) throws -> Any {
  try decodeReply(sda_ios_speakers_objects(decoder,consumed))
 }
 var preparingAudio: Bool { !started && balanceEnabled && !measurementReady }
 // MPEG-H bridge is process-global: analyze and play sequentially, not with
 // concurrent owners. Called under SdaPlayer.lock before any PCM is submitted.
 func restartAfterMeasurement(_ value: String, decodeReply: (UnsafeMutablePointer<CChar>?) throws -> Any) throws {
  guard !started, enqueued == 0 else { throw SdaError.message("响度分析不能重置已开始的播放") }
  sda_ios_speakers_close(decoder)
  closed = true // Keep close/deinit safe if recreation fails.
  var error: UnsafeMutablePointer<CChar>?
  guard let next = alacUpmix.map({ sda_ios_alac_speakers_create($0) }) ?? sda_ios_speakers_create(&error) else { _ = try decodeReply(error); throw SdaError.message("系统音频播放源重建失败") }
  decoder = next; closed = false
  sda_ios_speakers_balance(decoder,balanceEnabled)
  try setMeasurement(value,decodeReply:decodeReply)
 }
 func setMeasurement(_ value: String, decodeReply: (UnsafeMutablePointer<CChar>?) throws -> Any) throws {
  _ = try decodeReply(sda_ios_speakers_measured(decoder,value)); measurementReady = true
 }
 func setBalance(_ enabled: Bool) { balanceEnabled = enabled; sda_ios_speakers_balance(decoder,enabled) }
 func status() -> [String:Any] {
  let clock = consumed, decoded = enqueued + queued
  return ["decodedSamplePos":decoded,"consumedSamplePos":clock,"positionMs":Double(clock)/48,
   "fifoFrames":decoded-clock,"outputChannels":channels,"outputLayout":channels == 2 ? "2.0" : "7.1.4", "sourceCodec":alacUpmix == nil ? "mpegh" : "alac", "alacUpmixActive":alacUpmix == true,"systemSpatial360RAActive":true,
   "systemOutputUnderruns":underruns,"systemOutputBuffering":buffering,"systemOutputQueuedFrames":enqueued-clock,
   "preparingAudio":preparingAudio,"balanceMeasurementReady":measurementReady,"paused":paused,"sampleRate":48000,"hrtfBypassed":true,"roomBypassed":true,
   "channelOrder":Array(["L","R","C","LFE","Lb","Rb","Ls","Rs","Tfl","Tfr","Tbl","Tbr"].prefix(channels))]
 }
}
