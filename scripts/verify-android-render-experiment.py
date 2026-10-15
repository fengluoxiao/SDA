"""Validate selected experiment in actual APK, not just Gradle source inputs."""
import hashlib
import json
from pathlib import Path
import sys
import zipfile

repo=Path(__file__).resolve().parents[1]
active=json.loads((repo/'apps/mobile/ios-experiments/active.json').read_text())
subdirectory={'horizontal-support-218-v1':'horizontal-support','fullheight-triangle-commonphase-542-v1':'fullheight-combined'}[active['id']]
source=repo/'apps/mobile/ios-experiments'/subdirectory/'hrtf-dense'
manifest=json.loads((source/'hrtf-set.json').read_text())
paths=list(Path(sys.argv[1]).glob('*.apk'))
if not paths:raise RuntimeError('No APK to verify')
for path in paths:
    with zipfile.ZipFile(path) as apk:
        base='assets/hrtf-restored/'
        marker=json.loads(apk.read(base+'render-experiment.json'))
        assert marker['id']==active['id']
        assert apk.read(base+'hrtf-dense/hrtf-set.json')==(source/'hrtf-set.json').read_bytes()
        for position in manifest['positions']:
            data=apk.read(base+'hrtf-dense/'+position['dry'])
            assert len(data)==4096 and hashlib.sha256(data).hexdigest()==position['assets']['dry']['sha256']
        if manifest.get('triangleSupportFile'):
            data=apk.read(base+'hrtf-dense/'+manifest['triangleSupportFile'])
            assert hashlib.sha256(data).hexdigest()==active['triangleSupportSha256']
    print('Verified actual APK rendering experiment:',path.name,active['id'])
