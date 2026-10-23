"""Accepted auxiliary-bypass candidate at production presentation gain, plus regression."""
import importlib.util,json,os,subprocess
from pathlib import Path
import numpy as np
ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('writer',ROOT/'scripts/diag-main-layer-protection.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
def main():
 out=ROOT/'tmp/very-first-night-side-lead-2026-10-08/accepted-aux-bypass';out.mkdir(exist_ok=True)
 env=dict(os.environ,SDA_ATMOS_REPLAY_HRTF=str(ROOT/'apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json'),SDA_ATMOS_REPLAY_WET='0',SDA_ATMOS_REPLAY_DIRECT='1',SDA_ATMOS_REPLAY_FRONT_COMMON='0',SDA_ATMOS_REPLAY_SPATIAL_BALANCE='0',SDA_ATMOS_REPLAY_ENHANCEMENT='1',SDA_ATMOS_REPLAY_LEDGER='1',SDA_ATMOS_REPLAY_MAIN_TRIM_DB='0',SDA_ATMOS_REPLAY_PRESENTATION_DB='0')
 cases=[('Very-First-Night-0-65-accepted-balance',ROOT/'tmp/very-first-night-side-lead-2026-10-08/prefix-native',66,0,65*48000,0),('How-Love-55-63-aux-bypass',ROOT/'tmp/how-love-diag/native-parity-2026-10-08/native-replay',11,55*48000-2591232,63*48000-2591232,0),('How-Love-55-63-enhanced-reference',ROOT/'tmp/how-love-diag/native-parity-2026-10-08/native-replay',11,55*48000-2591232,63*48000-2591232,6)]
 rows=[]
 for tag,fixture,seconds,lo,hi,aux in cases:
  with (out/f'{tag}.log').open('w') as log:
   subprocess.run([str(m.H),'atmos_front_pair_probe::replay_atmos_object_stream','--exact','--ignored','--nocapture','--test-threads=1'],cwd=ROOT,env=dict(env,SDA_ATMOS_REPLAY_TAG=tag,SDA_ATMOS_REPLAY_DIR=str(fixture),SDA_ATMOS_REPLAY_SECONDS=str(seconds),SDA_ATMOS_REPLAY_AUX_DB=str(aux)),stdout=log,stderr=subprocess.STDOUT,check=True)
  cfg=json.loads((fixture/f'{tag}.json').read_text());assert cfg['masterPreampDb']==2 and cfg['spatialLayerMainRestoreDb']==2 and cfg['spatialLayerGainDb']==aux
  x=np.fromfile(fixture/f'{tag}.f32','<f4').reshape(-1,2).astype(float);row=m.write(out/f'{tag}.wav',x[lo:hi]);row.update(auxDb=aux,minimumGuard=cfg['minimumGuard'],presentationOffsetDb=0);rows.append(row);print(json.dumps(row),flush=True)
 (out/'manifest.json').write_text(json.dumps({'scope':'No production changes. Remove common test headroom; preserve master+2/main restore2; compare auxiliary bypass with earlier-track enhancement. Existing linked guard retained, no file normalization. Listening acceptance pending.','outputs':rows},indent=2),encoding='utf-8')
if __name__=='__main__':main()
