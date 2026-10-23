"""Offline smooth forward-presence hypothesis, NOT a proven vocal repair.

Keep source PCM/metadata and spatial renderer. Process each measured kernel
with the same filter in both ears, tapered by measurement angle. Preserve
rear/height anchors exactly. Intermediate moving filters still need auditing.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import numpy as np
from scipy import signal
import pyloudnorm as pyln


def helper(name, path):
    s=importlib.util.spec_from_file_location(name,path)
    m=importlib.util.module_from_spec(s);s.loader.exec_module(m);return m


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--harness',type=Path,required=True);args=parser.parse_args()
    repo=Path(__file__).resolve().parents[1]
    root=Path.home()/'Downloads/SDA-clarity-validation/subarashiki-front'
    out=root/'front-presence';out.mkdir(exist_ok=True)
    variant=helper('variant',repo/'scripts/diag-hrtf-manifest-variant.py')
    writer=helper('writer',repo/'scripts/diag-main-layer-protection.py')
    asset=repo/'apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json'
    path=variant.prepare_variant(asset,out/'assets',{})
    checks=[]
    for directory in ['hrtf-dense','hrtf']:
        p=out/'assets'/directory/'hrtf-set.json';m=json.loads(p.read_text())
        for pos in m['positions']:
            az=abs(pos['azimuth']);el=abs(pos['elevation'])
            def smooth(v):
                v=np.clip(v,0,1);return float(v*v*(3-2*v))
            weight=smooth(1-az/60)*smooth(1-el/30)
            if weight==0:
                checks.append({'az':pos['azimuth'],'el':pos['elevation'],'unchanged':True});continue
            original=np.fromfile(pos['dry'],dtype='<f4').reshape(2,-1).astype(float)
            # RBJ peaking +2dB at1.8kHz Q0.8, not high shelf/master gain.
            gain=2*weight;A=10**(gain/40);omega=2*np.pi*1800/48000
            alpha=np.sin(omega)/(2*.8);c=np.cos(omega)
            b=np.array([1+alpha*A,-2*c,1-alpha*A]);a=np.array([1+alpha/A,-2*c,1-alpha/A]);b/=a[0];a/=a[0]
            padded=signal.lfilter(b,a,np.pad(original,((0,0),(0,2048))),axis=1)
            tail=float(np.sum(padded[:,original.shape[1]:]**2)/np.sum(padded**2))
            assert tail<1e-5,'Refuse significant discarded kernel tail'
            changed=padded[:,:original.shape[1]].astype('<f4')
            # Retain original kernel length and common causal timing. Ear transfer
            # phase is only approximately invariant after finite-length cropping.
            target=p.parent/Path(pos['dry']).name;changed.tofile(target)
            pos['dry']=pos['wet']=str(target.resolve())
            digest=hashlib.sha256(target.read_bytes()).hexdigest()
            for key in ['dry','wet']:pos['assets'][key]['sha256']=digest
            checks.append({'az':pos['azimuth'],'el':pos['elevation'],'gainDb':gain,'discardedTailEnergyFraction':tail})
        p.write_text(json.dumps(m,indent=2),encoding='utf-8')
    fixture=root/'replay'
    env={k:v for k,v in os.environ.items() if not k.startswith(('SDA_ATMOS_REPLAY_','SDA_DIAGNOSTIC_'))}
    env.update(SDA_ATMOS_REPLAY_DIR=str(fixture),SDA_ATMOS_REPLAY_SECONDS='181',SDA_ATMOS_REPLAY_WET='0',SDA_ATMOS_REPLAY_DIRECT='1',SDA_ATMOS_REPLAY_FRONT_COMMON='0',SDA_ATMOS_REPLAY_SPATIAL_BALANCE='0',SDA_ATMOS_REPLAY_ENHANCEMENT='1',SDA_ATMOS_REPLAY_PRESENTATION_DB='-12')
    outputs=[]
    for tag,manifest in [('presence-baseline',asset),('presence-candidate',path)]:
        env.update(SDA_ATMOS_REPLAY_HRTF=str(manifest),SDA_ATMOS_REPLAY_TAG=tag)
        with (out/(tag+'.log')).open('w') as log:
            subprocess.run([str(args.harness),'atmos_front_pair_probe::replay_atmos_object_stream','--exact','--ignored','--nocapture','--test-threads=1'],cwd=repo,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
        meta=json.loads((fixture/(tag+'.json')).read_text());assert meta['minimumGuard']==1 and not meta['frontCommonActive']
        y=np.fromfile(fixture/(tag+'.f32'),dtype='<f4').reshape(-1,2).astype(float);assert np.isfinite(y).all();outputs.append(y)
        print('Rendered',tag,flush=True)
    lo,hi=140*48000,180*48000;a,b=[v[lo:hi] for v in outputs]
    meter=pyln.Meter(48000);match=float(meter.integrated_loudness(a)-meter.integrated_loudness(b));b=b*10**(match/20)
    peak=max(abs(signal.resample_poly(a,4,1,axis=0)).max(),abs(signal.resample_poly(b,4,1,axis=0)).max());common=min(10**(12/20),.95/peak);a=a*common;b=b*common
    report={'scope':'Hypothesis: forward measured-kernel presence EQ,1800Hzmax+2dB,Q0.8, smooth angular weight. Same filters both ears, no source replacement/ID/time production rule, no recenter. Rear/height anchors unchanged; moving composite perception NOT guaranteed. Source metadata/PCM unchanged. Offline matching/safety only. Not promoted or packaged.', 'checks':checks,'matchDb':match,'files':[writer.write(out/'B-forward-presence-140-180.wav',b),writer.write(out/'A-then-B-forward-presence-140-180.wav',np.concatenate([a,np.zeros((48000,2)),b]))],'defaultChanged':False,'movingRegression':'pending','threeSongRegression':'pending'}
    (out/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    print('Saved comparison; source/render paths retained, wider regressions pending.')


if __name__=='__main__':main()
