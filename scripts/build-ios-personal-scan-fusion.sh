#!/bin/bash
set -euo pipefail
out="${1:?Pass a dedicated build directory}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
ios="$PWD/apps/mobile/modules/sda-core/ios"
stage="$PWD/scripts/stage-ios-personal-scan-fusion.py"
probe="$PWD/scripts/test/personal-hrtf-fusion-resources.swift"
lifecycle="$PWD/scripts/patch-ios-personal-scan-fusion.py"
source="$(mktemp -d "$out/fusion-source.XXXXXX")"
pin=8c3bc698944ec41edaa6f627db15a6573e8e2af2
git init -q "$source"
git -C "$source" remote add origin https://github.com/StandardCyborg/StandardCyborgCocoa.git
git -C "$source" fetch --depth 1 origin "$pin"
GIT_LFS_SKIP_SMUDGE=1 git -C "$source" checkout --detach FETCH_HEAD
test "$(git -C "$source" rev-parse HEAD)" = "$pin"
python3 "$lifecycle" "$source"
# The upstream package's bundle loader expects a resource bundle, but its
# manifest omits shader resources. Package the Metal kernels explicitly. Ear
# and foot ML models are excluded from this reconstruction-only build; no
# claim is made that a face/ear detector identifies the acoustic ear entrance.
python3 - "$source" <<'PY'
from pathlib import Path
import sys,json,hashlib
root=Path(sys.argv[1]);path=root/'Package.swift';text=path.read_text()
marker='name: "StandardCyborgFusion",\n            dependencies:'
begin=text.index(marker)
end=text.index('// MARK: - Tests',begin)
segment=text[begin:end]
resources=sorted(p.relative_to(root/'StandardCyborgFusion/Sources').as_posix()for p in (root/'StandardCyborgFusion/Sources').rglob('*.metal'))
assert len(resources)>=9
insert='exclude: ["StandardCyborgFusion/Models"],\n            resources: ['+','.join('.process('+json.dumps(p)+')'for p in resources)+'],\n            '
segment=segment.replace('publicHeadersPath: "include",',insert+'publicHeadersPath: "include",',1)
segment=segment.replace(', "-ffast-math"','')
text=text[:begin]+segment+text[end:];path.write_text(text)
(root.parent/'fusion-provenance.json').write_text(json.dumps({'repository':'https://github.com/StandardCyborg/StandardCyborgCocoa','commit':'8c3bc698944ec41edaa6f627db15a6573e8e2af2','patches':['explicit on-device Metal resources','exclude unused ML models','preserve non-finite depth checks'],'packageSha256':hashlib.sha256(path.read_bytes()).hexdigest(),'deviceScanTested':False,'earAccuracyValidated':False},indent=2))
PY
cd "$source"
# The repository also contains an unrelated legacy Xcode project. Select the
# Swift package explicitly through a dedicated workspace to avoid that project.
mkdir -p "$out/FusionPackage.xcworkspace"
python3 - "$source" "$out/FusionPackage.xcworkspace/contents.xcworkspacedata" <<'PY'
import sys,xml.sax.saxutils
from pathlib import Path
Path(sys.argv[2]).write_text('<Workspace version="1.0"><FileRef location="absolute:'+xml.sax.saxutils.escape(sys.argv[1], {'"':'&quot;'})+'"></FileRef></Workspace>')
PY
xcodebuild -workspace "$out/FusionPackage.xcworkspace" -list > "$out/fusion-schemes.txt" 2>&1
xcodebuild archive -workspace "$out/FusionPackage.xcworkspace" -scheme StandardCyborgFusion -configuration Release -destination 'generic/platform=iOS' -derivedDataPath "$out/derived-device" -archivePath "$out/StandardCyborgFusion-ios.xcarchive" SKIP_INSTALL=NO BUILD_LIBRARY_FOR_DISTRIBUTION=YES CODE_SIGNING_ALLOWED=NO > "$out/fusion-ios-build.log" 2>&1 || { tail -100 "$out/fusion-ios-build.log"; exit 1; }
# SwiftPM's dynamic product archive contains the binary but omits the target's
# public module and resource bundle. Stage both explicitly for iOS consumers.
framework="$out/StandardCyborgFusion-ios.xcarchive/Products/Library/Frameworks/StandardCyborgFusion.framework"
test -f "$framework/StandardCyborgFusion"
python3 "$stage" "$source" "$framework" "$out/derived-device"
find "$out/StandardCyborgFusion-ios.xcarchive" -name '*.framework' -o -name '*.bundle' -o -name '*.metallib' -o -name '*.modulemap' > "$out/fusion-products.txt"
# Device packaging reuses the same pinned framework and resource staging.
if [[ "${SDA_FUSION_DEVICE_ONLY:-0}" == 1 ]]; then
  xcodebuild -create-xcframework -framework "$framework" -output "$out/StandardCyborgFusion.xcframework" > "$out/fusion-xcframework.log" 2>&1
  exit 0
