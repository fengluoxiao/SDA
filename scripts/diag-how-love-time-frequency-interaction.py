"""Narrow time/frequency source-interaction audit; no vocal isolation or production changes."""
import json
from pathlib import Path
import numpy as np
from scipy import signal

ROOT=Path(__file__).resolve().parents[1]
RATE=48000

def db(r):
    return float(10*np.log10(max(float(r),1e-30)))

def summarize(coherent,independent,freq,times):
    reports={}
    for low,high in [(500,1500),(1500,4000),(4000,8000)]:
        which=(freq>=low)&(freq<high)
        p=coherent[which];q=independent[which]
        # All ratios are conditional on measurable constituent power, not a vocal mask.
        mask=q>max(float(np.max(q))*1e-3,1e-12)
        ratio=p/np.maximum(q,1e-30)
        weight=q[mask];values=ratio[mask]
        worst=np.where(mask,ratio,np.inf)
        flat=np.argsort(worst.reshape(-1))[:8]
        cells=[]
        for k in flat:
            i,j,ear=np.unravel_index(k,worst.shape)
            if not np.isfinite(worst[i,j,ear]):continue
            cells.append({'frequencyHz':float(freq[which][i]),'timeSeconds':float(times[j]),
                          'ear':['L','R'][ear],'sumVsIndependentDb':db(worst[i,j,ear]),
                          'independentPowerDbfs':db(q[i,j,ear])})
        reports[f'{low}-{high}Hz']={'fullBandSumVsIndependentDb':db(np.sum(p)/np.sum(q)),
              'activeCellCount':int(mask.sum()),
              'constituentEnergyFractionAtCancellationDb':{str(level):float(np.sum(weight[values<10**(level/10)])/np.sum(weight)) for level in [-3,-6,-12]},
              'worstActiveCells':cells}
    return reports

def main():
    root=ROOT/'tmp/how-love-diag';out=root/'native-parity-2026-10-08/time-frequency';out.mkdir(exist_ok=True)
    labels=json.loads((root/'full/frames.jsonl').read_text().splitlines()[0])['labels']
    origin=json.loads((root/'frames.jsonl').read_text().splitlines()[0])['start']/RATE
    total=None;diag=None;support=None;support_diag=None
    for label in labels:
        x=np.fromfile(root/f'source-interaction/{label}/render.f32',dtype='<f4').reshape(-1,2)
        freq,t,z=signal.stft(x,fs=RATE,nperseg=2048,noverlap=1536,axis=0,boundary=None,padded=False)
        # scipy returns [frequency, ear, time].
        z=np.transpose(z,(0,2,1)).astype(np.complex128);times=t+origin
        selected=(times>=55)&(times<63);z=z[:,selected,:]
        if total is None:
            total=np.zeros_like(z);diag=np.zeros(z.shape);support=np.zeros_like(z);support_diag=np.zeros(z.shape)
        total+=z;diag+=abs(z)**2
        if label.startswith('Obj_') and int(label[4:])>=12:
            support+=z;support_diag+=abs(z)**2
    times=times[selected]
    full=np.fromfile(root/'source-interaction/full/render.f32',dtype='<f4').reshape(-1,2)
    _,_,z=signal.stft(full,fs=RATE,nperseg=2048,noverlap=1536,axis=0,boundary=None,padded=False)
    z=np.transpose(z,(0,2,1))[:,selected,:]
    additivity=float(np.max(abs(total-z)))
    assert additivity<1e-6
    report={'scope':'Original-position current-engine stems; STFT 2048 Hann, hop 512 (42.67ms window / 10.67ms hop). A correlated-source coherent sum can legitimately differ from independent energies. No semantic vocal masks; bins are not independent statistical trials. Negative values are not proof of renderer defect.',
            'windowSeconds':[55,63],'stftMaxAdditivityError':additivity,
            'activityFloor':'Constituent per-band maximum minus 30dB, and absolute power 1e-12; neither is a perceptual threshold.',
            'full':summarize(abs(total)**2,diag,freq,times),
            'supportGroup':summarize(abs(support)**2,support_diag,freq,times)}
    (out/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    np.savez_compressed(out/'spectra.npz',frequency=freq,time=times,fullCoherent=abs(total)**2,
                        fullIndependent=diag,supportCoherent=abs(support)**2,supportIndependent=support_diag)
    print(json.dumps({group:{band:{'bandDb':r['fullBandSumVsIndependentDb'],
          'energyFractionBelowMinus6Db':r['constituentEnergyFractionAtCancellationDb']['-6']} for band,r in report[group].items()} for group in ['full','supportGroup']},indent=2))

if __name__=='__main__':main()
