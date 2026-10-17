"""Offline source-weighted interpolation audit. Never used by playback.

The measured-power mixture is an implementation reference, not an authored
vocal stem or acoustic ground truth. Routes are approximations of metadata
ramps; short-window spectra cannot establish perceptual clarity.
"""
import argparse
import json
from pathlib import Path

import numpy as np
from scipy import fft


def run(repo, audit):
    rows = json.loads((audit / 'rows.json').read_text())
    routes = json.loads((audit / 'routes.json').read_text())
    bank = repo / 'apps/mobile/assets/hrtf-restored/hrtf-dense'
    positions = json.loads((bank / 'hrtf-set.json').read_text())['positions']
    nfft = 16384
    frequency = fft.rfftfreq(nfft, 1 / 48000)
    selected = (frequency >= 500) & (frequency < 4000)
    frequency = frequency[selected]
    measured = []
    for p in positions:
        h = np.fromfile(bank / p['dry'], dtype='<f4').reshape(2, -1)
        measured.append(abs(fft.rfft(h, n=nfft, axis=1)[:, selected]) ** 2)
    measured = np.array(measured)
    assert len(positions) == 61
    h = np.fromfile(audit / 'filters.f32', dtype='<f4')
    assert len({r['tapCount'] for r in rows}) == 1
    h = h.reshape(len(rows), 2, rows[0]['tapCount'])
    actual = abs(fft.rfft(h, n=nfft, axis=2)[:, :, selected]) ** 2
    reference = np.zeros_like(actual)
    for i, r in enumerate(rows):
        assert r['support'] == 61
        for index, weight in r['weights']:
            reference[i] += measured[index] * weight
    difference = 10 * np.log10(np.maximum(actual, 1e-30) / np.maximum(reference, 1e-30))
    active = reference > reference.max(axis=2, keepdims=True) * 1e-3
    affected = (difference < -1) & active

    fixture = repo / 'tmp/how-love-diag'
    frames = [json.loads(line) for line in (fixture / 'frames.jsonl').read_text().splitlines()]
    pcm = np.fromfile(fixture / 'sources.f32', dtype='<f4')
    channels = len(frames[0]['labels'])
    chunks, offset = [], 0
    for frame in frames:
        size = frame['samples'] * channels
        chunks.append(pcm[offset:offset + size].reshape(channels, frame['samples']).T)
        offset += size
    assert offset == len(pcm)
    samples = np.concatenate(chunks)
    origin = frames[0]['start']
    assert all(f['start'] == origin + sum(x['samples'] for x in frames[:i])
               for i, f in enumerate(frames))
    window = np.hanning(2048).astype(np.float32)
    reports, top = [], []
    total_actual = total_reference = total_affected = 0.0
    heat_num = np.zeros((80, len(frequency)))
    heat_den = np.zeros_like(heat_num)
    for object_id, track in routes.items():
        channel = frames[0]['labels'].index('Obj_' + object_id)
        indices = np.array([r['filter'] for r in track])
        times = np.array([r['sample'] for r in track])
        centers = times - origin
        offsets = np.arange(-1024, 1024)
        take = centers[:, None] + offsets[None, :]
        assert take.min() >= 0 and take.max() < len(samples)
        # FFT in batches limits temporary memory use on this workstation.
        power = np.concatenate([
            abs(fft.rfft(samples[take[i:i+128], channel] * window,
                         n=nfft, axis=1)[:, selected]) ** 2
            for i in range(0, len(take), 128)
        ])
        p = actual[indices] * power[:, None, :]
        q = reference[indices] * power[:, None, :]
        mask = affected[indices]
        a, b, c = float(p.sum()), float(q.sum()), float((q * mask).sum())
        total_actual += a
        total_reference += b
        total_affected += c
        timebin = np.clip(((times / 48000 - 55) * 10).astype(int), 0, 79)
        np.add.at(heat_num, timebin, p.sum(axis=1))
        np.add.at(heat_den, timebin, q.sum(axis=1))
        row = {'object': object_id, 'sourceWeightedBandDifferenceDb': float(10*np.log10(a/b)),
               'referenceEnergyInBelowMinus1DbBinsFraction': c/b,
               'affectedRouteSamples': int(np.any(mask, axis=(1, 2)).sum()),
               'referenceEnergy': b}
        reports.append(row)
        contribution = (q * mask).sum(axis=(1, 2))
        for j in np.argsort(contribution)[-3:][::-1]:
            if contribution[j] == 0:
                continue
            k = indices[j]
            top.append({'object': object_id, 'timeSeconds': float(times[j]/48000),
                        'directionIndex': int(k), 'az': rows[k]['az'], 'el': rows[k]['el'],
                        'weightedAffectedEnergy': float(contribution[j]),
                        'localBandDifferenceDb': float(10*np.log10(p[j].sum()/q[j].sum()))})
    for r in reports:
        r['referenceEnergyShare'] = r.pop('referenceEnergy') / total_reference
    report = {
        'scope': 'B2 approximated metadata trajectories, source-local 2048 Hann windows; 16384 FFT. Independent path power, not full coherent mix, isolated vocal, or audibility proof. Reference is weighted measured HRTF power. No playback changes.',
        'windowSeconds': [55, 63], 'frequencyBandHz': [500, 4000],
        'directionEarCasesWithBelowMinus1DbBins': int(np.any(affected, axis=2).sum()),
        'allPathsSourceWeightedBandDifferenceDb': float(10*np.log10(total_actual/total_reference)),
        'allPathsEnergyInBelowMinus1DbBinsFraction': total_affected/total_reference,
        'objects': reports, 'highestAffectedEnergyWindows': sorted(top, key=lambda r: -r['weightedAffectedEnergy'])[:12],
    }
    (audit / 'all-source-weighted-narrow-check.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
    np.savez_compressed(audit / 'source-weighted-narrow-heatmap.npz', frequency=frequency,
                        actual=heat_num, reference=heat_den)
    from PIL import Image, ImageDraw
    ratio = 10*np.log10(np.maximum(heat_num, 1e-30)/np.maximum(heat_den, 1e-30))
    canvas = Image.new('RGB', (1200, 780), 'white')
    draw = ImageDraw.Draw(canvas)
    panels = [(ratio, -.1, .1, 'Interpolation difference; blue=-0.1, white=0, red=+0.1 dB'),
              (10*np.log10(np.maximum(heat_den/heat_den.max(), 1e-10)), -60, 0,
               'Reference path energy; black=-60, yellow=0 dB. NOT isolated vocal.')]
    for k, (values, low, high, title) in enumerate(panels):
        v = np.clip((values.T[::-1] - low)/(high-low), 0, 1)
        if k == 0:
            rgb = np.stack([np.minimum(2*v, 1), 1-abs(2*v-1), np.minimum(2*(1-v), 1)], axis=2)
        else:
            rgb = np.stack([v, v*v, v*.2], axis=2)
        image = Image.fromarray((rgb*255).astype('uint8')).resize((1020, 280))
        y = 60 + k*370
        canvas.paste(image, (95, y))
        draw.text((95, y-25), title, fill='black')
        for hz in [500, 1000, 2000, 3000, 4000]:
            draw.text((20, y+int((4000-hz)/3500*280)), str(hz)+'Hz', fill='black')
        for t in range(55, 64):
            draw.text((95+int((t-55)/8*1020), y+285), str(t)+'s', fill='black')
    draw.text((95, 750), 'Approximate source trajectories; independent powers, not coherent mixed output or perceptual proof.', fill='black')
    canvas.save(audit / 'source-weighted-narrow-heatmap.png')
    print(json.dumps({k: v for k, v in report.items() if k not in ['objects', 'highestAffectedEnergyWindows']}, indent=2))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--audit', type=Path, required=True)
    args = parser.parse_args()
    run(Path(__file__).resolve().parents[1], args.audit)
