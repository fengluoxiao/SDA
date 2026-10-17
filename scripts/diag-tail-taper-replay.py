"""Replay test-only temporal residual taper at one common setting for 3 songs."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import numpy as np


def main():
    p=argparse.ArgumentParser();p.add_argument('--harness',type=Path,required=True);args=p.parse_args()
    repo=Path(__file__).resolve().parents[1];d=Path.home()/'Downloads'
    out=d/'SDA-clarity-validation/tail-taper';out.mkdir(exist_ok=True)
    cases=[('How',d/'SDA-current-render-audition/How-Do-I-Make-You-Love-Me-55-63',64),
           ('Very',d/'SDA-current-render-audition/Very-First-Night-0-65',66),
           ('Lose',d/'SDA-Lose-Yourself-audit/replay',139)]
    env={k:v for k,v in os.environ.items() if not k.startswith(('SDA_ATMOS_REPLAY_','SDA_DIAGNOSTIC_'))}
    env.update(SDA_ATMOS_REPLAY_HRTF=str(repo/'apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json'),
               SDA_ATMOS_REPLAY_WET='0',SDA_ATMOS_REPLAY_DIRECT='1',SDA_ATMOS_REPLAY_FRONT_COMMON='0',
               SDA_ATMOS_REPLAY_SPATIAL_BALANCE='0',SDA_ATMOS_REPLAY_ENHANCEMENT='1',
               SDA_ATMOS_REPLAY_TAG='cue-tail20',SDA_DIAGNOSTIC_CUE_TAIL_MS='20')
    # Verify new binary with experiment disabled before comparing candidates.
    control=dict(env);control.pop('SDA_DIAGNOSTIC_CUE_TAIL_MS')
    control.update(SDA_ATMOS_REPLAY_DIR=str(cases[0][1]),SDA_ATMOS_REPLAY_SECONDS='64',
                   SDA_ATMOS_REPLAY_TAG='tail-build-control')
    with (out/'baseline-control.log').open('w') as log:
        subprocess.run([str(args.harness),'atmos_front_pair_probe::replay_atmos_object_stream','--exact','--ignored','--nocapture','--test-threads=1'],cwd=repo,env=control,stdout=log,stderr=subprocess.STDOUT,check=True)
    reference=np.fromfile(cases[0][1]/'peak-stall-fixed.f32',dtype='<f4')
    check=np.fromfile(cases[0][1]/'tail-build-control.f32',dtype='<f4')
    error=float(abs(reference-check).max());assert error<1e-6,error
    (out/'control.json').write_text(json.dumps({'disabledExperimentBaselineMaxAbs':error,
        'scope':'How unity-guard baseline new binary control; not all-platform certification'},indent=2))
    for name,folder,seconds in cases:
        env.update(SDA_ATMOS_REPLAY_DIR=str(folder),SDA_ATMOS_REPLAY_SECONDS=str(seconds))
        with (out/(name+'.log')).open('w') as log:
            subprocess.run([str(args.harness),'atmos_front_pair_probe::replay_atmos_object_stream','--exact','--ignored','--nocapture','--test-threads=1'],cwd=repo,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
        meta=json.loads((folder/'cue-tail20.json').read_text())
        assert meta['masterPreampDb']==0 and meta['spatialLayerGainDb']==1.5
        print(name,'replay passed',flush=True)


if __name__=='__main__':main()
