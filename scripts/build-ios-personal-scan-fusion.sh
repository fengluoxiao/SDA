#!/bin/bash
set -euo pipefail
out="${1:?Pass a dedicated build directory}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
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
xcodebuild archive -scheme StandardCyborgFusion -configuration Release -destination 'generic/platform=iOS' -derivedDataPath "$out/derived-device" -archivePath "$out/StandardCyborgFusion-ios.xcarchive" SKIP_INSTALL=NO BUILD_LIBRARY_FOR_DISTRIBUTION=YES CODE_SIGNING_ALLOWED=NO > "$out/fusion-ios-build.log" 2>&1 || { tail -100 "$out/fusion-ios-build.log"; exit 1; }
find "$out/StandardCyborgFusion-ios.xcarchive" -name '*.framework' -o -name '*.bundle' > "$out/fusion-products.txt"
test -n "$(find "$out/StandardCyborgFusion-ios.xcarchive" -name StandardCyborgFusion.framework -print -quit)"
