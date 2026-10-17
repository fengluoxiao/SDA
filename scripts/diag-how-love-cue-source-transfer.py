"""Per-source cue bypass audit; groups/objects are not isolated vocal stems."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import numpy as np
from scipy import signal

ROOT=Path(__file__).resolve().parents[1]
RATE=48000

def db(r):
    return float(10*np.log10(max(float(r),1e-30)))

def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--harness',required=True,type=Path)
    args=ap.parse_args()
    root=ROOT/'tmp/how-love-diag'
    audit=root/'generic-gain-path-audit'
    prior=json.loads((audit/'report.json').read_text())
    corrected_manifest=audit/'cues-disabled-assets/hrtf-dense/hrtf-set.json'
    assert corrected_manifest.is_file(), 'Regenerate graph-preserving cue bypass audit before solos'
    assert prior.get('cueBypassFallbackPreserved') is True, 'Old flat-manifest audit is not a valid cue-only comparison'
    assert hashlib.sha256(corrected_manifest.read_bytes()).hexdigest()==prior['cueBypassManifestSha256']
    assert prior['comparisons']['previous-offline-baseline']['maxAbsDifference']<1e-5
    assert hashlib.sha256(args.harness.read_bytes()).hexdigest()==prior['harnessSha256']
    dest=audit/'cues-disabled-solos'
    dest.mkdir(exist_ok=True)
    frames=[json.loads(line) for line in (root/'full/frames.jsonl').read_text().splitlines()]
    labels=frames[0]['labels']
    raw=np.fromfile(root/'full/sources.pcm',dtype='<f4')
    assert hashlib.sha256(raw.tobytes()).hexdigest()==prior['inputSha256']
    (dest/'frames.jsonl').write_text((root/'full/frames.jsonl').read_text(),encoding='utf-8')
    origin=json.loads((root/'frames.jsonl').read_text().splitlines()[0])['start']
    lo,hi=55*RATE-origin,63*RATE-origin
    trace=json.loads((root/'surround-trace/report.json').read_text())
    bands=[(500,1500),(1500,4000),(4000,8000)]
    filters={f'{low}-{high}Hz':signal.butter(3,[low,high],fs=RATE,btype='bandpass',output='sos') for low,high in bands}
    result=[]
    for selected,name in enumerate(labels):
        if not name.startswith('Obj_'):
            continue
        solo=np.zeros_like(raw)
        offset=0
        for frame in frames:
            assert frame['labels']==labels
            n=frame['samples']
            begin=offset+selected*n
            solo[begin:begin+n]=raw[begin:begin+n]
            offset+=n*len(labels)
        assert offset==len(raw)
        solo.tofile(dest/'sources.pcm')
        clean_env={k:v for k,v in os.environ.items() if not k.startswith(('SDA_ATMOS_REPLAY_','SDA_DIAGNOSTIC_'))}
        env=dict(clean_env,SDA_ATMOS_REPLAY_DIR=str(dest.resolve()),
                 SDA_ATMOS_REPLAY_HRTF=str(corrected_manifest.resolve()),
                 SDA_ATMOS_REPLAY_TAG=name,SDA_ATMOS_REPLAY_WET='0',SDA_ATMOS_REPLAY_SECONDS='11',
                 SDA_ATMOS_REPLAY_DIRECT='1',SDA_ATMOS_REPLAY_FRONT_COMMON='0',
                 SDA_ATMOS_REPLAY_SPATIAL_BALANCE='0',SDA_ATMOS_REPLAY_LEDGER='0')
        with (dest/f'{name}.log').open('w') as log:
            subprocess.run([str(args.harness.resolve()),'atmos_front_pair_probe::replay_atmos_object_stream',
                            '--exact','--ignored','--nocapture','--test-threads=1'],cwd=ROOT,env=env,
                           stdout=log,stderr=subprocess.STDOUT,check=True)
        config=json.loads((dest/f'{name}.json').read_text())
        assert config['minimumGuard']==1
        a=np.fromfile(root/f'source-interaction/{name}/render.f32',dtype='<f4').reshape(-1,2).astype(float)
        b=np.fromfile(dest/f'{name}.f32',dtype='<f4').reshape(-1,2).astype(float)
        assert a.shape==b.shape and np.isfinite(b).all()
        row={'source':name,'coreSurroundFitCoefficients':{m['side']:m['coefficients'][name[4:]] for m in trace['metrics']},'bands':{}}
        for band,sos in filters.items():
            aa=signal.sosfilt(sos,a,axis=0)[lo:hi]
            bb=signal.sosfilt(sos,b,axis=0)[lo:hi]
            windows=[]
            for off in range(0,len(aa),1920):
                # Include only windows with useful signal; these are not vocal masks.
                pa=float(np.mean(aa[off:off+1920]**2))
                pb=float(np.mean(bb[off:off+1920]**2))
                if db(pa)>-60 and db(pb)>-60:
                    windows.append({'startSeconds':55+off/RATE,'powerChangeDb':db(pb/pa)})
            row['bands'][band]={'cueBypassPowerChangeDb':db(np.sum(bb*bb)/np.sum(aa*aa)),
                               'active40msWindowCount':len(windows),
                               'minActive40ms':min(windows,key=lambda w:w['powerChangeDb']) if windows else None,
                               'maxActive40ms':max(windows,key=lambda w:w['powerChangeDb']) if windows else None}
        result.append(row)
        print(name,{k:round(v['cueBypassPowerChangeDb'],3) for k,v in row['bands'].items()},flush=True)
    report={'scope':'Original-position per-object diagnostic cue bypass; no input gain changes. Not isolated target-vocal measurements. Fit coefficients are diagnostic core reconstruction, not object semantic labels.',
            'windowSeconds':[55,63],'harnessSha256':prior['harnessSha256'],
            'inputSha256':prior['inputSha256'],
            'soloPcmFileNote':'Single scratch sources.pcm reused per solo; final file contains only last object. Outputs/config/logs preserve all 15 replays.',
            'sources':result}
    (audit/'source-cue-transfer.json').write_text(json.dumps(report,indent=2),encoding='utf-8')

if __name__=='__main__':
    main()
