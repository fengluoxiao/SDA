import importlib.util
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("apple_toolchain", Path(__file__).with_name("select-apple-toolchain.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ToolchainTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.apps = Path(self.temp.name)
        self.calls = []

    def app(self, version):
        (self.apps / ("Xcode_" + version + ".app")).mkdir()

    def fake_run(self, command, env):
        self.calls.append(command)
        if command == ["sw_vers", "-productVersion"]:
            return "27.0.1"
        if command == ["xcodebuild", "-version"]:
            return "Xcode 27.1\nBuild version 27A9275"
        if "--show-sdk-version" in command:
            return "27.1" if "iphoneos" in command else "27.0"
        if "--show-sdk-path" in command:
            return "/SDK/MacOSX27.0.sdk"
        raise AssertionError(command)

    def select(self, run=None):
        return module.select_toolchain("27.1", "27.1", 27, self.apps, run or self.fake_run)

    def test_exact_minor_sdk_and_separate_host_version(self):
        self.app("27.1")
        env, info = self.select()
        self.assertTrue(Path(env["DEVELOPER_DIR"]).as_posix().endswith("Xcode_27.1.app/Contents/Developer"))
        self.assertEqual(info["macOSVersion"], "27.0.1")
        self.assertEqual(info["xcodeVersion"], "27.1")
        self.assertEqual(info["iphoneosSDKVersion"], "27.1")
        self.assertEqual(info["macosSDKVersion"], "27.0")
        self.assertEqual(len(info["fingerprint"]), 64)

    def test_does_not_match_27_10_or_beta_or_fall_back_to_27_2(self):
        for version in ["27.10", "27.1beta", "27.2"]:
            self.app(version)
        with self.assertRaisesRegex(RuntimeError, "Required Xcode 27.1"):
            self.select()
        self.assertEqual(len(self.calls), 1)

    def test_rejects_missing_required_ios_sdk(self):
        self.app("27.1")
        def wrong_sdk(command, env):
            return "27.10" if "iphoneos" in command else self.fake_run(command, env)
        with self.assertRaisesRegex(RuntimeError, "Expected iPhoneOS SDK 27.1"):
            self.select(wrong_sdk)

    def test_rejects_bundle_version_mismatch(self):
        self.app("27.1")
        def wrong_version(command, env):
            return "Xcode 27.2\nBuild version wrong" if command[0] == "xcodebuild" else self.fake_run(command, env)
        with self.assertRaisesRegex(RuntimeError, "unexpected Xcode version"):
            self.select(wrong_version)

    def test_rejects_old_host(self):
        self.app("27.1")
        def old_host(command, env):
            return "26.2" if command[0] == "sw_vers" else self.fake_run(command, env)
        with self.assertRaisesRegex(RuntimeError, "Expected macOS 27"):
            self.select(old_host)

    def test_intel_probe_requires_actual_intel_output(self):
        def probe(command, env):
            if "--find" in command:
                return "/Xcode/clang++"
            if command[0] == "/usr/bin/file":
                return "toolchain-probe: Mach-O 64-bit executable x86_64"
            self.assertIn("x86_64-apple-macos12.0", command)
            self.assertIn("AudioToolbox", command)
            return ""
        info = {"macosSDKPath": "/SDK"}
        module.probe_macos({}, info, "x86_64", self.apps, probe)
        self.assertIn("x86_64", info["macosArchitectureProbe"])
        with self.assertRaisesRegex(RuntimeError, "wrong architecture"):
            module.probe_macos({}, {"macosSDKPath": "/SDK"}, "x86_64", self.apps,
                lambda command, env: "Mach-O 64-bit executable arm64" if command[0] == "/usr/bin/file" else probe(command, env))


if __name__ == "__main__":
    unittest.main()
