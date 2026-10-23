"""Controlled prefix comparison; test-only, common -8 dB, no normalization."""
import importlib.util,json,os,subprocess
from pathlib import Path
import numpy as np
from scipy import signal
ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('writer',ROOT/'scripts/diag-main-layer-protection.py')
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
def main():
 root=ROOT/'tmp/very-first-night-side-lead-2026-10-08';fixture=root/'prefix-native';out=root/'fixed-headroom';out.mkdir(exist_ok=True)
 env=dict(os.environ,SDA_ATMOS_REPLAY_DIR=str(fixture),SDA_ATMOS_REPLAY_HRTF=str(ROOT/'apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json'),SDA_ATMOS_REPLAY_WET='0',SDA_ATMOS_REPLAY_SECONDS='66',SDA_ATMOS_REPLAY_DIRECT='1',SDA_ATMOS_REPLAY_FRONT_COMMON='0',SDA_ATMOS_REPLAY_SPATIAL_BALANCE='0',SDA_ATMOS_REPLAY_ENHANCEMENT='1',SDA_ATMOS_REPLAY_LEDGER='1',SDA_ATMOS_REPLAY_PRESENTATION_DB='-8')
 rows=[];mixes={}
 for tag,trim,aux in [('A-main0-aux6',0,6),('B-main-6-aux6',-6,6),('C-main0-aux0',0,0)]:
  with (out/f'{tag}.log').open('w') as log:
   subprocess.run([str(m.H),'atmos_front_pair_probe::replay_atmos_object_stream','--exact','--ignored','--nocapture','--test-threads=1'],cwd=ROOT,env=dict(env,SDA_ATMOS_REPLAY_TAG=tag,SDA_ATMOS_REPLAY_MAIN_TRIM_DB=str(trim),SDA_ATMOS_REPLAY_AUX_DB=str(aux)),stdout=log,stderr=subprocess.STDOUT,check=True)
  cfg=json.loads((fixture/f'{tag}.json').read_text());assert cfg['minimumGuard']==1, cfg
  x=np.fromfile(fixture/f'{tag}.f32','<f4').reshape(-1,2)[:65*48000].astype(float);mixes[tag]=x
  row=m.write(out/f'Very-First-Night-0-65-{tag}.wav',x);row.update(mainTrimDb=trim,auxLayerDb=aux,presentationOffsetDb=-8,minimumGuard=cfg['minimumGuard']);rows.append(row);print(json.dumps(row),flush=True)
 # Gain-ledger quantification is endpoint-based, not a sample-exact envelope.
 ledgers={db:json.loads((fixture/f'main{db}-gain-ledger.json').read_text()) for db in [-3,-6]}
 ratio=[20*np.log10(b['guardGain']/a['guardGain']) for a,b in zip(ledgers[-3],ledgers[-6]) if 50*48000<=a['samplePos']<=65*48000]
 stats={'auxiliaryGuardReleaseDb50to65':{'min':float(min(ratio)),'max':float(max(ratio)),'mean':float(np.mean(ratio))}}
 old={db:np.fromfile(fixture/f'main{db}.f32','<f4').reshape(-1,2)[50*48000:65*48000].astype(float) for db in [-3,-6]}
 stats['oldFullMixRmsChangeDb']=float(10*np.log10(np.sum(old[-6]**2)/np.sum(old[-3]**2)))
 for a,b in [('A-main0-aux6','B-main-6-aux6'),('A-main0-aux6','C-main0-aux0')]:
  stats[f'{b}_vs_{a}_50to65RmsDb']=float(10*np.log10(np.sum(mixes[b][50*48000:]**2)/np.sum(mixes[a][50*48000:]**2)))
 report={'scope':'Only this song 0-65 continuous. All candidates same -8dB presentation offset before guard; no independent normalization. Guard unity verified. C is a NEW diagnostic label, not historical accepted C. No vocal identity inferred; production unchanged.','outputs':rows,'measurements':stats,'accepted':False}
 (out/'manifest.json').write_text(json.dumps(report,indent=2),encoding='utf-8');print(json.dumps(stats,indent=2),flush=True)
if __name__=='__main__':main()
