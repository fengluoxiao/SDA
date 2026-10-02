import Foundation
import AVFoundation
import PHASE
import simd
import Darwin

/// Isolated CI prototype, NOT a production playback route or an Atmos decoder.
/// Loads at most eight seconds; all decode/JSON/pose work is off the realtime thread.
/// Head-pose/profile capabilities are intentionally not requested by this unsigned test.
@available(iOS 26.0, *)
final class Phase360Prototype {
 struct Pose {
  let sample: UInt64
  let duration: UInt64
  let direction: SIMD3<Float>
  let gain: Float
 }
 struct Track {
  let id: Int
  let pcm: [Float]
  let poses: [Pose]
 }
 static func appleDirection(_ p: SIMD3<Float>) -> SIMD3<Float> { SIMD3(p.x,p.z,-p.y) }
 static func check(_ ok: Bool, _ message: String) throws {
  if !ok { throw SdaError.message("PHASE 原型: " + message) }
 }
 static func gain(_ db: Double) -> Float { Float(pow(10,db/20)) }
 static func pose(_ track: Track, at sample: UInt64) -> SIMD3<Float> {
  var previous = track.poses[0].direction
  for event in track.poses {
   if sample < event.sample { return previous }
   if event.duration > 0 && sample-event.sample < event.duration {
    let t = Float(sample-event.sample)/Float(event.duration)
    let direction = previous+(event.direction-previous)*t
    return simd_length_squared(direction) > 0.00001 ? simd_normalize(direction) : event.direction
   }
   previous = event.direction
  }
  return previous
 }
 // Gain is applied exactly once in input PCM, including OAM subframe ramps.
 static func calibratedPCM(_ track: Track) -> [Float] {
  var pcm = track.pcm
  var cursor = 0
  var previous = track.poses[0].gain
  for i in pcm.indices {
   while cursor+1 < track.poses.count && track.poses[cursor+1].sample <= UInt64(i) {
    previous = track.poses[cursor].gain; cursor += 1
   }
   let event = track.poses[cursor]
   let t: Float = event.duration == 0 ? 1 : min(1,Float(UInt64(i) >= event.sample ? UInt64(i)-event.sample : 0)/Float(event.duration))
   pcm[i] *= previous+(event.gain-previous)*t
  }
  return pcm
 }
 static func load(_ url: URL, decodeReply: (UnsafeMutablePointer<CChar>?) throws -> Any) throws -> [Track] {
  let data = try Data(contentsOf:url, options:.mappedIfSafe)
  try check(data.count <= 2*1024*1024,"输入超过原型 2 MiB 上限")
  var error: UnsafeMutablePointer<CChar>?
  guard let decoder = sda_ios_sources_create(&error) else {
   _ = try decodeReply(error); throw SdaError.message("source decoder unavailable")
  }
  defer { sda_ios_sources_close(decoder) }
  var pcm = [Int:[Float]]()
  var poses = [Int:[Pose]]()
  var mapping = [Int:Int]()
  var frames: UInt64 = 0
  func drain() throws {
   while true {
    let raw = try decodeReply(sda_ios_sources_next(decoder))
    if raw is NSNull { break }
    guard let frame = raw as? [String:Any], let channels = frame["channels"] as? [[NSNumber]],
     let objects = frame["objects"] as? [[String:NSNumber]], let events = frame["events"] as? [[String:Any]],
     let start = frame["samplePos"] as? NSNumber, let rate = frame["sampleRate"] as? NSNumber,
     let beds = frame["bedLabels"] as? [String] else { throw SdaError.message("invalid source frame") }
    try check(rate.intValue == 48000 && start.uint64Value == frames,"源帧时钟/采样率不连续")
    try check(beds.isEmpty && !objects.isEmpty && objects.count == channels.count && objects.count <= 16,
     "第一阶段仅支持纯对象夹具；bed/HOA 不会伪装成对象")
    var nextMapping = [Int:Int]()
    for object in objects {
     guard let id = object["id"]?.intValue, let channel = object["channel"]?.intValue,
      channel >= 0 && channel < channels.count else { throw SdaError.message("invalid object mapping") }
     try check(nextMapping[id] == nil && !nextMapping.values.contains(channel),"重复对象/声道")
     nextMapping[id] = channel
    }
    try check(mapping.isEmpty || mapping == nextMapping,"原型不接受播放中对象拓扑变化")
    mapping = nextMapping
    let n = channels[0].count
    try check(n > 0 && frames+UInt64(n) <= 8*48000,"超过原型 8 秒上限")
    for (id,channel) in mapping {
     try check(channels[channel].count == n,"对象 PCM 长度不匹配")
     let samples = channels[channel].map { $0.floatValue }
     try check(samples.allSatisfy { $0.isFinite },"非有限 PCM")
     pcm[id,default:[]].append(contentsOf:samples)
    }
    for event in events {
     guard let id = (event["id"] as? NSNumber)?.intValue, mapping[id] != nil,
      let pos = event["pos"] as? [NSNumber], pos.count == 3,
      let timestamp = event["samplePos"] as? NSNumber,
      let ramp = event["rampDuration"] as? NSNumber,
      let db = event["gainDb"] as? NSNumber else { throw SdaError.message("invalid OAM") }
     try check(event["hasPos"] as? Bool == true && timestamp.uint64Value >= frames && timestamp.uint64Value < frames+UInt64(n),"OAM 位置/时间无效")
     let direction = appleDirection(SIMD3(pos[0].floatValue,pos[1].floatValue,pos[2].floatValue))
     let g = gain(db.doubleValue)
     try check(direction.x.isFinite && direction.y.isFinite && direction.z.isFinite && g.isFinite && simd_length_squared(direction)>0.00001,"OAM 非有限数值")
     poses[id,default:[]].append(Pose(sample:timestamp.uint64Value,duration:ramp.uint64Value,direction:simd_normalize(direction),gain:g))
    }
    frames += UInt64(n)
   }
  }
  try data.withUnsafeBytes { raw in
   let bytes = raw.bindMemory(to:UInt8.self)
   for offset in stride(from:0,to:bytes.count,by:1024) {
    _ = try decodeReply(sda_ios_sources_feed(decoder,bytes.baseAddress!.advanced(by:offset),min(1024,bytes.count-offset),false))
    try drain()
   }
  }
  _ = try decodeReply(sda_ios_sources_feed(decoder,nil,0,true)); try drain()
  try check(frames > 0,"没有对象音频")
  return try mapping.keys.sorted().map { id in
   let events = (poses[id] ?? []).sorted { $0.sample < $1.sample }
   try check(!events.isEmpty && events[0].sample == 0,"对象缺少初始位置")
   return Track(id:id,pcm:pcm[id]!,poses:events)
  }
 }
 static func run(_ url: URL, output: URL, decodeReply: (UnsafeMutablePointer<CChar>?) throws -> Any) throws -> [String:Any] {
  let axes: [(SIMD3<Float>,SIMD3<Float>)] = [(.init(0,1,0),.init(0,0,-1)),(.init(0,-1,0),.init(0,0,1)),
   (.init(1,0,0),.init(1,0,0)),(.init(-1,0,0),.init(-1,0,0)),(.init(0,0,1),.init(0,1,0))]
  for (sda,apple) in axes { try check(appleDirection(sda) == apple,"坐标映射失败") }
  let gainTrack = Track(id:0,pcm:[Float](repeating:1,count:8),poses:[
   Pose(sample:0,duration:0,direction:.init(0,0,-1),gain:1),
   Pose(sample:4,duration:4,direction:.init(1,0,0),gain:0)])
  let gainPCM = calibratedPCM(gainTrack)
  try check(gainPCM == [1,1,1,1,1,0.75,0.5,0.25],"OAM 增益重复或 ramp 错误")
  let tracks = try load(url,decodeReply:decodeReply)
  let engine = PHASEEngine(updateMode:.manual)
  engine.outputSpatializationMode = .alwaysUseBinaural
  let listener = PHASEListener(engine:engine)
  // Do not request privileged behavior without a valid signed profile.
  listener.automaticHeadTrackingFlags = []
  try engine.rootObject.addChild(listener)
  let container = PHASEContainerNodeDefinition()
  let parameters = PHASEMixerParameters()
  let format = AVAudioFormat(standardFormatWithSampleRate:48000,channels:1)!
  var sources = [Int:PHASESource]()
  var buffers = [Int:AVAudioPCMBuffer]()
  var completions = [Int:DispatchSemaphore]()
  for track in tracks {
   let source = PHASESource(engine:engine)
   source.gain = 1
   var transform = matrix_identity_float4x4
   let direction = pose(track,at:0)
   transform.columns.3 = SIMD4(direction.x,direction.y,direction.z,1)
   source.transform = transform
   try engine.rootObject.addChild(source)
   let mixer = PHASESpatialMixerDefinition(spatialPipeline:PHASESpatialPipeline(flags:[.directPathTransmission]),identifier:"object-\(track.id)")
   // No distance/directivity model, reflections or late reverb.
   mixer.distanceModelParameters = nil
   let node = PHASEPushStreamNodeDefinition(mixerDefinition:mixer,format:format,identifier:"pcm-\(track.id)")
   node.normalize = false
   container.addSubtree(node)
   parameters.addSpatialMixerParameters(identifier:mixer.identifier,source:source,listener:listener)
   let input = calibratedPCM(track)
   guard let buffer = AVAudioPCMBuffer(pcmFormat:format,frameCapacity:AVAudioFrameCount(input.count)),
    let channel = buffer.floatChannelData?[0] else { throw SdaError.message("PCM allocation failed") }
   buffer.frameLength = AVAudioFrameCount(input.count)
   input.withUnsafeBufferPointer { channel.update(from:$0.baseAddress!,count:$0.count) }
   sources[track.id] = source; buffers[track.id] = buffer
   completions[track.id] = DispatchSemaphore(value:0)
  }
  let assetID = "sda-phase-prototype-"+UUID().uuidString
  _ = try engine.assetRegistry.registerSoundEventAsset(rootNode:container,identifier:assetID)
  let event = try PHASESoundEvent(engine:engine,assetIdentifier:assetID,mixerParameters:parameters)
  defer { event.stopAndInvalidate(); engine.stop() }
  try engine.start()
  let prepared = DispatchSemaphore(value:0)
  event.prepare { reason in if reason == .prepared { prepared.signal() } }
  let prepareDeadline = Date().addingTimeInterval(10)
  var ready = false
  while Date() < prepareDeadline {
   engine.update()
   if prepared.wait(timeout:.now()+0.01) == .success { ready = true; break }
  }
  try check(ready,"PHASE prepare 失败/超时")
  try check(event.pushStreamNodes.count == tracks.count,"没有创建独立对象输入节点")
  for track in tracks {
   guard let node = event.pushStreamNodes["pcm-\(track.id)"] else { throw SdaError.message("missing PHASE stream") }
   let completion = completions[track.id]!
   node.scheduleBuffer(buffer:buffers[track.id]!,time:nil,options:[],completionCallbackType:.dataRendered) { _ in completion.signal() }
  }
  // A single sound event starts all streams together at one host timestamp.
  let startHost = mach_absolute_time()+AVAudioTime.hostTime(forSeconds:0.3)
  var epoch = startHost
  var minimumClockHost = startHost
  event.start(at:AVAudioTime(hostTime:startHost),completion:nil)
  engine.update()
  let total = UInt64(tracks[0].pcm.count)
  var position: UInt64 = 0
  var completed = Set<Int>()
  var poseUpdates = 0
  var maxUpdateGap: UInt64 = 0
  var previousPosition: UInt64 = 0
  var pausedChecked = false
  var clockSeen = false
  var trace = "sample,object,x,y,z\n"
  let deadline = Date().addingTimeInterval(15)
  while Date() < deadline && completed.count < tracks.count {
   engine.update()
   if let time = engine.lastRenderTime, time.isHostTimeValid && time.hostTime >= epoch && time.hostTime >= minimumClockHost {
    clockSeen = true
    position = min(total,UInt64(AVAudioTime.seconds(forHostTime:time.hostTime-epoch)*48000))
   }
   maxUpdateGap = max(maxUpdateGap,position >= previousPosition ? position-previousPosition : 0)
   try check(position >= previousPosition,"播放时钟倒退")
   previousPosition = position
   for track in tracks {
    let p = pose(track,at:position)
    var transform = matrix_identity_float4x4
    transform.columns.3 = SIMD4(p.x,p.y,p.z,1)
    sources[track.id]!.transform = transform
    if poseUpdates % 20 == 0 { trace += "\(position),\(track.id),\(p.x),\(p.y),\(p.z)\n" }
   }
   poseUpdates += 1
   engine.update()
   if !pausedChecked && position >= 12000 && position < total/2 {
    engine.pause()
    let pauseHost = mach_absolute_time()
    try check(engine.renderingState == .paused,"PHASE 未暂停")
    Thread.sleep(forTimeInterval:0.2)
    try check(engine.renderingState == .paused,"暂停状态不稳定")
    // Rendering callbacks must not complete while these 3-second streams are paused.
    for completion in completions.values { try check(completion.wait(timeout:.now()) == .timedOut,"暂停提前完成对象音频") }
    minimumClockHost = mach_absolute_time()
    epoch += minimumClockHost-pauseHost
    try engine.start()
    pausedChecked = true
   }
   for track in tracks where !completed.contains(track.id) {
    if completions[track.id]!.wait(timeout:.now()) == .success { completed.insert(track.id) }
   }
   Thread.sleep(forTimeInterval:0.005)
  }
  try check(completed.count == tracks.count && clockSeen && pausedChecked,"对象播放/时钟/暂停测试未完成")
  try check(position >= total-4096,"PCM 完成回调与播放时钟不匹配")
  try trace.write(to:output.appendingPathComponent("phase-object-motion.csv"),atomically:true,encoding:.utf8)
  return ["ok":true,"experimental":true,"route":"MPEG-H source objects -> PHASE",
   "objects":tracks.count,"sourceChannels":tracks.count,"sampleRate":48000,
   "decodedFramesPerObject":total,"renderedObjectStreams":completed.count,
   "poseUpdates":poseUpdates,"maxPoseUpdateGapSamples":maxUpdateGap,"lastPoseSample":position,
   "pauseStateVerified":pausedChecked,"coordinateMappingVerified":true,"gainRampVerified":true,
   "normalizationEnabled":false,"hrtfBypassed":true,"roomBypassed":true,
   "headTrackingRequested":false,"personalizedProfileVerified":false,"physicalSpatialListeningVerified":false,
   "poseTiming":"render-host clock, control-thread updates; not sample-accurate or measured acoustic latency",
   "limitations":"pure objects <=8s; bed/LFE/HOA/spread/diffuse/anchor not implemented; no production switch"]
 }
}
