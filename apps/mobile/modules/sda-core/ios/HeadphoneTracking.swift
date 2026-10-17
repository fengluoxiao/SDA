import Foundation
import CoreMotion
import simd

// All state and renderer commands are serialized by SdaPlayer.lock.
extension SdaPlayer {
 var allowsHeadTrackingNow: Bool {
  !inBackground || (prefs.bool(forKey:"sda.headTracking") &&
    handle != nil && systemSpatial == nil && !isPaused && !done && !sessionInterrupted && audio?.isRunning == true)
 }
 func stopHeadTracking() {
  let wasTracking = motionWatchdog != nil
  motionSession += 1
  if motionWatchdog != nil { headphoneMotion.stopDeviceMotionUpdates() }
  motionWatchdog?.cancel(); motionWatchdog = nil
  motionLastSample = 0; playbackYaw = 0
  motionWarmSamples.removeAll()
  motionReferenceAfter = ProcessInfo.processInfo.systemUptime
  if wasTracking { applyNeutralHeadPose() }
  headTrackingDetail = inBackground ? "后台已归零" : "未启用"
 }

 func startHeadTracking() {
  guard prefs.bool(forKey:"sda.headTracking"), allowsHeadTrackingNow, systemSpatial == nil else {
   if prefs.bool(forKey:"sda.headTracking") && systemSpatial != nil { headTrackingDetail = "系统输出由 Apple 控制" }
   return
  }
  guard !headphoneMotion.isDeviceMotionActive else { return }
  guard CMHeadphoneMotionManager.authorizationStatus() != .denied,
        CMHeadphoneMotionManager.authorizationStatus() != .restricted else {
   headTrackingDetail = "运动权限未授权"; return
  }
  // The manager can wait for a compatible headset to connect on iOS.
  motionSession += 1
  let session = motionSession
  let startedAt = ProcessInfo.processInfo.systemUptime
  var lastTimestamp: TimeInterval = 0
  motionLastSample = startedAt
  motionRetryAfter = startedAt + 3
  motionReferenceAfter = startedAt
  if handle != nil { _ = try? command("trackingProviderReset") }
  applyNeutralHeadPose()
  if handle != nil { _ = try? command("pose",["x":0.0,"y":0.0,"z":0.0,"w":1.0]); motionNeutralApplied = false }
  headTrackingDetail = "等待兼容耳机"
  headphoneMotion.startDeviceMotionUpdates(to:motionQueue) { [weak self] motion, error in
   guard let self else { return }
   self.locked {
    guard self.motionSession == session, self.allowsHeadTrackingNow,
          self.prefs.bool(forKey:"sda.headTracking") else { return }
    guard error == nil, let motion else {
     self.playbackYaw = 0
     self.applyNeutralHeadPose()
     self.headTrackingDetail = error?.localizedDescription ?? "等待兼容耳机"
     return
    }
    guard motion.timestamp >= self.motionReferenceAfter,
          ProcessInfo.processInfo.systemUptime - motion.timestamp <= 1,
          motion.timestamp - lastTimestamp >= 1.0 / 30.0 else { return }
    lastTimestamp = motion.timestamp
    let q = motion.attitude.quaternion
    let raw = simd_quatd(ix:q.x, iy:q.y, iz:q.z, r:q.w)
    guard raw.vector.x.isFinite, raw.vector.y.isFinite, raw.vector.z.isFinite,
          raw.vector.w.isFinite, simd_length(raw.vector) > 0.000001 else { return }
    let current = simd_normalize(raw)
    self.motionCallbackCount += 1
    if self.inBackground { self.motionBackgroundCallbackCount += 1 }
    self.diagnosticMotion = ["sampleTime":motion.timestamp,"receivedTime":ProcessInfo.processInfo.systemUptime,
      "raw":[current.imag.x,current.imag.y,current.imag.z,current.real],
      "callbacks":self.motionCallbackCount,"backgroundCallbacks":self.motionBackgroundCallbackCount,
      "background":self.inBackground]
    self.motionLastSample = ProcessInfo.processInfo.systemUptime
    self.motionRetryCount = 0
    self.motionWarmSamples.append((time:motion.timestamp,pose:[current.imag.x,current.imag.y,current.imag.z,current.real]))
    if self.motionWarmSamples.count > 12 { self.motionWarmSamples.removeFirst() }
    guard self.handle != nil, self.systemSpatial == nil else { return }
    // Native callback -> Rust helper filter -> KU100 renderer, with no JS hop.
    guard let filtered = try? self.command("trackingProvider", ["x":current.imag.x,
      "y":current.imag.y,"z":current.imag.z,"w":current.real]) else { return }
    guard filtered is [Double] else {
     self.headTrackingDetail = "姿态校准中"
     return
    }
    self.diagnosticMotion["filtered"] = filtered
    self.motionNeutralApplied = false
    self.headTrackingDetail = "追踪中"
   }
  }
  let timer = DispatchSource.makeTimerSource(queue:DispatchQueue.global(qos:.utility))
  timer.schedule(deadline:.now() + 1, repeating:0.25)
  timer.setEventHandler { [weak self] in
   guard let self else { return }
   self.locked {
    guard self.motionSession == session else { return }
    if !self.allowsHeadTrackingNow {
     // Keep the watchdog alive through a transient output reconfiguration.
     // Explicit pause/stop lifecycle actions still cancel the subscription.
     self.applyNeutralHeadPose(); return
    }
    guard self.motionSession == session,
          ProcessInfo.processInfo.systemUptime - self.motionLastSample > 1 else { return }
    self.playbackYaw = 0
    if self.motionLastSample > 0 {
     _ = try? self.command("trackingProviderReset")
     self.motionLastSample = 0
    }
     self.applyNeutralHeadPose()
    self.headTrackingDetail = "等待兼容耳机"
    let now = ProcessInfo.processInfo.systemUptime
    if now >= self.motionRetryAfter {
     // Core Motion may remain marked active after capture/session reconfiguration
     // even though no fresh samples arrive. Replace that subscription explicitly.
     self.motionRetryCount = min(self.motionRetryCount + 1, 5)
     let delay = min(30.0, pow(2.0, Double(self.motionRetryCount)))
     self.recordPlaybackEvent("headTrackingRestart")
     self.stopHeadTracking()
     self.startHeadTracking()
     self.motionRetryAfter = now + delay
    }
   }
  }
  motionWatchdog = timer; timer.resume()
 }

