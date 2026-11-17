"""Portable regression tests for the simulator's real bundle validator."""
import json
from pathlib import Path
import shutil
import tempfile
import unittest
from verify_mobile_ios_assets import verify_mobile_ios_assets

ROOT = Path(__file__).resolve().parent.parent


class MobileAssetsTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.app = Path(self.temp.name) / 'SDA.app'
        self.bundle = self.app / 'SdaCoreAssets.bundle'
        shutil.copytree(ROOT / 'apps/mobile/assets/hrtf-restored',
                        self.bundle / 'hrtf-restored')
        shutil.copy(ROOT / 'apps/mobile/rendering-presets.json', self.bundle)

    def stage_selected(self):
        marker = ROOT / 'apps/mobile/ios-experiments/active.json'
        active = json.loads(marker.read_text(encoding='utf-8'))
        source = ROOT / active['assetFamily'] if 'assetFamily' in active else marker.parent / active['assetSubdirectory']
        shutil.copytree(source / 'hrtf-dense', self.bundle / 'hrtf-restored/hrtf-dense', dirs_exist_ok=True)
        if (source / 'hrtf').exists():
            shutil.copytree(source / 'hrtf', self.bundle / 'hrtf-restored/hrtf', dirs_exist_ok=True)
        shutil.copy(marker, self.bundle / 'ios-experiment.json')
        return active

    def test_mobile_only_bundle_passes(self):
        verify_mobile_ios_assets(self.app)

    def test_horizontal_experiment_passes_and_rejects_corruption(self):
        source = ROOT / 'apps/mobile/ios-experiments'
        active = json.loads((source / 'active.json').read_text(encoding='utf-8'))
        self.stage_selected()
        verify_mobile_ios_assets(self.app)
        path = self.bundle / 'hrtf-restored/hrtf-dense/direction-100.f32'
        path.write_bytes(b'broken')
        with self.assertRaisesRegex(RuntimeError, 'Invalid mobile HRTF asset'):
            verify_mobile_ios_assets(self.app)

    def test_explicit_direct_reflection_trial_requires_both_cue_flags_off(self):
        path = self.bundle / 'rendering-presets.json'
        presets = json.loads(path.read_text(encoding='utf-8'))
        presets[0]['iosDirectReflectionTrial'] = True
        path.write_text(json.dumps(presets), encoding='utf-8')
        with self.assertRaisesRegex(RuntimeError, 'Invalid restored'):
            verify_mobile_ios_assets(self.app)
        for directory in ['hrtf-dense', 'hrtf']:
            manifest_path = self.bundle / 'hrtf-restored' / directory / 'hrtf-set.json'
            manifest = json.loads(manifest_path.read_text(encoding='utf-8'))
            manifest['processing']['spatialCues'] = False
            manifest_path.write_text(json.dumps(manifest), encoding='utf-8')
        verify_mobile_ios_assets(self.app)

    def test_combined_triangle_sidecar_corruption_rejected(self):
        source = ROOT / 'apps/mobile/ios-experiments'
        active = json.loads((source / 'active.json').read_text(encoding='utf-8'))
        if active['directionCount'] != 542:
            self.skipTest('Combined experiment not selected')
        self.stage_selected()
        verify_mobile_ios_assets(self.app)
        (self.bundle / 'hrtf-restored/hrtf-dense/triangle-grid.json').write_bytes(b'{}')
        with self.assertRaisesRegex(RuntimeError, 'triangle support corrupt'):
            verify_mobile_ios_assets(self.app)

    def test_approved_render_policy_cannot_silently_revert(self):
        source = ROOT / 'apps/mobile/ios-experiments'
        active = json.loads((source / 'active.json').read_text(encoding='utf-8'))
        self.stage_selected()
        path = self.bundle / 'hrtf-restored/hrtf-dense/hrtf-set.json'
        manifest = json.loads(path.read_text(encoding='utf-8'))
        verify_mobile_ios_assets(self.app)
        for key in (('historicalInterpolation', 'spatialCues', 'frontEarIsolation') if active['id']=='apple-hrtfhq-542-v1' else ('historicalInterpolationMode', 'spatialCueWindow')):
            with self.subTest(key=key):
                changed = json.loads(json.dumps(manifest))
                changed['processing'].pop(key, None)
                path.write_text(json.dumps(changed), encoding='utf-8')
                with self.assertRaisesRegex(RuntimeError, 'Approved render policy mismatch'):
                    verify_mobile_ios_assets(self.app)
        path.write_text(json.dumps(manifest), encoding='utf-8')

    def test_legacy_assets_rejected(self):
        for name in ['hrtf', 'hrtf-dense', 'hrtf-raw', 'hrtf-dense-raw', 'rooms']:
            with self.subTest(name=name):
                (self.bundle / name).mkdir()
                with self.assertRaisesRegex(RuntimeError, 'Unexpected legacy/room'):
                    verify_mobile_ios_assets(self.app)
                (self.bundle / name).rmdir()

    def test_corrupt_dry_rejected(self):
        root = self.bundle / 'hrtf-restored/hrtf-dense'
        manifest = json.loads((root / 'hrtf-set.json').read_text(encoding='utf-8'))
        (root / manifest['positions'][0]['dry']).write_bytes(b'bad')
        with self.assertRaisesRegex(RuntimeError, 'Invalid mobile HRTF asset'):
            verify_mobile_ios_assets(self.app)

    def test_wet_signal_rejected_even_with_matching_hash(self):
        import hashlib
        root = self.bundle / 'hrtf-restored/hrtf-dense'
        data = b'\x01' + bytes(7)
        (root / 'zero-wet.f32').write_bytes(data)
        manifest = json.loads((root / 'hrtf-set.json').read_text(encoding='utf-8'))
        for p in manifest['positions']:
            p['wet'] = 'zero-wet.f32'
            p['assets']['wet']['sha256'] = hashlib.sha256(data).hexdigest()
        (root / 'hrtf-set.json').write_text(json.dumps(manifest))
        with self.assertRaisesRegex(RuntimeError, 'Invalid mobile HRTF asset'):
            verify_mobile_ios_assets(self.app)

    def test_extra_preset_rejected(self):
        path = self.bundle / 'rendering-presets.json'
        presets = json.loads(path.read_text(encoding='utf-8'))
        path.write_text(json.dumps(presets * 2))
        with self.assertRaisesRegex(RuntimeError, 'exactly one'):
            verify_mobile_ios_assets(self.app)


if __name__ == '__main__':
    unittest.main()
