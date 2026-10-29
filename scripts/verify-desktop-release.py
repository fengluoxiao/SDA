#!/usr/bin/env python3
"""Verify real desktop payload architecture, version, native provenance and installer contents."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import struct
import subprocess
import tempfile

CPU = {0x1000007: 'x64', 0x100000c: 'arm64', 0x8664: 'x64', 0xaa64: 'arm64'}
def architectures(data):
    if data[:2] == b'MZ':
        at = struct.unpack_from('<I', data, 0x3c)[0]
        if data[at:at+4] != bytes([80,69,0,0]): raise ValueError('Invalid PE signature')
        return {CPU.get(struct.unpack_from('<H', data, at+4)[0], 'unsupported')}
    if data[:4] in (b'\xcf\xfa\xed\xfe',b'\xfe\xed\xfa\xcf'):
        return {CPU.get(struct.unpack_from('<I' if data[0]==0xcf else '>I',data,4)[0],'unsupported')}
    if data[:4] in (b'\xca\xfe\xba\xbe',b'\xca\xfe\xba\xbf'):
        count=struct.unpack_from('>I',data,4)[0]; stride=32 if data[3]==0xbf else 20
        if count>16: raise ValueError('Invalid fat architecture count')
        return {CPU.get(struct.unpack_from('>I',data,8+i*stride)[0],'unsupported') for i in range(count)}
    raise ValueError('Unknown executable format')
def sha(path):
    with path.open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()
def asar_package(path):
    with path.open('rb') as f:
        sizes=struct.unpack('<4I',f.read(16)); assert sizes[0]==4 and sizes[3]<64*1024*1024
        header=json.loads(f.read(sizes[3])); item=header['files']['package.json'];assert not item.get('unpacked')
        f.seek(8+sizes[1]+int(item['offset']));return json.loads(f.read(item['size']))
def validate_package_metadata(pkg, version):
    assert pkg['version']==version and Path(pkg.get('main','')).as_posix()=='main.cjs'
def verify_payload(app, platform, arch, commit, version):
    resources=app/'resources' if platform=='Windows' else app/'Contents/Resources'
    suffix='.exe' if platform=='Windows' else ''
    roles={'app':app/'SDA.exe' if platform=='Windows' else app/'Contents/MacOS/SDA',
           'renderer':resources/'native-renderer'/('SdaNativeRenderer'+suffix),
           'room':resources/'native-renderer'/('SdaRoomReference'+suffix),
           'headTracking':resources/'head-tracking-helper'/('SdaAirPodsHeadTracking'+suffix),
           'ffmpeg':resources/'audio-encoder'/('ffmpeg'+suffix)}
    for role,path in roles.items():
        with path.open('rb') as f: data=f.read(65536)
        assert arch in architectures(data), f'{role}: wrong architecture {architectures(data)}, expected {arch}'
    info=json.loads((resources/'native-renderer/build-info.json').read_text(encoding='utf-8'))
    assert info['sourceCommit']==commit,'Wrong native renderer source'
    assert info['sha256']==sha(roles['renderer']) and info['roomReferenceSha256']==sha(roles['room'])
    binary=roles['renderer'].read_bytes()
    for marker in [commit.encode(),b'bedExportReady',b'SDAR Bed Channel with Objects']:assert marker in binary,marker
    pkg=asar_package(resources/'app.asar');validate_package_metadata(pkg,version)
    if platform=='macOS':
        p=plistlib.loads((app/'Contents/Info.plist').read_bytes());assert p['CFBundleShortVersionString']==version
    if platform=='Windows' and arch=='arm64':assert not (resources/'head-tracking-driver').exists(),'x64 kernel driver in ARM64 package'
    roles['asar']=resources/'app.asar'
    return {role:{'path':path.relative_to(app).as_posix(),'sha256':sha(path)} for role,path in roles.items()}
def inspect_container(installer, platform, arch, commit, version):
    if platform=='Windows':
        with tempfile.TemporaryDirectory(prefix='sda-release-inspect-') as name:
            base=Path(name).resolve();assert base.is_relative_to(Path(tempfile.gettempdir()).resolve())
            subprocess.run(['7z','x','-y',str(installer),'-o'+str(base/'outer')],check=True,stdout=subprocess.DEVNULL)
            archives=list((base/'outer').rglob('app-*.7z'));assert len(archives)==1,'Expected one architecture per installer'
            subprocess.run(['7z','x','-y',str(archives[0]),'-o'+str(base/'inner')],check=True,stdout=subprocess.DEVNULL)
            mains=list((base/'inner').rglob('SDA.exe'));assert len(mains)==1
            return verify_payload(mains[0].parent,platform,arch,commit,version)
    else:
        reply=subprocess.check_output(['hdiutil','attach','-nobrowse','-readonly','-plist',str(installer)])
        mounts=[x['mount-point'] for x in plistlib.loads(reply)['system-entities'] if x.get('mount-point')];assert len(mounts)==1
        try: return verify_payload(Path(mounts[0])/'SDA.app',platform,arch,commit,version)
        finally: subprocess.run(['hdiutil','detach',mounts[0]],check=True,stdout=subprocess.DEVNULL)
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--platform',choices=['Windows','macOS'],required=True);p.add_argument('--arch',choices=['x64','arm64'],required=True);p.add_argument('--app-dir',type=Path);p.add_argument('--installer',type=Path,required=True);p.add_argument('--evidence-dir',type=Path,default=Path('desktop-artifacts'));p.add_argument('--verify-container',action='store_true');p.add_argument('--from-installer',action='store_true');a=p.parse_args()
    root=Path(os.environ.get('GITHUB_WORKSPACE',Path(__file__).resolve().parents[1]));version=json.loads((root/'apps/desktop/package.json').read_text(encoding='utf-8'))['version'];commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()
    assert a.installer.name==f'SDA-{version}-{a.platform}-{a.arch}'+('.exe' if a.platform=='Windows' else '.dmg')
    if a.from_installer:files=inspect_container(a.installer.resolve(),a.platform,a.arch,commit,version)
    else:
        assert a.app_dir,'--app-dir required unless inspecting an existing installer'
        files=verify_payload(a.app_dir.resolve(),a.platform,a.arch,commit,version)
        if a.verify_container:assert inspect_container(a.installer.resolve(),a.platform,a.arch,commit,version)==files,'Installer payload differs from verified application'
    record={'schema':'sda.desktop-release','version':1,'appVersion':version,'sourceCommit':commit,'platform':a.platform,'architecture':a.arch,'artifact':{'name':a.installer.name,'bytes':a.installer.stat().st_size,'sha256':sha(a.installer)},'verifiedPayload':files,'installerContentsVerified':a.verify_container or a.from_installer,'packagingVerifierCommit':os.environ.get('GITHUB_WORKFLOW_SHA'),'physicalDeviceVerified':False,'signed':False}
    a.evidence_dir.mkdir(parents=True,exist_ok=True);(a.evidence_dir/f'{a.platform}-{a.arch}-verification.json').write_text(json.dumps(record,indent=2)+'\n');print(json.dumps(record))
if __name__=='__main__':main()
