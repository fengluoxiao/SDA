#!/usr/bin/env python3
"""Build the standalone Android FP32 convolution-chain probe (no APK or audio output)."""
import argparse, hashlib, json, os, shutil, subprocess, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--ndk', type=Path, default=os.environ.get('ANDROID_NDK_HOME') or os.environ.get('ANDROID_NDK_ROOT'))
p.add_argument('--output', type=Path, required=True)
p.add_argument('--local-size', type=int, choices=[64, 128, 256], default=256)
p.add_argument('--algorithm', choices=['radix2', 'radix4'], default='radix4')
p.add_argument('--shader-only', action='store_true')
a = p.parse_args()
if not a.ndk:
    p.error('Pass --ndk or set ANDROID_NDK_HOME')
a.ndk, a.output = a.ndk.resolve(), a.output.resolve()
a.output.mkdir(parents=True, exist_ok=True)
host = 'windows-x86_64' if sys.platform == 'win32' else 'darwin-x86_64' if sys.platform == 'darwin' else 'linux-x86_64'
exe = '.exe' if sys.platform == 'win32' else ''
llvm = a.ndk / 'toolchains/llvm/prebuilt' / host / 'bin'
glslc = a.ndk / 'shader-tools' / host / ('glslc' + exe)
shader = a.output / 'vulkan-convolution.spv'
flags = ['--target-env=vulkan1.0', '-fshader-stage=compute', '-O', f'-DCHAIN_LOCAL_SIZE={a.local_size}']
if a.algorithm == 'radix4':
    flags.append('-DCHAIN_RADIX4=1')
subprocess.run([str(glslc), *flags, str(ROOT / 'apps/native-renderer/src/vulkan_convolution.comp'), '-o', str(shader)], check=True)
# Validate basic binary invariants without an additional SPIR-V SDK dependency.
words = [int.from_bytes(shader.read_bytes()[i:i+4], 'little') for i in range(0, shader.stat().st_size, 4)]
assert words[:2] == [0x07230203, 0x00010000], 'Expected SPIR-V 1.0'
i, float_widths = 5, []
while i < len(words):
    count, op = words[i] >> 16, words[i] & 65535
    assert count and i + count <= len(words), 'Malformed instruction'
    if op == 22:
        float_widths.append(words[i+2])
    i += count
assert float_widths and set(float_widths) == {32}, 'Only FP32 is allowed'
source_paths = ['apps/native-renderer/src/vulkan_convolution.comp', 'apps/native-renderer/src/vulkan_spectral.cpp', 'apps/native-renderer/examples/vulkan_convolution_probe.rs', 'scripts/vulkan-convolution-probe.cpp', 'scripts/vulkan-convolution-RUSTFFT-LICENSE.txt']
receipt = {'schema': 1, 'shaderAlgorithm': a.algorithm, 'localSize': a.local_size, 'shaderSha256': hashlib.sha256(shader.read_bytes()).hexdigest(), 'floatWidths': sorted(set(float_widths)), 'sourceHashes': {n: hashlib.sha256((ROOT/n).read_bytes()).hexdigest() for n in source_paths}, 'runtimeAdoption': False}
if not a.shader_only:
    env = os.environ.copy()
    wrapper = '.cmd' if sys.platform == 'win32' else ''
    env.update({'CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER': str(llvm/('clang'+exe)), 'CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS': '-C link-arg=--target=aarch64-linux-android26', 'CC_aarch64_linux_android': str(llvm/('aarch64-linux-android26-clang'+wrapper)), 'CXX_aarch64_linux_android': str(llvm/('aarch64-linux-android26-clang++'+wrapper)), 'AR_aarch64_linux_android': str(llvm/('llvm-ar'+exe))})
    subprocess.run(['cargo', 'build', '--manifest-path', str(ROOT/'apps/native-renderer/Cargo.toml'), '--example', 'vulkan_convolution_probe', '--target', 'aarch64-linux-android', '--release', '--no-default-features', '--features', 'vulkan-convolution-probe', '--locked', '--offline', '-j', '3'], env=env, check=True)
    target = Path(env.get('CARGO_TARGET_DIR', ROOT/'apps/native-renderer/target'))
    binary = target/'aarch64-linux-android/release/examples/vulkan_convolution_probe'
    dest = a.output / 'vulkan_convolution_probe'
    if binary.resolve() != dest.resolve():
        shutil.copyfile(binary, dest)
    receipt['binarySha256'] = hashlib.sha256(dest.read_bytes()).hexdigest()
(a.output/'build-receipt.json').write_text(json.dumps(receipt, indent=2), encoding='utf-8')
print(f'Built {shader}; FP32 verified. Probe is separate from the playback renderer.')
