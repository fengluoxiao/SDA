#!/bin/bash
set -euo pipefail
out="${1:?Pass a fresh test directory}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
ios="$PWD/apps/mobile/modules/sda-core/ios"
python3 - "$ios/SdaMotionArtworkModule.swift" "$out/MotionArtworkViewUnderTest.swift" <<'PY'
import pathlib,sys
source=pathlib.Path(sys.argv[1]).read_text()
view=source[source.index('private final class MotionArtworkWeakView'):]
probes='''
extension SdaMotionArtworkView {
 var testStarting:Bool{starting}
 var testPlayer:AVPlayer?{player}
 var testLoopCount:Int{looper?.loopCount ?? 0}
 var testPlaybackPosition:Double{if blurred,let leader{return leader.testPlaybackPosition};return player?.currentTime().seconds ?? 0}
 var testLocalPosition:Double{player?.currentTime().seconds ?? 0}
 var testReportedReady:Bool{reportedReady}
 var testState:[String:Any]{
  let time=testPlaybackPosition,duration=player?.currentItem?.duration.seconds ?? 0
  return ["active":active,"presented":presented,"attached":window != nil,"canAnimate":canAnimate,"loopCount":testLoopCount,"position":time.isFinite ? time:-1,"duration":duration.isFinite ? duration:-1,"rate":player?.rate ?? 0,"followerTimer":followerTimer != nil,"featherMode":featherMode]
 }
 func testStop(){stop()}
}
'''
pathlib.Path(sys.argv[2]).write_text('import Foundation\nimport AVFoundation\nimport CoreImage\nimport QuartzCore\n#if SDA_MOTION_UI_TEST\nimport UIKit\n#endif\n'+view+probes)
PY
core=("$ios/MotionArtworkRange.swift" "$ios/MotionArtworkSource.swift" "$ios/MotionArtworkBackdrop.swift" "$ios/MotionArtworkMask.swift")
sources=("${core[@]}" "$PWD/scripts/test/motion-artwork-view-stubs.swift" "$out/MotionArtworkViewUnderTest.swift" "$PWD/scripts/test/motion-artwork-view-lifecycle.swift" "$PWD/scripts/test/motion-artwork-mask.swift" "$PWD/scripts/test/embedded-motion-artwork.swift")
fixture="$PWD/scripts/test/fixtures/motion-artwork-tall.mp4"
xcrun swiftc -O -parse-as-library -swift-version 5 "${sources[@]}" -o "$out/motion-host-test"
"$out/motion-host-test" "$fixture" "$out/host-inputs" | tee "$out/motion-host-result.json"
sdk="$(xcrun --sdk iphoneos --show-sdk-path)"
xcrun --sdk iphoneos swiftc -typecheck -parse-as-library -swift-version 5 -target arm64-apple-ios16.4 -sdk "$sdk" "${sources[@]}"
sim_sdk="$(xcrun --sdk iphonesimulator --show-sdk-path)"
xcrun --sdk iphonesimulator swiftc -O -parse-as-library -swift-version 5 -target "$(uname -m)-apple-ios16.4-simulator" -sdk "$sim_sdk" "${sources[@]}" -o "$out/motion-simulator-test"
codesign --force --sign - "$out/motion-simulator-test"
xcrun simctl list devices available -j > "$out/simulators.json"
udid="$(python3 - "$out/simulators.json" "$out/runtime-choice.json" <<'PY'
import json,sys,re
from pathlib import Path
d=json.load(open(sys.argv[1]))
choices=[(tuple(map(int,k.rsplit('iOS-',1)[1].split('-'))),k,v)for k,vs in d['devices'].items()if re.fullmatch(r'com.apple.CoreSimulator.SimRuntime.iOS-[0-9]+-[0-9]+',k)for v in vs if v['name']=='iPhone 18 Pro Max'and v.get('isAvailable')]
assert choices,'A standard iPhone simulator is required for visible-frame rotation tests'
_,runtime,device=max(choices,key=lambda row:row[0])
Path(sys.argv[2]).write_text(json.dumps({'runtime':runtime,'device':device['name'],'udid':device['udid'],'physicalDeviceTested':False},indent=2))
print(device['udid'])
PY
)"
simulator_state="$(python3 -c 'import json,sys;d=json.load(open(sys.argv[1]));print(next(v["state"]for vs in d["devices"].values()for v in vs if v["udid"]==sys.argv[2]))' "$out/simulators.json" "$udid")"
if [[ "$simulator_state" != Booted ]]; then
 trap 'xcrun simctl shutdown "$udid" >/dev/null 2>&1 || true' EXIT
 xcrun simctl boot "$udid"
fi
xcrun simctl bootstatus "$udid" -b
bash scripts/check-motion-artwork-ui.sh "$out" "$udid"
xcrun simctl spawn "$udid" "$out/motion-simulator-test" "$fixture" "$out/simulator-inputs" | tee "$out/motion-simulator-result.json"
