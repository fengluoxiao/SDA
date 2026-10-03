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
}
