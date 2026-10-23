"""Offline earwise STFT difference plots. Mixed bins are not vocal masks."""
import json
import argparse
from pathlib import Path
import numpy as np
from scipy import signal
from PIL import Image, ImageDraw


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--tag',default='cue-minus12-valid')
    args=parser.parse_args()
    assert all(c.isalnum() or c in '-_' for c in args.tag)
    downloads=Path.home()/'Downloads'
    out=downloads/('SDA-clarity-validation/'+('rejected-cue-heatmaps' if args.tag=='cue-minus12-valid' else args.tag+'-heatmaps'))
    out.mkdir(exist_ok=True)
    cases=[('How-Love',downloads/'SDA-current-render-audition/How-Do-I-Make-You-Love-Me-55-63',55,63,0),
           ('Lose-Yourself',downloads/'SDA-Lose-Yourself-audit/replay',115,131,5471232)]
    reports=[]
    for name,folder,start,end,origin in cases:
        spectra=[]
        for tag in ['peak-stall-fixed',args.tag]:
            a=np.fromfile(folder/(tag+'.f32'),dtype='<f4').reshape(-1,2)
            f,t,z=signal.stft(a,fs=48000,nperseg=2048,noverlap=1536,axis=0,boundary=None,padded=False)
            use=(t+origin/48000>=start)&(t+origin/48000<end)
            band=(f>=200)&(f<=6000)
            spectra.append(abs(z[band][:,:,use].transpose(0,2,1))**2)
        p,q=spectra
        # Per-ear activity gates avoid ratios of silence; -35dB relative band max.
        active=p>np.maximum(p.max(axis=(0,1),keepdims=True)*10**(-35/10),1e-12)
        delta=10*np.log10(np.maximum(q,1e-30)/np.maximum(p,1e-30))
        joint=active[:,:,0]&active[:,:,1]
        lr=delta[:,:,1]-delta[:,:,0]
        time=t[use]+origin/48000;freq=f[band]
        canvas=Image.new('RGB',(1300,1050),'white');draw=ImageDraw.Draw(canvas)
        panels=[(delta[:,:,0],active[:,:,0],'Left ear power change'),
                (delta[:,:,1],active[:,:,1],'Right ear power change'),
                (lr,joint,'Right-minus-left change (red = more right-relative)')]
        for k,(values,mask,title) in enumerate(panels):
            v=np.clip(values/3,-1,1)
            rgb=np.stack([1-np.maximum(-v,0),1-abs(v),1-np.maximum(v,0)],axis=2)
            rgb[~mask]=.8
            image=Image.fromarray((rgb[::-1]*255).astype('uint8')).resize((1120,260))
            top=60+k*320
            canvas.paste(image,(90,top))
            draw.text((90,top-25),name+': '+title+'; blue=-3, white=0, red=+3dB; grey=inactive',fill='black')
            for hz in [200,1000,2000,4000,6000]:
                draw.text((10,top+int((6000-hz)/5800*260)),str(hz)+'Hz',fill='black')
            for sec in np.linspace(start,end,9):
                draw.text((90+int((sec-start)/(end-start)*1120),top+268),f'{sec:g}s',fill='black')
        draw.text((90,1020),'Same raw outputs; 42.7ms Hann, 10.7ms hop. Not isolated voice or perceptual acceptance.',fill='black')
        canvas.save(out/(name+'.png'))
        np.savez_compressed(out/(name+'.npz'),frequency=freq,time=time,baseline=p,candidate=q,active=active)
        cells=[]
        for low,high in [(500,1500),(1500,4000)]:
            b=(freq>=low)&(freq<high)
            weighted=p[b].sum(axis=2)*joint[b]
            adverse=lr[b]>.5
            cells.append({'band':[low,high],'jointActiveReferenceEnergyInRightRelativeAbove0p5DbFraction':float((weighted*adverse).sum()/weighted.sum()),
                          'activeCellRtoLDeltaDbP10P50P90':np.percentile(lr[b][joint[b]],[10,50,90]).tolist()})
        reports.append({'song':name,'bands':cells})
    (out/'report.json').write_text(json.dumps({'scope':'Candidate earwise spectrogram. Activity thresholds are diagnostics, not audibility thresholds. No EQ/gain changes inferred.', 'candidateTag':args.tag,'songs':reports},indent=2))
    print(json.dumps(reports,indent=2))


if __name__=='__main__':main()
