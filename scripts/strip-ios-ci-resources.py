"""Remove simulator-only audio from an unsigned physical-device app copy."""
from pathlib import Path
import json,plistlib,sys

def strip(app):
    app=Path(app).resolve()
    info=plistlib.loads((app/'Info.plist').read_bytes())
    if info.get('CFBundleSupportedPlatforms') != ['iPhoneOS']:
        raise ValueError('Only an iPhoneOS app may be stripped; simulator tests retain fixtures')
    removed=[]
    for name in ['ci-stereo-tones.m4a','ci-360ra.mhas']:
        target=(app/'SdaCoreAssets.bundle'/name).resolve()
        if not target.is_relative_to(app):
            raise ValueError('Resource target escaped app')
        if target.exists():
            target.unlink();removed.append(name)
        if target.exists():
            raise ValueError('Test audio remained in physical-device app')
    return {'physicalDevice':True,'testAudioAbsent':True,'removed':removed}

if __name__=='__main__':
    print(json.dumps(strip(sys.argv[1]),indent=2))
