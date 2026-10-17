"""Verify default shared cue profile versus accepted test-only tail audition."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import numpy as np
from scipy import signal


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--harness',type=Path,required=True);args=parser.parse_args()
    repo=Path(__file__).resolve().parents[1];d=Path.home()/'Downloads'
    out=d/'SDA-clarity-validation/tail-taper'
    cases=[('How',d/'SDA-current-render-audition/How-Do-I-Make-You-Love-Me-55-63',64,55*48000,63*48000),
           ('Very',d/'SDA-current-render-audition/Very-First-Night-0-65',66,0,65*48000),
           ('Lose',d/'SDA-Lose-Yourself-audit/replay',139,115*48000-5471232,251*48000-5471232)]
    env={k:v for k,v in os.environ.items() if not k.startswith(('SDA_ATMOS_REPLAY_','SDA_DIAGNOSTIC_'))}
    env.update(SDA_ATMOS_REPLAY_HRTF=str(repo/'apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json'),
               SDA_ATMOS_REPLAY_WET='0',SDA_ATMOS_REPLAY_DIRECT='1',SDA_ATMOS_REPLAY_FRONT_COMMON='0',
               SDA_ATMOS_REPLAY_SPATIAL_BALANCE='0',SDA_ATMOS_REPLAY_ENHANCEMENT='1',
               SDA_ATMOS_REPLAY_TAG='accepted-tail-production')
    rows=[]
    for name,folder,seconds,lo,hi in cases:
        env.update(SDA_ATMOS_REPLAY_DIR=str(folder),SDA_ATMOS_REPLAY_SECONDS=str(seconds))
        with (out/(name+'-production.log')).open('w') as log:
            subprocess.run([str(args.harness),'atmos_front_pair_probe::replay_atmos_object_stream','--exact','--ignored','--nocapture','--test-threads=1'],cwd=repo,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
        a=np.fromfile(folder/'cue-tail20.f32',dtype='<f4').reshape(-1,2).astype(float)[lo:hi]
        b=np.fromfile(folder/'accepted-tail-production.f32',dtype='<f4').reshape(-1,2).astype(float)[lo:hi]
        assert a.shape==b.shape and np.isfinite(b).all()
        delta=b-a
        row={'name':name,'maximumSampleDifference':float(abs(delta).max()),
             'relativeDifferenceEnergyDb':float(10*np.log10(max(np.sum(delta*delta)/np.sum(a*a),1e-30))),
             'wholeWindowEnergyDifferenceDb':float(10*np.log10(np.sum(b*b)/np.sum(a*a))),
             'maximum100msEnergyDifferenceDb':0.,
             'oversampled4xPeakDbfs':float(20*np.log10(abs(signal.resample_poly(b,4,1,axis=0)).max()))}
        for i in range(0,len(a),4800):
            x,y=a[i:i+4800],b[i:i+4800]
            if np.mean(x*x)>1e-10:
                row['maximum100msEnergyDifferenceDb']=max(row['maximum100msEnergyDifferenceDb'],abs(float(10*np.log10(np.sum(y*y)/np.sum(x*x)))))
        # Random float reduction order can influence the linked peak envelope.
        # Bound it explicitly rather than claiming bit-identical output.
        assert row['maximumSampleDifference']<.001
        assert row['maximum100msEnergyDifferenceDb']<.01
        assert row['oversampled4xPeakDbfs']<0
        rows.append(row);print(json.dumps(row),flush=True)
    report={'scope':'Shared default profile, no diagnostic environment, same decoded PCM/metadata/configuration as accepted tail candidate. Offline host integration, not mobile installed binary. Numerical tolerances do not replace user acceptance already recorded.',
            'harnessSha256':hashlib.sha256(args.harness.read_bytes()).hexdigest(),'songs':rows}
    (out/'production-parity.json').write_text(json.dumps(report,indent=2))


if __name__=='__main__':main()
