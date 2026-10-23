"""Offline pair/complement replays; no ID-based production rule."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import numpy as np
from scipy import signal


def main():
    ap=argparse.ArgumentParser();ap.add_argument('--harness',type=Path,required=True);args=ap.parse_args()
    repo=Path(__file__).resolve().parents[1];downloads=Path.home()/'Downloads'
    source=downloads/'SDA-Lose-Yourself-audit/replay'
    out=downloads/'SDA-clarity-validation/lose-pair-attribution';out.mkdir(exist_ok=True)
    frames=[json.loads(s) for s in (source/'frames.jsonl').read_text().splitlines()]
    frames=[f for f in frames if f['start']<18*48000]
    count=sum(f['samples']*len(f['labels']) for f in frames)
    raw=np.fromfile(source/'sources.pcm',dtype='<f4',count=count)
    (out/'frames.jsonl').write_text('\n'.join(json.dumps(f) for f in frames)+'\n')
    manifests=[repo/'apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json',downloads/'SDA-clarity-validation/cue-minus12-valid-candidate/assets/hrtf-dense/hrtf-set.json']
    env={k:v for k,v in os.environ.items() if not k.startswith(('SDA_ATMOS_REPLAY_','SDA_DIAGNOSTIC_'))}
    env.update(SDA_ATMOS_REPLAY_DIR=str(out),SDA_ATMOS_REPLAY_WET='0',SDA_ATMOS_REPLAY_SECONDS='18',
               SDA_ATMOS_REPLAY_DIRECT='1',SDA_ATMOS_REPLAY_FRONT_COMMON='0',SDA_ATMOS_REPLAY_SPATIAL_BALANCE='0',
               SDA_ATMOS_REPLAY_ENHANCEMENT='1',SDA_ATMOS_REPLAY_PRESENTATION_DB='-12')
    results={}
    for group in ['pair17-19','rest']:
        data=raw.copy();offset=0
        for f in frames:
            n=f['samples'];block=data[offset:offset+n*16].reshape(16,n);offset+=n*16
            for channel,label in enumerate(f['labels']):
                keep=label in ['Obj_17','Obj_19']
                if keep!=(group=='pair17-19'):block[channel]=0
        data.tofile(out/'sources.pcm')
        for k,manifest in enumerate(manifests):
            tag=group+'-'+str(k);env.update(SDA_ATMOS_REPLAY_HRTF=str(manifest),SDA_ATMOS_REPLAY_TAG=tag)
            with (out/(tag+'.log')).open('w') as log:
                subprocess.run([str(args.harness),'atmos_front_pair_probe::replay_atmos_object_stream','--exact','--ignored','--nocapture','--test-threads=1'],cwd=repo,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
            assert json.loads((out/(tag+'.json')).read_text())['minimumGuard']==1
            results[(group,k)]=np.fromfile(out/(tag+'.f32'),dtype='<f4').reshape(-1,2).astype(float)
    raw.tofile(out/'sources.pcm')
    errors=[]
    for k in [0,1]:
        full=np.fromfile(source/('cue-attribution-safe-'+str(k)+'.f32'),dtype='<f4').reshape(-1,2)[:len(results[('rest',k)])]
        error=float(abs(results[('pair17-19',k)]+results[('rest',k)]-full).max());assert error<1e-6;errors.append(error)
    report={'scope':'Pair17/19 previously linked diagnostically to come-on, not authored vocal stems. Complete event graph preserved, other PCM zeroed. Unity guard and additive validation. No production source rule.', 'additivityMaxAbs':errors,'bands':{}}
    lo=115*48000-5471232;hi=131*48000-5471232
    for low,high in [(500,1500),(1500,4000)]:
        sos=signal.butter(3,[low,high],fs=48000,btype='bandpass',output='sos')
        powers=[];cross=[]
        for k in [0,1]:
            a=signal.sosfilt(sos,results[('pair17-19',k)],axis=0)[lo:hi].reshape(160,4800,2)
            b=signal.sosfilt(sos,results[('rest',k)],axis=0)[lo:hi].reshape(160,4800,2)
            powers.append((np.mean(a*a,axis=1),np.mean(b*b,axis=1)))
            cross.append(2*np.mean(a*b,axis=1))
        p0=powers[0][0]+powers[0][1]+cross[0];p1=powers[1][0]+powers[1][1]+cross[1]
        d=10*np.log10(p1/p0);lr=d[:,1]-d[:,0]
        rows=[]
        for i in np.argsort(lr)[-4:][::-1]:
            rows.append({'songSeconds':115+int(i)*.1,'mixedRightRelativeDeltaDb':float(lr[i]),
                         'pairPowerChangeOverFullBaseline':((powers[1][0][i]-powers[0][0][i])/p0[i]).tolist(),
                         'restPowerChangeOverFullBaseline':((powers[1][1][i]-powers[0][1][i])/p0[i]).tolist(),
                         'pairRestCrossChangeOverFullBaseline':((cross[1][i]-cross[0][i])/p0[i]).tolist()})
        report['bands'][f'{low}-{high}']=rows
    (out/'report.json').write_text(json.dumps(report,indent=2));print(json.dumps(report,indent=2))


if __name__=='__main__':main()
