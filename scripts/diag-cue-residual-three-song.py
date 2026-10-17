"""Offline residual -12dB candidate vs existing -6dB, NOT source attenuation.

Keep PCM, positions, measured direct filters, and accepted gain preset.
No production default, scan, analysis cache or title-based dispatch changes.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess

import numpy as np
import pyloudnorm as pyln
from scipy import signal


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--harness', type=Path, required=True)
    parser.add_argument('--cue-db', type=int, choices=[-9,-12], default=-12)
    args = parser.parse_args()
    label='minus'+str(-args.cue_db)
    tag='cue-'+label+'-valid'
    repo = Path(__file__).resolve().parents[1]
    downloads = Path.home()/'Downloads'
    out = downloads/('SDA-clarity-validation/'+tag+'-candidate')
    out.mkdir(exist_ok=True)
    asset = repo/'apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json'
    manifest = json.loads(asset.read_text())
    def absolute(v, base):
        if isinstance(v, dict):
            return {k: absolute(x, base) for k, x in v.items()}
        if isinstance(v, list):
            return [absolute(x, base) for x in v]
        if isinstance(v, str) and v.endswith('.f32') and (base/v).is_file():
            return str((base/v).resolve())
        return v
    manifest = absolute(manifest, asset.parent)
    assert manifest['processing']['spatialCues']
    assert abs(manifest['processing']['spatialCueGain']-10**(-6/20))<1e-6
    manifest['processing']['spatialCueGain'] = 10**(args.cue_db/20)
    dense = out/'assets/hrtf-dense'
    standard = out/'assets/hrtf'
    dense.mkdir(parents=True, exist_ok=True)
    standard.mkdir(parents=True, exist_ok=True)
    standard_asset = asset.parent.parent/'hrtf/hrtf-set.json'
    standard_manifest = absolute(json.loads(standard_asset.read_text()), standard_asset.parent)
    # Native loader infers matching fallback from these directory basenames.
    # Preserve it; relocating a flat manifest would change more than cue gain.
    (standard/'hrtf-set.json').write_text(json.dumps(standard_manifest), encoding='utf-8')
    path = dense/'hrtf-set.json'
    path.write_text(json.dumps(manifest), encoding='utf-8')
    spec = importlib.util.spec_from_file_location('wav_writer', repo/'scripts/diag-main-layer-protection.py')
    writer = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(writer)
    fixtures = [
        ('How-Love', downloads/'SDA-current-render-audition/How-Do-I-Make-You-Love-Me-55-63', 64, 55*48000, 63*48000),
        ('Very-First-Night', downloads/'SDA-current-render-audition/Very-First-Night-0-65', 66, 0, 65*48000),
        ('Lose-Yourself', downloads/'SDA-Lose-Yourself-audit/replay', 139, 48000, 17*48000),
    ]
    env = {k:v for k,v in os.environ.items() if not k.startswith(('SDA_ATMOS_REPLAY_', 'SDA_DIAGNOSTIC_'))}
    env.update(SDA_ATMOS_REPLAY_HRTF=str(path), SDA_ATMOS_REPLAY_WET='0',
               SDA_ATMOS_REPLAY_DIRECT='1', SDA_ATMOS_REPLAY_FRONT_COMMON='0',
               SDA_ATMOS_REPLAY_SPATIAL_BALANCE='0', SDA_ATMOS_REPLAY_ENHANCEMENT='1')
    meter = pyln.Meter(48000)
    report = {'scope':'Offline residual -12dB vs baseline -6dB, current total auxiliary +1.5. Matching standard fallback retained. Same generic manifest used for all songs. No candidate acceptance or clarity proof. Per-audition scalar matching is test-only, not production normalization.', 'songs':[]}
    report['scope']=report['scope'].replace('-12dB',str(args.cue_db)+'dB')
    report['candidateCueDb']=args.cue_db
    # Mandatory relocation control at unchanged cue gain, before auditions.
    control = dict(manifest)
    control['processing'] = dict(manifest['processing'], spatialCueGain=10**(-6/20))
    path.write_text(json.dumps(control), encoding='utf-8')
    fixture = fixtures[0][1]
    env.update(SDA_ATMOS_REPLAY_DIR=str(fixture), SDA_ATMOS_REPLAY_SECONDS='64',
               SDA_ATMOS_REPLAY_TAG='cue-relocation-control')
    with (out/'relocation-control.log').open('w') as log:
        subprocess.run([str(args.harness),'atmos_front_pair_probe::replay_atmos_object_stream',
                        '--exact','--ignored','--nocapture','--test-threads=1'],cwd=repo,env=env,
                       stdout=log,stderr=subprocess.STDOUT,check=True)
    reference=np.fromfile(fixture/'peak-stall-fixed.f32',dtype='<f4')
    check=np.fromfile(fixture/'cue-relocation-control.f32',dtype='<f4')
    error=float(abs(reference-check).max())
    assert error<1e-6, f'Relocation changes graph/output: {error}; refuse candidate'
    report['unchangedGainRelocationMaxAbs']=error
    path.write_text(json.dumps(manifest), encoding='utf-8')
    for name, fixture, seconds, low, high in fixtures:
        env.update(SDA_ATMOS_REPLAY_DIR=str(fixture), SDA_ATMOS_REPLAY_SECONDS=str(seconds),
                   SDA_ATMOS_REPLAY_TAG=tag)
        with (out/(name+'.log')).open('w') as log:
            subprocess.run([str(args.harness),'atmos_front_pair_probe::replay_atmos_object_stream',
                            '--exact','--ignored','--nocapture','--test-threads=1'],cwd=repo,env=env,
                           stdout=log,stderr=subprocess.STDOUT,check=True)
        x = np.fromfile(fixture/'peak-stall-fixed.f32',dtype='<f4').reshape(-1,2).astype(float)
        y = np.fromfile(fixture/(tag+'.f32'),dtype='<f4').reshape(-1,2).astype(float)
        assert x.shape==y.shape and np.isfinite(y).all()
        metadata=json.loads((fixture/(tag+'.json')).read_text())
        assert metadata['spatialLayerGainDb']==1.5 and metadata['masterPreampDb']==0
        bands={}
        for lowhz,highhz in [(100,500),(500,1500),(1500,4000),(4000,8000)]:
            sos=signal.butter(3,[lowhz,highhz],fs=48000,btype='bandpass',output='sos')
            a=signal.sosfilt(sos,x,axis=0);b=signal.sosfilt(sos,y,axis=0)
            bands[f'{lowhz}-{highhz}']={'wholeReplayPowerChangeDb':float(10*np.log10(np.sum(b*b)/np.sum(a*a))),
                                    'wholeReplayDifferenceRelativeDb':float(10*np.log10(np.sum((b-a)**2)/np.sum(a*a)))}
        a,b=x[low:high],y[low:high]
        lufs_a,lufs_b=meter.integrated_loudness(a),meter.integrated_loudness(b)
        match=float(lufs_a-lufs_b)
        b=b*10**(match/20)
        files=[writer.write(out/(name+'-baseline.wav'),a),
               writer.write(out/(name+'-residual-'+label+'-matched.wav'),b)]
        report['songs'].append({'name':name,'auditionWindowSamplesRelativeToFixture':[low,high],
                                'wholeReplayBands':bands,'auditionBaselineLufs':lufs_a,
                                'auditionCandidateRawLufs':lufs_b,'auditionMatchDb':match,
                                'minimumGuard':metadata['minimumGuard'],
                                'wholeReplayRtoLPowerChangeDb':float(10*np.log10(np.sum(y[:,1]**2)/np.sum(y[:,0]**2))-10*np.log10(np.sum(x[:,1]**2)/np.sum(x[:,0]**2))),
                                'outputs':files})
        print(name, json.dumps({'matchDb':match,'bands':bands}),flush=True)
    (out/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')


if __name__=='__main__':
    main()
