"""Offline descriptive JOC/core mapping, never used as playback reconstruction.

Local held-out adjacent samples are dependent: no semantic/generalization claim.
Time-varying core mapping can legitimately differ from fixed object gains.
"""
import json
import argparse
from pathlib import Path
import numpy as np


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--drc-off',action='store_true')
    args=parser.parse_args()
    root=Path.home()/'Downloads/SDA-clarity-validation/subarashiki-front'
    folder=root/'replay'
    frames=[json.loads(s) for s in (folder/'frames.jsonl').read_text().splitlines()]
    raw=np.memmap(folder/'sources.pcm',dtype='<f4',mode='r')
    lo,hi=140*48000,180*48000;chunks=[];offset=0
    for frame in frames:
        n=frame['samples'];channels=len(frame['labels'])
        block=raw[offset:offset+n*channels].reshape(channels,n);offset+=n*channels
        left=max(0,lo-frame['start']);right=min(n,hi-frame['start'])
        if right>left:chunks.append(block[1:,left:right].T.copy())
    x=np.concatenate(chunks).astype(float)
    timing=json.loads((root/'core-downmix/report.json').read_text())
    start=timing['coreStartSample']
    core_name='core51-drc-off.f32' if args.drc_off else 'core51.f32'
    core=np.memmap(root/'core-downmix'/core_name,dtype='<f4',mode='r').reshape(-1,6)
    y=np.asarray(core[start:start+len(x),[0,1,2,4,5]],float)
    assert len(x)==40*48000 and np.isfinite(x).all()
    # Fixed coefficient model trained first19s, validated last19s.
    train=np.arange(len(x))<19*48000;valid=np.arange(len(x))>=21*48000
    fixed=np.linalg.lstsq(x[train],y[train],rcond=None)[0]
    fixed_error=x[valid]@fixed-y[valid]
    fixed_residual=10*np.log10(np.sum(fixed_error**2,axis=0)/np.sum(y[valid]**2,axis=0))
    rows=[];total_error=np.zeros(5);total_power=np.zeros(5)
    coefficients=[]
    for index in range(160):
        left=index*12000;right=left+12000
        a=x[left:right];b=y[left:right]
        # Chronological local fit90ms,10ms gap,validate150ms. Deliberately
        # not overlapping same samples or training on validation error.
        fit=np.linalg.lstsq(a[:4320],b[:4320],rcond=1e-7)[0]
        predicted=a[4800:]@fit;error=predicted-b[4800:]
        energy=np.sum(error**2,axis=0);power=np.sum(b[4800:]**2,axis=0)
        total_error+=energy;total_power+=power;coefficients.append(fit)
        residual=10*np.log10(np.maximum(energy,1e-30)/np.maximum(power,1e-30))
        rows.append({'songWindowSeconds':[140+index*.25,140+(index+1)*.25],
                     'localHeldoutResidualDb':residual.tolist(),
                     'conditionNumber':float(np.linalg.cond(a[:4320]))})
    report={'scope':'Same rawEC3 object PCM and independently aligned FFmpegcore5.1. Fixed and short-time unconstrained linear descriptions, no normative matrix/source-role claim. Local target samples adjacent/dependent, cannot certify decoder correctness, voice extraction or predict other songs. No renderer edits.',
            'referenceCoreFile':core_name,
            'channels':['FL','FR','C','SL','SR'],
            'fixedChronologicalHeldoutResidualDb':fixed_residual.tolist(),
            'localChronologicalHeldoutAggregateResidualDb':(10*np.log10(total_error/total_power)).tolist(),
            'windows':rows,
            'interpretation':'Improving local fit supports time-varying mapping as an explanation for fixed-fit residual, but does not prove JOC reconstruction parity. Do not turn fitted coefficients into a playback source selector.'}
    out=root/'core-downmix';tag='mapping-stability-drc-off' if args.drc_off else 'mapping-stability'
    (out/(tag+'.json')).write_text(json.dumps(report,indent=2))
    np.savez_compressed(out/(tag+'-coefficients.npz'),coefficients=np.stack(coefficients))
    print(json.dumps({k:report[k] for k in ['channels','fixedChronologicalHeldoutResidualDb','localChronologicalHeldoutAggregateResidualDb']},indent=2))


if __name__=='__main__':main()
