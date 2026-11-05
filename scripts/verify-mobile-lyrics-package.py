"""Verify lyrics UI in the actual packaged Hermes bundle, without audio/device claims."""
from pathlib import Path
import hashlib, json, subprocess, sys, zipfile
ROOT = Path(__file__).resolve().parents[1]
SOURCES = ['apps/mobile/src/' + name for name in [
    'PortraitLyricsPager.tsx', 'mobile-lyrics.ts', 'embedded-lyrics.ts', 'lyric-files.ts',
    'use-lyrics-follow.ts', 'use-lyrics-swipe.ts', 'use-track-lyrics.ts',
    'RemotePlayer.tsx', 'IOSPlayer.tsx', 'PlatformPlayerTabs.tsx',
    'KaraokeText.tsx', 'lyric-karaoke.ts', 'TabletLandscapePlayer.tsx', 'tablet-player-layout.ts',
    'PhoneLandscapeBlur.tsx', 'phone-landscape-blur-model.ts', 'IOSUIKitTabs.tsx', 'IOSSystemTabs.tsx',
    'ios-platform-version.ts', 'IOSMetadataSheet.tsx', 'IOSNativeSettings.tsx', 'IOSMiniPlayerBackdrop.tsx', 'SideNavigationRail.tsx', 'IOSVerticalTabBar.tsx']]
MARKERS = [b'portrait-lyrics-page', b'lyrics-scroll', b'lyric-backing-', b'landscape-lyrics-page', b'lyrics-track-header',
           b'tablet-landscape-lyrics', b'lyric-word-fill', b'lyric-held-note-glow']
IOS_GLASS_MARKERS = [b'metadata-done-native-label', b'native-mini-player-content', b'mini-player-liquid-glass-backdrop']
def main():
    platform, output = sys.argv[1], Path(sys.argv[2])
    assert platform in ['Android', 'iOS']
    config = json.loads((ROOT / 'apps/mobile/app.json').read_text(encoding='utf-8'))['expo']
    packages = list(output.glob('*.apk' if platform == 'Android' else '*.ipa'))
    assert len(packages) == 1, 'Expected exactly one mobile installer'
    package = packages[0]
    sha = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, encoding='utf-8').strip()
    info = json.loads((output / 'build-info.json').read_text(encoding='utf-8'))
    assert info['sourceCommit'] == sha
    hashes = info['sourceHashes' if platform == 'Android' else 'sources']
    for name in SOURCES:
        assert hashes[name] == hashlib.sha256((ROOT / name).read_bytes()).hexdigest(), name
    if platform == 'Android': assert info['androidVersionCode'] == config['android']['versionCode']
    else: assert str(info['appBuildNumber']) == str(config['ios']['buildNumber'])
    markers = MARKERS + (IOS_GLASS_MARKERS if platform == 'iOS' else [])
    with zipfile.ZipFile(package) as archive:
        bundles = [name for name in archive.namelist() if name == 'assets/index.android.bundle' or name.startswith('Payload/') and name.endswith('.app/main.jsbundle')]
        assert len(bundles) == 1, 'Missing packaged JavaScript bundle'
        bundle = archive.read(bundles[0])
        for marker in markers: assert marker in bundle, 'Missing lyrics UI: ' + marker.decode()
        for marker in ['preview://artwork/', 'expo-artwork-preview-', 'mock://']:
            assert marker.encode() not in bundle and marker.encode('utf-16-le') not in bundle, 'Development fixture shipped: ' + marker
    result = {'platform': platform, 'version': config['version'], 'build': config['android']['versionCode'] if platform == 'Android' else config['ios']['buildNumber'],
              'sourceCommit': sha, 'file': package.name, 'bytes': package.stat().st_size, 'sha256': hashlib.sha256(package.read_bytes()).hexdigest(),
              'bundleEntry': bundles[0], 'bundleSha256': hashlib.sha256(bundle).hexdigest(), 'lyricsUiMarkers': [m.decode() for m in markers],
              'sourceFilesVerified': SOURCES, 'developmentFixturesExcluded': True, 'physicalDeviceVerified': False}
    (output / 'lyrics-package-verification.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result, indent=2))
if __name__ == '__main__': main()
