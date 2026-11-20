"""Download fixed public research samples for CI only; never bundle in the IPA.
No training or identification. Source: UniData public ear detection samples.
"""
import hashlib, json, sys, urllib.parse, urllib.request
from pathlib import Path
manifest = json.loads(Path(__file__).with_name('personal-hrtf-ear-fixtures.json').read_text())
output = Path(sys.argv[1]); output.mkdir(parents=True, exist_ok=True)
for image in manifest['images']:
    url = 'https://raw.githubusercontent.com/' + manifest['repository'] + '/' + manifest['commit'] + '/' + urllib.parse.quote(image['path'])
    with urllib.request.urlopen(url, timeout=60) as response:
        data = response.read(12 * 1024 * 1024)
    assert len(data) == image['bytes'] and hashlib.sha256(data).hexdigest() == image['sha256'], image['filename']
    (output / image['filename']).write_bytes(data)
(output / 'source.json').write_text(json.dumps(manifest, indent=2))
print('Downloaded and verified four fixed ear images for inference regression; not product resources')
