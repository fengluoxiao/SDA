"""Execute the shared Rust fixture under the actual iOS simulator ABI."""
import json,os,subprocess,sys
from pathlib import Path
build,output=map(Path,sys.argv[1:]);root=Path(__file__).resolve().parents[1]
artifacts=[json.loads(line) for line in build.read_text(encoding='utf-8').splitlines() if line.startswith('{')]
binary=next(Path(x['executable']) for x in artifacts if x.get('reason')=='compiler-artifact' and x.get('executable') and x.get('target',{}).get('name')=='sda_native_renderer')
listing=json.loads(subprocess.check_output(['xcrun','simctl','list','devices','available','--json']))
choices=[d for runtime,devices in sorted(listing['devices'].items(),reverse=True) if 'iOS' in runtime for d in devices if 'iPhone' in d['name'] and d.get('isAvailable')]
assert choices,'No available iOS simulator';device=choices[0]
if device['state']!='Booted':subprocess.run(['xcrun','simctl','boot',device['udid']],check=True)
subprocess.run(['xcrun','simctl','bootstatus',device['udid'],'-b'],check=True)
subprocess.run(['codesign','--force','--sign','-',str(binary)],check=True)
output.mkdir(parents=True,exist_ok=True)
env=dict(os.environ,SIMCTL_CHILD_SDA_SHARED_PROFILE_MANIFEST=str(root/'assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json'),SIMCTL_CHILD_SDA_SHARED_PROFILE_PCM_DIR=str(output))
subprocess.run(['xcrun','simctl','spawn',device['udid'],str(binary),'shared_hrtfhq','--nocapture','--test-threads=1'],env=env,check=True)
for name in ['atmos.f32','360ra.f32','bed714.f32','stereo.f32']:assert (output/name).stat().st_size==98304*2*4
(output/'platform.json').write_text(json.dumps({'platform':'iOS simulator','device':device,'binary':str(binary)},indent=2),encoding='utf-8')
