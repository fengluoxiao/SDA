#!/bin/bash
set -euo pipefail
out="${1:?Pass a build output directory}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
native="$PWD/apps/mobile/modules/sda-core/ios/PersonalHrtfNative"
ios="$PWD/apps/mobile/modules/sda-core/ios"
# Fail immediately on actual iOS SDK API/type errors before long numerical runs.
device_sdk="$(xcrun --sdk iphoneos --show-sdk-path)"
xcrun --sdk iphoneos swiftc -typecheck -swift-version 5 -target arm64-apple-ios16.4 -sdk "$device_sdk" -import-objc-header "$ios/SdaBridge.h" "$ios/PersonalHeadDepthCapture.swift" "$ios/PersonalHeadScan.swift" "$ios/PersonalHrtfStore.swift" "$ios/PersonalHrtfCoordinator.swift" "$ios/PersonalHrtfOnDeviceSolver.swift" "$ios/PersonalHrtfResponseBuilder.swift" "$ios/PersonalHrtfProjectBuilder.swift" 2>&1 | tee "$out/swift-typecheck.log"
xcrun --sdk iphoneos swiftc -typecheck -parse-as-library -swift-version 5 -target arm64-apple-ios16.4 -sdk "$device_sdk" -import-objc-header "$ios/SdaBridge.h" "$ios/PersonalHrtfResponseBuilder.swift" "$PWD/scripts/test/personal-hrtf-response-builder.swift" 2>&1 | tee "$out/response-builder-swift-typecheck.log"
python3 scripts/generate-embedded-numcalc.py
xcrun clang++ -std=c++17 -O2 -g -fsanitize=address,undefined -fno-omit-frame-pointer -I "$native" "$native/SdaHrtfMesh.cpp" scripts/test/personal-hrtf-mesh.cpp -o "$out/mesh-host-test"
UBSAN_OPTIONS=halt_on_error=1:print_stacktrace=1 "$out/mesh-host-test" | tee "$out/mesh-host-result.json"
xcodebuild -version > "$out/xcode.txt"
# Actual Apple compiler, first running the same embedded core on the Mac host.
cp -R scripts/test/fixtures/personal-hrtf-sphere "$out/host-fixture"
# Keep a release-optimized check separate from the sanitizer build: optimizer
# dependent ownership/aliasing failures can disappear under instrumentation.
xcrun clang++ -std=c++17 -O2 -g -fno-omit-frame-pointer -I "$native" "$native/SdaHrtfSolver.cpp" "$native/SdaHrtfImpulse.cpp" scripts/test/personal-hrtf-native.cpp -o "$out/native-host-optimized-test"
"$out/native-host-optimized-test" "$out/host-fixture" --optimized-smoke | tee "$out/host-optimized-result.json"
xcrun clang++ -std=c++17 -O1 -g -fsanitize=address,undefined -fno-omit-frame-pointer -I "$native" "$native/SdaHrtfSolver.cpp" "$native/SdaHrtfImpulse.cpp" scripts/test/personal-hrtf-native.cpp -o "$out/native-host-test"
UBSAN_OPTIONS=halt_on_error=1:print_stacktrace=1 "$out/native-host-test" "$out/host-fixture" | tee "$out/host-result.json"
# Compile for the physical iPhone ABI and typecheck the front TrueDepth / Swift
# pipeline against the real iOS SDK, without substituting Mac platform types.
device_sdk="$(xcrun --sdk iphoneos --show-sdk-path)"
xcrun --sdk iphoneos clang++ -std=c++17 -O2 -fexceptions -target arm64-apple-ios16.4 -isysroot "$device_sdk" -I "$native" -c "$native/SdaHrtfSolver.cpp" -o "$out/SdaHrtfSolver-ios-arm64.o"
xcrun --sdk iphoneos clang++ -std=c++17 -O2 -fexceptions -target arm64-apple-ios16.4 -isysroot "$device_sdk" -I "$native" -c "$native/SdaHrtfImpulse.cpp" -o "$out/SdaHrtfImpulse-ios-arm64.o"
xcrun --sdk iphoneos clang++ -std=c++17 -O2 -fexceptions -target arm64-apple-ios16.4 -isysroot "$device_sdk" -I "$native" -c "$native/SdaHrtfMesh.cpp" -o "$out/SdaHrtfMesh-ios-arm64.o"
xcrun --sdk iphoneos clang++ -std=c++17 -O2 -target arm64-apple-ios16.4 -isysroot "$device_sdk" -I "$native" -c "$native/SdaHrtfResults.cpp" -o "$out/SdaHrtfResults-ios-arm64.o"
xcrun --sdk iphoneos clang++ -std=c++17 -O2 -fexceptions -target arm64-apple-ios16.4 -isysroot "$device_sdk" -I "$native" -c "$native/SdaHrtfProject.cpp" -o "$out/SdaHrtfProject-ios-arm64.o"
xcrun libtool -static -o "$out/libSdaPersonalHrtf-ios-arm64.a" "$out/SdaHrtfSolver-ios-arm64.o" "$out/SdaHrtfImpulse-ios-arm64.o" "$out/SdaHrtfMesh-ios-arm64.o" "$out/SdaHrtfResults-ios-arm64.o" "$out/SdaHrtfProject-ios-arm64.o"
# Run inside the iOS Simulator runtime as well. This verifies an iOS process,
# while still being explicitly distinct from a physical TrueDepth scan/test.
sim_sdk="$(xcrun --sdk iphonesimulator --show-sdk-path)"
arch="$(uname -m)"
for unit in SdaHrtfSolver SdaHrtfImpulse SdaHrtfResults; do
 xcrun --sdk iphonesimulator clang++ -std=c++17 -O2 -target "$arch-apple-ios16.4-simulator" -isysroot "$sim_sdk" -I "$native" -c "$native/$unit.cpp" -o "$out/$unit-simulator.o"
