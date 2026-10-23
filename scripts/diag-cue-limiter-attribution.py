"""Diagnostic safety-headroom replays separate cue interaction from limiting.

Presentation attenuation is offline only; no production limiter bypass or
program gain change. Output gains restored numerically only for comparison.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import numpy as np
from scipy import signal


def main():
    ap=argparse.ArgumentParser()
    ap.add_argument('--harness',type=Path,required=True)
    args=ap.parse_args()
    repo=Path(__file__).resolve().parents[1]
    downloads=Path.home()/'Downloads'
    out=downloads/'SDA-clarity-validation/cue-limiter-attribution'
    out.mkdir(exist_ok=True)
    manifests=[repo/'apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json',
               downloads/'SDA-clarity-validation/cue-minus12-valid-candidate/assets/hrtf-dense/hrtf-set.json']
    fixtures=[('How-Love',downloads/'SDA-current-render-audition/How-Do-I-Make-You-Love-Me-55-63',64,55,63),
              ('Very-First-Night',downloads/'SDA-current-render-audition/Very-First-Night-0-65',66,0,65),
              ('Lose-Yourself',downloads/'SDA-Lose-Yourself-audit/replay',139,0,138)]
    env={k:v for k,v in os.environ.items() if not k.startswith(('SDA_ATMOS_REPLAY_','SDA_DIAGNOSTIC_'))}
    env.update(SDA_ATMOS_REPLAY_WET='0',SDA_ATMOS_REPLAY_DIRECT='1',SDA_ATMOS_REPLAY_FRONT_COMMON='0',
               SDA_ATMOS_REPLAY_SPATIAL_BALANCE='0',SDA_ATMOS_REPLAY_ENHANCEMENT='1',
               SDA_ATMOS_REPLAY_PRESENTATION_DB='-12')
    report={'scope':'Two manifests at -12dB presentation headroom, guard must remain unity; raw prior outputs include guard. Unmatched 100ms powers; no voice semantics, no perceptual proof. Test-only attenuation is not production normalization.', 'songs':[]}
    for name,fixture,seconds,start,end in fixtures:
        safe=[]
        for k,manifest in enumerate(manifests):
            tag='cue-attribution-safe-'+str(k)
            env.update(SDA_ATMOS_REPLAY_DIR=str(fixture),SDA_ATMOS_REPLAY_HRTF=str(manifest),
                       SDA_ATMOS_REPLAY_SECONDS=str(seconds),SDA_ATMOS_REPLAY_TAG=tag)
            with (out/(name+'-'+str(k)+'.log')).open('w') as log:
                subprocess.run([str(args.harness),'atmos_front_pair_probe::replay_atmos_object_stream',
                                '--exact','--ignored','--nocapture','--test-threads=1'],cwd=repo,env=env,
                               stdout=log,stderr=subprocess.STDOUT,check=True)
            meta=json.loads((fixture/(tag+'.json')).read_text())
            assert meta['minimumGuard']==1, 'Cannot attribute nonlinear output to residual alone'
            assert meta['masterPreampDb']==-12 and meta['spatialLayerGainDb']==1.5
            safe.append(np.fromfile(fixture/(tag+'.f32'),dtype='<f4').reshape(-1,2).astype(float)*10**(12/20))
        actual=[np.fromfile(fixture/(tag+'.f32'),dtype='<f4').reshape(-1,2).astype(float)
                for tag in ['peak-stall-fixed','cue-minus12-valid']]
        assert all(v.shape==safe[0].shape for v in [*safe,*actual])
        song={'name':name,'windowSecondsRelativeToFixture':[start,end],'bands':{}}
        for low,high in [(100,500),(500,1500),(1500,4000),(4000,8000)]:
            sos=signal.butter(3,[low,high],fs=48000,btype='bandpass',output='sos')
            local=[]
            for pair in [safe,actual]:
                powers=[]
                for data in pair:
                    f=signal.sosfilt(sos,data,axis=0)[int(start*48000):int(end*48000)]
                    f=f[:len(f)//4800*4800].reshape(-1,4800,2)
                    powers.append(np.mean(f*f,axis=1))
                a,b=powers
                power_delta=10*np.log10(np.maximum(b.sum(axis=1),1e-30)/np.maximum(a.sum(axis=1),1e-30))
                lr_delta=10*np.log10(np.maximum(b[:,1],1e-30)/np.maximum(b[:,0],1e-30))-10*np.log10(np.maximum(a[:,1],1e-30)/np.maximum(a[:,0],1e-30))
                active=a.min(axis=1)>max(a.max()*1e-3,1e-10)
                local.append((power_delta,lr_delta,active))
            valid=local[0][2]&local[1][2]
            safe_delta,safe_lr,_=local[0];actual_delta,actual_lr,_=local[1]
            idx=np.flatnonzero(valid)
            worst=idx[np.argsort(abs(actual_lr[idx]))[-5:][::-1]]
            song['bands'][f'{low}-{high}']={
                'maxAbsPowerDeltaDbGuardUnity':float(abs(safe_delta[valid]).max()),
                'maxAbsRtoLDeltaDbGuardUnity':float(abs(safe_lr[valid]).max()),
                'maxAbsAdditionalPowerDeltaDbWithGuard':float(abs(actual_delta[valid]-safe_delta[valid]).max()),
                'maxAbsAdditionalRtoLDeltaDbWithGuard':float(abs(actual_lr[valid]-safe_lr[valid]).max()),
                'worstActualRtoLWindows':[{'timeSecondsRelativeToFixture':float(start+i*.1),
                                          'actualRtoLDeltaDb':float(actual_lr[i]),
                                          'guardUnityRtoLDeltaDb':float(safe_lr[i]),
                                          'guardContributionDb':float(actual_lr[i]-safe_lr[i])} for i in worst]}
        report['songs'].append(song)
        (out/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
        print(json.dumps(song,indent=2),flush=True)


if __name__=='__main__':
    main()
