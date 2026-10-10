"""Validate the actual iOS resource bundle, not desktop/legacy HRTF assets."""
import hashlib
import json
from pathlib import Path


def verify_mobile_ios_assets(app):
    bundle = Path(app) / 'SdaCoreAssets.bundle'
    for directory, count in [('hrtf-dense', 61), ('hrtf', 17)]:
        root = bundle / 'hrtf-restored' / directory
        manifest = json.loads((root / 'hrtf-set.json').read_text(encoding='utf-8'))
        positions = manifest['positions']
        processing = manifest.get('processing', {})
        if (len(positions) != count or manifest['sampleRate'] != 48000
                or processing.get('calibrated') is not True
                or processing.get('historicalInterpolation') is not True
                or processing.get('spatialCues') is not True
                or processing.get('mobileDirectOnly') is not False):
            raise RuntimeError('Invalid restored KU100 manifest')
        if len({(p['azimuth'], p['elevation']) for p in positions}) != count:
            raise RuntimeError('Duplicate mobile KU100 directions')
        for position in positions:
            if position['wet'] != position['dry']:
                raise RuntimeError('Invalid mobile KU100 asset: measured room residual forbidden')
            for kind in ['dry', 'wet']:
                path = (root / position[kind]).resolve()
                if not path.is_relative_to(root.resolve()):
                    raise RuntimeError('HRTF asset escapes resource directory')
                data = path.read_bytes()
                if (len(data) != 4096
                        or hashlib.sha256(data).hexdigest() != position['assets'][kind]['sha256']):
                    raise RuntimeError('Invalid mobile KU100 asset: ' + position[kind])
    for old in ['hrtf', 'hrtf-dense', 'hrtf-raw', 'hrtf-dense-raw', 'rooms']:
        if (bundle / old).exists():
            raise RuntimeError('Unexpected legacy/room resource: ' + old)
    presets = json.loads((bundle / 'rendering-presets.json').read_text(encoding='utf-8'))
    if len(presets) != 1:
        raise RuntimeError('Mobile must contain exactly one rendering preset')
    preset = presets[0]
    expected = dict(id='object-dense', assetDirectory='hrtf-restored/hrtf-dense',
                    direct=True, directional=True, nearField=False,
                    roomId='', hrtfWetWeight=0)
    if any(preset.get(key) != value for key, value in expected.items()):
        raise RuntimeError('Invalid mobile direct-only rendering preset')
