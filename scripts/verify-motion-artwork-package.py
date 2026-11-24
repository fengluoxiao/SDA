"""Audit a focused motion-artwork iOS package without unrelated feature tests."""
from pathlib import Path
import sys, zipfile, plistlib, json, hashlib

if len(sys.argv) != 2:
    raise SystemExit("usage: verify-motion-artwork-package.py PACKAGE.ipa")
ipa = Path(sys.argv[1])
app = "Payload/SDA.app/"
with zipfile.ZipFile(ipa) as z:
    names = z.namelist()
    info = plistlib.loads(z.read(app + "Info.plist"))
    assert info["CFBundleSupportedPlatforms"] == ["iPhoneOS"]
    assert info.get("CFBundleExecutable") == "SDA"
    assert info.get("MinimumOSVersion") == "16.4"
    runtime = z.read(app + info["CFBundleExecutable"])
    bundle = z.read(app + "main.jsbundle")
    assert b"SdaMotionArtwork" in runtime
    assert b"amdl-motion/1" in runtime
    assert b"sda-motion://" in runtime
    assert b"MotionArtworkRange" in runtime
    assert b"MotionArtworkSource" in runtime
    assert b"motionArtwork" in bundle
    assert b"SdaMotionArtwork" in bundle
    assert not any("motion-artwork-cache" in name or "motion-artwork-extracted" in name for name in names)
    assert not any(name.startswith(app + "motion-artwork-cache/") for name in names)

build_info = json.loads(Path("ios-artifacts/build-info.json").read_text(encoding="utf-8"))
assert build_info["appVersion"] == "0.9.40"
assert build_info["appBuildNumber"] == "983"
assert build_info["packagingMode"] == "motion-artwork-device-package"
assert build_info["testsExecuted"] is False
assert build_info["nativeDispatchStackRegression"]["passed"] is False
motion = build_info["embeddedMotionArtwork"]
assert motion["schema"] == "amdl-motion/1"
assert motion["variant"] == "tall"
assert motion["defaultEnabled"] is False
assert motion["sourceRanges"] is True
assert motion["listUsesStaticPoster"] is True
assert build_info["programmeOutputMakeupDb"] == 1.8
assert build_info["appleLoudnessOffsetDb"] == 0.7
assert build_info["appleLoudnessDefaultEnabled"] is False
assert build_info["appleLoudnessRequiresVolumeBalance"] is True
assert build_info["stereoProgrammeMakeupDb"] == 0
assert build_info["sourceCommit"]
assert build_info["nativeLibrarySha256"]

report = {
    "ipa": ipa.name,
    "sha256": hashlib.sha256(ipa.read_bytes()).hexdigest(),
    "version": build_info["appVersion"],
    "buildNumber": build_info["appBuildNumber"],
    "sourceCommit": build_info["sourceCommit"],
    "motionArtworkReader": True,
    "portraitVariantOnly": True,
    "extractedCoverCache": False,
    "focusedScope": True,
    "unrelatedFeatureTests": False,
}
Path("ios-artifacts/motion-artwork-package-audit.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
print(json.dumps(report, indent=2))
