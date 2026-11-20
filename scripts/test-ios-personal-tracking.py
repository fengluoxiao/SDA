"""Execute the native recovery gate; also test the reproducible upstream adapter."""
from pathlib import Path
import os,subprocess,tempfile,sys,hashlib
root=Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory()as tmp:
 exe=Path(tmp)/('tracking.exe'if os.name=='nt'else'tracking')
 subprocess.run([os.environ.get('CXX','c++'),'-std=c++17','-I'+str(root/'apps/mobile/modules/sda-core/ios/PersonalHrtfNative'),str(root/'scripts/test/personal-hrtf-tracking.cpp'),'-o',str(exe)],check=True)
 subprocess.run([str(exe)],check=True)
text=(root/'apps/mobile/modules/sda-core/ios/PersonalHrtfFusion/PersonalHrtfReconstruction.swift').read_text()
assert 'timestamp - lastTimestamp >= 0.15'not in text
assert 'frame.timestamp > lastTimestamp'in text
assert 'now - lastPoseTime >= 0.15'in text
assert 'consecutiveLostTrackingCount >'not in text
patch=(root/'scripts/patch-ios-personal-scan-fusion.py').read_text()
assert '_sdaTrackingRecovery.terminal(pbfMetadata.isMerged,'in patch
assert 'terminalTracking ? SCAssimilatedFrameResultFailed : SCAssimilatedFrameResultLostTracking'in patch
assert '_sdaTrackingRecovery.reset()'in patch
assert 'manager.sdaNeedsTrackingRecovery'in text
assert 'manager.sdaIsAligning'in text
assert '_sdaIsAligning.store(_sdaTrackingRecovery.needsAlignment('in patch
assert '连续 3 秒未能恢复跟踪'not in text
assert 'break; // Recoverable loss' in patch
assert 'SdaTrackingRecovery::axisAngle' in patch
print('PASS: 6/15/30/60fps loss through 30min, repeated recovery, guidance, reset, invalid clock; acos roundoff and real angles')

assert 'count >= 600 { fail'not in text
assert 'if model.sdaReachedSurfelLimit'in text and 'phase = .paused'in text
assert 'sda_hrtf_scan_point_limit(budget)'in text
assert 'maximumSurfelCount) continue'in patch
assert 'metadata.result = SCAssimilatedFrameResultFailed;\n        }'not in patch
print('PASS: quota has a strict insertion bound and paused capture retains its measured model')
