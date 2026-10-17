"""Only The Very First Night: continuous 0-65s auditions; not a time-window production change."""
import importlib.util,json,hashlib,os,subprocess
from pathlib import Path
import numpy as np
ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('protection',ROOT/'scripts/diag-main-layer-protection.py');mod=importlib.util.module_from_spec(spec);spec.loader.exec_module(mod)
def main():
 root=ROOT/'tmp/very-first-night-side-lead-2026-10-08';fixture=root/'prefix-native';out=root/'prefix-audition';out.mkdir(exist_ok=True)
 frames=[json.loads(line) for line in (fixture/'frames.jsonl').read_text().splitlines()];assert frames[0]['start']==0
 assert json.loads((fixture/'decode-summary.json').read_text())['errors']==[]
 env=dict(os.environ,SDA_ATMOS_REPLAY_DIR=str(fixture.resolve()),SDA_ATMOS_REPLAY_HRTF=str((ROOT/'apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json').resolve()),SDA_ATMOS_REPLAY_WET='0',SDA_ATMOS_REPLAY_SECONDS='66',SDA_ATMOS_REPLAY_DIRECT='1',SDA_ATMOS_REPLAY_FRONT_COMMON='0',SDA_ATMOS_REPLAY_SPATIAL_BALANCE='0',SDA_ATMOS_REPLAY_MASTER_DB='2',SDA_ATMOS_REPLAY_LAYER_DB='6',SDA_ATMOS_REPLAY_ENHANCEMENT='1',SDA_ATMOS_REPLAY_LEDGER='1')
 results=[]
 for db in [-3,-6]:
  tag=f'main{db}'
  with (out/f'{tag}.log').open('w') as log:
   subprocess.run([str(mod.H),'atmos_front_pair_probe::replay_atmos_object_stream','--exact','--ignored','--nocapture','--test-threads=1'],cwd=ROOT,env=dict(env,SDA_ATMOS_REPLAY_TAG=tag,SDA_ATMOS_REPLAY_MAIN_TRIM_DB=str(db)),stdout=log,stderr=subprocess.STDOUT,check=True)
  x=np.fromfile(fixture/f'{tag}.f32',dtype='<f4').reshape(-1,2).astype(float);assert len(x)>=65*48000
  cfg=json.loads((fixture/f'{tag}.json').read_text());assert cfg['spatialLayerMainRestoreDb']==2-db
  row=mod.write(out/f'Very-First-Night-0-65-main{db}dB.wav',x[:65*48000]);row.update(mainTrimDb=db,minimumGuard=cfg['minimumGuard'],sourceStartSeconds=0,sourceEndSeconds=65,normalization=False,entirePrefixSameRule=True);results.append(row);print(json.dumps(row,indent=2),flush=True)
 report={'scope':'User clarified only The Very First Night from 0s. Continuous original 0-65s, both net main-layer trims -3/-6dB; no How Love/Ether/splice, no special 50-65s treatment, no file normalization. Auxiliary layer enhancement retained; main-layer instruments are also affected, not isolated vocal stems.', 'harnessSha256':hashlib.sha256(mod.H.read_bytes()).hexdigest(),'originalSource':json.loads((root/'source.json').read_text()),'inputSha256':hashlib.sha256((fixture/'sources.pcm').read_bytes()).hexdigest(),'metadataSha256':hashlib.sha256((fixture/'frames.jsonl').read_bytes()).hexdigest(),'outputs':results,'userAcceptance':'pending','newIpaBuilt':False}
 (out/'manifest.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
if __name__=='__main__':main()
