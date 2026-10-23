"""Actual complete-subject diagnostic replays; not a production regression pass.

Compare cue-off KU100 vs existing H13 using identical decoded PCM/metadata.
Level match is offline only. Individual differences are not renderer defects.
"""
import argparse
import importlib.util
import json
import os
import subprocess
from pathlib import Path
import numpy as np
from scipy import signal
import pyloudnorm as pyln


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--harness',type=Path,required=True)
    p.add_argument('--songs',action='store_true')
    a=p.parse_args();repo=Path(__file__).resolve().parents[1];downloads=Path.home()/'Downloads'
    base=downloads/'SDA-clarity-validation';out=base/'subject-motion-regression';out.mkdir(exist_ok=True)
    s=importlib.util.spec_from_file_location('variant',repo/'scripts/diag-hrtf-manifest-variant.py')
    helper=importlib.util.module_from_spec(s);s.loader.exec_module(helper)
    manifests=[helper.prepare_variant(repo/'apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json',out/'ku100-assets',{'spatialCues':False}),
               helper.prepare_variant(repo/'apps/web/public/hrtf-h13-dense/hrtf-set.json',out/'h13-assets',{'spatialCues':False,'historicalInterpolation':False})]
    cases=[('cross-front',base/'motion-front-policy/cross-front',12,0,12,False),
           ('vertical',base/'motion-front-policy/vertical',12,0,12,False)]
    if a.songs:
        cases += [('How-Love',downloads/'SDA-current-render-audition/How-Do-I-Make-You-Love-Me-55-63',64,55,63,True),
                  ('Very-First-Night',downloads/'SDA-current-render-audition/Very-First-Night-0-65',66,0,65,True),
                  ('Lose-Yourself',downloads/'SDA-Lose-Yourself-audit/replay',139,None,None,True)]
    env={k:v for k,v in os.environ.items() if not k.startswith(('SDA_ATMOS_REPLAY_','SDA_DIAGNOSTIC_'))}
    env.update(SDA_ATMOS_REPLAY_WET='0',SDA_ATMOS_REPLAY_DIRECT='1',SDA_ATMOS_REPLAY_FRONT_COMMON='0',
               SDA_ATMOS_REPLAY_SPATIAL_BALANCE='0',SDA_ATMOS_REPLAY_PRESENTATION_DB='-12')
    report={'scope':'Complete HRTF subject and supported interpolation change, cues off; no formal improvement acceptance. Motion tests synthetic, not headset/headtracking validation.',
            'productionChanged':False,'cases':[]}
    for name,folder,seconds,start,end,song in cases:
        ys=[];meta=[]
        for subject,manifest in zip(('ku100','h13'),manifests):
            tag='subject-regression-'+subject
            env.update(SDA_ATMOS_REPLAY_DIR=str(folder),SDA_ATMOS_REPLAY_SECONDS=str(seconds),
                       SDA_ATMOS_REPLAY_ENHANCEMENT='1' if song else '0',SDA_ATMOS_REPLAY_TAG=tag,SDA_ATMOS_REPLAY_HRTF=str(manifest))
            with (out/(name+'-'+subject+'.log')).open('w') as log:
                subprocess.run([str(a.harness),'atmos_front_pair_probe::replay_atmos_object_stream',
                                '--exact','--ignored','--nocapture','--test-threads=1'],cwd=repo,env=env,
                               stdout=log,stderr=subprocess.STDOUT,check=True)
            m=json.loads((folder/(tag+'.json')).read_text());assert m['minimumGuard']==1
            y=np.fromfile(folder/(tag+'.f32'),dtype='<f4').reshape(-1,2).astype(float)
            assert np.isfinite(y).all();ys.append(y);meta.append(m)
        assert ys[0].shape==ys[1].shape
        lo=0 if start is None else start*48000;hi=len(ys[0]) if end is None else end*48000
        assert 0<=lo<hi<=len(ys[0])
        match=pyln.Meter(48000).integrated_loudness(ys[0][lo:hi])-pyln.Meter(48000).integrated_loudness(ys[1][lo:hi])
        row={'name':name,'metadata':meta,'offlineMatchDb':float(match),'windowSamples':[lo,hi],'bands':{}}
        for low,high in [(300,700),(700,1500),(1500,4000),(4000,8000)]:
            sos=signal.butter(3,[low,high],fs=48000,btype='bandpass',output='sos')
            x,z=[signal.sosfilt(sos,y,axis=0)[lo:hi] for y in ys]
            z*=10**(match/20);n=len(x)//4800
            pa=np.mean(x[:n*4800].reshape(n,4800,2)**2,axis=1)
            pb=np.mean(z[:n*4800].reshape(n,4800,2)**2,axis=1)
            active=pa.min(axis=1)>max(pa.max()*1e-3,1e-12)
            if not active.any():continue
            delta=10*np.log10(np.maximum(pb,1e-30)/np.maximum(pa,1e-30))
            ild=delta[:,0]-delta[:,1]
            row['bands'][f'{low}-{high}']={'powerChangeDb':float(10*np.log10(pb.sum()/pa.sum())),
                'max100msIldChangeDb':float(abs(ild[active]).max()),
                'p95IldChangeDb':float(np.percentile(abs(ild[active]),95)),
                'activeWindows':int(active.sum())}
        report['cases'].append(row)
        (out/('songs-report.json' if a.songs else 'motion-report.json')).write_text(json.dumps(report,indent=2))
        print(json.dumps({'name':name,'bands':row['bands']}),flush=True)


if __name__=='__main__':main()
