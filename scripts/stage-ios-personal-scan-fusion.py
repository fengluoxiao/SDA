"""Stage the pinned reconstruction product for an embedded iOS consumer."""
from pathlib import Path
import sys,shutil,plistlib,hashlib,subprocess
source,framework,derived=map(Path,sys.argv[1:])
# PointCloudIO asks NSBundle for this exact identifier and interpolates the
# short version with fprintf(%s). SwiftPM does not supply these product keys.
# Populate them so a local measured-support write cannot receive a nil version.
info=framework/'Info.plist'
with info.open('rb')as f:metadata=plistlib.load(f)
metadata.update(CFBundleIdentifier='com.standardcyborg.StandardCyborgFusion',CFBundleShortVersionString='1.0.0',CFBundleVersion='1')
with info.open('wb')as f:plistlib.dump(metadata,f)
headers=framework/'Headers';headers.mkdir(exist_ok=True)
for p in (source/'StandardCyborgFusion/Sources/include/StandardCyborgFusion').glob('*.h'):
    shutil.copy2(p,headers/p.name)
public=['SCFusionBundle.h','SCReconstructionManager.h','SCReconstructionManagerParameters.h','SCAssimilatedFrameMetadata.h','SCPointCloud.h','SCPointCloud+FileIO.h','SCMesh.h','SCMesh+FileIO.h','SCMeshingOperation.h']
(headers/'SDAFusionPublic.h').write_text('\n'.join('#import <StandardCyborgFusion/'+p+'>'for p in public)+'\n')
modules=framework/'Modules';modules.mkdir(exist_ok=True)
(modules/'module.modulemap').write_text('framework module StandardCyborgFusion { umbrella header "SDAFusionPublic.h" export * }\n')
bundles=[p for p in derived.rglob('StandardCyborg_StandardCyborgFusion.bundle')if (p/'default.metallib').is_file()]
assert bundles,'Compiled TrueDepth Metal resource bundle is missing'
shutil.copytree(bundles[0],framework/bundles[0].name,dirs_exist_ok=True)
assert (framework/'StandardCyborg_StandardCyborgFusion.bundle/default.metallib').stat().st_size>1024

# Explicit native ear detector; bundled for offline capture evidence only.
model=source/'StandardCyborgFusion/Sources/StandardCyborgFusion/Models/SCEarTrackingModel.mlmodel'
assert hashlib.sha256(model.read_bytes()).hexdigest() == 'a844d2b035061deb6a8a92dca2b1326ba455a50eedabd67d4381f81fe1df79b0'
subprocess.run(['xcrun','coremlcompiler','compile',str(model),str(framework/bundles[0].name)],check=True)
assert (framework/bundles[0].name/'SCEarTrackingModel.mlmodelc').is_dir()
