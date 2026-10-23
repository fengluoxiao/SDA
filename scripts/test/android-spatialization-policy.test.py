"""Guard per-stream spatializer opt-out and API-26-compatible native linking."""
from pathlib import Path
import subprocess
import sys

root = Path(__file__).resolve().parents[2]
native = (root / "apps/native-renderer/src/aaudio_output.rs").read_text()
java = (root / "apps/mobile/modules/sda-core/android/src/main/java/app/sda/mobile/sda/Media3Output.kt").read_text()
assert native.index("disable_system_spatialization(builder.0)") < native.index("AAudioStreamBuilder_openStream(builder.0")
for symbol in ("AAudioStreamBuilder_setSpatializationBehavior", "AAudioStreamBuilder_setIsContentSpatialized"):
    assert f'c"{symbol}"' in native
    assert f"aaudio::{symbol}" not in native, "Do not link API-32-only symbols on API 26"
assert "set(builder, 2)" in native
assert "set(builder, true)" in native
assert "if !behavior.is_null()" in native and "if !spatialized.is_null()" in native
assert java.index("setSpatializationBehavior(C.SPATIALIZATION_BEHAVIOR_NEVER)") < java.index("it.configure(")
gradle = (root / "apps/mobile/android/app/build.gradle").read_text()
assert "abiFilters(*((findProperty('reactNativeArchitectures')" in gradle, "APK must filter prebuilt JNI/AAR libraries too"
if len(sys.argv) == 3:
    symbols = subprocess.check_output([sys.argv[1], "--dyn-syms", "--wide", sys.argv[2]], text=True)
    for line in symbols.splitlines():
        if " UND " in line:
            assert "AAudioStreamBuilder_setSpatializationBehavior" not in line
            assert "AAudioStreamBuilder_setIsContentSpatialized" not in line
    print("PASS: optional API-32 functions are not required ELF imports")
print("PASS: AAudio and Media3 request no system spatialization before opening output")