fi
sdk="$(xcrun --sdk iphoneos --show-sdk-path)"
xcrun --sdk iphoneos swiftc -typecheck -swift-version 5 -target arm64-apple-ios16.4 -sdk "$sdk" -F "$out/StandardCyborgFusion-ios.xcarchive/Products/Library/Frameworks" -import-objc-header "$ios/SdaBridge.h" "$ios/PersonalHeadDepthCapture.swift" "$ios/PersonalHeadScan.swift" "$ios/PersonalHrtfStore.swift" "$ios/PersonalHrtfCoordinator.swift" "$ios/PersonalHrtfPipelineFactory.swift" "$ios/PersonalHrtfReviewGeometry.swift" "$ios/PersonalHrtfReviewController.swift" "$ios"/PersonalHrtfFusion/*.swift 2>&1 | tee "$out/fusion-swift-typecheck.log"

# Exercise framework/resource discovery in an actual iOS Simulator process.
# A runner without a Metal GPU reports that limitation explicitly in the JSON;
# it never substitutes a host-side camera or synthetic ear scan.
xcodebuild archive -workspace "$out/FusionPackage.xcworkspace" -scheme StandardCyborgFusion -configuration Release -destination 'generic/platform=iOS Simulator' -derivedDataPath "$out/derived-simulator" -archivePath "$out/StandardCyborgFusion-simulator.xcarchive" ARCHS="$(uname -m)" ONLY_ACTIVE_ARCH=YES SKIP_INSTALL=NO BUILD_LIBRARY_FOR_DISTRIBUTION=YES CODE_SIGNING_ALLOWED=NO > "$out/fusion-simulator-build.log" 2>&1 || { tail -100 "$out/fusion-simulator-build.log"; exit 1; }
sim_products="$out/StandardCyborgFusion-simulator.xcarchive/Products/Library/Frameworks"
python3 "$stage" "$source" "$sim_products/StandardCyborgFusion.framework" "$out/derived-simulator"
codesign --force --sign - "$sim_products/StandardCyborgFusion.framework"
xcrun otool -L "$sim_products/StandardCyborgFusion.framework/StandardCyborgFusion" > "$out/fusion-linked-libraries.txt"
sim_sdk="$(xcrun --sdk iphonesimulator --show-sdk-path)"
xcrun --sdk iphonesimulator swiftc -O -parse-as-library -swift-version 5 -target "$(uname -m)-apple-ios16.4-simulator" -sdk "$sim_sdk" -F "$sim_products" -framework StandardCyborgFusion -Xlinker -rpath -Xlinker "$sim_products" "$probe" "$ios/PersonalHrtfFusion/PersonalHrtfCaptureCoverage.swift" -o "$out/fusion-resource-test" 2>&1 | tee "$out/fusion-resource-compile.log"
codesign --force --sign - "$out/fusion-resource-test"
xcrun simctl list devices available -j > "$out/fusion-simulators.json"
udid="$(python3 -c 'import json,sys;d=json.load(open(sys.argv[1]));print(next(v["udid"]for k,vs in sorted(d["devices"].items(),reverse=True)if "iOS"in k for v in vs if "iPhone"in v["name"]and v.get("isAvailable")))' "$out/fusion-simulators.json")"
xcrun simctl boot "$udid" || true
xcrun simctl bootstatus "$udid" -b
xcrun simctl spawn "$udid" "$out/fusion-resource-test" | tee "$out/fusion-resource-runtime.log"
python3 - "$out" <<'PY'
from pathlib import Path
import json,sys
root=Path(sys.argv[1]);lines=(root/'fusion-resource-runtime.log').read_text().splitlines()
result=json.loads(next(line for line in reversed(lines)if line.startswith('{')))
assert result['bundleLocated']and result['versionMetadataValid']
assert result['poissonRoundTrip']['vertices']>100 and result['poissonRoundTrip']['faces']>100
assert result['captureCoverage']['earModelLoaded'] and result['captureCoverage']['invalidDepthRejected']
assert result['physicalScanTested']is False and result['earAccuracyValidated']is False
if result['metalGPUAvailable']:assert result['reconstructionInitialized']and result['metalFunctions']>=8 and result['atomicPointCountSnapshotValid']and result['resetCompletionPassed']
(root/'fusion-resource-result.json').write_text(json.dumps(result,indent=2)+'\n')
PY
