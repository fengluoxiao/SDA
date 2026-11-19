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
with open(sys.argv[1],'wb')as f:plistlib.dump({'CFBundleIdentifier':'app.sda.hrtf-review-test','CFBundleExecutable':'ReviewUITest','CFBundleName':'ReviewUITest','CFBundlePackageType':'APPL','CFBundleVersion':'1','CFBundleShortVersionString':'1.0','MinimumOSVersion':'16.4','LSRequiresIPhoneOS':True,'UILaunchScreen':{},'UIDeviceFamily':[1,2],'CFBundleSupportedPlatforms':['iPhoneSimulator'],'UISupportedInterfaceOrientations':['UIInterfaceOrientationPortrait','UIInterfaceOrientationLandscapeLeft','UIInterfaceOrientationLandscapeRight']},f)
PY
codesign --force --sign - "$out/review-simulator-test"
codesign --force --sign - "$app"
xcrun simctl list devices available -j > "$out/review-simulators.json"
udid="$(python3 - "$out/review-simulators.json" "$out/review-runtime-choice.json" <<'PY'
import json,sys,re
from pathlib import Path
d=json.load(open(sys.argv[1]))
choices=[(tuple(map(int,k.rsplit('iOS-',1)[1].split('-'))),k,v)for k,vs in d['devices'].items()if re.fullmatch(r'com.apple.CoreSimulator.SimRuntime.iOS-[0-9]+-[0-9]+',k)for v in vs if v['name']=='iPhone 18 Pro Max'and v.get('isAvailable')]
assert choices,'A standard iPhone simulator runtime is required; do not select a device-specific preview runtime'
_,runtime,device=max(choices,key=lambda row:row[0])
Path(sys.argv[2]).write_text(json.dumps({'runtime':runtime,'device':device['name'],'udid':device['udid'],'physicalDevice':False},indent=2))
print(device['udid'])
PY
)"
xcrun simctl boot "$udid" || true
xcrun simctl bootstatus "$udid" -b
xcrun simctl spawn "$udid" "$out/review-simulator-test" | tee "$out/review-simulator-result.json"
python3 - "$udid" "$app" "$out/review-ui-runtime.log" <<'PY'
import subprocess,sys
udid,app,log=sys.argv[1:]
print('Installing measured-model review test on standard iPhone simulator',flush=True)
subprocess.run(['xcrun','simctl','install',udid,app],check=True,timeout=120)
print('Launching measured-model review layout and cancellation checks',flush=True)
with open(log,'w')as output:
 try:subprocess.run(['xcrun','simctl','launch','--console',udid,'app.sda.hrtf-review-test'],check=True,timeout=120,stdout=output,stderr=subprocess.STDOUT)
 finally:
  output.flush()
  print(open(log).read(),flush=True)
PY
python3 - "$out" <<'PY'
from pathlib import Path
import json,sys
root=Path(sys.argv[1]);lines=(root/'review-ui-runtime.log').read_text().splitlines()
result=json.loads(next(s for s in reversed(lines)if s.startswith('{')))
assert result['passed']and result['layoutSizesChecked']==6 and result['cancelDismissedOnce']
assert result['physicalScanTested']is False and result['deviceVisualAcceptance']is False
(root/'review-ui-result.json').write_text(json.dumps(result,indent=2)+'\n')
PY
