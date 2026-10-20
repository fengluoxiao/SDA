#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
TARGET_ROOT="${CARGO_TARGET_DIR:-$ROOT/crates/sda-native/target}"
# Relative Cargo target paths are resolved from this checkout root.
if [[ "$TARGET_ROOT" != /* ]]; then TARGET_ROOT="$ROOT/$TARGET_ROOT"; fi
export IPHONEOS_DEPLOYMENT_TARGET=16.0
export MACINDECODE_AC4_SPEC_DIR="$ROOT/tmp/MacinDecode-AC4-Core/spec"
node scripts/prepare-ac4.mjs
TARGETS=(aarch64-apple-ios)
if [[ "${SDA_IOS_INCLUDE_SIMULATOR:-1}" != 0 ]]; then TARGETS+=(aarch64-apple-ios-sim); fi
rustup target add "${TARGETS[@]}"
for TARGET in "${TARGETS[@]}"; do
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
FRAMEWORK_ARGS=(-create-xcframework)
for TARGET in "${TARGETS[@]}"; do
 FRAMEWORK_ARGS+=(-library "$TARGET_ROOT/$TARGET/release/libsda_native.a" -headers "$ROOT/apps/mobile/modules/sda-core/ios/Headers")
done
xcodebuild "${FRAMEWORK_ARGS[@]}" -output "$OUT"
