"""Exercise fixture presence and the physical-device/symlink safety boundary."""
from pathlib import Path
import plistlib,tempfile,unittest
from ios_simulator_fixtures import stage,FIXTURES

class Fixtures(unittest.TestCase):
    def app(self,root,platform="iPhoneSimulator"):
        app=Path(root)/"SDA.app";app.mkdir()
        (app/"Info.plist").write_bytes(plistlib.dumps({"CFBundleSupportedPlatforms":[platform]}))
        (app/"SdaCoreAssets.bundle").mkdir();return app
    def test_real_public_fixtures_are_exact_and_staging_is_idempotent(self):
        with tempfile.TemporaryDirectory(prefix="sda-fixture-test-")as root:
            app=self.app(root);report=stage(app);self.assertEqual(len(report["fixtures"]),3)
            for name,source in FIXTURES.items():self.assertEqual((app/"SdaCoreAssets.bundle"/name).read_bytes(),source.read_bytes())
            self.assertEqual(stage(app),report)
    def test_physical_device_app_is_rejected_before_any_write(self):
        with tempfile.TemporaryDirectory(prefix="sda-fixture-test-")as root:
            app=self.app(root,"iPhoneOS")
            with self.assertRaisesRegex(ValueError,"iPhoneSimulator"):stage(app)
            self.assertEqual(list((app/"SdaCoreAssets.bundle").iterdir()),[])
    def test_mismatched_fixture_is_rejected_without_partial_staging(self):
        with tempfile.TemporaryDirectory(prefix="sda-fixture-test-")as root:
            app=self.app(root);bad=app/"SdaCoreAssets.bundle/ci-vivid.av3a";bad.write_bytes(b"wrong audio")
            with self.assertRaisesRegex(ValueError,"Unexpected existing"):stage(app)
            self.assertEqual(list(bad.parent.iterdir()),[bad]);self.assertEqual(bad.read_bytes(),b"wrong audio")
    def test_symlinked_fixture_cannot_escape_bundle(self):
        with tempfile.TemporaryDirectory(prefix="sda-fixture-test-")as root:
            app=self.app(root);outside=Path(root)/"outside.av3a";outside.write_bytes(b"preserve")
            link=app/"SdaCoreAssets.bundle/ci-vivid.av3a"
            try:link.symlink_to(outside)
            except OSError as e:self.skipTest("Host cannot create symlinks: "+str(e))
            with self.assertRaisesRegex(ValueError,"escaped"):stage(app)
            self.assertEqual(outside.read_bytes(),b"preserve")

if __name__=="__main__":unittest.main()
