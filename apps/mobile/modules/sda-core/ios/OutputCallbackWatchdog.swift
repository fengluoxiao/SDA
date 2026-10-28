// Called only on the serialized control/feeder path. The realtime callback
// already publishes an atomic counter in Rust, including silent callbacks.
struct OutputCallbackWatchdog {
 private var baseline: (callbacks: UInt64, time: Double)?
 private var seenCallback = false
 mutating func reset() { baseline = nil; seenCallback = false }
 mutating func stalled(now: Double, callbacks: UInt64, expectedRunning: Bool) -> Bool {
  guard expectedRunning else { reset(); return false }
  guard let last = baseline else {
   baseline = (callbacks, now); seenCallback = callbacks > 0; return false
  }
  if last.callbacks != callbacks {
   baseline = (callbacks, now); seenCallback = true; return false
  }
  return now - last.time >= (seenCallback ? 2 : 5)
 }
}
