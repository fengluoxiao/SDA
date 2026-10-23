"""Actual moving/audio fixtures for test-only direction-independent cue window."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import numpy as np
from scipy import signal
import pyloudnorm as pyln


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--harness',type=Path,required=True);args=parser.parse_args()
    repo=Path(__file__).resolve().parents[1];base=Path.home()/'Downloads/SDA-clarity-validation'
    out=base/'temporal-reflection-window';out.mkdir(exist_ok=True)
    spec=importlib.util.spec_from_file_location('writer',repo/'scripts/diag-main-layer-protection.py');writer=importlib.util.module_from_spec(spec);spec.loader.exec_module(writer)
    cases=[('cross-front',base/'motion-front-policy/cross-front',12,0,12,False),
           ('vertical',base/'motion-front-policy/vertical',12,0,12,False),
           ('subarashiki',base/'subarashiki-front/replay',181,140,180,True)]
    env={k:v for k,v in os.environ.items() if not k.startswith(('SDA_ATMOS_REPLAY_','SDA_DIAGNOSTIC_'))}
    env.update(SDA_ATMOS_REPLAY_HRTF=str(repo/'apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json'),SDA_ATMOS_REPLAY_WET='0',SDA_ATMOS_REPLAY_DIRECT='1',SDA_ATMOS_REPLAY_FRONT_COMMON='0',SDA_ATMOS_REPLAY_SPATIAL_BALANCE='0')
    results=[]
    for name,folder,seconds,start,end,song in cases:
        env.update(SDA_ATMOS_REPLAY_DIR=str(folder),SDA_ATMOS_REPLAY_SECONDS=str(seconds),SDA_ATMOS_REPLAY_ENHANCEMENT='1' if song else '0')
        if song:env['SDA_ATMOS_REPLAY_PRESENTATION_DB']='-12'
        else:env.pop('SDA_ATMOS_REPLAY_PRESENTATION_DB',None)
        ys=[]
        for k in [0,1]:
            tag='temporal-window-'+str(k);env['SDA_ATMOS_REPLAY_TAG']=tag
            if k:env['SDA_DIAGNOSTIC_REFLECTION_WINDOW']='12-35'
            else:env.pop('SDA_DIAGNOSTIC_REFLECTION_WINDOW',None)
            with (out/(name+'-'+tag+'.log')).open('w') as log:
                subprocess.run([str(args.harness),'atmos_front_pair_probe::replay_atmos_object_stream','--exact','--ignored','--nocapture','--test-threads=1'],cwd=repo,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
            meta=json.loads((folder/(tag+'.json')).read_text());assert meta['minimumGuard']==1
            ys.append(np.fromfile(folder/(tag+'.f32'),dtype='<f4').reshape(-1,2).astype(float))
        a,b=ys;lo,hi=start*48000,end*48000
        assert a.shape==b.shape and np.isfinite(b).all()
        row={'name':name,'songTimeSeconds':[start,end] if song else None,'bands':{}}
        for low,high in [(100,500),(500,1500),(1500,4000),(4000,8000)]:
            sos=signal.butter(3,[low,high],fs=48000,btype='bandpass',output='sos');x=signal.sosfilt(sos,a,axis=0)[lo:hi];y=signal.sosfilt(sos,b,axis=0)[lo:hi];n=len(x)//4800;x=x[:n*4800].reshape(n,4800,2);y=y[:n*4800].reshape(n,4800,2);pa=np.mean(x*x,axis=1);pb=np.mean(y*y,axis=1);active=pa.min(axis=1)>max(pa.max()*1e-3,1e-12)
            if not active.any():continue
            change=10*np.log10(np.maximum(pb,1e-30)/np.maximum(pa,1e-30));ild=change[:,1]-change[:,0]
            row['bands'][f'{low}-{high}']={'maximum100msEarRatioChangeDb':float(abs(ild[active]).max()),'maximum100msEarPowerChangeDb':float(abs(change[active]).max())}
        a=a[lo:hi];b=b[lo:hi];match=0.
        if song:
            meter=pyln.Meter(48000);match=float(meter.integrated_loudness(a)-meter.integrated_loudness(b));b*=10**(match/20)
        peak=max(abs(signal.resample_poly(a,4,1,axis=0)).max(),abs(signal.resample_poly(b,4,1,axis=0)).max());gain=min(10**(12/20) if song else 1.,.95/peak);a*=gain;b*=gain
        row['offlineMatchDb']=match
        row['comparison']=writer.write(out/(name+'-current-then-short-window.wav'),np.concatenate([a,np.zeros((48000,2)),b]))
        row['candidate']=writer.write(out/(name+'-short-window.wav'),b)
        results.append(row);print(json.dumps(row),flush=True)
        (out/'report.json').write_text(json.dumps({'scope':'12ms unchanged optional reflections, smooth fade to zero35ms, SAME kernel-relative time envelope every speaker/ear. Original direct HRTF, positions and source levels retained. Test-only, not proven stable spatial perception or vocal repair. No file prescan in playback.','rows':results,'defaultChanged':False},indent=2))


if __name__=='__main__':main()
