"""Run the same shared-renderer fixture inside Android, without an app install."""
import json,os,subprocess,sys
from pathlib import Path
build,output=map(Path,sys.argv[1:]);root=Path(__file__).resolve().parents[1]
rows=[json.loads(line) for line in build.read_text(encoding='utf-8-sig').splitlines() if line.startswith('{')]
binary=next(x['executable'] for x in rows if x.get('reason')=='compiler-artifact' and x.get('executable') and x.get('target',{}).get('name')=='sda_native_renderer')
adb=[os.environ.get('SDA_ADB','adb')]
if os.environ.get('ANDROID_SERIAL'):adb+=['-s',os.environ['ANDROID_SERIAL']]
def run(args):
 result=subprocess.run(adb+args,capture_output=True,text=True,encoding='utf-8',errors='replace')
 if result.returncode:raise RuntimeError(result.stdout+result.stderr)
 return result.stdout+result.stderr
dest='/data/local/tmp/sda-shared-hrtfhq-test';run(['shell','mkdir','-p',dest])
run(['push',binary,dest+'/renderer-tests']);run(['shell','chmod','700',dest+'/renderer-tests'])
for directory in ['hrtf-dense','hrtf']:
 # Push the contents to an explicit child directory; repeated runs stay flat.
 run(['shell','mkdir','-p',dest+'/'+directory])
 run(['push',str(root/'assets/hrtf/apple-hrtfhq-v1'/directory)+os.sep+'.',dest+'/'+directory])
log=run(['shell','env','TMPDIR='+dest,'SDA_SHARED_PROFILE_MANIFEST='+dest+'/hrtf-dense/hrtf-set.json','SDA_SHARED_PROFILE_PCM_DIR='+dest+'/pcm',dest+'/renderer-tests','shared_hrtfhq','--nocapture','--test-threads=1']);print(log)
output.mkdir(parents=True,exist_ok=True);(output/'tests.log').write_text(log,encoding='utf-8')
for name in ['atmos.f32','360ra.f32','bed714.f32','stereo.f32']:
 run(['pull',dest+'/pcm/'+name,str(output/name)]);assert (output/name).stat().st_size==98304*2*4
(output/'platform.json').write_text(json.dumps({'platform':'Android','abi':run(['shell','getprop','ro.product.cpu.abi']).strip()},indent=2),encoding='utf-8')