done
xcrun --sdk iphonesimulator clang++ -std=c++17 -O2 -target "$arch-apple-ios16.4-simulator" -isysroot "$sim_sdk" -I "$native" "$out/SdaHrtfSolver-simulator.o" "$out/SdaHrtfImpulse-simulator.o" scripts/test/personal-hrtf-native.cpp -o "$out/native-simulator-test"
codesign --force --sign - "$out/native-simulator-test"
xcrun simctl list devices available -j > "$out/simulators.json"
udid="$(python3 -c 'import json,sys;d=json.load(open(sys.argv[1]));print(next(v["udid"]for k,vs in sorted(d["devices"].items(),reverse=True)if "iOS"in k for v in vs if "iPhone"in v["name"]and v.get("isAvailable")))' "$out/simulators.json")"
xcrun simctl boot "$udid" || true
xcrun simctl bootstatus "$udid" -b
cp -R scripts/test/fixtures/personal-hrtf-sphere "$out/simulator-fixture"
xcrun simctl spawn "$udid" "$out/native-simulator-test" "$out/simulator-fixture" | tee "$out/simulator-result.json"
xcrun --sdk iphonesimulator clang++ -std=c++17 -O2 -target "$arch-apple-ios16.4-simulator" -isysroot "$sim_sdk" -I "$native" "$native/SdaHrtfMesh.cpp" scripts/test/personal-hrtf-mesh.cpp -o "$out/mesh-simulator-test"
codesign --force --sign - "$out/mesh-simulator-test"
xcrun simctl spawn "$udid" "$out/mesh-simulator-test" | tee "$out/mesh-simulator-result.json"

# Validate native result reading under sanitizers, then convert and package the
# same upstream reference pressures inside an actual iOS Simulator process.
xcrun clang++ -std=c++17 -O1 -g -fsanitize=address,undefined -fno-omit-frame-pointer -I "$native" "$native/SdaHrtfSolver.cpp" "$native/SdaHrtfImpulse.cpp" "$native/SdaHrtfResults.cpp" scripts/test/personal-hrtf-results.cpp -o "$out/results-host-test"
UBSAN_OPTIONS=halt_on_error=1:print_stacktrace=1 "$out/results-host-test" "$PWD/scripts/test/fixtures/personal-hrtf-sphere" "$out/results-host-project" | tee "$out/results-host-result.json"
xcrun --sdk iphonesimulator clang++ -std=c++17 -O2 -target "$arch-apple-ios16.4-simulator" -isysroot "$sim_sdk" -I "$native" "$out/SdaHrtfSolver-simulator.o" "$out/SdaHrtfImpulse-simulator.o" "$out/SdaHrtfResults-simulator.o" scripts/test/personal-hrtf-results.cpp -o "$out/results-simulator-test"
codesign --force --sign - "$out/results-simulator-test"
xcrun simctl spawn "$udid" "$out/results-simulator-test" "$PWD/scripts/test/fixtures/personal-hrtf-sphere" "$out/results-simulator-project" | tee "$out/results-simulator-result.json"
xcrun --sdk iphonesimulator swiftc -swift-version 5 -target "$arch-apple-ios16.4-simulator" -sdk "$sim_sdk" -import-objc-header "$ios/SdaBridge.h" "$ios/PersonalHrtfResponseBuilder.swift" "$PWD/scripts/test/personal-hrtf-response-builder.swift" "$out/SdaHrtfSolver-simulator.o" "$out/SdaHrtfImpulse-simulator.o" "$out/SdaHrtfResults-simulator.o" -Xlinker -lc++ -lz -o "$out/response-builder-simulator-test"
codesign --force --sign - "$out/response-builder-simulator-test"
xcrun simctl spawn "$udid" "$out/response-builder-simulator-test" "$out/results-simulator-project" "$PWD/scripts/test/fixtures/personal-hrtf-sphere" "$out/response-builder-simulator-project" | tee "$out/response-builder-simulator-result.json"
