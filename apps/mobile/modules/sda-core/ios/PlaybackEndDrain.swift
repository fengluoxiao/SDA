import Foundation

/// FIFO consumption means samples reached the output callback, not the device.
/// Wait only the measured output latency after the last real sample is pulled.
struct PlaybackEndDrain {
 private var drainedAt: TimeInterval?
 mutating func reset() { drainedAt=nil }
 mutating func ready(inputFinished: Bool, decoded: UInt64, consumed: UInt64,
                     fifoFrames: Int, pendingBatches: Int,
                     now: TimeInterval, outputLatency: TimeInterval) -> Bool {
  guard inputFinished, consumed >= decoded, fifoFrames == 0, pendingBatches == 0 else {
   reset(); return false
  }
  if drainedAt == nil { drainedAt=now }
  let latency=outputLatency.isFinite ? max(0,outputLatency) : 0
  return now - drainedAt! >= latency
 }
}
