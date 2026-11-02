"""Stage public audio only into an explicitly verified simulator application."""
from pathlib import Path
import hashlib,json,plistlib,shutil,sys

ROOT=Path(__file__).resolve().parents[1]
FIXTURES={
    "ci-stereo-tones.m4a":ROOT/"crates/sda-native/tests/fixtures/ios-stereo-tones.m4a",
    "ci-360ra.mhas":ROOT/"packages/core/mpegh/fixtures/motion.mhas",
    "ci-vivid.av3a":ROOT/"packages/core/tests/fixtures/av3a-7-1-4-three-frames.av3a",
}

def stage(app):
    app=Path(app).resolve()
    info=plistlib.loads((app/"Info.plist").read_bytes())
    if info.get("CFBundleSupportedPlatforms") != ["iPhoneSimulator"]:
        raise ValueError("Fixture staging accepts only an iPhoneSimulator app")
    assets=(app/"SdaCoreAssets.bundle").resolve()
    if not assets.is_relative_to(app) or not assets.is_dir():
        raise ValueError("Missing or escaped SdaCoreAssets simulator bundle")
    records=[]
    # Validate every destination before writing even the first fixture.
    for name,source in FIXTURES.items():
        target=(assets/name).resolve()
        if not target.is_relative_to(assets):
            raise ValueError("Simulator fixture target escaped its asset bundle")
        data=source.read_bytes()
        if target.exists() and target.read_bytes()!=data:
            raise ValueError("Unexpected existing simulator fixture: "+name)
        records.append((name,source,target,hashlib.sha256(data).hexdigest(),len(data)))
    for name,source,target,digest,size in records:
        if not target.exists():shutil.copyfile(source,target)
        if hashlib.sha256(target.read_bytes()).hexdigest()!=digest:
            raise ValueError("Simulator fixture copy mismatch: "+name)
    return {"simulatorOnly":True,"fixtures":[{"name":name,"sha256":digest,"bytes":size}for name,source,target,digest,size in records]}

if __name__=="__main__":
    print(json.dumps(stage(sys.argv[1]),indent=2))
