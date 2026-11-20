#!/bin/bash
set -euo pipefail
out="${1:?Pass a dedicated build directory}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
ios="$PWD/apps/mobile/modules/sda-core/ios"
xcrun swiftc -O -parse-as-library "$ios/PersonalHeadCaptureGuide.swift" "$PWD/scripts/test/personal-hrtf-capture-guide.swift" -o "$out/capture-guide-test"
"$out/capture-guide-test" | tee "$out/capture-guide-result.json"
stage="$PWD/scripts/stage-ios-personal-scan-fusion.py"
probe="$PWD/scripts/test/personal-hrtf-fusion-resources.swift"
lifecycle="$PWD/scripts/patch-ios-personal-scan-fusion.py"
generator="$PWD/scripts/generate-embedded-numcalc.py"
source="$(mktemp -d "$out/fusion-source.XXXXXX")"
pin=8c3bc698944ec41edaa6f627db15a6573e8e2af2
git init -q "$source"
git -C "$source" remote add origin https://github.com/StandardCyborg/StandardCyborgCocoa.git
git -C "$source" fetch --depth 1 origin "$pin"
GIT_LFS_SKIP_SMUDGE=1 git -C "$source" checkout --detach FETCH_HEAD
test "$(git -C "$source" rev-parse HEAD)" = "$pin"
python3 "$lifecycle" "$source"
python3 "$PWD/scripts/test-ios-personal-tracking.py" > "$out/fusion-tracking-regression.log" 2>&1
python3 "$PWD/scripts/test-ios-personal-prediction.py" "$source/CppDependencies/Eigen/include" > "$out/fusion-prediction-regression.log" 2>&1
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
if [ "${SDA_FUSION_DEVICE_ONLY:-0}" != "1" ]; then
mkdir -p "$out/poisson-host"
xcrun clang++ -std=c++17 -O1 -g -fsanitize=address,undefined -fno-omit-frame-pointer -I "$source/CppDependencies/happly/include" "$PWD/scripts/test/personal-hrtf-ply-reader.cpp" -o "$out/ply-reader-test" > "$out/fusion-ply-reader-compile.log" 2>&1
ASAN_OPTIONS=abort_on_error=1 UBSAN_OPTIONS=halt_on_error=1 "$out/ply-reader-test" > "$out/fusion-ply-reader-runtime.log" 2>&1 || { cat "$out/fusion-ply-reader-runtime.log"; exit 1; }

xcrun clang++ -std=c++17 -O1 -g -fsanitize=address,undefined -fno-omit-frame-pointer "-DSDA_PLY_SOURCE=\"$source/CppDependencies/PoissonRecon/Sources/src/PlyFile.cpp\"" -I "$source/CppDependencies/PoissonRecon/Sources/include" "$PWD/scripts/test/personal-hrtf-ply-numeric.cpp" -o "$out/poisson-numeric-test" > "$out/fusion-ply-numeric-compile.log" 2>&1
ASAN_OPTIONS=abort_on_error=1 UBSAN_OPTIONS=halt_on_error=1 "$out/poisson-numeric-test" > "$out/fusion-ply-numeric-runtime.log" 2>&1 || { cat "$out/fusion-ply-numeric-runtime.log"; exit 1; }

xcrun clang++ -std=c++17 -O1 -g -fsanitize=address,undefined -fno-omit-frame-pointer -DSTD_LIB_FLAG -I "$source/CppDependencies/PoissonRecon/Sources/include" -I "$source/CppDependencies/PoissonRecon/Sources/include/PoissonRecon" "$PWD/scripts/test/personal-hrtf-poisson.cpp" "$source/CppDependencies/PoissonRecon/Sources/src/ExecuteEntryFunctions.cpp" "$source/CppDependencies/PoissonRecon/Sources/src/PlyFile.cpp" -o "$out/poisson-host-test" > "$out/fusion-poisson-host-compile.log" 2>&1
(cd "$out/poisson-host" && ASAN_OPTIONS=abort_on_error=1 UBSAN_OPTIONS=halt_on_error=1 "$out/poisson-host-test" 100000) > "$out/fusion-poisson-host-runtime.log" 2>&1 || { cat "$out/fusion-poisson-host-runtime.log"; exit 1; }

