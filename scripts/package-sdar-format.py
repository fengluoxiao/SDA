#!/usr/bin/env python3
"""Package the SDAR codec sources/docs and an explicitly supplied Windows tool."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import struct
import subprocess
import zipfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--ref', required=True)
parser.add_argument('--codec', type=Path, required=True)
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--git-dir')
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
git = ['git'] + (['--git-dir=' + args.git_dir] if args.git_dir else [])
def read_git(*values):
    return subprocess.check_output(git + list(values), cwd=root)
commit = read_git('rev-parse', args.ref + '^{commit}').decode().strip()
version = json.loads(read_git('show', commit + ':apps/mobile/app.json'))['expo']['version']
paths = ['readmeFormat.md', 'crates/sda-format', 'vendor/faad2',
         'docs/sda-interchange-v1.md', 'docs/sdar-object-constraints.md',
         'docs/sda-compact-quality.md', 'docs/sdar-wave-archive.md',
         'docs/sdar-bed-channel-v1.md', 'docs/sdar-bed-lossless-v1.md',
         'scripts/package-sdar-format.py']
archive = read_git('archive', '--format=zip', commit, '--', *paths)
exe = args.codec.read_bytes()
assert exe[:2] == b'MZ', 'Expected Windows executable'
pe = struct.unpack_from('<I', exe, 0x3c)[0]
assert exe[pe:pe+4] == b'PE\0\0' and struct.unpack_from('<H', exe, pe+4)[0] == 0x8664, 'Expected x64 PE executable'
identity = json.loads(subprocess.check_output([str(args.codec), '--version'], cwd=root))
assert identity['codecName'] == 'SDAR Bed Channel'
manifest = {'schema': 'sda.format-package', 'version': 1, 'appVersion': version,
            'sourceCommit': commit, 'codec': identity,
            'windowsTool': {'path': 'bin/windows-x64/sdar-codec.exe',
                            'sha256': hashlib.sha256(exe).hexdigest()},
            'scope': 'codec, file metadata and scene resources; integrate a compatible authored-scene renderer for playback'}
args.output.parent.mkdir(parents=True, exist_ok=True)
with zipfile.ZipFile(io.BytesIO(archive)) as source, zipfile.ZipFile(args.output, 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=9) as result:
    names = source.namelist()
    assert 'crates/sda-format/src/lib.rs' in names
    assert 'vendor/faad2/COPYING' in names
    for item in source.infolist():
        if not item.is_dir():
            result.writestr(item.filename, source.read(item.filename))
    result.writestr(manifest['windowsTool']['path'], exe)
    result.writestr('FORMAT-PACKAGE.json', json.dumps(manifest, ensure_ascii=False, indent=2) + '\n')
with zipfile.ZipFile(args.output) as result:
    assert result.testzip() is None
    assert json.loads(result.read('FORMAT-PACKAGE.json'))['sourceCommit'] == commit
print(json.dumps({'file': str(args.output), 'bytes': args.output.stat().st_size,
                  'sha256': hashlib.sha256(args.output.read_bytes()).hexdigest(), 'sourceCommit': commit}))
