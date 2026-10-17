"""Offline source-additivity/cross-energy audit; never changes production DSP.

Runs the current engine test harness with all original metadata and gains. Each
solo zeroes other PCM slots, not metadata. A source/group is NOT a vocal stem.
"""
import argparse, hashlib, json, os, subprocess
from pathlib import Path
import numpy as np
from scipy import signal

ROOT = Path(__file__).resolve().parents[1]
RATE = 48000

def db(ratio):
    return float(10 * np.log10(max(float(ratio), 1e-30)))

def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--harness', type=Path, required=True)
    args = ap.parse_args()
    fixture = ROOT / 'tmp/how-love-diag/full'
    out = ROOT / 'tmp/how-love-diag/source-interaction'
    out.mkdir(parents=True, exist_ok=True)
    frames = [json.loads(s) for s in (fixture / 'frames.jsonl').read_text().splitlines()]
    labels = frames[0]['labels']
    assert all(f['labels'] == labels for f in frames)
    raw = np.fromfile(fixture / 'sources.pcm', dtype='<f4')
    assert np.isfinite(raw).all()
    n_total = sum(f['samples'] for f in frames)
    assert len(raw) == n_total * len(labels)
    manifest = ROOT / 'apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json'
    env = dict(os.environ, SDA_ATMOS_REPLAY_HRTF=str(manifest),
               SDA_ATMOS_REPLAY_TAG='render', SDA_ATMOS_REPLAY_WET='0',
               SDA_ATMOS_REPLAY_SECONDS='11', SDA_ATMOS_REPLAY_DIRECT='1',
               SDA_ATMOS_REPLAY_FRONT_COMMON='0', SDA_ATMOS_REPLAY_SPATIAL_BALANCE='0')
    results, source_signals, reports = {}, {}, {}
    for selected in [None, *range(len(labels))]:
        name = 'full' if selected is None else labels[selected]
        dest = out / name
        dest.mkdir(exist_ok=True)
        (dest / 'frames.jsonl').write_text((fixture / 'frames.jsonl').read_text())
        pcm = raw.copy() if selected is None else np.zeros_like(raw)
        if selected is not None:
            offset = 0
            chunks = []
            for f in frames:
                n = f['samples']
                begin = offset + selected * n
                pcm[begin:begin+n] = raw[begin:begin+n]
                chunks.append(raw[begin:begin+n])
                offset += n * len(labels)
            source_signals[name] = np.concatenate(chunks)
        pcm.tofile(dest / 'sources.pcm')
        run_env = dict(env, SDA_ATMOS_REPLAY_DIR=str(dest))
        with (dest / 'render.log').open('w') as log:
            subprocess.run([str(args.harness.resolve()),
                            'atmos_front_pair_probe::replay_atmos_object_stream',
                            '--exact', '--ignored', '--nocapture', '--test-threads=1'],
                           cwd=ROOT, env=run_env, stdout=log, stderr=subprocess.STDOUT, check=True)
        result = np.fromfile(dest / 'render.f32', dtype='<f4').reshape(-1, 2)
        assert len(result) == n_total and np.isfinite(result).all()
        results[name] = result.astype(float)
        reports[name] = json.loads((dest / 'render.json').read_text())
        assert reports[name]['minimumGuard'] == 1, 'Nonlinear guard invalidates source attribution'
        print('Rendered', name, flush=True)
    full = results.pop('full')
    stems = np.stack([results[name] for name in labels])
    err = np.sum(stems, axis=0) - full
    additivity = {'maxAbs': float(np.max(abs(err))),
                  'residualRelativeDb': db(np.sum(err**2) / np.sum(full**2))}
    assert additivity['maxAbs'] < 1e-5, 'Independent renders are not additive'
    # Original source origin from preserved decoder records, not a hardcoded offset.
    origin = json.loads((ROOT / 'tmp/how-love-diag/frames.jsonl').read_text().splitlines()[0])['start']
    lo, hi = 55 * RATE - origin, 63 * RATE - origin
    assert 0 <= lo < hi <= n_total
    bands = {}
    for low, high in [(150, 500), (500, 1500), (1500, 4000), (4000, 8000)]:
        sos = signal.butter(3, [low, high], fs=RATE, btype='bandpass', output='sos')
        filtered = signal.sosfilt(sos, stems, axis=1)[:, lo:hi, :]
        gram = np.einsum('ine,jne->ij', filtered, filtered, optimize=True)
        diag = np.diag(gram)
        total = float(gram.sum())
        rows = []
        for i, name in enumerate(labels):
            src = source_signals[name]
            src = signal.sosfilt(sos, src)[lo:hi]
            own = float(diag[i])
            rest = max(float(total - 2 * gram[i].sum() + own), 0)
            cross = float(2 * (gram[i].sum() - own))
            rows.append({'source': name, 'outputEnergyVsFullDb': db(own / total),
                         'binauralVsMonoSourcePowerDb': db(own / max(float(src @ src), 1e-30)),
                         'crossVsOwnEnergy': cross / max(own, 1e-30),
                         'ownPlusRestVsIncoherentDb': db(total / max(own + rest, 1e-30))})
        pairs = sorted([{'a': labels[i], 'b': labels[j],
                         'correlation': float(gram[i, j] / max(np.sqrt(diag[i] * diag[j]), 1e-30)),
                         'crossVsFullEnergy': float(2 * gram[i, j] / total)}
                        for i in range(len(labels)) for j in range(i+1, len(labels))],
                       key=lambda p: p['crossVsFullEnergy'])
        windows = []
        for offset in range(0, hi-lo, RATE//4):
            block = filtered[:, offset:offset+RATE//4, :]
            coherent = np.sum(block, axis=0)
            independent = np.sum(block**2, axis=(0,1))
            ear_power = np.sum(coherent**2, axis=0)
            windows.append({'startSeconds':55+offset/RATE,
                            'sumVsIndependentPowerDb':db(ear_power.sum()/independent.sum()),
                            'perEarDb':[db(a/b) for a,b in zip(ear_power, independent)]})
        bands[f'{low}-{high}Hz'] = {'fullVsSumIndependentPowerDb': db(total / diag.sum()),
                                   'sources': rows, 'mostNegativePairs': pairs[:8],
                                   'quarterSecondWindows': windows}
    report = {'scope': 'Offline current-engine replay, original PCM/metadata/gains; not device capture or vocal isolation. Cross terms can be authored content, not proof of renderer defect.',
              'head': subprocess.check_output(['git','rev-parse','HEAD'], cwd=ROOT, text=True).strip(),
              'harnessSha256': hashlib.sha256(args.harness.read_bytes()).hexdigest(),
              'inputSha256': hashlib.sha256((fixture/'sources.pcm').read_bytes()).hexdigest(),
              'windowSeconds': [55, 63], 'sourceOriginSamples': origin,
              'additivity': additivity, 'minimumGuards': {k:v['minimumGuard'] for k,v in reports.items()},
              'bands': bands}
    (out/'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
    print(json.dumps({'additivity':additivity, 'bandCrossEnergyDb':{k:v['fullVsSumIndependentPowerDb'] for k,v in bands.items()}}, indent=2))

if __name__ == '__main__':
    main()