fi
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
xcrun --sdk iphoneos swiftc -typecheck -swift-version 5 -target arm64-apple-ios16.4 -sdk "$sdk" -F "$out/StandardCyborgFusion-ios.xcarchive/Products/Library/Frameworks" -import-objc-header "$ios/SdaBridge.h" "$ios/PersonalHeadCaptureGuide.swift" "$ios/PersonalHeadDepthCapture.swift" "$ios/PersonalHeadScan.swift" "$ios/PersonalHrtfStore.swift" "$ios/PersonalHrtfCoordinator.swift" "$ios/PersonalHrtfPipelineFactory.swift" "$ios/PersonalHrtfReviewGeometry.swift" "$ios/PersonalHrtfReviewController.swift" "$ios"/PersonalHrtfFusion/*.swift 2>&1 | tee "$out/fusion-swift-typecheck.log"

# Exercise framework/resource discovery in an actual iOS Simulator process.
# A runner without a Metal GPU reports that limitation explicitly in the JSON;
# it never substitutes a host-side camera or synthetic ear scan.
cp "$(dirname "$probe")/personal-hrtf-tracking-fusion.mm" "$source/StandardCyborgFusion/Sources/StandardCyborgFusion/Public/SdaTrackingFusionTest.mm"
xcodebuild archive -workspace "$out/FusionPackage.xcworkspace" -scheme StandardCyborgFusion -configuration Release -destination 'generic/platform=iOS Simulator' -derivedDataPath "$out/derived-simulator" -archivePath "$out/StandardCyborgFusion-simulator.xcarchive" ARCHS="$(uname -m)" ONLY_ACTIVE_ARCH=YES SKIP_INSTALL=NO BUILD_LIBRARY_FOR_DISTRIBUTION=YES CODE_SIGNING_ALLOWED=NO STRIP_INSTALLED_PRODUCT=NO DEBUG_INFORMATION_FORMAT=dwarf > "$out/fusion-simulator-build.log" 2>&1 || { tail -100 "$out/fusion-simulator-build.log"; exit 1; }
sim_products="$out/StandardCyborgFusion-simulator.xcarchive/Products/Library/Frameworks"
python3 "$stage" "$source" "$sim_products/StandardCyborgFusion.framework" "$out/derived-simulator"
codesign --force --sign - "$sim_products/StandardCyborgFusion.framework"
xcrun otool -L "$sim_products/StandardCyborgFusion.framework/StandardCyborgFusion" > "$out/fusion-linked-libraries.txt"
sim_sdk="$(xcrun --sdk iphonesimulator --show-sdk-path)"
xcrun --sdk iphonesimulator clang++ -std=c++17 -g -target "$(uname -m)-apple-ios16.4-simulator" -isysroot "$sim_sdk" -c "$(dirname "$probe")/personal-hrtf-crash-trace.mm" -o "$out/fusion-crash-trace.o"
python3 "$generator"
for unit in SdaHrtfMesh SdaHrtfSolver; do
 xcrun --sdk iphonesimulator clang++ -std=c++17 -O2 -fexceptions -target "$(uname -m)-apple-ios16.4-simulator" -isysroot "$sim_sdk" -I "$ios/PersonalHrtfNative" -c "$ios/PersonalHrtfNative/$unit.cpp" -o "$out/$unit-fusion-simulator.o"
done
xcrun --sdk iphonesimulator swiftc -O -parse-as-library -swift-version 5 -target "$(uname -m)-apple-ios16.4-simulator" -sdk "$sim_sdk" -F "$sim_products" -framework StandardCyborgFusion -Xlinker -rpath -Xlinker "$sim_products" -import-objc-header "$ios/SdaBridge.h" "$probe" "$ios/PersonalHeadCaptureGuide.swift" "$ios/PersonalHrtfFusion/PersonalHrtfCaptureCoverage.swift" "$ios/PersonalHrtfReviewGeometry.swift" "$ios/PersonalHrtfFusion/PersonalHrtfMeshCheck.swift" "$out/SdaHrtfMesh-fusion-simulator.o" "$out/SdaHrtfSolver-fusion-simulator.o" "$out/fusion-crash-trace.o" -Xlinker -lc++ -o "$out/fusion-resource-test" 2>&1 | tee "$out/fusion-resource-compile.log"
codesign --force --sign - "$out/fusion-resource-test"
xcrun simctl list devices available -j > "$out/fusion-simulators.json"
udid="$(python3 - "$out/fusion-simulators.json" <<'PY'
import json,sys,re
items=json.load(open(sys.argv[1]))['devices']
choices=[(tuple(map(int,k.rsplit('iOS-',1)[1].split('-'))),v)for k,vs in items.items()if re.fullmatch(r'com.apple.CoreSimulator.SimRuntime.iOS-[0-9]+-[0-9]+',k)for v in vs if v['name']=='iPhone 18 Pro Max'and v.get('isAvailable')]
assert choices,'Standard iPhone simulator is required'
print(max(choices,key=lambda row:row[0])[1]['udid'])
PY
)"
xcrun simctl boot "$udid" || true
xcrun simctl bootstatus "$udid" -b
set +e
python3 scripts/test/fetch-personal-hrtf-ear-fixtures.py "$out/ear-positive-fixtures"
xcrun simctl spawn "$udid" "$out/fusion-resource-test" "$out/ear-positive-fixtures" 2>&1 | tee "$out/fusion-resource-runtime.log"
probe_status=${PIPESTATUS[0]}
set -e
python3 - "$out/fusion-resource-runtime.log" "$out" "$sim_products" <<'PY'
from pathlib import Path
import sys,re,subprocess
log,out,products=map(Path,sys.argv[1:])
frames=re.findall(r'SDA_FRAME (\d+) address=(0x[0-9a-f]+) base=(0x[0-9a-f]+) image=(.*?) symbol=(.*)',log.read_text())
with (out/'fusion-crash-symbols.log').open('w') as f:
 for number,address,base,image,symbol in frames:
  if 'StandardCyborgFusion' not in image:continue
  binary=products/'StandardCyborgFusion.framework/StandardCyborgFusion'
  result=subprocess.run(['xcrun','atos','-o',str(binary),'-l',base,address],capture_output=True,text=True)
  f.write(f'{number}: {symbol}: {result.stdout}\n')
PY
if [ "$probe_status" -ne 0 ]; then cat "$out/fusion-crash-symbols.log"; exit "$probe_status"; fi
python3 - "$out" <<'PY'
from pathlib import Path
import json,sys
root=Path(sys.argv[1]);lines=(root/'fusion-resource-runtime.log').read_text().splitlines()
result=json.loads(next(line for line in reversed(lines)if line.startswith('{')))
assert result['bundleLocated']and result['versionMetadataValid']
assert result['poissonRoundTrip']['vertices']>100 and result['poissonRoundTrip']['faces']>100
assert result['partialNoRearRoundTrip']['noRearInput'] and result['partialNoRearRoundTrip']['partialMeasurementFaces']>1000
assert result['captureCoverage']['earModelLoaded'] and result['captureCoverage']['invalidDepthRejected']
assert result['physicalScanTested']is False and result['earAccuracyValidated']is False
if result['metalGPUAvailable']:assert result['reconstructionInitialized']and result['metalFunctions']>=8 and result['atomicPointCountSnapshotValid']and result['resetCompletionPassed']
(root/'fusion-resource-result.json').write_text(json.dumps(result,indent=2)+'\n')
PY
