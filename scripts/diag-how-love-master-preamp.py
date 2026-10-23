"""Audition the shared master stage, with original sources/positions/gains untouched."""
import argparse,hashlib,json,os,subprocess,wave
from pathlib import Path
import numpy as np
from scipy import signal
ROOT=Path(__file__).resolve().parents[1]
RATE=48000

def db(r):return float(10*np.log10(max(float(r),1e-30)))
def main():
    ap=argparse.ArgumentParser(description=__doc__);ap.add_argument('--harness',required=True,type=Path);args=ap.parse_args()
    root=ROOT/'tmp/how-love-diag';fixture=root/'native-parity-2026-10-08/native-replay';out=root/'master-preamp-audition-2026-10-08';out.mkdir(exist_ok=True)
    origin=json.loads((root/'native-parity-2026-10-08/chunk8192/frames.jsonl').read_text().splitlines()[0])['start']
    lo,hi=55*RATE-origin,63*RATE-origin
    input_bytes=(fixture/'sources.pcm').read_bytes();meta=(fixture/'frames.jsonl').read_bytes()
    env=dict(os.environ,SDA_ATMOS_REPLAY_HRTF=str((ROOT/'apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json').resolve()),
             SDA_ATMOS_REPLAY_WET='0',SDA_ATMOS_REPLAY_SECONDS='11',SDA_ATMOS_REPLAY_DIRECT='1',
             SDA_ATMOS_REPLAY_FRONT_COMMON='0',SDA_ATMOS_REPLAY_SPATIAL_BALANCE='0',SDA_ATMOS_REPLAY_TAG='render',SDA_ATMOS_REPLAY_LEDGER='1')
    outputs=[];reference=None
    for gain_db in [0,2,4]:
        dest=out/f'master-{gain_db}dB';dest.mkdir(exist_ok=True)
        (dest/'sources.pcm').write_bytes(input_bytes);(dest/'frames.jsonl').write_bytes(meta)
        run_env=dict(env,SDA_ATMOS_REPLAY_DIR=str(dest.resolve()),SDA_ATMOS_REPLAY_MASTER_DB=str(gain_db))
        with (dest/'render.log').open('w') as log:
            subprocess.run([str(args.harness.resolve()),'atmos_front_pair_probe::replay_atmos_object_stream','--exact','--ignored','--nocapture','--test-threads=1'],cwd=ROOT,env=run_env,stdout=log,stderr=subprocess.STDOUT,check=True)
        audio=np.fromfile(dest/'render.f32',dtype='<f4').reshape(-1,2).astype(float);assert np.isfinite(audio).all()
        config=json.loads((dest/'render.json').read_text());ledger=json.loads((dest/'render-gain-ledger.json').read_text())
        assert config['masterPreampDb']==gain_db and not config['spatialBalanceEnabled'] and not config['frontCommonActive']
        assert all(all(s['gain']==1 and s['targetGain']==1 and s['distanceGain']==1 for s in row['sources'].values()) for row in ledger)
        excerpt=audio[lo:hi];assert len(excerpt)==8*RATE
        if reference is None:reference=excerpt
        factor=10**(gain_db/20);pure_error=float(np.max(abs(excerpt-reference*factor)))
        if config['minimumGuard']==1:assert pure_error<1e-6
        sample_peak=float(np.max(abs(excerpt)));true_peak=float(np.max(abs(signal.resample_poly(excerpt,4,1,axis=0))))
        assert sample_peak<1 and true_peak<1,'Refuse audition with clipping or >0dBFS oversampled peak'
        pcm=np.rint(excerpt*8388607).astype('<i4');packed=pcm.reshape(-1).view(np.uint8).reshape(-1,4)[:,:3].copy()
        wav=out/f'How-Love-55-63-master-plus{gain_db}dB.wav'
        with wave.open(str(wav),'wb') as w:w.setparams((2,3,RATE,0,'NONE','not compressed'));w.writeframes(packed.tobytes())
        with wave.open(str(wav),'rb') as w:
            assert w.getnframes()==8*RATE and w.getnchannels()==2
            d=np.frombuffer(w.readframes(w.getnframes()),dtype=np.uint8).reshape(-1,3)
            signed=d[:,0].astype(np.int32)|(d[:,1].astype(np.int32)<<8)|(d[:,2].astype(np.int32)<<16);signed=(signed^0x800000)-0x800000
            assert np.max(abs(signed.reshape(-1,2)/8388607-excerpt))<0.51/8388607
        outputs.append({'file':str(wav),'requestedMasterDb':gain_db,'minimumGuard':config['minimumGuard'],
                        'samplePeakDbfs':db(sample_peak**2),'oversampled4xPeakDbfs':db(true_peak**2),
                        'rmsChangeDb':db(np.sum(excerpt**2)/np.sum(reference**2)),
                        'maxDifferenceVsUnprotectedStaticGain':pure_error,'normalization':False,
                        'sourceInputGainChanged':False,'sha256':hashlib.sha256(wav.read_bytes()).hexdigest()})
        print(json.dumps(outputs[-1]),flush=True)
    baseline=np.fromfile(fixture/'render.f32',dtype='<f4').reshape(-1,2).astype(float)[lo:hi]
    difference=float(np.max(abs(reference-baseline)));assert difference<1e-6
    report={'scope':'Current shared master stage, applied only to complete stereo mix before existing linked peak guard. No vocal/object selection, weak-source lift, song rules, downmix or HRTF changes. Not Apple output match or proof of solved relative vocal clarity.',
            'date':'2026-10-08','windowSeconds':[55,63],'defaultProductionPreampDb':0,
            'disabledVsPriorNativeReplayMaxAbs':difference,'harnessSha256':hashlib.sha256(args.harness.read_bytes()).hexdigest(),
            'sourceSha256':hashlib.sha256(input_bytes).hexdigest(),'metadataSha256':hashlib.sha256(meta).hexdigest(),'outputs':outputs}
    (out/'manifest.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
if __name__=='__main__':main()
