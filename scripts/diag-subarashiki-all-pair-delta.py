"""Compare full-scene pair correlations before/after actual directional render.

Diagnostic ordinals are never production labels. Equal-weight input sum is NOT
an authored downmix. Correlation delta is descriptive, not a correction rule.
"""
import json
from pathlib import Path
import numpy as np
from scipy import signal
from PIL import Image, ImageDraw


def main():
    root = Path.home()/'Downloads/SDA-clarity-validation/subarashiki-front'
    out = root/'all-pair-delta-audit'; out.mkdir(exist_ok=True)
    frames = [json.loads(s) for s in (root/'replay/frames.jsonl').read_text().splitlines()]
    raw = np.memmap(root/'replay/sources.pcm', dtype='<f4', mode='r')
    lo, hi, delay = 140*48000, 180*48000, 2288
    chunks = []; offset = 0
    for frame in frames:
        n = frame['samples']; c = len(frame['labels'])
        block = raw[offset:offset+n*c].reshape(c,n); offset += n*c
        l = max(0, lo-delay-frame['start']); r = min(n, hi-delay-frame['start'])
        if r > l:
            assert frame['labels'] == ['LFE']+['Obj_'+str(i) for i in range(10,25)]
            chunks.append(block[1:,l:r].copy())
    inputs = np.concatenate(chunks,axis=1); del chunks
    outputs = []
    for ident in range(10,25):
        meta = json.loads((root/f'forward-pair-audit/Obj_{ident}.json').read_text())
        assert meta['minimumGuard'] == 1 and not meta['frontCommonActive']
        y = np.memmap(root/f'forward-pair-audit/Obj_{ident}.f32',dtype='<f4',mode='r').reshape(-1,2)
        outputs.append(y[lo:hi].copy())
    assert inputs.shape[1] == hi-lo
    report = {'scope':'All15 actual cue-off full-history solos; no LFE in Gram. Input sum not authored mix. Negative/delta not defect proof.',
              'productionChanged':False,'bands':{}}
    plots = []
    for low,high in [(300,700),(700,1500),(1500,4000),(4000,8000)]:
        sos = signal.butter(3,[low,high],fs=48000,btype='bandpass',output='sos')
        x = signal.sosfilt(sos, inputs, axis=1).astype('float32')
        z = np.stack([signal.sosfilt(sos,y,axis=0).T.reshape(-1).astype('float32') for y in outputs])
        # Flatten ears separately: dot-product sums ear energies, not L+R mono.
        gx = (x@x.T).astype(float); gz = (z@z.T).astype(float)
        def normalize(g):
            return g/np.maximum(np.sqrt(np.diag(g)[:,None]*np.diag(g)[None,:]),1e-30)
        cx,cz = normalize(gx),normalize(gz); delta = cz-cx
        total = gz.sum(); assert total > 0
        pairs=[]
        for i in range(15):
            for j in range(i+1,15):
                pairs.append({'a':i+10,'b':j+10,'inputCorrelation':float(cx[i,j]),
                              'outputCorrelation':float(cz[i,j]),'correlationDelta':float(delta[i,j]),
                              'outputCrossOverScene':float(2*gz[i,j]/total)})
        sources=[]
        for i in range(15):
            # Projection distinguishes interference from unrelated masker power,
            # but not semantic vocal. No limiter/prescan policy is derived here.
            own=gz[i,i]; cross=gz[i].sum()-own
            rest=total-own-2*cross
            sources.append({'ordinal':i+10,'soloEnergyOverScene':float(own/total),
                            'crossOverScene':float(2*cross/total),
                            'restEnergyOverSoloDb':float(10*np.log10(max(rest/own,1e-30))),
                            'restProjectionOntoSolo':float(cross/own)})
        report['bands'][f'{low}-{high}']={'mostNegativeOutput':sorted(pairs,key=lambda p:p['outputCrossOverScene'])[:10],
                                         'largestCorrelationDrop':sorted(pairs,key=lambda p:p['correlationDelta'])[:10],
                                         'sourceProjection':sources}
        np.savez_compressed(out/f'gram-{low}-{high}.npz',inputGram=gx,outputGram=gz,inputCorrelation=cx,outputCorrelation=cz)
        for title,matrix in [('input',cx),('actual output',cz),('output minus input',delta/2)]:
            v=np.clip(matrix,-1,1); c=np.ones((15,15,3))*255
            c[:,:,0]*=1-np.maximum(-v,0); c[:,:,1]*=1-abs(v);c[:,:,2]*=1-np.maximum(v,0)
            plots.append((f'{low}-{high} Hz {title}',Image.fromarray(c.astype('uint8')).resize((300,300),Image.Resampling.NEAREST)))
        print(low,high,'most negative',report['bands'][f'{low}-{high}']['mostNegativeOutput'][:2],flush=True)
        del x,z
    canvas=Image.new('RGB',(1060,1430),'white');d=ImageDraw.Draw(canvas)
    for k,(title,img) in enumerate(plots):
        row,col=divmod(k,3);x=45+col*345;y=20+row*350
        d.text((x,y),title,fill='black');canvas.paste(img,(x,y+28))
        d.text((x,y+330),'Rows/cols Obj10..24; blue negative',fill='black')
    canvas.save(out/'all-pair-correlations.png')
    (out/'report.json').write_text(json.dumps(report,indent=2))


if __name__=='__main__':main()
