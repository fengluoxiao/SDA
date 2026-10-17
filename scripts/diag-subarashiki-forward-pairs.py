"""Offline actual per-source forward pair phase-energy attribution.

Diagnostic IDs follow this fixture's observed geometry, never production roles.
No phase/gain changes, reference injection or source replacement.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess

import numpy as np
from scipy import signal


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--harness',type=Path,required=True)
    args=parser.parse_args()
    repo=Path(__file__).resolve().parents[1]
    root=Path.home()/'Downloads/SDA-clarity-validation/subarashiki-front'
    fixture=root/'replay';out=root/'forward-pair-audit';out.mkdir(exist_ok=True)
    frames=[json.loads(s) for s in (fixture/'frames.jsonl').read_text().splitlines()]
    labels=frames[0]['labels'];ids=[10,16,19,20,24]
    raw=np.memmap(fixture/'sources.pcm',dtype='<f4',mode='r')
    (out/'frames.jsonl').write_bytes((fixture/'frames.jsonl').read_bytes())
    env={k:v for k,v in os.environ.items() if not k.startswith(('SDA_ATMOS_REPLAY_','SDA_DIAGNOSTIC_'))}
    env.update(SDA_ATMOS_REPLAY_DIR=str(out),
               SDA_ATMOS_REPLAY_HRTF=str(root/'reflection-ablation/direct-assets/hrtf-dense/hrtf-set.json'),
               SDA_ATMOS_REPLAY_SECONDS='181',SDA_ATMOS_REPLAY_WET='0',SDA_ATMOS_REPLAY_DIRECT='1',
               SDA_ATMOS_REPLAY_FRONT_COMMON='0',SDA_ATMOS_REPLAY_SPATIAL_BALANCE='0',
               SDA_ATMOS_REPLAY_ENHANCEMENT='1',SDA_ATMOS_REPLAY_PRESENTATION_DB='-12')
    outputs=[];inputs=[];lo=140*48000;hi=180*48000
    for ident in ids:
        tag='Obj_'+str(ident);channel=labels.index(tag);offset=0;chunks=[]
        with (out/'sources.pcm').open('wb') as file:
            for frame in frames:
                n=frame['samples'];c=len(labels);original=raw[offset:offset+n*c].reshape(c,n);offset+=n*c
                data=np.zeros((c,n),dtype='<f4');data[channel]=original[channel];file.write(data.tobytes())
                l=max(0,lo-frame['start']);h=min(n,hi-frame['start'])
                if h>l:chunks.append(data[channel,l:h].copy())
        env['SDA_ATMOS_REPLAY_TAG']=tag
        with (out/(tag+'.log')).open('w') as log:
            subprocess.run([str(args.harness),'atmos_front_pair_probe::replay_atmos_object_stream',
                            '--exact','--ignored','--nocapture','--test-threads=1'],cwd=repo,
                           env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
        meta=json.loads((out/(tag+'.json')).read_text());assert meta['minimumGuard']==1
        outputs.append(np.fromfile(out/(tag+'.f32'),dtype='<f4').reshape(-1,2).astype(float))
        inputs.append(np.concatenate(chunks).astype(float))
        print('Rendered',tag,flush=True)
    (out/'input-scope.json').write_text(json.dumps({'sources.pcm':'Last solo only, Obj24; original full fixture is unchanged.'}))
    group=np.fromfile(root/'forward-group-audit/replay/forward-group-direct-only.f32',dtype='<f4').reshape(-1,2).astype(float)
    rest=np.fromfile(root/'forward-group-audit/replay/remaining-scene-direct-only.f32',dtype='<f4').reshape(-1,2).astype(float)
    error=float(abs(sum(outputs)-group).max());assert error<1e-6
    bands={}
    for low,high in [(300,700),(700,1500),(1500,4000),(4000,8000)]:
        sos=signal.butter(3,[low,high],fs=48000,btype='bandpass',output='sos')
        z=np.stack([signal.sosfilt(sos,v,axis=0)[lo:hi] for v in outputs])
        r=signal.sosfilt(sos,rest,axis=0)[lo:hi]
        x=np.stack([signal.sosfilt(sos,v) for v in inputs])
        gram=np.einsum('ine,jne->ij',z,z,optimize=True)
        input_gram=x@x.T
        own=np.diag(gram);input_own=np.diag(input_gram)
        total=float(np.sum((z.sum(axis=0)+r)**2))
        pairs=[]
        for i in range(len(ids)):
            for j in range(i+1,len(ids)):
                pairs.append({'a':ids[i],'b':ids[j],
                              'inputCorrelation':float(input_gram[i,j]/max(np.sqrt(input_own[i]*input_own[j]),1e-30)),
                              'outputCorrelation':float(gram[i,j]/max(np.sqrt(own[i]*own[j]),1e-30)),
                              'crossEnergyOverFullPower':float(2*gram[i,j]/total)})
        bands[f'{low}-{high}']={
            'frontInternalCoherentVsIndependentDb':float(10*np.log10(gram.sum()/own.sum())),
            'mostNegativeFrontPairs':sorted(pairs,key=lambda v:v['crossEnergyOverFullPower'])[:4],
            'perSourceCrossWithRemainingScene':[{'id':ids[i],
                                                'crossEnergyOverFullPower':float(2*np.sum(z[i]*r)/total)} for i in range(len(ids))]}
    report={'scope':'Five diagnostic frontal object PCM solos through same direct-only actual renderer, full-prefix histories and original metadata. Input sums not authored downmix/vocal stems. Cross energy ratios descriptive, not automatic defect or listening criterion. No default changes.',
            'groupAdditivityMaxAbs':error,'bands':bands,'productionChanged':False}
    (out/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    print(json.dumps({k:{'internalDb':v['frontInternalCoherentVsIndependentDb'],
                         'pairs':v['mostNegativeFrontPairs'][:2],
                         'withRest':v['perSourceCrossWithRemainingScene']} for k,v in bands.items()},indent=2))


if __name__=='__main__':main()
