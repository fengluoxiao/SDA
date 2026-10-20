"""Run the production Swift scheduling policy on macOS (no AVFoundation mocks)."""
from pathlib import Path
import subprocess
import tempfile

source = Path("apps/mobile/modules/sda-core/ios/SystemAudioBufferPolicy.swift").resolve()
with tempfile.TemporaryDirectory(prefix="sda-buffer-policy-") as directory:
    root = Path(directory)
    main = root / "main.swift"
    main.write_text(r'''func check(_ enqueued: UInt64, _ consumed: UInt64, _ queued: UInt64,
           _ finished: Bool, _ recovering: Bool, _ paused: Bool, _ expected: Bool) {
 precondition(SystemAudioBufferPolicy.shouldStart(enqueued:enqueued, consumed:consumed,
   queued:queued, inputFinished:finished, rebuffering:recovering, paused:paused) == expected)
}
// First packet, bounded startup, rebuffer hysteresis and advancing clock.
check(1024, 0, 0, false, false, false, false)
check(11999, 0, 0, false, false, false, false)
check(12000, 0, 0, false, false, false, true)
check(12000, 0, 0, false, true, false, false)
check(24000, 0, 0, false, true, false, true)
check(49024, 48000, 0, false, true, false, false)
check(72000, 48000, 0, false, true, false, true)
// Short EOF and final recovery tail drain without threshold deadlock.
check(512, 0, 0, true, false, false, true)
check(48512, 48000, 0, true, true, false, true)
check(512, 0, 1, true, true, false, false)
// Empty/end/invalid clocks, paused startup and paused recovery never start.
check(0, 0, 0, true, false, false, false)
check(48000, 48000, 0, true, true, false, false)
check(48000, 49000, 0, true, true, false, false)
check(24000, 0, 0, false, false, true, false)
check(24000, 0, 0, false, true, true, false)
// Loading is not ready while compressed data is parsed but no PCM exists.
precondition(SystemAudioBufferPolicy.needsStartupPreparation(started:false,waitingForBalance:false,enqueued:0,queued:0,inputFinished:false))
precondition(SystemAudioBufferPolicy.needsStartupPreparation(started:false,waitingForBalance:false,enqueued:0,queued:11999,inputFinished:false))
// Paused preparation can finish entirely in the decoder queue; no audio clock runs.
precondition(!SystemAudioBufferPolicy.needsStartupPreparation(started:false,waitingForBalance:false,enqueued:0,queued:12000,inputFinished:false))
precondition(SystemAudioBufferPolicy.needsStartupPreparation(started:false,waitingForBalance:true,enqueued:0,queued:48000,inputFinished:false))
precondition(!SystemAudioBufferPolicy.needsStartupPreparation(started:true,waitingForBalance:true,enqueued:0,queued:0,inputFinished:false))
// A ready short EOF and an empty EOF do not wait for an unreachable threshold.
precondition(!SystemAudioBufferPolicy.needsStartupPreparation(started:false,waitingForBalance:false,enqueued:0,queued:512,inputFinished:true))
precondition(!SystemAudioBufferPolicy.needsStartupPreparation(started:false,waitingForBalance:true,enqueued:0,queued:0,inputFinished:true))
// Very large counters cannot overflow the readiness calculation.
precondition(SystemAudioBufferPolicy.hasStartupPCM(enqueued:UInt64.max,queued:UInt64.max,inputFinished:false))
print("System audio buffer policy: 23 checks passed")
''', encoding="utf-8")
    binary = root / "buffer-policy-test"
    subprocess.run(["xcrun", "swiftc", "-swift-version", "5", str(source), str(main), "-o", str(binary)], check=True)
    subprocess.run([str(binary)], check=True)
