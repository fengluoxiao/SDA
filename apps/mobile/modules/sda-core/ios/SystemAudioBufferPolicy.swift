// Scheduling policy only: never alter channel samples, gain, or timestamps.
enum SystemAudioBufferPolicy {
 static let startupReserve: UInt64 = 12000 // 250 ms @ 48 kHz
 static let recoveryReserve: UInt64 = 24000 // 500 ms @ 48 kHz
 static func shouldStart(enqueued: UInt64, consumed: UInt64, queued: UInt64,
                         inputFinished: Bool, rebuffering: Bool, paused: Bool) -> Bool {
  guard !paused, enqueued > consumed else { return false }
  return enqueued - consumed >= (rebuffering ? recoveryReserve : startupReserve) ||
         (inputFinished && queued == 0)
 }
 // Readiness includes decoder PCM which can be submitted immediately on
 // resume. It must not require a running clock or a second startup reserve.
 static func hasStartupPCM(enqueued: UInt64, queued: UInt64, inputFinished: Bool) -> Bool {
  enqueued >= startupReserve || queued >= startupReserve - min(enqueued, startupReserve) ||
   (inputFinished && (enqueued > 0 || queued > 0))
 }
 static func needsStartupPreparation(started: Bool, waitingForBalance: Bool,
                                     enqueued: UInt64, queued: UInt64, inputFinished: Bool) -> Bool {
  if started || (inputFinished && enqueued == 0 && queued == 0) { return false }
  return waitingForBalance || !hasStartupPCM(enqueued:enqueued, queued:queued, inputFinished:inputFinished)
 }
}
