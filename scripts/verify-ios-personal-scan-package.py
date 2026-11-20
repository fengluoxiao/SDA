"""Audit the produced device IPA, not just build-directory sources."""
from pathlib import Path
import sys,zipfile,plistlib,json,hashlib,subprocess,tempfile
ipa=Path(sys.argv[1])
with zipfile.ZipFile(ipa)as z:
    names=z.namelist();app='Payload/SDA.app/'
    info=plistlib.loads(z.read(app+'Info.plist'))
    assert info['CFBundleSupportedPlatforms']==['iPhoneOS']
    assert 'TrueDepth' in info.get('NSCameraUsageDescription','')
    fw=app+'Frameworks/StandardCyborgFusion.framework/'
    metadata=plistlib.loads(z.read(fw+'Info.plist'))
    assert metadata['CFBundleIdentifier']=='com.standardcyborg.StandardCyborgFusion'
    assert metadata['CFBundleShortVersionString']=='1.0.0'
    assert len(z.read(fw+'StandardCyborgFusion'))>100000
    metal=fw+'StandardCyborg_StandardCyborgFusion.bundle/default.metallib'
    assert len(z.read(metal))>1024
    model=fw+'StandardCyborg_StandardCyborgFusion.bundle/SCEarTrackingModel.mlmodelc/'
    assert sum(z.getinfo(p).file_size for p in names if p.startswith(model))>1000000,'Offline ear detector is missing'
    notice=app+'SdaCoreAssets.bundle/Licenses/StandardCyborg/provenance.json'
    provenance=json.loads(z.read(notice))
    assert provenance['commit']=='8c3bc698944ec41edaa6f627db15a6573e8e2af2'
    assert provenance['measurementOnly'] and not provenance['personalGenerationReady']
    assert len(provenance['notices'])>=1
    base=notice.rsplit('/',1)[0]+'/'
    for row in provenance['notices']:
        assert hashlib.sha256(z.read(base+row['path'])).hexdigest()==row['sha256']
    ff=app+'SdaCoreAssets.bundle/Licenses/FaceFrame/'
    faceframe=json.loads(z.read(ff+'provenance.json'))
    assert faceframe['commit']=='122fdc04ae2070991c82bf31d2c3c07b7f158d0b' and faceframe['license']=='Apache-2.0'
    assert hashlib.sha256(z.read(ff+'LICENSE')).hexdigest()==faceframe['files']['LICENSE']['upstreamSHA256']
    assert 'FaceFrame/Core/FusionEngine.swift' in faceframe['files'] and 'FaceFrame/Core/SDFAligner.swift' in faceframe['files']
    assert not any('GNMFrames/' in name or name.endswith(('frontal-control.png','faceframe-sweep.json','_medium.jpg')) for name in names)
    zip_notice=app+'SdaCoreAssets.bundle/Licenses/ZipArchive/provenance.json'
    zip_verified=False
    if zip_notice in names:
        dependency=json.loads(z.read(zip_notice))
        assert dependency['commit']=='df35718ea19a94e015b91dc4881dee028ce4cdba'
        for name,digest in dependency['files'].items():
            assert hashlib.sha256(z.read(zip_notice.rsplit('/',1)[0]+'/'+name)).hexdigest()==digest
        zip_verified=True
    if '--require-zip-notices'in sys.argv:assert zip_verified,'Final package is missing ZipArchive notices'
    bundle=z.read(app+'main.jsbundle')
    assert '个人 HRTF'.encode()in bundle or b'personalHrtfAction'in bundle
    assert not any('/ci-stereo-tones.m4a'in p or '/ci-360ra.mhas'in p for p in names)
    with tempfile.TemporaryDirectory()as tmp:
        binary=Path(tmp)/'SDA';runtime=z.read(app+info['CFBundleExecutable']);binary.write_bytes(runtime)
        for title in ['1 / 3  扫描正脸','2 / 3  扫描左耳','3 / 3  扫描右耳']:
            assert title.encode('utf8') in runtime,'Guided capture stage absent from compiled device executable: '+title
        for cue in ['手机不动，慢慢向自己的右边转头，让左耳朝向相机。','左耳已记录。手机不动，经过正面慢慢向左转头，让右耳朝向相机。','FaceFrame · 双耳侧面已记录']:
            assert cue.encode() in runtime,'Anatomical ear guide absent: '+cue
        assert '尚未获得当前侧的有效三维视角'.encode() not in runtime
        assert '已对准右耳'.encode() not in runtime
        links=subprocess.check_output(['otool','-L',str(binary)],text=True)
        assert '@rpath/StandardCyborgFusion.framework/StandardCyborgFusion'in links
    report={'ipa':ipa.name,'sha256':hashlib.sha256(ipa.read_bytes()).hexdigest(),
      'frontCameraPermission':True,'guidedCaptureEmbedded':True,'reconstructionFrameworkEmbedded':True,'metalResourcesEmbedded':True,'earDetectorEmbedded':True,
      'scanEngine':'FaceFrame','faceFrameRevision':faceframe['commit'],
      'zipArchiveNoticesVerified':zip_verified,'noticesVerified':len(provenance['notices']),'nativeEntryInBundle':True,'measurementOnly':True,
      'personalGenerationReady':False,'deviceScanTested':False,'requiresUserSigning':True}
    ipa.with_name('personal-scan-package-audit.json').write_text(json.dumps(report,indent=2))
    print(json.dumps(report,indent=2))
