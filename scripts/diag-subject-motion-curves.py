"""View actual subject diagnostic motion/track ILD curves; no pass by means."""
import json
from pathlib import Path
import numpy as np
from scipy import signal
from PIL import Image, ImageDraw


def main():
    downloads=Path.home()/'Downloads';base=downloads/'SDA-clarity-validation'
    out=base/'subject-motion-regression'
    cases=[('cross-front',base/'motion-front-policy/cross-front',0,12),
           ('vertical',base/'motion-front-policy/vertical',0,12),
           ('Lose-Yourself-come-on',downloads/'SDA-Lose-Yourself-audit/replay',115,139)]
    canvas=Image.new('RGB',(1150,1020),'white');d=ImageDraw.Draw(canvas);report=[]
    for row,(name,folder,lo,hi) in enumerate(cases):
        curves=[];entry={'name':name,'windowSeconds':[lo,hi],'subjects':{}}
        sos=signal.butter(3,[4000,8000],fs=48000,btype='bandpass',output='sos')
        for subject in ('ku100','h13'):
            raw=np.memmap(folder/f'subject-regression-{subject}.f32',dtype='<f4',mode='r').reshape(-1,2)
            y=signal.sosfilt(sos,raw,axis=0)[lo*48000:hi*48000];n=len(y)//4800
            power=np.mean(y[:n*4800].reshape(n,4800,2)**2,axis=1)
            ild=10*np.log10(np.maximum(power[:,0],1e-30)/np.maximum(power[:,1],1e-30))
            active=power.min(axis=1)>max(power.max()*1e-3,1e-12)
            curves.append((ild,active))
            adjacent=active[1:]&active[:-1]
            entry['subjects'][subject]={'meanIldDb':float(10*np.log10(power[:,0].sum()/power[:,1].sum())),
                'maxAdjacent100msIldStepDb':float(abs(np.diff(ild)[adjacent]).max())}
        delta=curves[1][0]-curves[0][0];active=curves[0][1]&curves[1][1]
        peak=int(np.argmax(np.where(active,abs(delta),-1)))
        entry['largestDifference']={'absoluteTimeSeconds':lo+(peak+.5)*.1,
            'ildDeltaDb':float(delta[peak]),'activeInBoth':bool(active[peak])}
        if name in ('cross-front','vertical'):
            axis=1 if name=='cross-front' else 2
            events=[e for line in (folder/'frames.jsonl').read_text().splitlines()
                    for e in json.loads(line)['events'] if e['id']==10 and e.get('hasPos')]
            closest=min(events,key=lambda e:abs(e['pos'][axis]))
            crossing=closest['samplePos']/48000
            indices=np.arange(len(delta))*.1+.05
            nearby=active&(abs(indices-crossing)<=.5)
            assert nearby.any()
            entry['axisZeroNeighborhood']={'axis':'y' if axis==1 else 'z',
                'nearestMetadataTimeSeconds':crossing,'position':closest['pos'],
                'maxIldSubjectDifferenceDb':float(abs(delta[nearby]).max()),
                'scope':'100ms short-time power; not movement audibility or headtracking acceptance'}
        report.append(entry)
        x0,y0,w,h=90,45+row*325,1000,240
        d.text((x0,y0-25),name+' 4-8kHz; blue KU100, red H13; L/R power dB',fill='black')
        limit=24
        for db in (-24,-12,0,12,24):
            yy=y0+h/2-db*h/(2*limit);d.line((x0,yy,x0+w,yy),fill=(205,205,205));d.text((x0-35,yy),str(db),fill='black')
        for k,(ild,mask) in enumerate(curves):
            points=[(x0+j*w/(len(ild)-1),y0+h/2-np.clip(v,-limit,limit)*h/(2*limit)) for j,v in enumerate(ild)]
            for j in range(1,len(points)):
                if mask[j]&mask[j-1]:d.line((points[j-1],points[j]),fill=('blue' if k==0 else 'red'),width=2)
        d.text((x0,y0+h+10),f'{lo}s',fill='black');d.text((x0+w-35,y0+h+10),f'{hi}s',fill='black')
    canvas.save(out/'motion-and-lose-ild.png')
    (out/'motion-and-lose-curves.json').write_text(json.dumps(report,indent=2))
    print(json.dumps(report,indent=2))


if __name__=='__main__':main()
