"""Record and verify Android release artifacts; never reads signing credentials."""
from pathlib import Path
import hashlib, json, os, re, struct, subprocess, sys, zipfile
ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'android-artifacts'
CERT = '6f34c9f4f1f04e2c7b1506514981db1d61f2c4d2d981602aa6fa0b653662217b'
ABIS = {'arm64-v8a': 183, 'x86_64': 62}
def digest(data): return hashlib.sha256(data).hexdigest()
def elf_sections(data):
    assert data[:6] == bytes([127,69,76,70,2,1])
    off = struct.unpack_from('<Q', data, 40)[0]
    size, count, index = struct.unpack_from('<HHH', data, 58)
    rows = [struct.unpack_from('<IIQQQQIIQQ', data, off + i * size) for i in range(count)]
    strings = rows[index]; names = data[strings[4]:strings[4] + strings[5]]
    return {names[row[0]:].split(bytes([0]), 1)[0].decode(): data[row[4]:row[4] + row[5]] for row in rows if row[1] != 8}
def verify_elf_alignment(data):
    assert data[:6] == bytes([127,69,76,70,2,1])
    off = struct.unpack_from('<Q', data, 32)[0]
    size, count = struct.unpack_from('<HH', data, 54)
    for i in range(count):
        row = struct.unpack_from('<IIQQQQQQ', data, off + i * size)
        if row[0] == 1: assert row[7] >= 16384, 'Native ELF requires 16 KiB LOAD alignment'
def allocated_hashes(data):
    sections = elf_sections(data)
    return {name: digest(sections[name]) for name in ['.text', '.rodata', '.data.rel.ro', '.dynsym', '.dynstr', '.eh_frame']}
def git(*args): return subprocess.check_output(['git', *args], cwd=ROOT, encoding='utf-8').strip()
def main():
    OUT.mkdir(exist_ok=True)
    config = json.loads((ROOT / 'apps/mobile/app.json').read_text(encoding='utf-8'))['expo']
    sha = git('rev-parse', 'HEAD')
    assert sha == os.environ.get('GITHUB_SHA', sha)
    if sys.argv[1] == 'stage':
        shader = (ROOT / 'apps/native-renderer/src/vulkan_resident.spv').read_bytes()
        files = subprocess.check_output(['git', 'ls-files', '-z'], cwd=ROOT).decode().split(chr(0))
        prefixes = ('apps/mobile/', 'apps/native-renderer/', 'crates/sda-native/', 'packages/core/', 'packages/player/', 'scripts/')
        extensions = ('.rs','.cpp','.c','.h','.spv','.comp','.kt','.java','.swift','.tsx','.ts','.mjs','.py','.ps1','.json','.gradle','.toml','.lock','.yaml','.yml','.xml')
        info = {'sourceCommit': sha, 'appVersion': config['version'], 'androidVersionCode': config['android']['versionCode'],
                'stereoProgrammeMakeupDb': 2.5, 'stereoCodecAnnouncement': 'common-pcm-queue-v1', 'stereoBassPreservation': 'front-direct-crossfeed-lowband-v1',
                'officialAndroidRelease': True, 'signingCertificateSha256': CERT, 'buildHost': 'GitHub Actions windows-2022',
                'workflowRunId': os.environ.get('GITHUB_RUN_ID'), 'physicalDeviceVerifiedForThisPackage': False,
                'sourceHashes': {name: digest((ROOT / name).read_bytes()) for name in files if name and name.startswith(prefixes) and name.endswith(extensions)},
                'vulkanShaderSha256': digest(shader), 'nativeLibraries': {}}
        for abi, machine in ABIS.items():
            data = (ROOT / f'apps/mobile/android/app/src/main/jniLibs/{abi}/libsda_native.so').read_bytes()
            assert struct.unpack_from('<H', data, 18)[0] == machine
            for marker in [sha.encode(), b'android-system-vulkan-compute-1.0', b'aaudio-callback', shader]: assert marker in data, abi
            info['nativeLibraries'][abi] = {'sha256': digest(data), 'allocatedSectionHashes': allocated_hashes(data)}
        text = json.dumps(info, indent=2) + chr(10)
        (ROOT / 'apps/mobile/android/app/src/main/assets/sda-build-info.json').write_text(text, encoding='utf-8')
        (OUT / 'build-info.json').write_text(text, encoding='utf-8')
        print('Recorded exact commit, native ABIs, GPU shader, UI and balance source hashes')
    elif sys.argv[1] == 'verify':
        apk = OUT / f"SDA-{config['version']}-Android-universal.apk"
        signature = (OUT / 'signature.txt').read_text(encoding='utf-8-sig')
        certificates = re.findall(r'certificate SHA-256 digest: *([0-9a-f]+)', signature, re.I)
        assert certificates and set(value.lower() for value in certificates) == {CERT}, 'Official signing certificate mismatch'
        badging = (OUT / 'badging.txt').read_text(encoding='utf-8-sig')
        assert "name='app.sda.mobile'" in badging and f"versionName='{config['version']}'" in badging
        assert f"versionCode='{config['android']['versionCode']}'" in badging
        with zipfile.ZipFile(apk) as archive:
            assert archive.testzip() is None
            info = json.loads(archive.read('assets/sda-build-info.json'))
            assert info == json.loads((OUT / 'build-info.json').read_text(encoding='utf-8'))
            assert info['sourceCommit'] == sha
            for name, value in info['sourceHashes'].items(): assert digest((ROOT / name).read_bytes()) == value, name
            assert sorted(name.split('/')[1] for name in archive.namelist() if name.endswith('/libsda_native.so')) == sorted(ABIS)
            for abi, machine in ABIS.items():
                data = archive.read(f'lib/{abi}/libsda_native.so')
                assert struct.unpack_from('<H', data, 18)[0] == machine and sha.encode() in data
                assert allocated_hashes(data) == info['nativeLibraries'][abi]['allocatedSectionHashes'], abi
            for name in archive.namelist():
                if name.startswith('lib/') and name.endswith('.so'): verify_elf_alignment(archive.read(name))
            js = archive.read('assets/index.android.bundle')
            for marker in [b'SdaSystemVolume', b'shared-mini-player-placeholder', b'player-artwork-edge-overlay']: assert marker in js, marker
            for marker in ['mock://', 'ci-stereo-tones.m4a', 'ci-360ra.mhas']:
                assert marker.encode() not in js and marker.encode('utf-16-le') not in js, marker
            assert not any(name.startswith('assets/sda-npu/') or name.endswith('/libvulkan.so') for name in archive.namelist())
        result = {'version': config['version'], 'versionCode': config['android']['versionCode'], 'sourceCommit': sha,
                  'file': apk.name, 'sha256': digest(apk.read_bytes()), 'bytes': apk.stat().st_size, 'ABIs': sorted(ABIS),
                  'officialSignatureVerified': True, 'signingCertificateSha256': CERT, 'zipAlignment16KiBVerified': True,
                  'nativeAllocatedSectionsVerified': True, 'allNativeElfAlignment16KiBVerified': True, 'buildHost': info['buildHost'], 'physicalDeviceVerified': False}
        (OUT / 'package-verification.json').write_text(json.dumps(result, indent=2) + chr(10), encoding='utf-8')
        print(json.dumps(result, indent=2))
    else: raise ValueError('Expected stage or verify')
if __name__ == '__main__': main()