 func setHeadTracking(_ enabled: Bool) {
  // Repeated delivery is not a recenter request: retain live warm samples.
  if prefs.bool(forKey:"sda.headTracking") == enabled {
   if enabled { startHeadTracking() }
   return
  }
  prefs.set(enabled,forKey:"sda.headTracking")
  stopHeadTracking()
  if enabled { startHeadTracking() }
 }

 func prepareHeadTrackingForPlayback() {
  guard prefs.bool(forKey:"sda.headTracking"), handle != nil else { return }
  // A new subscription resets the helper, so start it before warm replay.
  startHeadTracking()
  _ = try? command("trackingProviderReset")
  motionNeutralApplied = false
  // Enter tracked rendering before PCM is queued, not after calibration.
  // Identity is neutral; this only selects the short rendered-audio reserve.
  _ = try? command("pose",["x":0.0,"y":0.0,"z":0.0,"w":1.0])
  let now=ProcessInfo.processInfo.systemUptime
  for sample in motionWarmSamples where now-sample.time <= 0.5 {
   let q=sample.pose
   _ = try? command("trackingProvider",["x":q[0],"y":q[1],"z":q[2],"w":q[3]])
  }
 }

 func recenterHeadTracking() {
  playbackYaw = 0
  motionReferenceAfter = ProcessInfo.processInfo.systemUptime
  if handle != nil { _ = try? command("trackingProviderReset") }
  applyNeutralHeadPose()
 }

 func applyNeutralHeadPose() {
  if handle != nil && !motionNeutralApplied {
   do { _ = try command("resetPose"); motionNeutralApplied = true }
   catch { headTrackingDetail = "姿态归零失败" }
  }
 }
}
