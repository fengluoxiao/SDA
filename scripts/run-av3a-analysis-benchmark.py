"""Compare bounded analysis and normal playback using only the public 3-frame fixture."""
from pathlib import Path
import argparse, hashlib, io, json, os, platform, shutil, statistics, subprocess, tarfile
parser=argparse.ArgumentParser();parser.add_argument("--baseline",required=True);parser.add_argument("--output",required=True);parser.add_argument("--repeat",type=int,default=2048);args=parser.parse_args()
root=Path(__file__).resolve().parents[1];out=Path(args.output).resolve();out.mkdir(parents=True,exist_ok=True)
work=out/"work";work.mkdir(exist_ok=True);baseline=work/"baseline";baseline.mkdir()
archive=subprocess.check_output(["git","archive",args.baseline,"vendor/avs3a"],cwd=root)
with tarfile.open(fileobj=io.BytesIO(archive)) as files:
    assert all(m.name=="vendor" or m.name.startswith("vendor/avs3a") for m in files.getmembers())
    files.extractall(baseline,filter="data")
env=dict(os.environ,CARGO_TARGET_DIR=str(work/"target"),SDA_AV3A_BENCH_FIXTURE=str(root/"packages/core/tests/fixtures/av3a-7-1-4-three-frames.av3a"))
source=root/"scripts/av3a-analysis-benchmark";executables={};suffix=".exe" if os.name=="nt" else ""
for label,vendor in [("baseline",baseline/"vendor/avs3a"),("candidate",root/"vendor/avs3a")]:
    crate=work/(label+"-harness");(crate/"src").mkdir(parents=True)
    manifest=(source/"Cargo.toml").read_text().replace('"../../vendor/avs3a"',json.dumps(vendor.as_posix()))
    (crate/"Cargo.toml").write_text(manifest);shutil.copyfile(source/"Cargo.lock",crate/"Cargo.lock");shutil.copyfile(source/"src/main.rs",crate/"src/main.rs")
    subprocess.run(["cargo","build","--manifest-path",str(crate/"Cargo.toml"),"--locked","--release"],cwd=root,env=env,check=True)
    executable=out/(label+suffix);shutil.copy2(work/"target/release"/("av3a-analysis-benchmark"+suffix),executable);executables[label]=executable
records=[];expected=None
for workers in [1,3]:
    runenv=dict(env,RAYON_NUM_THREADS=str(workers))
    for mode in ["analysis","playback"]:
        for turn in range(3):
            for label in (["baseline","candidate"] if turn%2==0 else ["candidate","baseline"]):
                result=json.loads(subprocess.check_output([str(executables[label]),str(args.repeat),str(workers),mode],cwd=root,env=runenv))
                identity=(result["frames"],result["pcmHash"])
                if expected is None: expected=identity
                assert identity==expected,(label,mode,workers,result,expected)
                record=dict(result,label=label,mode=mode,turn=turn);records.append(record);print(json.dumps(record),flush=True)
                report={"platform":platform.platform(),"machine":platform.machine(),"baseline":args.baseline,"candidate":os.environ.get("SDA_SOURCE_COMMIT") or subprocess.check_output(["git","rev-parse","HEAD"],cwd=root,text=True).strip(),"publicFixtureOnly":True,"smokeOnly":args.repeat<128,"candidateSourceSha256":hashlib.sha256((root/"vendor/avs3a/src/mc_core.rs").read_bytes()).hexdigest(),"wholePcmExact":True,"runs":records}
                (out/"results.json").write_text(json.dumps(report,indent=2))
report["median"]={f"{workers}-{mode}":{label:statistics.median(r["seconds"] for r in records if r["workers"]==workers and r["mode"]==mode and r["label"]==label) for label in executables} for workers in [1,3] for mode in ["analysis","playback"]}
(out/"results.json").write_text(json.dumps(report,indent=2));print(json.dumps(report["median"],indent=2))
