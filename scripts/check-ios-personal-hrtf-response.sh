#!/bin/bash
set -euo pipefail
out="${1:?Pass a fresh build directory}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
ios="$PWD/apps/mobile/modules/sda-core/ios"
native="$ios/PersonalHrtfNative"
fixture="$PWD/scripts/test/fixtures/personal-hrtf-sphere"
# The device variant MUST retain unknown/low OS headroom rejection. Simulator
# 0 headroom is not a measurement of a physical phone's available RAM.
device_sdk="$(xcrun --sdk iphoneos --show-sdk-path)"
xcrun --sdk iphoneos swiftc -typecheck -parse-as-library -swift-version 5 -target arm64-apple-ios16.4 -sdk "$device_sdk" -import-objc-header "$ios/SdaBridge.h" "$ios/PersonalHrtfResponseBuilder.swift" "$PWD/scripts/test/personal-hrtf-response-builder.swift" 2>&1 | tee "$out/response-device-typecheck.log"
sim_sdk="$(xcrun --sdk iphonesimulator --show-sdk-path)"
arch="$(uname -m)"
for unit in SdaHrtfSolver SdaHrtfImpulse SdaHrtfResults; do
 xcrun --sdk iphonesimulator clang++ -std=c++17 -O2 -target "$arch-apple-ios16.4-simulator" -isysroot "$sim_sdk" -I "$native" -c "$native/$unit.cpp" -o "$out/$unit.o"
done
xcrun --sdk iphonesimulator clang++ -std=c++17 -O2 -target "$arch-apple-ios16.4-simulator" -isysroot "$sim_sdk" -I "$native" "$out/SdaHrtfSolver.o" "$out/SdaHrtfImpulse.o" "$out/SdaHrtfResults.o" scripts/test/personal-hrtf-results.cpp -o "$out/results-test"
xcrun --sdk iphonesimulator swiftc -O -parse-as-library -swift-version 5 -target "$arch-apple-ios16.4-simulator" -sdk "$sim_sdk" -import-objc-header "$ios/SdaBridge.h" "$ios/PersonalHrtfResponseBuilder.swift" "$PWD/scripts/test/personal-hrtf-response-builder.swift" "$out/SdaHrtfSolver.o" "$out/SdaHrtfImpulse.o" "$out/SdaHrtfResults.o" -Xlinker -lc++ -lz -o "$out/response-test"
codesign --force --sign - "$out/results-test"
codesign --force --sign - "$out/response-test"
xcrun simctl list devices available -j > "$out/simulators.json"
udid="$(python3 -c 'import json,sys;d=json.load(open(sys.argv[1]));print(next(v["udid"]for k,vs in sorted(d["devices"].items(),reverse=True)if "iOS"in k for v in vs if "iPhone"in v["name"]and v.get("isAvailable")))' "$out/simulators.json")"
xcrun simctl boot "$udid" || true
xcrun simctl bootstatus "$udid" -b
xcrun simctl spawn "$udid" "$out/results-test" "$fixture" "$out/result-project" | tee "$out/result-reader-result.json"
xcrun simctl spawn "$udid" "$out/response-test" "$out/result-project" "$fixture" "$out/response-project" | tee "$out/response-builder-result.json"
