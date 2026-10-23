"""Actual full-prefix Atmos reflection ablation; not authored voice isolation."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess

import numpy as np
import pyloudnorm as pyln
from scipy import signal


def load_helper(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--harness', type=Path, required=True)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[1]
    root = Path.home()/'Downloads/SDA-clarity-validation/subarashiki-front'
    fixture = root/'replay'
    out = root/'reflection-ablation'
    out.mkdir(exist_ok=True)
    summary = json.loads((fixture/'decode-summary.json').read_text())
    assert summary['errors'] == []
    variant = load_helper('variant', repo/'scripts/diag-hrtf-manifest-variant.py')
    writer = load_helper('writer', repo/'scripts/diag-main-layer-protection.py')
    asset = repo/'apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json'
    # Keep the directory-derived standard speaker fallback in BOTH controls.
    control = variant.prepare_variant(asset, out/'control-assets', {})
    direct = variant.prepare_variant(asset, out/'direct-assets', {'spatialCues': False})
    env = {k:v for k,v in os.environ.items()
           if not k.startswith(('SDA_ATMOS_REPLAY_', 'SDA_DIAGNOSTIC_'))}
    env.update(SDA_ATMOS_REPLAY_DIR=str(fixture), SDA_ATMOS_REPLAY_SECONDS='181',
               SDA_ATMOS_REPLAY_WET='0', SDA_ATMOS_REPLAY_DIRECT='1',
               SDA_ATMOS_REPLAY_FRONT_COMMON='0', SDA_ATMOS_REPLAY_SPATIAL_BALANCE='0',
               SDA_ATMOS_REPLAY_ENHANCEMENT='1', SDA_ATMOS_REPLAY_PRESENTATION_DB='-12')
    outputs, metadata = [], []
    for tag, manifest in [('reflection-current', asset),
                          ('reflection-control', control),
                          ('reflection-direct-only', direct)]:
        env.update(SDA_ATMOS_REPLAY_HRTF=str(manifest), SDA_ATMOS_REPLAY_TAG=tag)
        with (out/(tag+'.log')).open('w') as log:
            subprocess.run([str(args.harness), 'atmos_front_pair_probe::replay_atmos_object_stream',
                            '--exact', '--ignored', '--nocapture', '--test-threads=1'],
                           cwd=repo, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
        m = json.loads((fixture/(tag+'.json')).read_text())
        assert m['minimumGuard'] == 1 and not m['frontCommonActive']
        y = np.fromfile(fixture/(tag+'.f32'), dtype='<f4').reshape(-1, 2).astype(float)
        assert np.isfinite(y).all()
        outputs.append(y)
        metadata.append(m)
        print('Rendered', tag, flush=True)
    a, control_y, b = outputs
    relocation = float(abs(a-control_y).max())
    assert relocation < 1e-6, 'Relocation changed renderer graph'
    lo, hi = 140*48000, 180*48000
    meter = pyln.Meter(48000)
    match_db = float(meter.integrated_loudness(a[lo:hi])-meter.integrated_loudness(b[lo:hi]))
    bands = {}
    for low, high in [(100,500), (500,1500), (1500,4000), (4000,8000), (8000,16000)]:
        sos = signal.butter(3, [low,high], fs=48000, btype='bandpass', output='sos')
        x = signal.sosfilt(sos,a,axis=0)[lo:hi]
        y = signal.sosfilt(sos,b,axis=0)[lo:hi]*10**(match_db/20)
        n = len(x)//4800
        px = np.mean(x[:n*4800].reshape(n,4800,2)**2,axis=1)
        py = np.mean(y[:n*4800].reshape(n,4800,2)**2,axis=1)
        active = px.min(axis=1)>max(px.max()*1e-3,1e-12)
        changes = 10*np.log10(np.maximum(py,1e-30)/np.maximum(px,1e-30))
        ild = changes[:,1]-changes[:,0]
        bands[f'{low}-{high}'] = {
            'matchedWholeBandPowerDeltaDb':float(10*np.log10(np.sum(y*y)/np.sum(x*x))),
            'max100msAbsoluteEarRatioChangeDb':float(abs(ild[active]).max()),
            'binauralCorrelationCurrent':float(np.corrcoef(x.T)[0,1]),
            'binauralCorrelationDirectOnly':float(np.corrcoef(y.T)[0,1])}
    a = a[lo:hi].copy()
    b = b[lo:hi]*10**(match_db/20)
    peak = max(abs(signal.resample_poly(a,4,1,axis=0)).max(),
               abs(signal.resample_poly(b,4,1,axis=0)).max())
    common_gain = min(10**(12/20), .95/peak)
    a *= common_gain
    b *= common_gain
    files = [writer.write(out/'A-current-140-180.wav',a),
             writer.write(out/'B-direct-only-140-180.wav',b),
             writer.write(out/'A-then-B-140-180.wav',np.concatenate([a,np.zeros((48000,2)),b]))]
    report = {'scope':'Fresh full-prefix actual Atmos replays; same current renderer and authored metadata. Cue-off retains direct HRTF, not stereo downmix or vocal extraction. Reflection removal is diagnostic, not a balanced/universally better production candidate.',
              'windowSongSeconds':[140,180], 'relocationControlMaxAbs':relocation,
              'harnessSha256':hashlib.sha256(args.harness.read_bytes()).hexdigest(),
              'matchDb':match_db, 'commonAuditionGainDb':float(20*np.log10(common_gain)),
              'metadata':metadata, 'bands':bands, 'files':files,
              'continuousLayout':{'0-40':'current','40-41':'silence','41-81':'direct HRTF only'},
              'productionChanged':False}
    (out/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    print(json.dumps({'relocation':relocation,'matchDb':match_db,'bands':bands},indent=2))


if __name__ == '__main__':
    main()
