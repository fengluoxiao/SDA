#!/usr/bin/env python3
"""Exercise cached iOS archive paths without an Apple SDK or production files."""
from pathlib import Path
import importlib.util
import os
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("ios_timing", ROOT / "scripts/summarize-ios-build-timings.py")
timing = importlib.util.module_from_spec(spec)
spec.loader.exec_module(timing)

class TimingTests(unittest.TestCase):
    def test_multiple_cargo_runs_and_parallel_xcode_tasks(self):
        report = timing.inspect_log("""Finished `test` profile [optimized + debuginfo] target(s) in 2m 3.25s
 test result: ok. 12 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 5.50s
Finished `test` profile [optimized + debuginfo] target(s) in 0.05s
 test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.10s
CompileC (12 tasks) | 37.500 seconds
SwiftCompile (1 task) | 3.000 seconds
""")
        self.assertEqual(report['cargoBuildElapsedSeconds'], [123.25, 0.05])
        self.assertEqual(report['testRuns'][1]['failed'], 1)
        self.assertEqual(report['testRuns'][0]['elapsedSeconds'], 5.5)
        self.assertEqual(report['xcodeTasks'][0]['aggregateTaskSeconds'], 37.5)
        self.assertEqual(len(report['xcodeTasks']), 2)

    def test_partial_failure_log_does_not_invent_success(self):
        report = timing.inspect_log('error: native compilation failed\n')
        self.assertEqual(report, {'cargoBuildElapsedSeconds': [], 'testRuns': [], 'xcodeTasks': []})

class ArchivePaths(unittest.TestCase):
    def test_release_libraries_follow_cargo_target_directory(self):
        bash = shutil.which('bash')
        if os.name == 'nt':
            bash = str(Path(os.environ.get('ProgramFiles', 'C:/Program Files')) / 'Git/bin/bash.exe')
        if not bash or not Path(bash).exists():
            self.skipTest('bash is unavailable')
        for mode in ['default', 'relative', 'absolute']:
            with self.subTest(mode=mode), tempfile.TemporaryDirectory(prefix='sda-ios-cache-') as temporary:
                root = Path(temporary).resolve()
                scripts = root / 'scripts'
                scripts.mkdir()
                shutil.copyfile(ROOT / 'scripts/build-ios-native.sh', scripts / 'build-ios-native.sh')
                mocks = root / 'mock-bin'
                mocks.mkdir()
                def mock(name, content):
                    path = mocks / name
                    with path.open('w', encoding='utf-8', newline='\n') as output:
                        output.write('#!/usr/bin/env bash\nset -euo pipefail\n' + content)
                    path.chmod(0o755)
                mock('node', 'exit 0\n')
                mock('rustup', 'exit 0\n')
                mock('xcrun', 'if [[ "$*" == *--find* ]]; then printf "/mock/%s\\n" "${@: -1}"; else printf "/mock/sdk\\n"; fi\n')
                mock('cargo', '''[[ " $* " == *" --release "* ]]
[[ " $* " == *" --locked "* ]]
[[ " $* " != *" --profile "* ]]
target="${@: -1}"
base="${CARGO_TARGET_DIR:-$PWD/crates/sda-native/target}"
mkdir -p "$base/$target/release"
printf 'archive' > "$base/$target/release/libsda_native.a"
printf '%s\\n' "$target" >> "$PWD/targets.txt"
''')
                mock('xcodebuild', '''count=0
while (( $# )); do
 if [[ "$1" == -library ]]; then
  shift
  [[ -f "$1" ]] || { echo "Missing release archive: $1" >&2; exit 1; }
  count=$((count+1))
 fi
 shift
done
[[ "$count" == 2 ]]
''')
                setup = 'set -euo pipefail\nexport PATH="$PWD/mock-bin:$PATH"\n'
                if mode == 'relative':
                    setup += 'export CARGO_TARGET_DIR="shared cache/cargo"\n'
                elif mode == 'absolute':
                    setup += 'export CARGO_TARGET_DIR="$PWD/absolute cache/cargo"\n'
                else:
                    setup += 'unset CARGO_TARGET_DIR\n'
                setup += 'bash scripts/build-ios-native.sh\n'
                result = subprocess.run([bash, '-c', setup], cwd=root, text=True, capture_output=True)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual((root / 'targets.txt').read_text().splitlines(), ['aarch64-apple-ios', 'aarch64-apple-ios-sim'])

if __name__ == '__main__':
    unittest.main()
