#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export IPHONEOS_DEPLOYMENT_TARGET=16.0
export MACINDECODE_AC4_SPEC_DIR="$ROOT/tmp/MacinDecode-AC4-Core/spec"
node scripts/prepare-ac4.mjs
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
for TARGET in aarch64-apple-ios aarch64-apple-ios-sim; do
 SDK=iphoneos
 if [[ "$TARGET" == *-sim ]]; then SDK=iphonesimulator; fi
 export SDKROOT="$(xcrun --sdk "$SDK" --show-sdk-path)"
 export CC="$(xcrun --sdk "$SDK" --find clang)"
 export AR="$(xcrun --sdk "$SDK" --find ar)"
 # cc-rs selects the Rust target triple and deployment flag; do not override
 # the simulator target with a device-only -miphoneos-version-min flag.
 cargo build --manifest-path crates/sda-native/Cargo.toml --release --locked --no-default-features --features ios-host --target "$TARGET"
done
unset SDKROOT CC AR
OUT="$ROOT/apps/mobile/modules/sda-core/ios/SdaNative.xcframework"
# This path is fixed inside the checkout, never a caller-supplied directory.
if [[ -d "$OUT" ]]; then mv "$OUT" "$OUT.previous.$(date +%s)"; fi
xcodebuild -create-xcframework \
 -library "$ROOT/crates/sda-native/target/aarch64-apple-ios/release/libsda_native.a" -headers "$ROOT/apps/mobile/modules/sda-core/ios/Headers" \
 -library "$ROOT/crates/sda-native/target/aarch64-apple-ios-sim/release/libsda_native.a" -headers "$ROOT/apps/mobile/modules/sda-core/ios/Headers" \
 -output "$OUT"
