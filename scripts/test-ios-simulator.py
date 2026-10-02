"""Release launch smoke test; not a substitute for physical-device listening."""
import json,os,pathlib,subprocess,sys,time
app=pathlib.Path(sys.argv[1]).resolve();out=pathlib.Path(sys.argv[2]).resolve()
def run(*args,check=True):
 return subprocess.run(args,check=check,text=True,capture_output=True)
runtimes=json.loads(run('xcrun','simctl','list','runtimes','-j').stdout)['runtimes']
ios=[r for r in runtimes if r.get('isAvailable') and '.iOS-' in r['identifier']]
if not ios: raise SystemExit('No available iOS simulator runtime')
runtime=max(ios,key=lambda r:tuple(map(int,r['version'].split('.'))))
types=json.loads(run('xcrun','simctl','list','devicetypes','-j').stdout)['devicetypes']
phone=next(t for t in reversed(types) if t['name'].startswith('iPhone'))
udid=run('xcrun','simctl','create','SDA-CI',phone['identifier'],runtime['identifier']).stdout.strip()
try:
 run('xcrun','simctl','boot',udid)
 run('xcrun','simctl','bootstatus',udid,'-b')
 run('xcrun','simctl','install',udid,str(app))
 launch=run('xcrun','simctl','launch','--stdout='+str(out/'app.stdout'),'--stderr='+str(out/'app.stderr'),udid,'app.sda.mobile')
 (out/'launch.txt').write_text(launch.stdout)
 pid=launch.stdout.strip().split(':')[-1].strip()
 time.sleep(20)
 alive=run('xcrun','simctl','spawn',udid,'launchctl','list').stdout
 if not any(pid==line.split()[0] and 'app.sda.mobile' in line for line in alive.splitlines() if line.split()):
  raise RuntimeError('SDA exited after launch; inspect device logs')
 run('xcrun','simctl','io',udid,'screenshot',str(out/'simulator.png'))
 # Verify copied folder resources before claiming an app can load HRTF.
 for name in ['hrtf','hrtf-dense','hrtf-raw','hrtf-dense-raw']:
  if not (app/'SdaCoreAssets.bundle'/name/'hrtf-set.json').is_file(): raise RuntimeError('Missing bundled asset '+name)
 (out/'smoke.txt').write_text('Release app launched and remained alive for 20s. KU100 resources present. No real-device audio validation.\n')
finally:
 log=run('xcrun','simctl','spawn',udid,'log','show','--last','2m','--style','compact','--predicate','process == "SDA"',check=False)
 (out/'simulator.log').write_text(log.stdout+log.stderr)
 run('xcrun','simctl','shutdown',udid,check=False)
