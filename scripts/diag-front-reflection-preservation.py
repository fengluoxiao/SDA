"""Test-only front reflection ablation; all direct and non-front kernels retained."""
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
    repo=Path(__file__).resolve().parents[1];root=Path.home()/'Downloads/SDA-clarity-validation/subarashiki-front';fixture=root/'replay';out=root/'front-reflection-preservation';out.mkdir(exist_ok=True)
    env={k:v for k,v in os.environ.items() if not k.startswith(('SDA_ATMOS_REPLAY_','SDA_DIAGNOSTIC_'))}
    env.update(SDA_ATMOS_REPLAY_DIR=str(fixture),SDA_ATMOS_REPLAY_SECONDS='181',SDA_ATMOS_REPLAY_HRTF=str(repo/'apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json'),SDA_ATMOS_REPLAY_WET='0',SDA_ATMOS_REPLAY_DIRECT='1',SDA_ATMOS_REPLAY_FRONT_COMMON='0',SDA_ATMOS_REPLAY_SPATIAL_BALANCE='0',SDA_ATMOS_REPLAY_ENHANCEMENT='1',SDA_ATMOS_REPLAY_PRESENTATION_DB='-12')
    outputs=[];metadata=[]
    for tag,enabled in [('front-reflection-control',False),('front-reflection-reduced',True)]:
        env['SDA_ATMOS_REPLAY_TAG']=tag
        if enabled:env['SDA_DIAGNOSTIC_FRONT_REFLECTIONS']='reduced'
        else:env.pop('SDA_DIAGNOSTIC_FRONT_REFLECTIONS',None)
        with (out/(tag+'.log')).open('w') as log:
            subprocess.run([str(args.harness),'atmos_front_pair_probe::replay_atmos_object_stream','--exact','--ignored','--nocapture','--test-threads=1'],cwd=repo,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
        meta=json.loads((fixture/(tag+'.json')).read_text());assert meta['minimumGuard']==1 and not meta['frontCommonActive'];metadata.append(meta)
        outputs.append(np.fromfile(fixture/(tag+'.f32'),dtype='<f4').reshape(-1,2).astype(float))
        print('Rendered',tag,flush=True)
    a,b=outputs;old=np.fromfile(fixture/'reflection-current.f32',dtype='<f4').reshape(-1,2).astype(float)
    error=float(abs(old-a).max());assert error<1e-6,'Baseline changed'
    lo,hi=140*48000,180*48000;m=pyln.Meter(48000);match=float(m.integrated_loudness(a[lo:hi])-m.integrated_loudness(b[lo:hi]));bands={}
    for low,high in [(500,1500),(1500,4000),(4000,8000)]:
        sos=signal.butter(3,[low,high],fs=48000,btype='bandpass',output='sos');x=signal.sosfilt(sos,a,axis=0)[lo:hi].reshape(400,4800,2);y=signal.sosfilt(sos,b,axis=0)[lo:hi].reshape(400,4800,2);pa=np.mean(x*x,axis=1);pb=np.mean(y*y,axis=1);active=pa.min(axis=1)>max(pa.max()*1e-3,1e-12);delta=10*np.log10(np.maximum(pb,1e-30)/np.maximum(pa,1e-30));lr=delta[:,1]-delta[:,0];bands[f'{low}-{high}']={'max100msEarRatioChangeDb':float(abs(lr[active]).max()),'rawBandPowerDeltaDb':float(10*np.log10(pb.sum()/pa.sum()))}
    a=a[lo:hi];b=b[lo:hi]*10**(match/20);peak=max(abs(signal.resample_poly(a,4,1,axis=0)).max(),abs(signal.resample_poly(b,4,1,axis=0)).max());gain=min(10**(12/20),.95/peak);a*=gain;b*=gain
    spec=importlib.util.spec_from_file_location('writer',repo/'scripts/diag-main-layer-protection.py');writer=importlib.util.module_from_spec(spec);spec.loader.exec_module(writer)
    files=[writer.write(out/'A-then-B-140-180.wav',np.concatenate([a,np.zeros((48000,2)),b])),writer.write(out/'B-front-reflections-reduced-140-180.wav',b)]
    report={'scope':'Test-only optional front ear-level speaker residual x0.25; direct HRTFs and rear/side/height residual kernels unchanged. Does not identify vocals or guarantee mixed-image perception. Same original positions, no center relocation. Not production or packaged change.', 'baselineMaxAbs':error,'offlineMatchDb':match,'commonAuditionGainDb':float(20*np.log10(gain)),'bands':bands,'files':files,'metadata':metadata,'listeningAcceptance':'pending'}
    (out/'report.json').write_text(json.dumps(report,indent=2));print(json.dumps(bands,indent=2))


if __name__=='__main__':main()
