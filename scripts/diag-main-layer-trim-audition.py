"""Listening-only main trim (-3/-6dB); auxiliary spatial lift untouched. No deployment changes."""
import importlib.util,json,hashlib,os,subprocess
from pathlib import Path
import numpy as np
ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('main_protection',ROOT/'scripts/diag-main-layer-protection.py');module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
def main():
    root=ROOT/'tmp/very-first-night-side-lead-2026-10-08';out=root/'main-trim-audition';out.mkdir(exist_ok=True)
    fixture=root/'replay';origin=json.loads((root/'native/frames.jsonl').read_text().splitlines()[0])['start'];lo,hi=50*48000-origin,65*48000-origin
    env=dict(os.environ,SDA_ATMOS_REPLAY_HRTF=str((ROOT/'apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json').resolve()),SDA_ATMOS_REPLAY_WET='0',SDA_ATMOS_REPLAY_SECONDS='18',SDA_ATMOS_REPLAY_DIRECT='1',SDA_ATMOS_REPLAY_FRONT_COMMON='0',SDA_ATMOS_REPLAY_SPATIAL_BALANCE='0',SDA_ATMOS_REPLAY_MASTER_DB='2',SDA_ATMOS_REPLAY_LAYER_DB='6',SDA_ATMOS_REPLAY_ENHANCEMENT='1',SDA_ATMOS_REPLAY_LEDGER='1',SDA_ATMOS_REPLAY_TAG='trim')
    reports=[]
    for db in [-3,-6]:
        dest=out/f'main{db}';dest.mkdir(exist_ok=True);pcm=(fixture/'sources.pcm').read_bytes();meta=(fixture/'frames.jsonl').read_bytes();(dest/'sources.pcm').write_bytes(pcm);(dest/'frames.jsonl').write_bytes(meta)
        with (dest/'render.log').open('w') as log:
            subprocess.run([str(module.H),'atmos_front_pair_probe::replay_atmos_object_stream','--exact','--ignored','--nocapture','--test-threads=1'],cwd=ROOT,env=dict(env,SDA_ATMOS_REPLAY_DIR=str(dest.resolve()),SDA_ATMOS_REPLAY_MAIN_TRIM_DB=str(db)),stdout=log,stderr=subprocess.STDOUT,check=True)
        x=np.fromfile(dest/'trim.f32',dtype='<f4').reshape(-1,2).astype(float);cfg=json.loads((dest/'trim.json').read_text());assert cfg['spatialLayerMainRestoreDb']==2-db
        ledger=[r for r in json.loads((dest/'trim-gain-ledger.json').read_text()) if lo<=r['samplePos']<=hi];stats={}
        for id in ['obj:11','obj:13','obj:15','obj:21']:
            gains=np.array([s['sources'][id]['spatialLayerGain']*s['masterPreampGain'] for s in ledger]);actual=20*np.log10(gains);assert np.max(abs(actual-db))<0.001;stats[id]=[float(actual.min()),float(actual.max())]
        row=module.write(out/f'Very-First-Night-50-65-main{db}dB.wav',x[lo:hi]);row.update(mainTrimDb=db,mainLayerNetGainDb=stats,minimumGuard=cfg['minimumGuard'],inputSha256=hashlib.sha256(pcm).hexdigest(),metadataSha256=hashlib.sha256(meta).hexdigest());reports.append(row);print(json.dumps(row,indent=2),flush=True)
    report={'scope':'Listening-only reduction of metadata-defined horizontal main layer. Not isolated vocals; main-layer instruments also change. Auxiliary fully-weighted paths retain +8dB net pre-protection. No whole-file normalization, source/time/song production rule or new IPA.','windowSeconds':[50,65],'harnessSha256':hashlib.sha256(module.H.read_bytes()).hexdigest(),'outputs':reports,'userAcceptance':'pending'}
    (out/'manifest.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
if __name__=='__main__':main()
