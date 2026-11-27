#!/bin/bash
set -euo pipefail
out="${1:?Pass motion artwork test directory}"
udid="${2:?Pass the booted standard iPhone simulator}"
ios="$PWD/apps/mobile/modules/sda-core/ios"
app="$out/MotionArtworkUITest.app"
mkdir -p "$app"
sim_sdk="$(xcrun --sdk iphonesimulator --show-sdk-path)"
xcrun swiftc -O scripts/test/motion-artwork-screen-difference.swift -o "$out/screen-difference"
xcrun --sdk iphonesimulator swiftc -O -parse-as-library -swift-version 5 -D SDA_MOTION_UI_TEST -target "$(uname -m)-apple-ios16.4-simulator" -sdk "$sim_sdk" \
 "$ios/MotionArtworkRange.swift" "$ios/MotionArtworkSource.swift" "$ios/MotionArtworkReverseClock.swift" "$ios/MotionArtworkBackdrop.swift" "$ios/MotionArtworkReverseFrames.swift" "$ios/MotionArtworkMask.swift" \
 "$out/MotionArtworkViewUnderTest.swift" scripts/test/motion-artwork-ui.swift -o "$app/MotionArtworkUITest"
cp "$out/host-inputs/long-gop.m4a" "$app/motion.m4a"
python3 - "$app/Info.plist" <<'PY'
import plistlib,sys
with open(sys.argv[1],'wb')as f:plistlib.dump({'CFBundleIdentifier':'app.sda.motion-artwork-ui-test','CFBundleExecutable':'MotionArtworkUITest','CFBundleName':'MotionArtworkUITest','CFBundlePackageType':'APPL','CFBundleVersion':'1','CFBundleShortVersionString':'1.0','MinimumOSVersion':'16.4','LSRequiresIPhoneOS':True,'UILaunchScreen':{},'UIApplicationSceneManifest':{'UIApplicationSupportsMultipleScenes':False,'UISceneConfigurations':{'UIWindowSceneSessionRoleApplication':[{'UISceneConfigurationName':'Motion','UISceneDelegateClassName':'MotionArtworkTestScene'}]}},'UIDeviceFamily':[1,2],'CFBundleSupportedPlatforms':['iPhoneSimulator'],'UISupportedInterfaceOrientations':['UIInterfaceOrientationPortrait','UIInterfaceOrientationLandscapeLeft','UIInterfaceOrientationLandscapeRight']},f)
PY
codesign --force --sign - "$app"
python3 - "$out" "$udid" "$app" <<'PY'
import json,subprocess,sys,time,os,shutil
from pathlib import Path
out,udid,app=Path(sys.argv[1]),sys.argv[2],sys.argv[3]
bundle='app.sda.motion-artwork-ui-test'
subprocess.run(['xcrun','simctl','uninstall',udid,bundle],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=30)
subprocess.run(['xcrun','simctl','install',udid,app],check=True,timeout=120)
launch=subprocess.check_output(['xcrun','simctl','launch',udid,bundle],text=True,timeout=120)
print(launch,flush=True);pid=int(launch.strip().rsplit(':',1)[1])
container=Path(subprocess.check_output(['xcrun','simctl','get_app_container',udid,bundle,'data'],text=True,timeout=120).strip())/'Documents'
container.mkdir(parents=True,exist_ok=True)
request,result,ack=container/'request.json',container/'result.json',container/'ack.json'
assert not result.exists(),'Screen checks require a fresh test application'
(container/'harness-ready').write_text('ready')
last=0;before=None;deadline=time.monotonic()+305;last_step=''
try:
 while not result.exists() and time.monotonic()<deadline:
  step=container/'step.json'
  if step.exists():
   current_step=step.read_text()
   if current_step!=last_step:
    last_step=current_step;print(current_step,flush=True);(out/'motion-ui-last-step.json').write_text(current_step)
  try:os.kill(pid,0)
  except ProcessLookupError:break
  if request.exists():
   value=json.loads(request.read_text())
   if value['id']!=last:
    last=value['id'];screenshot=out/f"screen-{last:02d}-{value['label']}.png"
    screenshot.with_suffix('.json').write_text(json.dumps(value,indent=2))
    subprocess.run(['xcrun','simctl','io',udid,'screenshot','--type=png',str(screenshot)],check=True,timeout=90,stdout=subprocess.DEVNULL)
    difference=0.0
    if value['label'].endswith('-after'):
     assert before is not None,'Missing matching before screenshot'
     difference=float(subprocess.check_output([str(out/'screen-difference'),str(before),str(screenshot),*map(str,value['rect'])],text=True,timeout=30))
    else:before=screenshot
    pending=ack.with_suffix('.tmp');pending.write_text(json.dumps({'id':last,'difference':difference}));pending.replace(ack)
  time.sleep(.1)
 assert result.exists(),'Real UIKit motion application did not finish'
 value=json.loads(result.read_text());(out/'motion-ui-result.json').write_text(json.dumps(value,indent=2)+'\n');print(json.dumps(value),flush=True)
 assert value.get('passed'),value
 assert value['realUIKitWindow'] and value['realOrientationChanges']==6 and len(value['visibleMotionChecks'])==15
 assert value['nativeNavigationPushPop'] and value['settingsReturns']==6
 assert value['styleSwitches']==[False,True]
finally:
 if not result.exists():
  with (out/'motion-ui-sample.txt').open('w')as sample:
   subprocess.run(['/usr/bin/sample',str(pid),'3','-file',str(out/'motion-ui-thread-stacks.txt')],stdout=sample,stderr=subprocess.STDOUT,timeout=30)
 for crash in (Path.home()/'Library/Logs/DiagnosticReports').glob('MotionArtworkUITest*.ips'):
  shutil.copy2(crash,out/crash.name)
 with (out/'motion-ui-runtime.log').open('w')as log:
  subprocess.run(['xcrun','simctl','spawn',udid,'log','show','--last','3m','--style','compact','--predicate','process == "MotionArtworkUITest"'],stdout=log,stderr=subprocess.STDOUT,timeout=30)
 subprocess.run(['xcrun','simctl','terminate',udid,bundle],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=10)
PY
