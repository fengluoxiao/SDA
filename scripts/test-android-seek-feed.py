"""Execute the production Android feed gate on the JVM, including seek preroll.

The expression is extracted from SdaModule rather than reimplemented here. This
catches the original circular wait even though the underlying Rust seek tests
pass: renderer-paused is true until the host feeds enough PCM for StartAt.
"""
from pathlib import Path
import argparse
import hashlib
import os
import re
import subprocess
import tempfile

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument('--source', type=Path, default=root / 'apps/mobile/modules/sda-core/android/src/main/java/app/sda/mobile/sda/SdaModule.kt')
parser.add_argument('--fetch-test-dependencies', action='store_true', help='Allow a pinned test-only JSON jar when Gradle has not resolved it')
args = parser.parse_args()
source = args.source.read_text(encoding='utf-8')
gates = re.findall(r'if \((paused \|\| [^\n]+)\) \{\s+lastProgressNs = System.nanoTime\(\)', source)
assert len(gates) == 1, 'Find the actual feeder pause/recovery gate'
# Both status() and playbackSnapshot() use this function. Renderer startup
# telemetry must not turn a seek into a user-visible pause/toggle inversion.
snapshot = source.split('private fun playbackStatus(): JSONObject', 1)[1].split('private fun queueJson()', 1)[0]
assert '.put("paused", paused)' in snapshot, 'Publish the same pause intent as iOS playbackSnapshot'

home = Path(os.environ.get('GRADLE_USER_HOME', str(Path.home() / '.gradle'))) / 'caches/modules-2/files-2.1'
java = Path(os.environ['JAVA_HOME']) / 'bin' / ('java.exe' if os.name == 'nt' else 'java')

def jar(group, name, version=None):
    base = home / group / name
    if version is None:
        version = sorted((d.name for d in base.iterdir()), key=lambda v: tuple(int(x) if x.isdigit() else 0 for x in v.split('.')))[-1]
    return next((base / version).rglob('*.jar'))

compiler = jar('org.jetbrains.kotlin', 'kotlin-compiler-embeddable')
stdlib = jar('org.jetbrains.kotlin', 'kotlin-stdlib', compiler.parent.parent.name)
annotations = jar('org.jetbrains', 'annotations')
compiler_cp = [compiler, stdlib, jar('org.jetbrains.kotlin', 'kotlin-reflect'), jar('org.jetbrains.kotlinx', 'kotlinx-coroutines-core-jvm'), annotations]
try:
    jsonjar = jar('org.json', 'json', '20180813')
except (FileNotFoundError, StopIteration):
    if not args.fetch_test_dependencies:
        raise
    jsonjar = None
template = (root / 'apps/mobile/test/seek-feed/SeekFeedRegression.kt').read_text(encoding='utf-8')
with tempfile.TemporaryDirectory(prefix='sda-seek-feed-') as temporary:
    out = Path(temporary)
    if jsonjar is None:
        import urllib.request
        jsonjar = out / 'json-20180813.jar'
        with urllib.request.urlopen('https://repo.maven.apache.org/maven2/org/json/json/20180813/json-20180813.jar', timeout=30) as response:
            data = response.read()
        assert hashlib.sha256(data).hexdigest() == '518080049ba83181914419d11a25d9bc9833a2d729b6a6e7469fa52851356da8', 'Test dependency checksum mismatch'
        jsonjar.write_bytes(data)
    kotlin = out / 'SeekFeedRegression.kt'
    kotlin.write_text(template.replace('__PRODUCTION_GATE__', gates[0]), encoding='utf-8')
    target = out / 'seek-feed.jar'
    subprocess.run([str(java), '-cp', os.pathsep.join(map(str, compiler_cp)), 'org.jetbrains.kotlin.cli.jvm.K2JVMCompiler', '-no-stdlib', '-no-reflect', '-classpath', os.pathsep.join(map(str, [stdlib, jsonjar, annotations])), '-d', str(target), str(kotlin)], check=True, cwd=root)
    subprocess.run([str(java), '-cp', os.pathsep.join(map(str, [target, stdlib, jsonjar])), 'app.sda.mobile.sda.SeekFeedRegressionKt'], check=True, cwd=root)
