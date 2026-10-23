#!/usr/bin/env python3
"""Compile the real capture bridge against tagged synthetic source lanes."""
from pathlib import Path
import os, shutil, subprocess, tempfile
root=Path(__file__).resolve().parents[1]
compiler=os.environ.get('CC') or shutil.which('cc') or shutil.which('clang') or shutil.which('gcc')
if not compiler: raise SystemExit('A C compiler is required for source-capacity verification')
with tempfile.TemporaryDirectory(prefix='sda-source-capacity-') as temporary:
    build=Path(temporary).resolve()
    subprocess.run([os.environ.get('SDA_NODE','node'),str(root/'scripts/prepare-mpegh.mjs'),str(build)],check=True)
    executable=build/('capture-test.exe' if os.name=='nt' else 'capture-test')
    subprocess.run([compiler,'-std=c99','-O2','-DLC_LEVEL_4','-I'+str(build/'decoder'),
                    str(root/'packages/core/mpegh/bridge.c'),str(root/'packages/core/mpegh/source-capacity-test.c'),
                    '-o',str(executable)],check=True)
    subprocess.run([str(executable)],check=True)
