"""Validate the actual iOS resource bundle, not desktop/legacy HRTF assets."""
import hashlib
import json
from pathlib import Path


def verify_mobile_ios_assets(app):
    bundle = Path(app) / 'SdaCoreAssets.bundle'
    presets = json.loads((bundle / 'rendering-presets.json').read_text(encoding='utf-8'))
    direct_trial = len(presets) == 1 and presets[0].get('iosDirectReflectionTrial') is True
    experiment_path = bundle / 'ios-experiment.json'
    experiment = json.loads(experiment_path.read_text(encoding='utf-8')) if experiment_path.exists() else None
    supported = {'horizontal-support-218-v1':218, 'fullheight-triangle-commonphase-542-v1':542}
    if experiment and (experiment.get('id') not in supported or experiment.get('directionCount') != supported[experiment['id']]):
        raise RuntimeError('Unknown iOS experiment')
    for directory, count in [('hrtf-dense', experiment['directionCount'] if experiment else 61), ('hrtf', 17)]:
        root = bundle / 'hrtf-restored' / directory
        manifest = json.loads((root / 'hrtf-set.json').read_text(encoding='utf-8'))
        positions = manifest['positions']
        processing = manifest.get('processing', {})
        if experiment and directory == 'hrtf-dense':
            if manifest.get('iosExperiment') != experiment['id'] or manifest.get('subjectId') != 'ku100' or manifest.get('completeSubject') is not True:
                raise RuntimeError('Invalid experiment provenance')
            if experiment['id'] == 'horizontal-support-218-v1' and any(p['elevation'] != 0 for p in positions[74:]):
                raise RuntimeError('Unexpected added height response in horizontal experiment')
            if experiment['id'] == 'fullheight-triangle-commonphase-542-v1':
                if manifest.get('triangleSupportFile') != 'triangle-grid.json':
                    raise RuntimeError('Combined experiment triangle support missing')
                data = (root / 'triangle-grid.json').read_bytes()
                if hashlib.sha256(data).hexdigest() != experiment.get('triangleSupportSha256'):
                    raise RuntimeError('Combined experiment triangle support corrupt')
                grid = json.loads(data)
                if [(p['azimuth'],p['elevation']) for p in grid['positions']] != [(p['azimuth'],p['elevation']) for p in positions]:
                    raise RuntimeError('Triangle direction order mismatch')
        if (len(positions) != count or manifest['sampleRate'] != 48000
                or processing.get('calibrated') is not True
                or processing.get('historicalInterpolation') is not True
                or processing.get('spatialCues') is not (False if direct_trial else True)
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
