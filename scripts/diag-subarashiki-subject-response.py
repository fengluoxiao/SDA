"""Audit complete-subject diagnostic render and prepare level-matched listening.

Not a production candidate. HRTF/subject interpolation both differ; no source
replacement, ordinary-downmix injection, vocal extraction or song-specific DSP.
"""
import importlib.util
import json
from pathlib import Path
import numpy as np
import pyloudnorm as pyln
from scipy import signal
from PIL import Image, ImageDraw


def main():
    repo=Path(__file__).resolve().parents[1]
    root=Path.home()/'Downloads/SDA-clarity-validation/subarashiki-front'
    out=root/'subject-response-audit'
    lo,hi=140*48000,180*48000
    tags=['reflection-direct-only','subject-h13-direct']
    ys=[]; metas=[]
    for tag in tags:
        meta=json.loads((root/'replay'/f'{tag}.json').read_text())
        assert meta['minimumGuard']==1 and not meta['frontCommonActive']
        assert meta['wetWeight']==0 and meta['direct'] and meta['spatialLayerGainDb']==1.5
        raw=np.memmap(root/'replay'/f'{tag}.f32',dtype='<f4',mode='r').reshape(-1,2)
        assert len(raw)==8689152
        ys.append(np.array(raw[lo:hi],dtype=float));metas.append(meta)
    meter=pyln.Meter(48000)
    match=meter.integrated_loudness(ys[0])-meter.integrated_loudness(ys[1])
    ys[1]*=10**(match/20)
    bands={}
    for low,high in [(300,700),(700,1500),(1500,4000),(4000,8000),(8000,16000)]:
        sos=signal.butter(3,[low,high],fs=48000,btype='bandpass',output='sos')
        a,b=[signal.sosfilt(sos,y,axis=0) for y in ys]
        pa,pb=np.sum(a*a,axis=0),np.sum(b*b,axis=0)
        bands[f'{low}-{high}']={'loudnessMatchedPowerChangeDb':float(10*np.log10(pb.sum()/pa.sum())),
                               'wholeBandIldChangeDb':float(10*np.log10(pb[0]/pb[1])-10*np.log10(pa[0]/pa[1]))}
    f,t,A=signal.stft(ys[0].T,fs=48000,nperseg=2048,noverlap=1536,boundary=None)
    _,_,B=signal.stft(ys[1].T,fs=48000,nperseg=2048,noverlap=1536,boundary=None)
    powerA, powerB=abs(A)**2,abs(B)**2
    delta=10*np.log10(np.maximum(powerB,1e-30)/np.maximum(powerA,1e-30))
    mask=(f>=300)&(f<=8000)
    canvas=Image.new('RGB',(1120,470),'white');d=ImageDraw.Draw(canvas)
    for ear in range(2):
        v=np.clip(delta[ear,mask]/6,-1,1)
        c=np.ones((*v.shape,3))*255;c[:,:,0]*=1-np.maximum(-v,0);c[:,:,1]*=1-abs(v);c[:,:,2]*=1-np.maximum(v,0)
        active=powerA[ear,mask]>powerA[ear].max()*1e-5;c[~active]=190
        img=Image.fromarray(c[::-1].astype('uint8')).resize((1000,170))
        y=20+ear*210;d.text((80,y),'Left' if ear==0 else 'Right',fill='black');canvas.paste(img,(80,y+20))
    d.text((80,440),'140s -> 180s, 300-8000Hz; H13 minus KU100 after offline LUFS match; blue -6/red +6dB.',fill='black')
    canvas.save(out/'subject-spectral-delta.png')
    # Common playback safety scaling, no online normalization requirement.
    gain=min(2.0,.88/max(float(abs(signal.resample_poly(y,4,1,axis=0)).max()) for y in ys))
    spec=importlib.util.spec_from_file_location('writer',repo/'scripts/diag-main-layer-protection.py')
    writer=importlib.util.module_from_spec(spec);spec.loader.exec_module(writer)
    files=[writer.write(out/'A-KU100-direct-140-180.wav',ys[0]*gain),
           writer.write(out/'B-H13-direct-diagnostic-140-180.wav',ys[1]*gain),
           writer.write(out/'A-then-B-subject-diagnostic.wav',np.concatenate([ys[0]*gain,np.zeros((48000,2)),ys[1]*gain]))]
    report={'scope':'Whole scene complete-subject diagnostic only; direction/original PCM preserved. HRTF plus supported interpolation differs. No added cues/room; no listening acceptance or regression claim.',
            'productionChanged':False,'window':[140,180],'offlineLufsMatchDb':float(match),
            'commonSafetyGain':gain,'metadata':metas,'bands':bands,'files':files,
            'layout':{'0-40':'KU100 direct only','40-41':'silence','41-81':'H13 direct diagnostic'}}
    (out/'report.json').write_text(json.dumps(report,indent=2));print(json.dumps(bands,indent=2))


if __name__=='__main__':main()
