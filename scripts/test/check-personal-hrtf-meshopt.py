from pathlib import Path
import hashlib,json
ios=Path(__file__).resolve().parents[2]/'apps/mobile/modules/sda-core/ios'
vendor=ios/'PersonalHrtfNative/MeshOpt'
manifest=json.loads((vendor/'UPSTREAM.json').read_text())
assert manifest['commit']=='7b2d4f4c817aea55d74dcd65d9763ac2ca608026'
for path,digest in manifest['files'].items():
    assert hashlib.sha256((vendor/Path(path).name).read_bytes()).hexdigest()==digest,path
assert (ios/'Licenses/Meshoptimizer-MIT.txt').read_bytes()==(vendor/'LICENSE.md').read_bytes()
