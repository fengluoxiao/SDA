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
 recovery=result.get('compressedReaderRecovery',{})
 if recovery.get('byteIdentical') is not True:raise RuntimeError('Compressed reader recovery duplicated/lost packets: '+str(recovery))
 restored=result.get('native360RAAfterSystem',{})
 if restored.get('ok') is not True or restored.get('status',{}).get('volumeBalanceEnabled') is not True:raise RuntimeError('System -> KU100 lost balance preference: '+str(restored))
 native=result.get('native360RA',{})
 if native.get('ok') is not True or native.get('route')!='KU100' or native.get('decodeQueue')!='sda.ios.decode' or native.get('displayedObjects')!=2:
  raise RuntimeError('360RA default KU100 decode queue regression: '+str(native))
 spatial=result.get('system360RA',{})
 if spatial.get('ok') is not True or spatial.get('status',{}).get('outputChannels')!=12 or spatial.get('allowedMultichannel') is not True or spatial.get('displayedObjects')!=2 or spatial.get('nowPlayingMetadataVerified') is not True or spatial.get('balanceToggleVerified') is not True or spatial.get('endedStateVerified') is not True:
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
 # A separate process tests the first 7.1.4 scene after cold launch, without
 # visiting the spherical 360RA scene first. Native audio smoke cannot prove GL.
 run('xcrun','simctl','terminate',udid,'app.sda.mobile')
 os.environ.pop('SIMCTL_CHILD_SDA_IOS_SMOKE',None)
 os.environ['SIMCTL_CHILD_SDA_IOS_SCENE_SMOKE']='1'
 sceneReport=container/'Documents/sda-ci-scene.json'
 if sceneReport.exists():sceneReport.unlink()
 launch=run('xcrun','simctl','launch','--stdout='+str(out/'scene.stdout'),'--stderr='+str(out/'scene.stderr'),udid,'app.sda.mobile')
 deadline=time.monotonic()+60
 while not sceneReport.is_file() and time.monotonic()<deadline:time.sleep(1)
 if not sceneReport.is_file():raise RuntimeError('Cold-start 7.1.4 scene did not report a rendered frame')
 scene=json.loads(sceneReport.read_text())
 (out/'scene-smoke.json').write_text(json.dumps(scene,indent=2))
 if scene.get('ok') is not True or scene.get('layout')!='7.1.4' or scene.get('calls',0)<=0 or scene.get('triangles',0)<=0:
  raise RuntimeError('Cold-start 7.1.4 scene failed: '+str(scene))
 time.sleep(3) # Allow submitted GL frames to reach the system compositor.
 run('xcrun','simctl','io',udid,'screenshot',str(out/'simulator.png'))
 run(sys.executable,'scripts/check-ios-scene-image.py',str(out/'simulator.png'),str(out/'scene-smoke.json'))
 # Check that registered UIKit views really use iOS 26 glass, rather than
 # silently falling back to the previous JS controls. Capture both UI surfaces.
 def chrome_report(stage):
  report=container/'Documents/sda-ci-chrome.json'
  deadline=time.monotonic()+30
  while time.monotonic()<deadline:
   if report.is_file():
    data=json.loads(report.read_text())
    controls=data['controls']
    required={'tabs','更多设置','volumeSlider'}
    if stage.startswith('settings'):required.update({'settingsSurface','distanceStepper','重新播放','停止播放'})
    ready=required.issubset(controls)
    if ready:break
   time.sleep(1)
  else:raise RuntimeError('Native chrome did not mount: '+stage)
  (out/('chrome-'+stage+'.json')).write_text(json.dumps(data,indent=2))
  expected='liquidGlass' if int(data['ios'].split('.')[0])>=26 else 'legacyMaterial'
  if data['controls']['tabs']['material']!=expected:raise RuntimeError('Wrong native tabs material: '+str(data))
  expectedButton='liquidGlass' if int(data['ios'].split('.')[0])>=26 else 'legacyButton'
  if data['controls']['更多设置']['material']!=expectedButton:raise RuntimeError('Native glass button unavailable: '+str(data))
  return data
 chrome_report('scene')
 for stage in ['player','library','settings','settings-spatial','settings-room']:
  run('xcrun','simctl','terminate',udid,'app.sda.mobile')
  os.environ.pop('SIMCTL_CHILD_SDA_IOS_SCENE_SMOKE',None)
  os.environ['SIMCTL_CHILD_SDA_IOS_CHROME_SMOKE']=stage
  (container/'Documents/sda-ci-chrome.json').unlink(missing_ok=True)
  run('xcrun','simctl','launch',udid,'app.sda.mobile')
  chrome_report(stage)
  time.sleep(3)
  run('xcrun','simctl','io',udid,'screenshot',str(out/('chrome-'+stage+'.png')))
 # Verify copied folder resources before claiming an app can load HRTF.
 for name in ['hrtf','hrtf-dense','hrtf-raw','hrtf-dense-raw']:
  if not (app/'SdaCoreAssets.bundle'/name/'hrtf-set.json').is_file(): raise RuntimeError('Missing bundled asset '+name)
 (out/'smoke.txt').write_text('Release app launched. Compressed E-AC-3 MP4 -> Rust/KU100 -> AVAudioEngine consumed playback; live preset clock and pause/resume checks passed. 360RA MPEG-H -> 12-channel 7.1.4 -> Apple sample-buffer renderer clock/drain/pause/toggle checks passed. KU100 resources present. No real-device spatial listening validation.\n')
finally:
 log=run('xcrun','simctl','spawn',udid,'log','show','--last','2m','--style','compact','--predicate','process == "SDA"',check=False)
 (out/'simulator.log').write_text(log.stdout+log.stderr)
 run('xcrun','simctl','shutdown',udid,check=False)
