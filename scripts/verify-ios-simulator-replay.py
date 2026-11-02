"""Pin an existing simulator build and reject reuse after production-code edits."""
from pathlib import Path,PurePosixPath
import hashlib,json,plistlib,subprocess,sys,zipfile
PIN_RUN=41522638180
PIN_COMMIT="df223e5aee3071877980a1b6dfb487eefe1cfd43"
PIN_ZIP="c8840d047805321a2ee953d846d003062f67d9a483cc38e7c6a3e6aa8c8e2ec0"
run=json.loads(Path(sys.argv[1]).read_text())
assert run["id"]==PIN_RUN and run["head_sha"]==PIN_COMMIT
assert run["path"]==".github/workflows/mobile-native-validation.yml"and run["conclusion"]in ("success","failure")
assert run["repository"]["full_name"]=="fengluoxiao/SDA"
changed=subprocess.check_output(["git","diff","--name-only",PIN_COMMIT,"HEAD"],text=True).splitlines()
assert all(p.startswith(("scripts/",".github/workflows/"))for p in changed),"Production code changed: rebuild the simulator app"
archive=Path(sys.argv[2]);actual=hashlib.sha256(archive.read_bytes()).hexdigest();assert actual==PIN_ZIP,"Simulator archive does not match the inspected build"
with zipfile.ZipFile(archive)as z:
 for name in z.namelist():
  p=PurePosixPath(name);assert not p.is_absolute()and ".."not in p.parts
 info=plistlib.loads(z.read("SDA.app/Info.plist"));assert info["CFBundleSupportedPlatforms"]==["iPhoneSimulator"]
 assert PIN_COMMIT.encode()in z.read("SDA.app/"+info["CFBundleExecutable"])
report={"compiledAppCommit":PIN_COMMIT,"testHarnessCommit":subprocess.check_output(["git","rev-parse","HEAD"],text=True).strip(),"productionCodeChanged":False,"zipSha256":actual,"changedFiles":changed}
Path(sys.argv[3]).write_text(json.dumps(report,indent=2));print(json.dumps(report,indent=2))
