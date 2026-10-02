"""Release launch smoke test; not a substitute for physical-device listening."""
import json,os,pathlib,re,subprocess,sys,time
app=pathlib.Path(sys.argv[1]).resolve();out=pathlib.Path(sys.argv[2]).resolve()
def run(*args,check=True):
 return subprocess.run(args,check=check,text=True,capture_output=True)
runtimes=json.loads(run('xcrun','simctl','list','runtimes','-j').stdout)['runtimes']
ios=[r for r in runtimes if r.get('isAvailable') and '.iOS-' in r['identifier']]
if not ios: raise SystemExit('No available iOS simulator runtime')
runtime=max(ios,key=lambda r:tuple(map(int,r['version'].split('.'))))
types=json.loads(run('xcrun','simctl','list','devicetypes','-j').stdout)['devicetypes']
# simctl returns newest phones first on current images; never assume reverse order.
# Try newest numbered phones and let CoreSimulator check runtime compatibility.
phones=[t for t in types if t['name'].startswith('iPhone')]
phones.sort(key=lambda t:(int(re.search(r'iPhone (\d+)',t['name']).group(1)) if re.search(r'iPhone (\d+)',t['name']) else 0,t['name']),reverse=True)
udid=None
attempts=[]
for phone in phones:
 created=run('xcrun','simctl','create','SDA-CI',phone['identifier'],runtime['identifier'],check=False)
 attempts.append({'device':phone['name'],'returncode':created.returncode,'stderr':created.stderr})
 if created.returncode==0:
  udid=created.stdout.strip()
  (out/'simulator-selection.json').write_text(json.dumps({'device':phone['name'],'runtime':runtime['name'],'attempts':attempts},indent=2))
  break
if not udid:raise RuntimeError('No compatible iPhone simulator: '+json.dumps(attempts))
try:
 run('xcrun','simctl','boot',udid)
 run('xcrun','simctl','bootstatus',udid,'-b')
 run('xcrun','simctl','install',udid,str(app))
 os.environ['SIMCTL_CHILD_SDA_IOS_SMOKE']='1'
 launch=run('xcrun','simctl','launch','--stdout='+str(out/'app.stdout'),'--stderr='+str(out/'app.stderr'),udid,'app.sda.mobile')
 (out/'launch.txt').write_text(launch.stdout)
 pid=launch.stdout.strip().split(':')[-1].strip()
 container=pathlib.Path(run('xcrun','simctl','get_app_container',udid,'app.sda.mobile','data').stdout.strip())
 report=container/'Documents/sda-ci-smoke.json'
 deadline=time.monotonic()+90
 while not report.is_file() and time.monotonic()<deadline:time.sleep(2)
 if not report.is_file():raise RuntimeError('Native module did not complete audio smoke test')
 result=json.loads(report.read_text())
 (out/'audio-smoke.json').write_text(json.dumps(result,indent=2))
 if result.get('ok') is not True:raise RuntimeError('Native audio smoke failed: '+str(result))
 spatial=result.get('system360RA',{})
 if spatial.get('ok') is not True or spatial.get('status',{}).get('outputChannels')!=12 or spatial.get('allowedMultichannel') is not True:
  raise RuntimeError('360RA 7.1.4 system renderer smoke failed: '+str(spatial))
 phase=result.get('phase360RA',{})
 if (phase.get('ok') is not True or phase.get('objects')!=2 or phase.get('renderedObjectStreams')!=2
     or phase.get('coordinateMappingVerified') is not True or phase.get('gainRampVerified') is not True
     or phase.get('pauseStateVerified') is not True or phase.get('poseUpdates',0)<100):
  raise RuntimeError('PHASE object prototype failed: '+str(phase))
 motion=container/'Documents/phase-object-motion.csv'
 if not motion.is_file():raise RuntimeError('Missing PHASE object motion trace')
 (out/'phase-object-motion.csv').write_bytes(motion.read_bytes())
 alive=run('xcrun','simctl','spawn',udid,'launchctl','list').stdout
 if not any(pid==line.split()[0] and 'app.sda.mobile' in line for line in alive.splitlines() if line.split()):
  raise RuntimeError('SDA exited after launch; inspect device logs')
 run('xcrun','simctl','io',udid,'screenshot',str(out/'simulator.png'))
 # Verify copied folder resources before claiming an app can load HRTF.
 for name in ['hrtf','hrtf-dense','hrtf-raw','hrtf-dense-raw']:
  if not (app/'SdaCoreAssets.bundle'/name/'hrtf-set.json').is_file(): raise RuntimeError('Missing bundled asset '+name)
 (out/'smoke.txt').write_text('Release app launched. Compressed E-AC-3 MP4 -> Rust/KU100 -> AVAudioEngine consumed playback; live preset clock and pause/resume checks passed. 360RA MPEG-H -> 12-channel 7.1.4 -> Apple sample-buffer renderer clock/drain/pause/toggle checks passed. KU100 resources present. No real-device spatial listening validation.\n')
finally:
 log=run('xcrun','simctl','spawn',udid,'log','show','--last','2m','--style','compact','--predicate','process == "SDA"',check=False)
 (out/'simulator.log').write_text(log.stdout+log.stderr)
 run('xcrun','simctl','shutdown',udid,check=False)
