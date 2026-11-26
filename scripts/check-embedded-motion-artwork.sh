#!/bin/bash
set -euo pipefail
out="${1:?Pass a fresh test directory}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
ios="$PWD/apps/mobile/modules/sda-core/ios"
sources=("$ios/MotionArtworkRange.swift" "$ios/MotionArtworkSource.swift" "$ios/MotionArtworkReverseClock.swift" "$ios/MotionArtworkBackdrop.swift" "$ios/MotionArtworkReverseFrames.swift" "$PWD/scripts/test/embedded-motion-artwork.swift")
fixture="$PWD/scripts/test/fixtures/motion-artwork-tall.mp4"
xcrun swiftc -O -parse-as-library -swift-version 5 "${sources[@]}" -o "$out/motion-host-test"
"$out/motion-host-test" "$fixture" "$out/host-inputs" | tee "$out/motion-host-result.json"
sdk="$(xcrun --sdk iphoneos --show-sdk-path)"
xcrun --sdk iphoneos swiftc -typecheck -parse-as-library -swift-version 5 -target arm64-apple-ios16.4 -sdk "$sdk" "${sources[@]}"
sim_sdk="$(xcrun --sdk iphonesimulator --show-sdk-path)"
xcrun --sdk iphonesimulator swiftc -O -parse-as-library -swift-version 5 -target "$(uname -m)-apple-ios16.4-simulator" -sdk "$sim_sdk" "${sources[@]}" -o "$out/motion-simulator-test"
codesign --force --sign - "$out/motion-simulator-test"
xcrun simctl list devices available -j > "$out/simulators.json"
udid="$(python3 -c 'import json,sys;d=json.load(open(sys.argv[1]));print(next(v["udid"]for k,vs in sorted(d["devices"].items(),reverse=True)if "iOS"in k for v in vs if "iPhone"in v["name"]and v.get("isAvailable")))' "$out/simulators.json")"
simulator_state="$(python3 -c 'import json,sys;d=json.load(open(sys.argv[1]));print(next(v["state"]for vs in d["devices"].values()for v in vs if v["udid"]==sys.argv[2]))' "$out/simulators.json" "$udid")"
if [[ "$simulator_state" != Booted ]]; then
 trap 'xcrun simctl shutdown "$udid" >/dev/null 2>&1 || true' EXIT
 xcrun simctl boot "$udid"
fi
xcrun simctl bootstatus "$udid" -b
xcrun simctl spawn "$udid" "$out/motion-simulator-test" "$fixture" "$out/simulator-inputs" | tee "$out/motion-simulator-result.json"
