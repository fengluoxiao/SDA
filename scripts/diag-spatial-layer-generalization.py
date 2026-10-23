"""Cross-track audition of master +2dB / metadata spatial-layer lift +6dB. No ID or loudness selection."""
import argparse,hashlib,json,os,subprocess,wave
from pathlib import Path
import numpy as np
from scipy import signal
ROOT=Path(__file__).resolve().parents[1];RATE=48000

def main():
    ap=argparse.ArgumentParser(description=__doc__);ap.add_argument('--harness',required=True,type=Path);a=ap.parse_args()
    out=ROOT/'tmp/spatial-layer-generalization-2026-10-08';out.mkdir(exist_ok=True)
    fixtures=[('How-Love',ROOT/'tmp/how-love-diag/native-parity-2026-10-08/native-replay',2591232),
              ('Ether',ROOT/'tmp/how-love-regression-ether',None)]
    env=dict(os.environ,SDA_ATMOS_REPLAY_HRTF=str((ROOT/'apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json').resolve()),
             SDA_ATMOS_REPLAY_WET='0',SDA_ATMOS_REPLAY_SECONDS='11',SDA_ATMOS_REPLAY_DIRECT='1',
             SDA_ATMOS_REPLAY_FRONT_COMMON='0',SDA_ATMOS_REPLAY_SPATIAL_BALANCE='0',SDA_ATMOS_REPLAY_TAG='render',SDA_ATMOS_REPLAY_LEDGER='1',SDA_ATMOS_REPLAY_MASTER_DB='2')
    results=[];how=None
    for name,fixture,origin in fixtures:
        for db in [0,6]:
            dest=out/f'{name}-layer{db}';dest.mkdir(exist_ok=True)
            pcm=(fixture/'sources.pcm').read_bytes();meta=(fixture/'frames.jsonl').read_bytes()
            (dest/'sources.pcm').write_bytes(pcm);(dest/'frames.jsonl').write_bytes(meta)
            run_env=dict(env,SDA_ATMOS_REPLAY_DIR=str(dest.resolve()),SDA_ATMOS_REPLAY_LAYER_DB=str(db))
            with (dest/'render.log').open('w') as log:
                subprocess.run([str(a.harness.resolve()),'atmos_front_pair_probe::replay_atmos_object_stream','--exact','--ignored','--nocapture','--test-threads=1'],cwd=ROOT,env=run_env,stdout=log,stderr=subprocess.STDOUT,check=True)
            x=np.fromfile(dest/'render.f32',dtype='<f4').reshape(-1,2).astype(float)
            config=json.loads((dest/'render.json').read_text());assert config['spatialLayerGainDb']==db and config['masterPreampDb']==2
            assert not config['spatialBalanceEnabled'] and not config['frontCommonActive']
            ledger=json.loads((dest/'render-gain-ledger.json').read_text())
            assert all(all(s['gain']==1 and s['targetGain']==1 and 1<=s['spatialLayerGain']<=2.00001 for s in row['sources'].values()) for row in ledger)
            if origin is not None:lo,hi=55*RATE-origin,63*RATE-origin
            else:lo,hi=RATE,9*RATE # Interior eight seconds of existing second-track fixture, no re-inferred source timestamp.
            y=x[lo:hi];assert len(y)==8*RATE and np.isfinite(y).all()
            peak=float(np.max(abs(signal.resample_poly(y,4,1,axis=0))));assert peak<1,'Refuse >0dBFS oversampled audition'
            wav=out/f'{name}-master2-layer{db}-8s.wav';ints=np.rint(y*8388607).astype('<i4');packed=ints.reshape(-1).view(np.uint8).reshape(-1,4)[:,:3].copy()
            with wave.open(str(wav),'wb') as w:w.setparams((2,3,RATE,0,'NONE','not compressed'));w.writeframes(packed.tobytes())
            with wave.open(str(wav),'rb') as w:
                assert w.getnframes()==8*RATE
                b=np.frombuffer(w.readframes(w.getnframes()),dtype=np.uint8).reshape(-1,3);v=b[:,0].astype(np.int32)|(b[:,1].astype(np.int32)<<8)|(b[:,2].astype(np.int32)<<16);v=(v^0x800000)-0x800000
                assert np.max(abs(v.reshape(-1,2)/8388607-y))<0.51/8388607
            gain_stats={id:{'min':min(r['sources'][id]['spatialLayerGain'] for r in ledger),'max':max(r['sources'][id]['spatialLayerGain'] for r in ledger)} for id in ledger[0]['sources']}
            result={'track':name,'masterDb':2,'spatialLayerMaxDb':db,'minimumGuard':config['minimumGuard'],
                    'oversampled4xPeakDbfs':float(20*np.log10(peak)),'file':str(wav),
                    'gainStats':gain_stats,'inputSha256':hashlib.sha256(pcm).hexdigest(),
                    'metadataSha256':hashlib.sha256(meta).hexdigest(),'wavSha256':hashlib.sha256(wav.read_bytes()).hexdigest()}
            if name=='How-Love' and db==0:
                previous=np.fromfile(ROOT/'tmp/how-love-diag/master-preamp-audition-2026-10-08/master-2dB/render.f32',dtype='<f4').reshape(-1,2).astype(float)
                error=float(np.max(abs(x-previous)));assert error<1e-5;result['disabledVsPriorMasterMaxAbs']=error
            if name=='How-Love' and db==6:how=(x,dest)
            results.append(result);print(json.dumps({k:v for k,v in result.items() if k!='gainStats'}),flush=True)
    # Same signal/metadata under new arbitrary IDs: detect accidental ID-specific behavior.
    base,dest0=how;dest=out/'How-Love-renumbered';dest.mkdir(exist_ok=True)
    frames=[json.loads(l) for l in (dest0/'frames.jsonl').read_text().splitlines()]
    for f in frames:
        f['labels']=[f'Obj_{int(s[4:])+100}' if s.startswith('Obj_') else s for s in f['labels']]
        for e in f['events']:e['id']+=100
        for d in f['objectChannels']:d['id']+=100
    (dest/'frames.jsonl').write_text('\n'.join(json.dumps(f) for f in frames)+'\n',encoding='utf-8');(dest/'sources.pcm').write_bytes((dest0/'sources.pcm').read_bytes())
    with (dest/'render.log').open('w') as log:
        subprocess.run([str(a.harness.resolve()),'atmos_front_pair_probe::replay_atmos_object_stream','--exact','--ignored','--nocapture','--test-threads=1'],cwd=ROOT,
                       env=dict(env,SDA_ATMOS_REPLAY_DIR=str(dest.resolve()),SDA_ATMOS_REPLAY_LAYER_DB='6'),stdout=log,stderr=subprocess.STDOUT,check=True)
    renamed=np.fromfile(dest/'render.f32',dtype='<f4').reshape(-1,2).astype(float);error=float(np.max(abs(renamed-base)));assert error<1e-5
    report={'scope':'Optional candidate enhancement, not isolated vocal identification, faithful authored mix, proven Apple equivalence or exact original C replication. World-coordinate spatial layer definition is a chosen generic rendering policy. No source ID/song/weakness detection.',
            'recipe':{'masterDb':2,'spatialLayerMaxDb':6,'defaultMasterDb':0,'defaultLayerDb':0},
            'CUserAccepted':True,'DAndEUserBothAudible':True,'candidateUserAccepted':False,
            'objectRenumberMaxAbs':error,'harnessSha256':hashlib.sha256(a.harness.read_bytes()).hexdigest(),'results':results}
    (out/'manifest.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
if __name__=='__main__':main()
