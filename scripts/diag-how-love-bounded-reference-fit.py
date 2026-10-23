"""Descriptive offline feasibility of constant gains approximating accepted C.

No production classifier, vocal extraction, or perceptual loss function.
The fit uses object IDs solely to reproduce an historical diagnostic reference.
"""
import json
from pathlib import Path
import numpy as np
from scipy import optimize
import pyloudnorm as pyln


def main():
    repo=Path(__file__).resolve().parents[1]
    fixture=repo/'tmp/how-love-diag'
    out=Path.home()/'Downloads/SDA-clarity-validation'
    frames=[json.loads(s) for s in (fixture/'frames.jsonl').read_text().splitlines()]
    labels=frames[0]['labels']
    origin=frames[0]['start'];lo,hi=55*48000-origin,63*48000-origin
    raw=[];current=[]
    for label in labels:
        p=fixture/f'source-interaction/{label}/render.f32'
        assert json.loads(p.with_suffix('.json').read_text())['minimumGuard']==1
        raw.append(np.fromfile(p,dtype='<f4').reshape(-1,2).astype(float)[lo:hi])
        q=out/'coherence-transfer/current-replay'/f'{label}.f32'
        assert json.loads(q.with_suffix('.json').read_text())['minimumGuard']==1
        current.append(np.fromfile(q,dtype='<f4').reshape(-1,2).astype(float)[lo:hi])
    x=np.stack(raw)
    baseline=x.sum(axis=0)
    present=sum(current)
    historic=np.array([10**(6/20) if label.startswith('Obj_') and int(label[4:])>=12 else 1
                       for label in labels])
    target=np.einsum('i,ine->ne',historic,x)*10**(2/20)
    meter=pyln.Meter(48000)
    matching=meter.integrated_loudness(present)-meter.integrated_loudness(target)
    target*=10**(matching/20)
    equivalent_db=2+matching+20*np.log10(historic)
    # Exclude 100ms around the one chronological boundary. This is descriptive
    # held-out waveform error, not independent validation of a voice classifier.
    train=np.arange(len(target))<int(3.9*48000)
    valid=np.arange(len(target))>=int(4.1*48000)
    a=x[:,train].reshape(len(labels),-1).T
    b=target[train].ravel()
    gram=a.T@a;rhs=a.T@b
    eig,u=np.linalg.eigh(gram)
    keep=eig>eig.max()*1e-12
    design=np.sqrt(eig[keep])[:,None]*u[:,keep].T
    response=(u[:,keep].T@rhs)/np.sqrt(eig[keep])
    reports=[]
    for name,upper,allow_attenuation in [('totalCap1p5',1.5,False),('totalCap3',3,False),
                                         ('totalCap6',6,False),('unrestrictedDiagnostic',12,True)]:
        lower=np.full(len(labels),.05 if allow_attenuation else 1.)
        maximum=np.full(len(labels),10**(upper/20))
        # LFE must stay unity. Remove its column rather than almost fixing it.
        lfe=labels.index('LFE');dynamic=[i for i in range(len(labels)) if i!=lfe]
        fit=optimize.lsq_linear(design[:,dynamic],response-design[:,lfe],
                                bounds=(lower[dynamic],maximum[dynamic]),tol=1e-12,max_iter=1000)
        assert fit.success
        coefficients=np.ones(len(labels));coefficients[dynamic]=fit.x
        candidate=np.einsum('i,ine->ne',coefficients,x)
        residual=candidate[valid]-target[valid]
        reports.append({'case':name,'scope':'constant object gains, more flexible than geometry rule; diagnostic only',
                        'coefficientsDb':{label:float(20*np.log10(v)) for label,v in zip(labels,coefficients)},
                        'heldoutWaveformResidualRelativeDb':float(10*np.log10(np.sum(residual**2)/np.sum(target[valid]**2))),
                        'wholeWindowLufs':float(meter.integrated_loudness(candidate)),
                        'maxSamplePeak':float(abs(candidate).max())})
    result={'scope':'Fit constant gains to historical accepted C at current 8s LUFS, same unity-guard baseline stem bank. Waveform matching is not clarity, identity or audibility. Training first3.9s, held-out last3.9s with100ms boundary guard. No code or audio candidates generated for production.',
            'referenceLoudnessMatchingDb':float(matching),'currentLufs':float(meter.integrated_loudness(present)),
            'baselineLufs':float(meter.integrated_loudness(baseline)),
            'exactReferenceEquivalentSourceGainsDb':{label:float(v) for label,v in zip(labels,equivalent_db)},
            'interpretationLimit':'Constrained fit is only the best constant-gain waveform approximation under these bounds, not a proof that clarity improvement requires higher gain or attenuation. Baseline bank and current solo bank are compact replay states, not the full-prefix reference used for prior auditions.',
            'currentVsTargetHeldoutResidualRelativeDb':float(10*np.log10(np.sum((present[valid]-target[valid])**2)/np.sum(target[valid]**2))),
            'cases':reports}
    (out/'bounded-reference-fit.json').write_text(json.dumps(result,indent=2),encoding='utf-8')
    print(json.dumps(result,indent=2))


if __name__=='__main__':
    main()
