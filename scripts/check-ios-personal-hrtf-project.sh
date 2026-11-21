#!/bin/bash
set -euo pipefail
out="${1:?Pass a fresh output directory}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
ios="$PWD/apps/mobile/modules/sda-core/ios"
native="$ios/PersonalHrtfNative"
python3 scripts/test/check-personal-hrtf-meshopt.py
python3 scripts/generate-embedded-numcalc.py
git diff --exit-code -- "$native/Generated"
device_sdk="$(xcrun --sdk iphoneos --show-sdk-path)"
swift=("$ios/PersonalHrtfReviewGeometry.swift" "$ios/PersonalHrtfResponseBuilder.swift" "$ios/PersonalHrtfProjectBuilder.swift" "$PWD/scripts/test/personal-hrtf-review-fixture.swift" "$PWD/scripts/test/personal-hrtf-project-builder.swift")
xcrun --sdk iphoneos swiftc -typecheck -parse-as-library -swift-version 5 -target arm64-apple-ios16.4 -sdk "$device_sdk" -import-objc-header "$ios/SdaBridge.h" "${swift[@]}" 2>&1 | tee "$out/project-device-typecheck.log"
# Optimized + sanitizer builds catch native ABI, ownership, I/O and indexing
# problems independently. Tiny synthetic geometry is not an ear accuracy test.
sources=()
for unit in SdaHrtfSolver SdaHrtfImpulse SdaHrtfResults SdaHrtfMesh SdaHrtfProject; do sources+=("$native/$unit.cpp"); done
sources+=("$native/MeshOpt/simplifier.cpp")
xcrun clang++ -std=c++17 -O2 -I "$native" "${sources[@]}" scripts/test/personal-hrtf-project.cpp -o "$out/project-host-test"
"$out/project-host-test" "$out/project-host" | tee "$out/project-host-result.json"
xcrun clang++ -std=c++17 -O2 -I "$native" "${sources[@]}" scripts/test/personal-hrtf-reviewed-input.cpp -o "$out/review-project-test"
"$out/review-project-test" "$out/review-project" | tee "$out/review-project-result.json"
xcrun clang++ -std=c++17 -O1 -g -fsanitize=address,undefined -fno-omit-frame-pointer -I "$native" "${sources[@]}" scripts/test/personal-hrtf-project.cpp -o "$out/project-sanitized-test"
UBSAN_OPTIONS=halt_on_error=1:print_stacktrace=1 "$out/project-sanitized-test" "$out/project-sanitized" | tee "$out/project-sanitized-result.json"
sim_sdk="$(xcrun --sdk iphonesimulator --show-sdk-path)"
arch="$(uname -m)"
objects=()
for unit in SdaHrtfSolver SdaHrtfImpulse SdaHrtfResults SdaHrtfMesh SdaHrtfProject simplifier; do
 source="$native/$unit.cpp"; if [[ "$unit" == simplifier ]]; then source="$native/MeshOpt/simplifier.cpp"; fi
 xcrun --sdk iphoneos clang++ -std=c++17 -O2 -fexceptions -target arm64-apple-ios16.4 -isysroot "$device_sdk" -I "$native" -c "$source" -o "$out/$unit-device.o"
 xcrun --sdk iphonesimulator clang++ -std=c++17 -O2 -fexceptions -target "$arch-apple-ios16.4-simulator" -isysroot "$sim_sdk" -I "$native" -c "$source" -o "$out/$unit-simulator.o"
 objects+=("$out/$unit-simulator.o")
done
xcrun libtool -static -o "$out/libSdaPersonalHrtf-ios-arm64.a" "$out/"*-device.o
xcrun --sdk iphonesimulator clang++ -std=c++17 -O2 -target "$arch-apple-ios16.4-simulator" -isysroot "$sim_sdk" -I "$native" "${objects[@]}" scripts/test/personal-hrtf-project.cpp -o "$out/project-simulator-test"
xcrun --sdk iphonesimulator swiftc -O -parse-as-library -swift-version 5 -target "$arch-apple-ios16.4-simulator" -sdk "$sim_sdk" -import-objc-header "$ios/SdaBridge.h" "${swift[@]}" "${objects[@]}" -Xlinker -lc++ -lz -o "$out/project-swift-test"
codesign --force --sign - "$out/project-simulator-test"
codesign --force --sign - "$out/project-swift-test"
xcrun simctl list devices available -j > "$out/simulators.json"
udid="$(python3 -c 'import json,sys;d=json.load(open(sys.argv[1]));print(next(v["udid"]for k,vs in sorted(d["devices"].items(),reverse=True)if "iOS"in k for v in vs if "iPhone"in v["name"]and v.get("isAvailable")))' "$out/simulators.json")"
xcrun simctl boot "$udid" || true
xcrun simctl bootstatus "$udid" -b
xcrun simctl spawn "$udid" "$out/project-simulator-test" "$out/project-simulator" | tee "$out/project-simulator-result.json"
xcrun simctl spawn "$udid" "$out/project-swift-test" "$out/project-simulator" "$out/project-swift" | tee "$out/project-swift-result.json"
