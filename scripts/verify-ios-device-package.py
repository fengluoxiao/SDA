"""Check IPA identity/provenance only; never run feature or playback tests."""
from pathlib import Path
import hashlib
import json
import plistlib
import re
import struct
import subprocess
import sys
import zipfile

if len(sys.argv) != 2:
    raise SystemExit("usage: verify-ios-device-package.py PACKAGE.ipa")

root = Path(__file__).resolve().parents[1]
ipa = Path(sys.argv[1])
config = json.loads((root / "apps/mobile/app.json").read_text(encoding="utf-8"))["expo"]
build = json.loads((ipa.parent / "build-info.json").read_text(encoding="utf-8"))
source = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
assert re.fullmatch(r"[a-f0-9]{40}", source)
assert build["sourceCommit"] == build["sourceCheckoutCommit"] == source
assert build["packagingMode"] == "device-package-only"
assert build["testsExecuted"] is False
assert build["nativeDispatchStackRegression"]["passed"] is False
assert build["devicePlaybackTested"] is False
assert build["appVersion"] == config["version"]
assert str(build["appBuildNumber"]) == config["ios"]["buildNumber"]
assert re.fullmatch(r"[a-f0-9]{64}", build["nativeLibrarySha256"])

for name in ("lib.rs", "near_field.rs", "authored.rs", "protocol.rs", "object_mixer.rs",
             "spatial.rs", "convolution.rs", "headphone.rs", "render_command.rs", "perceptual_masking.rs"):
    relative = "apps/native-renderer/src/" + name
    assert build["sources"][relative] == hashlib.sha256((root / relative).read_bytes()).hexdigest(), relative

with zipfile.ZipFile(ipa) as archive:
    prefix = "Payload/SDA.app/"
    info = plistlib.loads(archive.read(prefix + "Info.plist"))
    assert info["CFBundleSupportedPlatforms"] == ["iPhoneOS"]
    assert info["CFBundleIdentifier"] == config["ios"]["bundleIdentifier"]
    assert info["CFBundleShortVersionString"] == config["version"]
    assert str(info["CFBundleVersion"]) == config["ios"]["buildNumber"]
    assert info["MinimumOSVersion"] == "16.4"
    runtime = archive.read(prefix + info["CFBundleExecutable"])
    assert struct.unpack("<II", runtime[:8]) == (0xFEEDFACF, 0x0100000C), "Expected arm64 Mach-O"
    assert source.encode("ascii") in runtime, "Native executable does not embed the selected source commit"
    assert archive.getinfo(prefix + "main.jsbundle").file_size > 0

report = {
    "ipa": ipa.name,
    "sha256": hashlib.sha256(ipa.read_bytes()).hexdigest(),
    "version": build["appVersion"],
    "buildNumber": build["appBuildNumber"],
    "sourceCommit": source,
    "nativeSourceCommitEmbedded": True,
    "architecture": "arm64",
    "packageOnly": True,
    "testsExecuted": False,
    "physicalDeviceTested": False,
}
(ipa.parent / "device-package-audit.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
print(json.dumps(report, indent=2))
