"""Short-time regression audit of existing offline residual candidate outputs.

No semantic voice mask: loudness/power cannot certify clarity. Never imported
by playback. Local results supplement, not replace, candidate listening.
"""
import json
import argparse
from pathlib import Path
import numpy as np
from scipy import signal
from PIL import Image, ImageDraw


def db(a):
    return 10*np.log10(np.maximum(a, 1e-30))


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--cue-db',type=int,choices=[-9,-12],default=-12)
    args=parser.parse_args()
    tag='cue-minus'+str(-args.cue_db)+'-valid'
    downloads = Path.home()/'Downloads'
    out = downloads/('SDA-clarity-validation/'+tag+'-candidate')
    fixtures = [
        ('How-Love', downloads/'SDA-current-render-audition/How-Do-I-Make-You-Love-Me-55-63',55,63),
        ('Very-First-Night',downloads/'SDA-current-render-audition/Very-First-Night-0-65',0,65),
        ('Lose-Yourself',downloads/'SDA-Lose-Yourself-audit/replay',0,138),
    ]
    reports, traces = [], []
    hop = 4800
    for name, fixture, start, end in fixtures:
        x = np.fromfile(fixture/'peak-stall-fixed.f32',dtype='<f4').reshape(-1,2).astype(float)
        y = np.fromfile(fixture/(tag+'.f32'),dtype='<f4').reshape(-1,2).astype(float)
        assert x.shape==y.shape and np.isfinite(y).all()
        lo,hi=int(start*48000),int(end*48000)
        report={'name':name,'windowSecondsRelativeToFixture':[start,end],'bands':{}}
        for low,high in [(100,500),(500,1500),(1500,4000),(4000,8000)]:
            sos=signal.butter(3,[low,high],fs=48000,btype='bandpass',output='sos')
            a=signal.sosfilt(sos,x,axis=0)[lo:hi]
            b=signal.sosfilt(sos,y,axis=0)[lo:hi]
            n=len(a)//hop
            a=a[:n*hop].reshape(n,hop,2);b=b[:n*hop].reshape(n,hop,2)
            pa=np.mean(a*a,axis=1);pb=np.mean(b*b,axis=1)
            before=pa.sum(axis=1);after=pb.sum(axis=1)
            active=before>max(before.max()*1e-3,1e-10)
            delta=db(after)-db(before)
            lr_delta=(db(pb[:,1])-db(pb[:,0]))-(db(pa[:,1])-db(pa[:,0]))
            # Ear ratio is meaningful only if both baseline ears have energy.
            ears_active=active & (pa.min(axis=1)>max(pa.max()*1e-3,1e-10))
            def extrema(values,mask):
                indices=np.flatnonzero(mask)
                return [{'timeSecondsRelativeToFixture':float(start+i*.1),
                         'differenceDb':float(values[i]),'baselinePowerDb':float(db(before[i]))}
                        for i in indices[np.argsort(abs(values[indices]))[-5:][::-1]]]
            key=f'{low}-{high}'
            report['bands'][key]={
                'activeWindowCount':int(active.sum()),
                'powerDeltaDbP1P50P99':np.percentile(delta[active],[1,50,99]).tolist(),
                'maximumAbsolutePowerDeltaDb':float(abs(delta[active]).max()),
                'maximumAbsoluteRtoLDeltaDb':float(abs(lr_delta[ears_active]).max()),
                'windowsAbove0p5DbPowerChange':int((abs(delta[active])>.5).sum()),
                'largestPowerChanges':extrema(delta,active),
                'largestRtoLChanges':extrema(lr_delta,ears_active)}
            if low==500:
                traces.append((name,start,end,delta,active))
        reports.append(report)
    result={'scope':'100ms filtered local power, raw unmatched outputs. Activity floor relative max -30dB and absolute1e-10. Coordinates relative to fixture; Lose origin not inferred. No authored target isolation; local excursions can be legitimate residual interference, not automatically a bug.',
            'songs':reports}
    (out/'local-regression.json').write_text(json.dumps(result,indent=2),encoding='utf-8')
    canvas=Image.new('RGB',(1200,850),'white');draw=ImageDraw.Draw(canvas)
    for j,(name,start,end,delta,active) in enumerate(traces):
        top=70+j*260;left=110;width=1020;height=180
        draw.text((left,top-35),name+' 500-1500Hz / 100ms raw candidate power change',fill='black')
        for value in [-1,-.5,0,.5,1]:
            py=top+int((1-value)/2*height)
            draw.line((left,py,left+width,py),fill='#dddddd')
            draw.text((25,py-6),str(value)+'dB',fill='black')
        points=[]
        for i,v in enumerate(delta):
            px=left+int((i+.5)/len(delta)*width);py=top+int((1-np.clip(v,-1,1))/2*height)
            if active[i]:
                points.append((px,py))
            else:
                if len(points)>1:draw.line(points,fill='#24589b',width=2)
                points=[]
        if len(points)>1:draw.line(points,fill='#24589b',width=2)
        for t in np.linspace(start,end,9):
            draw.text((left+int((t-start)/(end-start)*width),top+190),f'{t:g}s',fill='black')
    draw.text((110,820),'Relative fixture time; graph clips to +/-1dB; JSON records all excursions. Not vocal clarity.',fill='black')
    canvas.save(out/'local-regression.png')
    print(json.dumps({s['name']:{b:{k:r[k] for k in ['powerDeltaDbP1P50P99','maximumAbsolutePowerDeltaDb','maximumAbsoluteRtoLDeltaDb','windowsAbove0p5DbPowerChange']} for b,r in s['bands'].items()} for s in reports},indent=2))


if __name__=='__main__':
    main()
