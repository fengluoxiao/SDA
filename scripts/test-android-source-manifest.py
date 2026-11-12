"""Ensure staged Android sources satisfy the actual package verification contract."""
from pathlib import Path
import hashlib
import importlib.util
import unittest

ROOT = Path(__file__).resolve().parents[1]

def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / 'scripts' / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

record = load('android_release_record', 'record-android-release.py')
verify = load('mobile_package_verify', 'verify-mobile-lyrics-package.py')

class AndroidSourceManifestTests(unittest.TestCase):
    def test_manifest_covers_every_required_packaged_source(self):
        names = list(verify.SOURCES)
        names += ['vendor/folia/paper-shaders/dist/shader-sizing.js',
                  'vendor/folia/paper-shaders/dist/shader-utils.js']
        hashes = record.source_hashes(names)
        self.assertEqual(set(names) - hashes.keys(), set(), 'Stage omitted required package sources')
        for name in names:
            self.assertEqual(hashes[name], hashlib.sha256((ROOT / name).read_bytes()).hexdigest())

    def test_unrelated_files_do_not_enter_source_manifest(self):
        self.assertEqual(record.source_hashes(['', 'node_modules/not-a-source.js',
                                             'apps/mobile/assets/not-a-source.png']), {})

if __name__ == '__main__':
    unittest.main()
