"""Verify the exact audition-derived profiles embedded by the shared Rust engine."""
import hashlib
import json
from pathlib import Path

root = Path(__file__).resolve().parents[1] / 'apps/native-renderer/assets/spatial-cues'
provenance = json.loads((root / 'provenance.json').read_text(encoding='utf8'))
for name, expected in provenance['profiles'].items():
    data = (root / name).read_bytes()
    assert len(data) == expected['bytes'], name
    assert hashlib.sha256(data).hexdigest() == expected['sha256'], name
    profile = json.loads(data)
    assert profile['layout'] == {'height714.json': '7.1.4', 'height360.json': '360RA-13'}[name]
    print(name, expected['sha256'])
print('Exact retained depth/elevation profile hashes verified')
