"""Offline two-cue-gain linear decomposition with unity output protection.

Direct means complete cue-free renderer mixture, not dry isolated vocal.
Cross terms can be negative in a legitimate authored binaural scene.
"""
import json
from pathlib import Path
import numpy as np
from scipy import signal
from PIL import Image, ImageDraw


def main():
    downloads=Path.home()/'Downloads'
    out=downloads/'SDA-clarity-validation/cue-cross-energy'
    out.mkdir(exist_ok=True)
    ratio=10**(-6/20)
    cases=[('How-Love',downloads/'SDA-current-render-audition/How-Do-I-Make-You-Love-Me-55-63',55,63,0),
           ('Lose-Yourself',downloads/'SDA-Lose-Yourself-audit/replay',115,131,5471232),
           ('Very-First-Night',downloads/'SDA-current-render-audition/Very-First-Night-0-65',50,65,0)]
    report={'scope':'Two cue levels, same graph and -12dB presentation, unity guard. Algebraic cue-free mixture and added residual decomposition; not isolated objects/vocals or causal perceptual proof. Fixed presentation undone for analysis only.', 'songs':[]}
    for name,folder,start,end,origin in cases:
        arrays=[]
        for k in [0,1]:
            tag='cue-attribution-safe-'+str(k)
            metadata=json.loads((folder/(tag+'.json')).read_text())
            assert metadata['minimumGuard']==1 and metadata['masterPreampDb']==-12
            arrays.append(np.fromfile(folder/(tag+'.f32'),dtype='<f4').reshape(-1,2).astype(float)*10**(12/20))
        baseline,candidate=arrays
        assert baseline.shape==candidate.shape
        direct=(candidate-ratio*baseline)/(1-ratio)
        wet=baseline-direct
        assert abs(direct+ratio*wet-candidate).max()<1e-12
        lo,hi=int(start*48000-origin),int(end*48000-origin)
        row={'name':name,'songWindowSeconds':[start,end],'bands':{}}
        if name=='How-Love':
            cueoff=np.fromfile(folder/'cue-bypass-valid.f32',dtype='<f4').reshape(-1,2).astype(float)
            error=float(abs(cueoff[lo:hi]-direct[lo:hi]).max())
            assert error<1e-5
            row['cueOffIndependentValidationMaxAbs']=error
        traces=[]
        for low,high in [(100,500),(500,1500),(1500,4000),(4000,8000)]:
            sos=signal.butter(3,[low,high],fs=48000,btype='bandpass',output='sos')
            d=signal.sosfilt(sos,direct,axis=0)[lo:hi]
            w=signal.sosfilt(sos,wet,axis=0)[lo:hi]
            n=len(d)//4800
            d=d[:n*4800].reshape(n,4800,2);w=w[:n*4800].reshape(n,4800,2)
            pd=np.mean(d*d,axis=1);pw=np.mean(w*w,axis=1);cross=2*np.mean(d*w,axis=1)
            old=pd+pw+cross;new=pd+ratio*ratio*pw+ratio*cross
            residual_delta=(ratio*ratio-1)*pw
            cross_delta=(ratio-1)*cross
            assert np.max(abs(new-old-residual_delta-cross_delta))<1e-15
            active=old.min(axis=1)>max(old.max()*1e-3,1e-10)
            delta=10*np.log10(np.maximum(new,1e-30)/np.maximum(old,1e-30))
            lr=delta[:,1]-delta[:,0]
            indices=np.flatnonzero(active)
            worst=indices[np.argsort(lr[indices])[-4:][::-1]]
            summaries=[]
            for i in worst:
                summaries.append({'songTimeSeconds':float(start+i*.1),
                                  'earPowerChangeDb':delta[i].tolist(),'rightRelativeChangeDb':float(lr[i]),
                                  'baselineCrossTermOverDirectPower':(cross[i]/np.maximum(pd[i],1e-30)).tolist(),
                                  'baselineResidualOverDirectPower':(pw[i]/np.maximum(pd[i],1e-30)).tolist(),
                                  'residualEnergyChangeOverBaselinePower':(residual_delta[i]/old[i]).tolist(),
                                  'crossTermChangeOverBaselinePower':(cross_delta[i]/old[i]).tolist()})
            row['bands'][f'{low}-{high}']={'integratedCrossTermOverDirectPower':(cross.sum(axis=0)/pd.sum(axis=0)).tolist(),
                                         'largestRightRelativeChanges':summaries}
            if low==500:
                traces=[cross_delta/np.maximum(old,1e-30),residual_delta/np.maximum(old,1e-30)]
        report['songs'].append(row)
        # Compare linear contributions without mislabeling them as dB.
        canvas=Image.new('RGB',(1200,650),'white');draw=ImageDraw.Draw(canvas)
        for k,(values,title) in enumerate(zip(traces,['Cross-term change / baseline power','Residual energy change / baseline power'])):
            top=65+k*290;left=100;width=1000;height=190
            draw.text((left,top-30),name+': '+title+'; blue=L / red=R',fill='black')
            for value in [-.2,-.1,0,.1,.2]:
                py=top+int((.2-value)/.4*height);draw.line((left,py,left+width,py),fill='#dddddd');draw.text((15,py),f'{value:+.1f}',fill='black')
            for ear,color in [(0,'#2060b0'),(1,'#c03030')]:
                points=[(left+int((i+.5)/len(values)*width),top+int((.2-np.clip(v,-.2,.2))/.4*height)) for i,v in enumerate(values[:,ear])]
                draw.line(points,fill=color,width=2)
            for sec in np.linspace(start,end,9):draw.text((left+int((sec-start)/(end-start)*width),top+200),f'{sec:g}s',fill='black')
        draw.text((100,625),'100ms power decomposition; plot clips +/-0.2. No semantic voice measurement.',fill='black')
        canvas.save(out/(name+'.png'))
    (out/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    print(json.dumps({r['name']:r['bands']['500-1500']['largestRightRelativeChanges'][:2] for r in report['songs']},indent=2))


if __name__=='__main__':main()
