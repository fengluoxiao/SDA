"""Verify the packaged native preset table AND strings in the compiled UI.
Run after assembleRelease: python scripts/test/mobile-rendering-apk.test.py APK
This catches JSON-only edits skipped by React Native's default bundle inputs.
"""
import json
from pathlib import Path
import sys
import zipfile

root = Path(__file__).resolve().parents[2]
expected = json.loads((root / "apps/mobile/rendering-presets.json").read_text(encoding="utf-8"))
with zipfile.ZipFile(sys.argv[1]) as apk:
    actual = json.loads(apk.read("assets/rendering-presets.json"))
    assert actual == expected, "Packaged native preset table differs from source"
    bundle = apk.read("assets/index.android.bundle")
    for profile in expected:
        for field in ("label", "description"):
            value = profile[field]
            assert any(value.encode(encoding) in bundle for encoding in ("utf-8", "utf-16-le")), (
                f"Compiled UI has stale/missing {profile['id']} {field}: {value}"
            )
print("APK native preset table and compiled UI preset strings match source.")
