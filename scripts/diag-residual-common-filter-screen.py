"""Offline causal common residual filter screen, not a renderer replacement.

Use guard-unity decomposition. Filters are deliberately fixed across songs,
not optimized per title/object/time. No semantic clarity loss function exists.
"""
import json
from pathlib import Path
import numpy as np
from scipy import signal


def main():
    downloads=Path.home()/'Downloads'
    out=downloads/'SDA-clarity-validation/common-residual-filter-screen'
    out.mkdir(exist_ok=True)
    # Fixed experiments: no ear-independent adjustment. All process wet only.
    definitions=[('bypass',None,1.),('scalar-extra-minus6',None,10**(-6/20)),
                 ('high-residual-half-above1500',1500.,.5),
                 ('high-residual-half-above4000',4000.,.5)]
    fixtures=[('How-Love',downloads/'SDA-current-render-audition/How-Do-I-Make-You-Love-Me-55-63',55,63,0),
              ('Very-First-Night',downloads/'SDA-current-render-audition/Very-First-Night-0-65',0,65,0),
              ('Lose-Yourself',downloads/'SDA-Lose-Yourself-audit/replay',115,251,5471232)]
    result={'scope':'Same common causal filter on added residual only, full history processed before crop. Synthetic branch transfer and three actual guard-unity replay decompositions. No output limiting, normalization, audio export, production change, or clarity proof.', 'songs':[]}
    rate=48000
    ratio=10**(-6/20)
    for name,folder,start,end,origin in fixtures:
        pair=[]
        for k in [0,1]:
            tag='cue-attribution-safe-'+str(k)
            meta=json.loads((folder/(tag+'.json')).read_text())
            assert meta['minimumGuard']==1 and meta['masterPreampDb']==-12
            pair.append(np.fromfile(folder/(tag+'.f32'),dtype='<f4').reshape(-1,2).astype(float))
        baseline,reduced=pair
        direct=(reduced-ratio*baseline)/(1-ratio)
        wet=baseline-direct
        assert abs(direct+ratio*wet-reduced).max()<1e-12
        lo,hi=int(start*rate-origin),int(end*rate-origin)
        song={'name':name,'windowSongSeconds':[start,end],'candidates':[]}
        for tag,cutoff,high_gain in definitions:
            if cutoff is None:
                changed=wet*high_gain
            else:
                # Causal one-pole shelf formed by dry wet signal plus LP wet.
                # It preserves DC and bounds isolated wet magnitude <=unity,
                # but phase/mixed interference may still change localization.
                pole=np.exp(-2*np.pi*cutoff/rate)
                low=signal.lfilter([1-pole],[1,-pole],wet,axis=0)
                changed=wet*high_gain+(1-high_gain)*low
                w,h=signal.freqz([high_gain+(1-high_gain)*(1-pole),-high_gain*pole],
                                [1,-pole],worN=8192)
                assert np.max(abs(h))<=1+1e-12
            output=direct+changed
            row={'tag':tag,'bands':{},'samplePeakAtOfflineSafetyGain':float(abs(output[lo:hi]).max())}
            for lowhz,highhz in [(100,500),(500,1500),(1500,4000),(4000,8000)]:
                sos=signal.butter(3,[lowhz,highhz],btype='bandpass',fs=rate,output='sos')
                a=signal.sosfilt(sos,baseline,axis=0)[lo:hi]
                b=signal.sosfilt(sos,output,axis=0)[lo:hi]
                n=len(a)//4800
                pa=np.mean(a[:n*4800].reshape(n,4800,2)**2,axis=1)
                pb=np.mean(b[:n*4800].reshape(n,4800,2)**2,axis=1)
                active=pa.min(axis=1)>max(pa.max()*1e-3,1e-12)
                delta=10*np.log10(np.maximum(pb,1e-30)/np.maximum(pa,1e-30))
                lr=delta[:,1]-delta[:,0]
                overall=10*np.log10(pb.sum()/pa.sum())
                if tag=='bypass':assert abs(lr[active]).max()<1e-10
                row['bands'][f'{lowhz}-{highhz}']={
                    'maximumAbsoluteLocalIldChangeDb':float(abs(lr[active]).max()),
                    'rightRelativeDeltaDbP1P50P99':np.percentile(lr[active],[1,50,99]).tolist(),
                    'wholeWindowPowerDeltaDb':float(overall)}
            song['candidates'].append(row)
        result['songs'].append(song)
        (out/'report.json').write_text(json.dumps(result,indent=2),encoding='utf-8')
        print(json.dumps({name:{r['tag']:{band:stats['maximumAbsoluteLocalIldChangeDb'] for band,stats in r['bands'].items()} for r in song['candidates']}}),flush=True)
    result['interpretation']='Neither non-boosting wet magnitude nor identical ear filter guarantees mixed-output ILD stability. Results are screening constraints; cannot choose a clarity winner without evidence.'
    (out/'report.json').write_text(json.dumps(result,indent=2),encoding='utf-8')


if __name__=='__main__':main()
