#!/usr/bin/env python3
"""Run the actual Swift callback policy without an audio device."""
from pathlib import Path
import subprocess
import tempfile
root = Path(__file__).resolve().parents[1]
policy = (root / "apps/mobile/modules/sda-core/ios/OutputCallbackWatchdog.swift").read_text()
tests = r"""
var policy = OutputCallbackWatchdog()
// No callback yet: startup gets five seconds, not the live two-second limit.
assert(!policy.stalled(now:0,callbacks:0,expectedRunning:true))
assert(!policy.stalled(now:4.99,callbacks:0,expectedRunning:true))
assert(policy.stalled(now:5,callbacks:0,expectedRunning:true))
policy.reset()
assert(!policy.stalled(now:6,callbacks:1,expectedRunning:true))
assert(!policy.stalled(now:7.99,callbacks:1,expectedRunning:true))
assert(policy.stalled(now:8,callbacks:1,expectedRunning:true))
// Silence / missing PCM still delivers callbacks: do not rebuild the device.
for tick in 1...1000 {
 assert(!policy.stalled(now:8+Double(tick),callbacks:UInt64(tick+1),expectedRunning:true))
}
// Pauses, interruptions and explicitly stopped engines must reset the deadline.
assert(!policy.stalled(now:2000,callbacks:1001,expectedRunning:false))
assert(!policy.stalled(now:4000,callbacks:1001,expectedRunning:true))
assert(!policy.stalled(now:4001.99,callbacks:1001,expectedRunning:true))
assert(policy.stalled(now:4002,callbacks:1001,expectedRunning:true))
// A rebuilt graph gets a fresh deadline; never change the callback/PCM clock.
policy.reset()
assert(!policy.stalled(now:5000,callbacks:1001,expectedRunning:true))
assert(!policy.stalled(now:5001,callbacks:1002,expectedRunning:true))
assert(!policy.stalled(now:5002.99,callbacks:1002,expectedRunning:true))
assert(policy.stalled(now:5003,callbacks:1002,expectedRunning:true))
print("iOS output callback watchdog: passed")
"""
with tempfile.TemporaryDirectory(prefix="sda-ios-watchdog-") as directory:
 script = Path(directory) / "main.swift"
 script.write_text(policy + tests)
 subprocess.run(["xcrun", "swift", str(script)], check=True)
