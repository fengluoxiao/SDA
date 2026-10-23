"""Actual fixture replays for the peak-envelope stall fix; offline only."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

import numpy as np


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--harness', type=Path, required=True)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[1]
    downloads = Path.home() / 'Downloads'
    out = downloads / 'SDA-clarity-validation/peak-stall-regression'
    out.mkdir(exist_ok=True)
    fixtures = [
        ('How-Love', downloads/'SDA-current-render-audition/How-Do-I-Make-You-Love-Me-55-63', 64, 55, 63),
        ('Very-First-Night', downloads/'SDA-current-render-audition/Very-First-Night-0-65', 66, 0, 65),
        ('Lose-Yourself', downloads/'SDA-Lose-Yourself-audit/replay', 139, None, None),
    ]
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(('SDA_ATMOS_REPLAY_', 'SDA_DIAGNOSTIC_'))}
    env.update(SDA_ATMOS_REPLAY_HRTF=str(repo/'apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json'),
               SDA_ATMOS_REPLAY_WET='0', SDA_ATMOS_REPLAY_DIRECT='1',
               SDA_ATMOS_REPLAY_FRONT_COMMON='0', SDA_ATMOS_REPLAY_SPATIAL_BALANCE='0',
               SDA_ATMOS_REPLAY_ENHANCEMENT='1')
    reports = []
    for name, fixture, seconds, start, end in fixtures:
        env.update(SDA_ATMOS_REPLAY_DIR=str(fixture), SDA_ATMOS_REPLAY_SECONDS=str(seconds),
                   SDA_ATMOS_REPLAY_TAG='peak-stall-fixed')
        with (out/(name+'.log')).open('w') as log:
            subprocess.run([str(args.harness), 'atmos_front_pair_probe::replay_atmos_object_stream',
                            '--exact', '--ignored', '--nocapture', '--test-threads=1'],
                           cwd=repo, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
        path = fixture/'peak-stall-fixed.f32'
        new = np.fromfile(path, dtype='<f4').reshape(-1, 2).astype(float)
        before = np.fromfile(fixture/'production-total-lift-verified.f32', dtype='<f4').reshape(-1, 2).astype(float)
        assert new.shape == before.shape and np.isfinite(new).all()
        metadata = json.loads((fixture/'peak-stall-fixed.json').read_text())
        assert metadata['masterPreampDb'] == 0 and metadata['spatialLayerGainDb'] == 1.5
        assert metadata['spatialLayerMainRestoreDb'] == 0
        low, high = (int(start*48000), int(end*48000)) if start is not None else (0, len(new))
        x, y = before[low:high], new[low:high]
        difference = y-x
        scale = np.sum(x*y, axis=1)/np.maximum(np.sum(x*x, axis=1), 1e-30)
        residual = y-scale[:, None]*x
        row = {'name': name, 'windowScope': 'absolute song window' if start is not None else 'full rebased fixture',
               'minimumGuard': metadata['minimumGuard'], 'maxAbsDifference': float(abs(difference).max()),
               'residualRelativeDb': float(10*np.log10(max(np.sum(difference*difference)/np.sum(x*x), 1e-30))),
               'energyChangeDb': float(10*np.log10(np.sum(y*y)/np.sum(x*x))),
               'commonStereoScaleResidualMax': float(abs(residual).max()),
               'peakSample': float(abs(y).max()),
               'inputPcmSha256': hashlib.file_digest((fixture/'sources.pcm').open('rb'), 'sha256').hexdigest(),
               'max100msEnergyChangeDb': 0.0}
        for offset in range(0, len(x), 4800):
            a, b = x[offset:offset+4800], y[offset:offset+4800]
            if np.mean(a*a) < 1e-10:
                continue
            delta = 10*np.log10(np.sum(b*b)/np.sum(a*a))
            row['max100msEnergyChangeDb'] = max(row['max100msEnergyChangeDb'], abs(float(delta)))
        reports.append(row)
        print(json.dumps(row, indent=2), flush=True)
    report = {'scope': 'Peak-envelope convergence fix versus prior verified production-total-lift replay. Hash reduction variation may remain. Offline current engine, not device capture or perceptual acceptance.',
              'harnessSha256': hashlib.file_digest(args.harness.open('rb'), 'sha256').hexdigest(),
              'songs': reports}
    (out/'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')


if __name__ == '__main__':
    main()
