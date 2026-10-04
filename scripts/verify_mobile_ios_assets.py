"""Validate the actual iOS resource bundle, not desktop/legacy HRTF assets."""
import hashlib
import json
from pathlib import Path


def verify_mobile_ios_assets(app):
    bundle = Path(app) / 'SdaCoreAssets.bundle'
    root = bundle / 'hrtf-mobile-direct'
    manifest = json.loads((root / 'hrtf-set.json').read_text(encoding='utf-8'))
    positions = manifest['positions']
    if (len(positions) != 128 or manifest['sampleRate'] != 48000
            or manifest.get('completeSubject') is not True
            or manifest.get('processing', {}).get('mobileDirectOnly') is not True
            or manifest.get('processing', {}).get('spatialCues') is not True
            or manifest.get('processing', {}).get('preserveMeasurements') is not True
            or manifest.get('source', {}).get('brPath') is not None):
        raise RuntimeError('Invalid mobile direct-only KU100 manifest')
    if len({(p['azimuth'], p['elevation']) for p in positions}) != 128:
        raise RuntimeError('Duplicate mobile KU100 directions')
    for position in positions:
        for kind, size in [('dry', 2048), ('wet', 8)]:
            path = (root / position[kind]).resolve()
            if not path.is_relative_to(root.resolve()):
                raise RuntimeError('HRTF asset escapes resource directory')
            data = path.read_bytes()
            if (len(data) != size
                    or hashlib.sha256(data).hexdigest() != position['assets'][kind]['sha256']
                    or (kind == 'wet' and any(data))):
                raise RuntimeError('Invalid mobile KU100 asset: ' + position[kind])
    for old in ['hrtf', 'hrtf-dense', 'hrtf-raw', 'hrtf-dense-raw', 'rooms']:
        if (bundle / old).exists():
            raise RuntimeError('Unexpected legacy/room resource: ' + old)
    presets = json.loads((bundle / 'rendering-presets.json').read_text(encoding='utf-8'))
    if len(presets) != 1:
        raise RuntimeError('Mobile must contain exactly one rendering preset')
    preset = presets[0]
    expected = dict(id='object-dense', assetDirectory='hrtf-mobile-direct',
                    direct=True, directional=True, nearField=False,
                    roomId='', hrtfWetWeight=0)
    if any(preset.get(key) != value for key, value in expected.items()):
        raise RuntimeError('Invalid mobile direct-only rendering preset')
