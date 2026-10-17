"""Localize an accepted audition using core-surround fit ablation; NOT a production source selector."""
import hashlib,json,wave
from pathlib import Path
import numpy as np
from scipy import signal
ROOT=Path(__file__).resolve().parents[1]
RATE=48000

def db(r):return float(10*np.log10(max(float(r),1e-30)))
def main():
    root=ROOT/'tmp/how-love-diag';out=root/'c-reference-ablation-2026-10-08';out.mkdir(exist_ok=True)
    native=root/'native-parity-2026-10-08/chunk8192'
    frames=[json.loads(l) for l in (native/'frames.jsonl').read_text().splitlines()]
    raw=np.fromfile(native/'sources.pcm',dtype='<f4');labels=frames[0]['labels'];parts=[];offset=0
    for f in frames:
        assert f['labels']==labels
        n=f['samples'];parts.append(raw[offset:offset+n*len(labels)].reshape(len(labels),n).T);offset+=n*len(labels)
    assert offset==len(raw)
    x=np.concatenate(parts).astype(float);origin=frames[0]['start'];lo,hi=55*RATE-origin,63*RATE-origin
    timing=json.loads((root/'surround-trace/report.json').read_text());alignment=timing['core_alignment_samples'];lag=timing['additional_joc_lag_samples']
    core=np.memmap(root/'core-reference/core-0-70.f32',dtype='<f4',mode='r').reshape(-1,6)
    y=np.asarray(core[55*RATE+alignment:63*RATE+alignment,4:6],dtype=float)
    objects=[i for i,label in enumerate(labels) if label.startswith('Obj_')]
    X=x[lo+lag:hi+lag,objects]
    train=(np.arange(len(y))//12000)%2==0
    coef=np.linalg.lstsq(X[train],y[train],rcond=None)[0];prediction=X[~train]@coef
    mse=float(np.mean((prediction-y[~train])**2));denom=float(np.mean(y[~train]**2))
    importance=[]
    for j,channel in enumerate(objects):
        if int(labels[channel][4:])<12:continue
        kept=[k for k in range(len(objects)) if k!=j]
        reduced=X[:,kept];fit=np.linalg.lstsq(reduced[train],y[train],rcond=None)[0]
        loss=float(np.mean((reduced[~train]@fit-y[~train])**2))
        importance.append({'source':labels[channel],'heldoutRemovalLossChangeDb':db(loss/mse),
                           'heldoutRemovalMse':loss,'fullFitCoefficients':coef[j].tolist()})
    importance.sort(key=lambda row:row['heldoutRemovalLossChangeDb'],reverse=True)
    # Diagnostic split of C, not a classifier of vocals/weak objects or a playback rule.
    related=[row['source'] for row in importance[:6]]
    complement=[label for label in labels if label.startswith('Obj_') and int(label[4:])>=12 and label not in related]
    all_support=[label for label in labels if label.startswith('Obj_') and int(label[4:])>=12]
    full=np.fromfile(root/'source-interaction/full/render.f32',dtype='<f4').reshape(-1,2).astype(float)[lo:hi]
    stems={label:np.fromfile(root/f'source-interaction/{label}/render.f32',dtype='<f4').reshape(-1,2).astype(float)[lo:hi] for label in labels}
    assert np.max(abs(sum(stems.values())-full))<1e-5
    groups=[('C-confirmed-full-support.wav',all_support),('D-full-boost-reference-dependence.wav',related),('E-full-boost-complement.wav',complement)]
    results=[]
    for filename,group in groups:
        support=sum(stems[label] for label in group)
        audio=(full+(10**(6/20)-1)*support)*10**(2/20)
        peak=float(np.max(abs(signal.resample_poly(audio,4,1,axis=0))))
        assert np.isfinite(audio).all() and peak<1,'Refuse clipped audition; no added limiter/normalization'
        pcm=np.rint(audio*8388607).astype('<i4');packed=pcm.reshape(-1).view(np.uint8).reshape(-1,4)[:,:3].copy();path=out/filename
        with wave.open(str(path),'wb') as w:w.setparams((2,3,RATE,0,'NONE','not compressed'));w.writeframes(packed.tobytes())
        with wave.open(str(path),'rb') as w:
            assert w.getnframes()==8*RATE and w.getnchannels()==2
            d=np.frombuffer(w.readframes(w.getnframes()),dtype=np.uint8).reshape(-1,3);v=d[:,0].astype(np.int32)|(d[:,1].astype(np.int32)<<8)|(d[:,2].astype(np.int32)<<16);v=(v^0x800000)-0x800000
            assert np.max(abs(v.reshape(-1,2)/8388607-audio))<0.51/8388607
        results.append({'file':str(path),'boostedSources':group,'globalGainDb':2,'extraGroupGainDb':6,
                        'oversampled4xPeakDbfs':db(peak**2),'sha256':hashlib.sha256(path.read_bytes()).hexdigest()})
    original=root/'gain-audition-2026-10-08/C-full-plus2dB-support-plus6dB.wav'
    assert hashlib.sha256(original.read_bytes()).hexdigest()==results[0]['sha256'],'C reference must remain byte-identical'
    report={'scope':'Offline diagnostic ablation of user-confirmed C. All are full mixes; no source muted or attenuated. Only split C extra gain for listening localization. NOT isolated vocals, production weak-source detection, source-role semantics or proof of renderer defect.',
            'userCheckpoint':'User confirms target vocal is audible in C; pure master audition does not meet vocal goal.',
            'windowSeconds':[55,63],'fit':'Alternating 250ms held-out chunks, original core SL/SR timing; rank by heldout error increase after removing each input and refitting. Six chosen only to make two diagnostic groups.',
            'fullFitHeldoutResidualRelativeDb':db(mse/denom),'heldoutImportance':importance,
            'nativeInputSha256':hashlib.sha256((native/'sources.pcm').read_bytes()).hexdigest(),
            'CByteIdenticalToConfirmedReference':True,'outputs':results}
    (out/'manifest.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    print(json.dumps({'related':related,'complement':complement,'heldoutResidualDb':report['fullFitHeldoutResidualRelativeDb'],'outputs':results},indent=2))
if __name__=='__main__':main()
