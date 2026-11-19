#!/bin/bash
set -euo pipefail
out="${1:?Pass a dedicated build directory}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
adapter="$PWD/apps/mobile/modules/sda-core/ios/PersonalHrtfFusion/PersonalHrtfReconstruction.swift"
source="$(mktemp -d "$out/fusion-source.XXXXXX")"
pin=8c3bc698944ec41edaa6f627db15a6573e8e2af2
git init -q "$source"
git -C "$source" remote add origin https://github.com/StandardCyborg/StandardCyborgCocoa.git
git -C "$source" fetch --depth 1 origin "$pin"
GIT_LFS_SKIP_SMUDGE=1 git -C "$source" checkout --detach FETCH_HEAD
test "$(git -C "$source" rev-parse HEAD)" = "$pin"
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
python3 - "$source" "$framework" "$out/derived-device" <<'PY'
from pathlib import Path
import sys,shutil
source,framework,derived=map(Path,sys.argv[1:])
headers=framework/'Headers';headers.mkdir(exist_ok=True)
for p in (source/'StandardCyborgFusion/Sources/include/StandardCyborgFusion').glob('*.h'):
    shutil.copy2(p,headers/p.name)
public=['SCFusionBundle.h','SCReconstructionManager.h','SCReconstructionManagerParameters.h','SCAssimilatedFrameMetadata.h','SCPointCloud.h','SCPointCloud+FileIO.h','SCMesh.h','SCMesh+FileIO.h','SCMeshingOperation.h']
(headers/'SDAFusionPublic.h').write_text('\n'.join('#import <StandardCyborgFusion/'+p+'>'for p in public)+'\n')
modules=framework/'Modules';modules.mkdir(exist_ok=True)
(modules/'module.modulemap').write_text('framework module StandardCyborgFusion { umbrella header "SDAFusionPublic.h" export * }\n')
bundles=[p for p in derived.rglob('StandardCyborg_StandardCyborgFusion.bundle')if (p/'default.metallib').is_file()]
assert bundles,'Compiled TrueDepth Metal resource bundle is missing'
shutil.copytree(bundles[0],framework/bundles[0].name,dirs_exist_ok=True)
assert (framework/'StandardCyborg_StandardCyborgFusion.bundle/default.metallib').stat().st_size>1024
PY
find "$out/StandardCyborgFusion-ios.xcarchive" -name '*.framework' -o -name '*.bundle' -o -name '*.metallib' -o -name '*.modulemap' > "$out/fusion-products.txt"
sdk="$(xcrun --sdk iphoneos --show-sdk-path)"
xcrun --sdk iphoneos swiftc -typecheck -swift-version 5 -target arm64-apple-ios16.4 -sdk "$sdk" -F "$out/StandardCyborgFusion-ios.xcarchive/Products/Library/Frameworks" "$adapter" 2>&1 | tee "$out/fusion-swift-typecheck.log"
