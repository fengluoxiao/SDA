"""Exercise actual SDK ZIP staging and environment export without a network dependency."""
from pathlib import Path
import contextlib
import importlib.util
import io
import os
import tempfile
import unittest
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('prepare_asio_sdk', ROOT / 'scripts/prepare-asio-sdk.py')
sdk = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sdk)


class AsioSdkPreparationTests(unittest.TestCase):
    def make_archive(self, root, prefix, missing=None):
        archive = root / 'sdk.zip'
        with zipfile.ZipFile(archive, 'w') as stream:
            for name in sdk.REQUIRED:
                if name != missing:
                    stream.writestr(prefix + name, '// SDK fixture\n')
            stream.writestr('asio-libclang/clang/native/libclang.dll', 'distractor')
        return archive

    def test_nested_sdk_is_selected_by_contents_without_renaming_libclang(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            archive = self.make_archive(root, 'ASIO SDK/ASIOSDK/')
            actual = sdk.extract_sdk(archive, root / 'stage')
            self.assertEqual(actual, (root / 'stage/ASIO SDK/ASIOSDK').resolve())
            self.assertTrue((root / 'stage/asio-libclang/clang/native/libclang.dll').is_file())

    def test_archive_without_a_wrapper_directory_is_supported(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.assertEqual(sdk.extract_sdk(self.make_archive(root, ''), root / 'stage'), (root / 'stage').resolve())

    def test_incomplete_sdk_is_rejected_before_export(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            with self.assertRaisesRegex(RuntimeError, 'complete ASIO SDK'):
                sdk.extract_sdk(self.make_archive(root, 'ASIOSDK/', 'host/pc/asiolist.cpp'), root / 'stage')

    def test_unsafe_archive_is_rejected_before_any_extraction(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            archive = self.make_archive(root, 'ASIOSDK/')
            with zipfile.ZipFile(archive, 'a') as stream:
                stream.writestr('../outside.txt', 'invalid')
            with self.assertRaisesRegex(RuntimeError, 'archive path'):
                sdk.extract_sdk(archive, root / 'stage')
            self.assertFalse((root / 'outside.txt').exists())
            self.assertFalse((root / 'stage').exists())

    def test_valid_configured_sdk_skips_download_and_exports_literal_path(self):
        with tempfile.TemporaryDirectory(prefix='asio sdk ') as temp:
            root = Path(temp)
            for name in sdk.REQUIRED:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('// SDK fixture\n')
            environment = root / 'github.env'
            with patch.dict(os.environ, {'CPAL_ASIO_DIR': str(root), 'GITHUB_ENV': str(environment)}), patch.object(sdk.urllib.request, 'urlopen') as network, contextlib.redirect_stdout(io.StringIO()):
                sdk.main()
            network.assert_not_called()
            self.assertEqual(environment.read_text(), 'CPAL_ASIO_DIR=' + str(root.resolve()) + '\n')


if __name__ == '__main__':
    unittest.main()
