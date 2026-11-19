#!/bin/bash
set -euo pipefail
out="${1:?Pass a dedicated build directory}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
ios="$PWD/apps/mobile/modules/sda-core/ios"
geometry="$ios/PersonalHrtfReviewGeometry.swift"
controller="$ios/PersonalHrtfReviewController.swift"
fixture="$PWD/scripts/test/personal-hrtf-review-fixture.swift"
test="$PWD/scripts/test/personal-hrtf-review.swift"
xcrun swiftc -O -parse-as-library -swift-version 5 "$geometry" "$fixture" "$test" -o "$out/review-host-test"
"$out/review-host-test" | tee "$out/review-host-result.json"
device_sdk="$(xcrun --sdk iphoneos --show-sdk-path)"
xcrun --sdk iphoneos swiftc -typecheck -swift-version 5 -target arm64-apple-ios16.4 -sdk "$device_sdk" "$geometry" "$controller" 2>&1 | tee "$out/review-device-typecheck.log"
sim_sdk="$(xcrun --sdk iphonesimulator --show-sdk-path)"
arch="$(uname -m)"
xcrun --sdk iphonesimulator swiftc -O -parse-as-library -swift-version 5 -target "$arch-apple-ios16.4-simulator" -sdk "$sim_sdk" "$geometry" "$fixture" "$test" -o "$out/review-simulator-test"
app="$out/ReviewUITest.app"
mkdir -p "$app"
xcrun --sdk iphonesimulator swiftc -O -parse-as-library -swift-version 5 -target "$arch-apple-ios16.4-simulator" -sdk "$sim_sdk" "$geometry" "$controller" "$fixture" "$PWD/scripts/test/personal-hrtf-review-ui.swift" -o "$app/ReviewUITest" 2>&1 | tee "$out/review-ui-compile.log"
python3 - "$app/Info.plist" <<'PY'
import plistlib,sys
with open(sys.argv[1],'wb')as f:plistlib.dump({'CFBundleIdentifier':'app.sda.hrtf-review-test','CFBundleExecutable':'ReviewUITest','CFBundleName':'ReviewUITest','CFBundlePackageType':'APPL','CFBundleVersion':'1','CFBundleShortVersionString':'1.0','MinimumOSVersion':'16.4','LSRequiresIPhoneOS':True,'UILaunchScreen':{},'UISupportedInterfaceOrientations':['UIInterfaceOrientationPortrait','UIInterfaceOrientationLandscapeLeft','UIInterfaceOrientationLandscapeRight']},f)
PY
codesign --force --sign - "$out/review-simulator-test"
codesign --force --sign - "$app"
xcrun simctl list devices available -j > "$out/review-simulators.json"
udid="$(python3 -c 'import json,sys;d=json.load(open(sys.argv[1]));print(next(v["udid"]for k,vs in sorted(d["devices"].items(),reverse=True)if "iOS"in k for v in vs if "iPhone"in v["name"]and v.get("isAvailable")))' "$out/review-simulators.json")"
xcrun simctl boot "$udid" || true
xcrun simctl bootstatus "$udid" -b
xcrun simctl spawn "$udid" "$out/review-simulator-test" | tee "$out/review-simulator-result.json"
xcrun simctl install "$udid" "$app"
xcrun simctl launch --console "$udid" app.sda.hrtf-review-test | tee "$out/review-ui-runtime.log"
python3 - "$out" <<'PY'
from pathlib import Path
import json,sys
root=Path(sys.argv[1]);lines=(root/'review-ui-runtime.log').read_text().splitlines()
result=json.loads(next(s for s in reversed(lines)if s.startswith('{')))
assert result['passed']and result['layoutSizesChecked']==6 and result['cancelDismissedOnce']
assert result['physicalScanTested']is False and result['deviceVisualAcceptance']is False
(root/'review-ui-result.json').write_text(json.dumps(result,indent=2)+'\n')
PY
