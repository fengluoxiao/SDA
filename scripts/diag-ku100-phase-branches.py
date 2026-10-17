"""Offline check phase-branch ambiguity in actual KU100 spectral interpolation.

No branch alternative is applied to playback. Branch differences are hypotheses,
not proof that physical all-pass information is erroneous.
"""
import argparse
import json
import os
import subprocess
from pathlib import Path
import numpy as np
from PIL import Image, ImageDraw


def main():
    ap=argparse.ArgumentParser(description=__doc__);ap.add_argument('--harness',type=Path,required=True)
    a=ap.parse_args();repo=Path(__file__).resolve().parents[1]
    root=Path.home()/'Downloads/SDA-clarity-validation/subarashiki-front/phase-branch-audit';root.mkdir(exist_ok=True)
    requests=[{'az':float(az),'el':float(el)} for el in (0,30,60) for az in range(-175,180,10)]
    requests += [{'az':az,'el':0.} for az in (44.9999,45.0001,-45.0001,-44.9999)]
    (root/'requests.json').write_text(json.dumps(requests))
    env={k:v for k,v in os.environ.items() if not k.startswith(('SDA_FILTER_AUDIT_','SDA_DIAGNOSTIC_','SDA_ATMOS_REPLAY_'))}
    env.update(SDA_FILTER_AUDIT_REQUESTS=str(root/'requests.json'),SDA_FILTER_AUDIT_OUT=str(root))
    with (root/'export.log').open('w') as log:
        subprocess.run([str(a.harness),'directional::spectral_interpolation_audit::export_actual_direction_filters',
                        '--exact','--ignored','--nocapture','--test-threads=1'],cwd=repo,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
    rows=json.loads((root/'rows.json').read_text());asset=repo/'apps/mobile/assets/hrtf-restored/hrtf-dense'
    manifest=json.loads((asset/'hrtf-set.json').read_text());nfft=2048
    h=np.stack([np.fromfile(asset/p['dry'],dtype='<f4').reshape(2,-1).astype(float) for p in manifest['positions']])
    H=np.fft.rfft(h,n=nfft,axis=-1);phase=np.unwrap(np.angle(H),axis=-1)
    exported=np.fromfile(root/'filters.f32',dtype='<f4').reshape(len(rows),2,rows[0]['tapCount']).astype(float)
    report={'scope':'Actual exported Grid weights; alternative local circular branches are diagnostic only. No default changes or hearing claims.',
            'productionChanged':False,'directions':[]}
    plots=[];alternative=[];predicted=[];prediction_errors=[]
    f=np.fft.rfftfreq(nfft,1/48000)
    for row_index,row in enumerate(rows):
        weights=row['weights'];dominant=max(weights,key=lambda pair:pair[1])[0]
        idx=np.array([i for i,w in weights]);w=np.array([w for i,w in weights])
        raw_phase=np.sum(phase[idx]*w[:,None,None],axis=0)
        # Remove common linear delay before inspecting branches; don't equate
        # normal physical direction delay with an unwrap discontinuity.
        means=np.sum(h*h*np.arange(h.shape[-1]),axis=(1,2))/np.sum(h*h,axis=(1,2))
        residual=phase + 2*np.pi*f[None,None,:]*means[:,None,None]/48000
        anchor=residual[dominant]
        diff=residual[idx]-anchor[None,:,:]
        wrapped=(diff+np.pi)%(2*np.pi)-np.pi
        branch_turns=np.rint((diff-wrapped)/(2*np.pi))
        delta=-2*np.pi*np.sum(branch_turns*w[:,None,None],axis=0)
        power=np.sum(abs(H[idx])**2*w[:,None,None],axis=0)
        def synth(ph):
            spectrum=np.sqrt(power)*np.exp(1j*ph)
            spectrum[:,0]=np.sum(H[idx,:,0]*w[:,None],axis=0).real
            spectrum[:,-1]=np.sum(H[idx,:,-1]*w[:,None],axis=0).real
            return np.fft.irfft(spectrum,n=nfft,axis=-1)
        raw=synth(raw_phase);alt=synth(raw_phase+delta)
        prediction_errors.append(float(np.max(abs(raw[:,:rows[0]['tapCount']]-exported[row_index]))))
        predicted.append(raw);alternative.append(alt)
        alt_energy=np.sum(alt*alt)
        pre_energy=float(np.sum(alt[:,:100]**2)/alt_energy)
        tail_energy=float(np.sum(alt[:,rows[0]['tapCount']:]**2)/alt_energy)
        active=(np.sum(abs(H[idx])**2*w[:,None,None],axis=0)>np.max(abs(H)**2)*1e-4)
        band=(f>=700)&(f<=8000)
        eligible=active&band[None,:]
        angular=np.angle(np.exp(1j*delta))
        frac=float(np.mean((abs(angular[eligible])>.5)))
        bins=np.where(np.any(eligible&(abs(angular)>.5),axis=0))[0]
        report['directions'].append({'az':row['az'],'el':row['el'],'dominant':int(dominant),
            'activePhaseDifferenceOverHalfRadianFraction':frac,
            'alternativePre100SampleEnergyFraction':pre_energy,
            'alternativeBeyondStoredLengthEnergyFraction':tail_energy,
            'firstAffectedFrequencyHz':float(f[bins[0]]) if len(bins) else None})
        plots.append(np.max(abs(angular),axis=0)[band])
    matrix=np.stack(plots[:108])
    canvas=Image.new('RGB',(1120,600),'white');d=ImageDraw.Draw(canvas)
    for k,el in enumerate((0,30,60)):
        v=matrix[k*36:(k+1)*36]/np.pi
        c=np.stack([np.full_like(v,255),255*(1-v),255*(1-v)],axis=-1)
        img=Image.fromarray(c.astype('uint8')).resize((1000,150),Image.Resampling.NEAREST)
        y=30+k*180;d.text((80,y),f'el{el}: az -175..175 vertical; f700..8000 horizontal; red phase branch difference',fill='black');canvas.paste(img,(80,y+20))
    canvas.save(root/'phase-branch-difference.png')
    report['affectedDirectionCount']=sum(r['activePhaseDifferenceOverHalfRadianFraction']>.1 for r in report['directions'])
    report['actualExportPredictionMaxAbs']=max(prediction_errors)
    assert report['actualExportPredictionMaxAbs']<1e-6, 'Audit does not match actual runtime interpolation'
    def relative(a,b):return float(np.sqrt(np.sum((a-b)**2)/np.sum(a*a)))
    report['dominantSwitchStress']=[{'nearAz':45 if k==108 else -45,
        'actualRelativeWaveformDifference':relative(predicted[k],predicted[k+1]),
        'nearestBranchAlternativeRelativeWaveformDifference':relative(alternative[k],alternative[k+1])} for k in (108,110)]
    report['alternativeMaxPre100EnergyFraction']=max(r['alternativePre100SampleEnergyFraction'] for r in report['directions'])
    report['alternativeMaxTailFraction']=max(r['alternativeBeyondStoredLengthEnergyFraction'] for r in report['directions'])
    (root/'report.json').write_text(json.dumps(report,indent=2));print(json.dumps({'directions':len(rows),'affected':report['affectedDirectionCount']}))


if __name__=='__main__':main()
