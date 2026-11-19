#!/bin/bash
set -euo pipefail
out="${1:?Pass a build output directory}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
native="$PWD/apps/mobile/modules/sda-core/ios/PersonalHrtfNative"
ios="$PWD/apps/mobile/modules/sda-core/ios"
python3 scripts/generate-embedded-numcalc.py
xcodebuild -version > "$out/xcode.txt"
# Actual Apple compiler, first running the same embedded core on the Mac host.
cp -R scripts/test/fixtures/personal-hrtf-sphere "$out/host-fixture"
xcrun clang++ -std=c++17 -O2 -g -I "$native" "$native/SdaHrtfSolver.cpp" scripts/test/personal-hrtf-native.cpp -o "$out/native-host-test"
"$out/native-host-test" "$out/host-fixture" | tee "$out/host-result.json"
# Compile for the physical iPhone ABI and typecheck the front TrueDepth / Swift
# pipeline against the real iOS SDK, without substituting Mac platform types.
device_sdk="$(xcrun --sdk iphoneos --show-sdk-path)"
xcrun --sdk iphoneos clang++ -std=c++17 -O2 -fexceptions -target arm64-apple-ios16.4 -isysroot "$device_sdk" -I "$native" -c "$native/SdaHrtfSolver.cpp" -o "$out/SdaHrtfSolver-ios-arm64.o"
xcrun libtool -static -o "$out/libSdaPersonalHrtf-ios-arm64.a" "$out/SdaHrtfSolver-ios-arm64.o"
xcrun --sdk iphoneos swiftc -typecheck -swift-version 5 -target arm64-apple-ios16.4 -sdk "$device_sdk" -import-objc-header "$ios/SdaBridge.h" "$ios/PersonalHeadScan.swift" "$ios/PersonalHrtfStore.swift" "$ios/PersonalHrtfCoordinator.swift" "$ios/PersonalHrtfOnDeviceSolver.swift" 2>&1 | tee "$out/swift-typecheck.log"
# Run inside the iOS Simulator runtime as well. This verifies an iOS process,
# while still being explicitly distinct from a physical TrueDepth scan/test.
sim_sdk="$(xcrun --sdk iphonesimulator --show-sdk-path)"
arch="$(uname -m)"
xcrun --sdk iphonesimulator clang++ -std=c++17 -O2 -target "$arch-apple-ios16.4-simulator" -isysroot "$sim_sdk" -I "$native" "$native/SdaHrtfSolver.cpp" scripts/test/personal-hrtf-native.cpp -o "$out/native-simulator-test"
codesign --force --sign - "$out/native-simulator-test"
xcrun simctl list devices available -j > "$out/simulators.json"
udid="$(python3 -c 'import json,sys;d=json.load(open(sys.argv[1]));print(next(v["udid"]for k,vs in sorted(d["devices"].items(),reverse=True)if "iOS"in k for v in vs if "iPhone"in v["name"]and v.get("isAvailable")))' "$out/simulators.json")"
xcrun simctl boot "$udid" || true
xcrun simctl bootstatus "$udid" -b
cp -R scripts/test/fixtures/personal-hrtf-sphere "$out/simulator-fixture"
xcrun simctl spawn "$udid" "$out/native-simulator-test" "$out/simulator-fixture" | tee "$out/simulator-result.json"
