#!/usr/bin/env python3
"""Compile the checked-in Vulkan 1.0 FP32 shader; never runs audio or a GPU."""
import argparse,os,subprocess,sys
from pathlib import Path
root=Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--glslc',type=Path)
a=p.parse_args()
ndk=os.environ.get('ANDROID_NDK_HOME') or os.environ.get('ANDROID_NDK_ROOT')
compiler=a.glslc or (Path(ndk)/'shader-tools'/('windows-x86_64' if sys.platform=='win32' else 'darwin-x86_64' if sys.platform=='darwin' else 'linux-x86_64')/('glslc.exe' if sys.platform=='win32' else 'glslc') if ndk else None)
if compiler is None:p.error('Set ANDROID_NDK_HOME or pass --glslc')
source=root/'apps/native-renderer/src/vulkan_spectral.comp'
output=source.with_suffix('.spv')
subprocess.run([str(compiler),'--target-env=vulkan1.0','-fshader-stage=compute','-O',str(source),'-o',str(output)],check=True)
print(f'Compiled {output.name}: {output.stat().st_size} bytes')
