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

    def test_mobile_only_bundle_passes(self):
        verify_mobile_ios_assets(self.app)

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
        with self.assertRaisesRegex(RuntimeError, 'Invalid mobile KU100 asset'):
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
        with self.assertRaisesRegex(RuntimeError, 'Invalid mobile KU100 asset'):
            verify_mobile_ios_assets(self.app)

    def test_extra_preset_rejected(self):
        path = self.bundle / 'rendering-presets.json'
        presets = json.loads(path.read_text(encoding='utf-8'))
        path.write_text(json.dumps(presets * 2))
        with self.assertRaisesRegex(RuntimeError, 'exactly one'):
            verify_mobile_ios_assets(self.app)


if __name__ == '__main__':
    unittest.main()
