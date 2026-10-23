"""Main-layer protection audition and accepted-track regression; no production song/time rules."""
import json,hashlib,os,subprocess,wave
from pathlib import Path
import numpy as np
from scipy import signal
ROOT=Path(__file__).resolve().parents[1];RATE=48000
H=Path(r'E:\SDA\tools\rust-host-tests\x86_64-pc-windows-gnu\release\deps\sda_native_renderer-56b4fd303324ebd4.exe')
def write(path,y):
    assert np.isfinite(y).all();peak=float(np.max(abs(signal.resample_poly(y,4,1,axis=0))));assert peak<1
    ints=np.rint(y*8388607).astype('<i4');packed=ints.reshape(-1).view(np.uint8).reshape(-1,4)[:,:3].copy()
    with wave.open(str(path),'wb') as w:w.setparams((2,3,RATE,0,'NONE','not compressed'));w.writeframes(packed.tobytes())
    with wave.open(str(path),'rb') as w:
        assert w.getnframes()==len(y)
        b=np.frombuffer(w.readframes(w.getnframes()),dtype=np.uint8).reshape(-1,3);v=b[:,0].astype(np.int32)|(b[:,1].astype(np.int32)<<8)|(b[:,2].astype(np.int32)<<16);v=(v^0x800000)-0x800000
        assert np.max(abs(v.reshape(-1,2)/8388607-y))<0.51/8388607
    return {'file':str(path),'seconds':len(y)/RATE,'oversampled4xPeakDbfs':float(20*np.log10(peak)),'sha256':hashlib.sha256(path.read_bytes()).hexdigest()}
def main():
    out=ROOT/'tmp/very-first-night-side-lead-2026-10-08';dest=out/'audition';dest.mkdir(exist_ok=True)
    origin=json.loads((out/'native/frames.jsonl').read_text().splitlines()[0])['start']
    cases=[('Very-First-Night',out/'replay',50*RATE-origin,65*RATE-origin,18),
           ('How-Love',ROOT/'tmp/how-love-diag/native-parity-2026-10-08/native-replay',55*RATE-2591232,63*RATE-2591232,11)]
    reports=[]
    env=dict(os.environ,SDA_ATMOS_REPLAY_HRTF=str((ROOT/'apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json').resolve()),SDA_ATMOS_REPLAY_WET='0',SDA_ATMOS_REPLAY_DIRECT='1',SDA_ATMOS_REPLAY_FRONT_COMMON='0',SDA_ATMOS_REPLAY_SPATIAL_BALANCE='0',SDA_ATMOS_REPLAY_MASTER_DB='2',SDA_ATMOS_REPLAY_LAYER_DB='6',SDA_ATMOS_REPLAY_ENHANCEMENT='1',SDA_ATMOS_REPLAY_LEDGER='1',SDA_ATMOS_REPLAY_TAG='v2')
    for name,fixture,lo,hi,seconds in cases:
        d=out/(name+'-v2');d.mkdir(exist_ok=True);pcm=(fixture/'sources.pcm').read_bytes();meta=(fixture/'frames.jsonl').read_bytes();(d/'sources.pcm').write_bytes(pcm);(d/'frames.jsonl').write_bytes(meta)
        with (d/'render.log').open('w') as log:
            subprocess.run([str(H),'atmos_front_pair_probe::replay_atmos_object_stream','--exact','--ignored','--nocapture','--test-threads=1'],cwd=ROOT,env=dict(env,SDA_ATMOS_REPLAY_DIR=str(d.resolve()),SDA_ATMOS_REPLAY_SECONDS=str(seconds)),stdout=log,stderr=subprocess.STDOUT,check=True)
        x=np.fromfile(d/'v2.f32',dtype='<f4').reshape(-1,2).astype(float);cfg=json.loads((d/'v2.json').read_text());assert cfg['spatialLayerMainRestoreDb']==2 and cfg['spatialLayerGainDb']==6 and cfg['masterPreampDb']==2
        ledger=json.loads((d/'v2-gain-ledger.json').read_text());selected=[r for r in ledger if lo<=r['samplePos']<=hi]
        row=write(dest/(name+'-protected-main.wav'),x[lo:hi]);row['minimumGuard']=cfg['minimumGuard'];row['nativeInputSha256']=hashlib.sha256(pcm).hexdigest();row['metadataSha256']=hashlib.sha256(meta).hexdigest()
        row['netGainDb']={id:{'min':float(20*np.log10(min(r['sources'][id]['spatialLayerGain']*r['masterPreampGain'] for r in selected))),'max':float(20*np.log10(max(r['sources'][id]['spatialLayerGain']*r['masterPreampGain'] for r in selected)))} for id in selected[0]['sources']}
        if name=='Very-First-Night':
            for id in ['obj:11','obj:13','obj:15','obj:21']:assert abs(row['netGainDb'][id]['max'])<0.001 and abs(row['netGainDb'][id]['min'])<0.001
            old=np.fromfile(fixture/'v1-enhanced.f32',dtype='<f4').reshape(-1,2).astype(float)[lo:hi];row['oldMix']=write(dest/'Very-First-Night-previous-enhancement.wav',old)
            row['rmsChangeVsPreviousDb']=float(10*np.log10(np.sum(x[lo:hi]**2)/np.sum(old**2)))
        reports.append(row);print(json.dumps({k:v for k,v in row.items() if k!='netGainDb'},indent=2),flush=True)
    report={'scope':'Generic revision of accepted enhancement, not vocal isolation. Protect horizontal front/side main; cancel only its automatic master lift, retain upper/lower/rear auxiliary enhancement. No song/object/time conditions, no loudness-based selector. New revision listening pending; previous IPA unchanged.', 'sourceWindowSeconds':[50,65],'harnessSha256':hashlib.sha256(H.read_bytes()).hexdigest(),'reports':reports}
    (out/'v2-manifest.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
if __name__=='__main__':main()
