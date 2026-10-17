"""Offline bounded common HRTF spectral equalization hypothesis.

Approximate equal-area directional weighting is NOT listener calibration.
Same post-render FIR both ears: does not establish unchanged spatial perception.
No frequency bins inferred from the song; no production analysis/caches.
"""
import importlib.util
import json
from pathlib import Path
import numpy as np
from scipy import signal
import pyloudnorm as pyln
from PIL import Image, ImageDraw


def main():
    repo=Path(__file__).resolve().parents[1]
    root=Path.home()/'Downloads/SDA-clarity-validation/subarashiki-front'
    out=root/'common-coloration';out.mkdir(exist_ok=True)
    bank=repo/'apps/mobile/assets/hrtf-restored/hrtf-dense'
    manifest=json.loads((bank/'hrtf-set.json').read_text())
    positions=manifest['positions'];h=np.array([np.fromfile(bank/p['dry'],dtype='<f4').reshape(2,-1) for p in positions],dtype=float)
    # Equal-area Fibonacci samples, nearest measured-direction cells. The
    # lower hemisphere is undersampled; report this approximation explicitly.
    count=20000;i=np.arange(count);z=1-2*(i+.5)/count
    phi=i*np.pi*(3-np.sqrt(5));r=np.sqrt(1-z*z)
    sphere=np.column_stack([r*np.cos(phi),r*np.sin(phi),z])
    az=np.radians([p['azimuth'] for p in positions]);el=np.radians([p['elevation'] for p in positions])
    directions=np.column_stack([-np.sin(az)*np.cos(el),np.cos(az)*np.cos(el),np.sin(el)])
    cells=np.argmax(sphere@directions.T,axis=1);weights=np.bincount(cells,minlength=len(positions))/count
    nfft=16384;freq=np.fft.rfftfreq(nfft,1/48000)
    power=(abs(np.fft.rfft(h,n=nfft,axis=2))**2).mean(axis=1)
    diffuse=weights@power
    centers=np.geomspace(200,16000,81)
    smooth=np.array([diffuse[(freq>=c/2**(1/6))&(freq<c*2**(1/6))].mean() for c in centers])
    db=10*np.log10(np.maximum(smooth,1e-20));reference=float(np.mean(db[(centers>=500)&(centers<=2000)]))
    correction=np.clip((reference-db)*.5,-3,3)
    # Soft fade outside midrange; avoid extending missing low/high measurements.
    def step(x):
        x=np.clip(x,0,1);return x*x*(3-2*x)
    taper=step(np.log2(centers/250))*step(np.log2(14000/centers))
    correction*=taper
    grid=np.r_[0,centers,24000];gain=np.r_[1,10**(correction/20),1]
    fir=signal.firwin2(513,grid,gain,fs=48000)
    assert np.isfinite(fir).all() and np.max(abs(fir-fir[::-1]))<1e-12
    f,response=signal.freqz(fir,worN=8192,fs=48000)
    assert 20*np.log10(abs(response).max())<3.1
    # Test exact shared linear filtering against phase-scaled stereo signals.
    rng=np.random.default_rng(123);test=rng.normal(0,.01,48000)
    pair=np.column_stack([test,-test*.37]);filtered=signal.lfilter(fir,[1],pair,axis=0)
    assert np.max(abs(filtered[:,1]+.37*filtered[:,0]))<1e-12
    data=np.fromfile(root/'replay/reflection-current.f32',dtype='<f4').reshape(-1,2).astype(float)
    candidate=signal.lfilter(fir,[1],data,axis=0)
    lo,hi=140*48000,180*48000;delay=256
    assert hi+delay<=len(candidate)
    a=data[lo:hi];b=candidate[lo+delay:hi+delay]
    meter=pyln.Meter(48000);match=float(meter.integrated_loudness(a)-meter.integrated_loudness(b));b=b*10**(match/20)
    peak=max(abs(signal.resample_poly(a,4,1,axis=0)).max(),abs(signal.resample_poly(b,4,1,axis=0)).max())
    common=min(10**(12/20),.95/peak);a=a*common;b=b*common
    spec=importlib.util.spec_from_file_location('w',repo/'scripts/diag-main-layer-protection.py');w=importlib.util.module_from_spec(spec);spec.loader.exec_module(w)
    files=[w.write(out/'B-common-HRTF-coloration-140-180.wav',b),w.write(out/'A-then-B-common-HRTF-coloration.wav',np.concatenate([a,np.zeros((48000,2)),b]))]
    report={'scope':'Measurement-derived common coloration compensation hypothesis; approximate nearest-cell equal-area61directionHRTF weighting, undersampled lower sphere. Half inverse one-third-octave averaged power, bounded +/-3dB, tapered250-14000Hz. NOT exact diffuse-field calibration or proven vocal fix. Shared513taplinearphaseFIRadds256sampleslatencyinpossibleproduction; offset undone for offline audition. Not installed or packaged.',
            'weights':weights.tolist(),'frequencyCentersHz':centers.tolist(),'correctionDb':correction.tolist(),
            'fir':fir.tolist(),'offlineMatchDb':match,'commonSafetyDb':float(20*np.log10(common)),
            'files':files,'defaultChanged':False,'rearHeightPerceptionAcceptance':'pending',
            'productionLimit':'Post-render after prior limiting, not integrated preguard DSP; complete renderer and device validation required before adoption.'}
    (out/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    canvas=Image.new('RGB',(1100,520),'white');draw=ImageDraw.Draw(canvas)
    draw.text((90,20),'Common HRTF coloration hypothesis: grey=smoothed bank relative response; blue=correction',fill='black')
    for v in [-6,-3,0,3,6]:
        y=70+int((6-v)/12*340);draw.line((85,y,1030,y),fill='#dddddd');draw.text((15,y),str(v)+'dB',fill='black')
    for values,color in [(db-reference,'#888888'),(correction,'#245d99')]:
        points=[(85+int(np.log(c/200)/np.log(80)*945),70+int((6-np.clip(v,-6,6))/12*340)) for c,v in zip(centers,values)]
        draw.line(points,fill=color,width=2)
    for hz in [200,500,1000,2000,4000,8000,16000]:draw.text((85+int(np.log(hz/200)/np.log(80)*945),435),str(hz)+'Hz',fill='black')
    draw.text((85,485),'Measured kernel estimate, NOT listener-neutral reference; audition acceptance pending.',fill='black')
    canvas.save(out/'response.png')
    print(json.dumps({'matchDb':match,'correctionMinMax': [float(correction.min()),float(correction.max())],'outputFiles':[r['file'] for r in files]},indent=2))


if __name__=='__main__':main()
