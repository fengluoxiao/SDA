"""Offline pairwise coherence transfer; not a vocal detector or DSP rule."""
import json
import argparse
import os
import subprocess
from pathlib import Path

import numpy as np
from scipy import signal


ROOT = Path(__file__).resolve().parents[1]
OUT = Path.home() / 'Downloads/SDA-clarity-validation/coherence-transfer'


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--refresh', action='store_true')
    parser.add_argument('--harness', type=Path)
    args = parser.parse_args()
    fixture = ROOT / 'tmp/how-love-diag'
    frames = [json.loads(s) for s in (fixture / 'frames.jsonl').read_text().splitlines()]
    labels = frames[0]['labels']
    raw = np.fromfile(fixture / 'sources.f32', dtype='<f4')
    offset, chunks = 0, []
    for f in frames:
        n = f['samples']
        size = n * len(labels)
        chunks.append(raw[offset:offset+size].reshape(len(labels), n).T)
        offset += size
    assert offset == len(raw)
    pcm = np.concatenate(chunks)
    origin = frames[0]['start'] / 48000
    OUT.mkdir(exist_ok=True)
    if args.refresh:
        assert args.harness and args.harness.is_file()
        replay = OUT / 'current-replay'
        replay.mkdir(exist_ok=True)
        rebased = fixture / 'full'
        (replay / 'frames.jsonl').write_bytes((rebased / 'frames.jsonl').read_bytes())
        original = np.fromfile(rebased / 'sources.pcm', dtype='<f4')
        replay_frames = [json.loads(s) for s in (replay / 'frames.jsonl').read_text().splitlines()]
        assert len(original) == sum(f['samples'] for f in replay_frames)*len(labels)
        env = {k: v for k, v in os.environ.items()
               if not k.startswith(('SDA_ATMOS_REPLAY_', 'SDA_DIAGNOSTIC_'))}
        env.update(SDA_ATMOS_REPLAY_DIR=str(replay),
                   SDA_ATMOS_REPLAY_HRTF=str(ROOT/'apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json'),
                   SDA_ATMOS_REPLAY_WET='0', SDA_ATMOS_REPLAY_SECONDS='11',
                   SDA_ATMOS_REPLAY_DIRECT='1', SDA_ATMOS_REPLAY_FRONT_COMMON='0',
                   SDA_ATMOS_REPLAY_SPATIAL_BALANCE='0', SDA_ATMOS_REPLAY_ENHANCEMENT='1')
        for selected, name in [(None, 'full'), *enumerate(labels)]:
            data = original.copy() if selected is None else np.zeros_like(original)
            if selected is not None:
                offset = 0
                for frame in replay_frames:
                    n = frame['samples']
                    begin = offset + selected*n
                    data[begin:begin+n] = original[begin:begin+n]
                    offset += n*len(labels)
            data.tofile(replay / 'sources.pcm')
            env['SDA_ATMOS_REPLAY_TAG'] = name
            with (replay / (name+'.log')).open('w') as log:
                subprocess.run([str(args.harness), 'atmos_front_pair_probe::replay_atmos_object_stream',
                                '--exact', '--ignored', '--nocapture', '--test-threads=1'],
                               cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
            print('Current preset rendered:', name, flush=True)
        # Restore full input after solo loop so later diagnostics cannot silently
        # mistake the final solo input for the original complete program.
        original.tofile(replay / 'sources.pcm')
    replay = OUT / 'current-replay'
    current = (replay / 'full.f32').exists()
    source_spectra, output_spectra = [], []
    for channel, name in enumerate(labels):
        render_path = replay / (name+'.f32') if current else fixture / f'source-interaction/{name}/render.f32'
        metadata_path = replay / (name+'.json') if current else fixture / f'source-interaction/{name}/render.json'
        y = np.fromfile(render_path, dtype='<f4').reshape(-1, 2)
        meta = json.loads(metadata_path.read_text())
        assert meta['minimumGuard'] == 1
        if current:
            assert meta['masterPreampDb'] == 0 and meta['spatialLayerGainDb'] == 1.5
            assert meta['spatialLayerMainRestoreDb'] == 0
        assert len(y) == len(pcm)
        f, t, x = signal.stft(pcm[:, channel], fs=48000, nperseg=2048,
                             noverlap=1536, boundary=None, padded=False)
        _, _, z = signal.stft(y, fs=48000, nperseg=2048, noverlap=1536,
                             axis=0, boundary=None, padded=False)
        use = (t+origin >= 55) & (t+origin < 63)
        source_spectra.append(x[:, use])
        output_spectra.append(z[:, :, use].transpose(0, 2, 1))
    x = np.array(source_spectra, dtype=np.complex128)
    z = np.array(output_spectra, dtype=np.complex128)
    if current:
        full = np.fromfile(replay / 'full.f32', dtype='<f4').reshape(-1, 2)
        _, _, full_z = signal.stft(full, fs=48000, nperseg=2048, noverlap=1536,
                                  axis=0, boundary=None, padded=False)
        full_z = full_z[:, :, use].transpose(0, 2, 1)
        additivity = float(np.max(abs(z.sum(axis=0)-full_z)))
        assert additivity < 1e-6
    else:
        additivity = None
    rows, band_rows = [], {}
    for low, high in [(500, 1500), (1500, 4000)]:
        select = (f >= low) & (f < high)
        source = x[:, select]
        rendered = z[:, select]
        gi = np.einsum('ift,jft->ij', source, source.conj(), optimize=True).real
        go = np.einsum('ifte,jfte->ij', rendered, rendered.conj(), optimize=True).real
        ei, eo = np.diag(gi), np.diag(go)
        ri = gi / np.maximum(np.sqrt(ei[:, None]*ei[None, :]), 1e-30)
        ro = go / np.maximum(np.sqrt(eo[:, None]*eo[None, :]), 1e-30)
        contribution = 2*go/go.sum()
        pairs = []
        for i in range(len(labels)):
            for j in range(i+1, len(labels)):
                pair = {'a': labels[i], 'b': labels[j],
                        'inputCorrelation': float(ri[i, j]), 'outputCorrelation': float(ro[i, j]),
                        'correlationChange': float(ro[i, j]-ri[i, j]),
                        'crossEnergyFractionOfFullOutput': float(contribution[i, j]),
                        'pairIndependentEnergyFractionOfFullOutput': float((eo[i]+eo[j])/go.sum())}
                pairs.append(pair)
        band_rows[f'{low}-{high}'] = {
            'mostNegativeCrossEnergy': sorted(pairs, key=lambda r: r['crossEnergyFractionOfFullOutput'])[:10],
            'largestCorrelationChanges': sorted(pairs, key=lambda r: -abs(r['correlationChange']))[:10],
            'inputCoherentVsIndependentDb': float(10*np.log10(gi.sum()/ei.sum())),
            'outputCoherentVsIndependentDb': float(10*np.log10(go.sum()/eo.sum()))}
        rows.append((f'{low}-{high}', ri, ro))
    OUT.mkdir(exist_ok=True)
    report = {'scope': 'Original-position solo renders with unity peak guard. Input mono correlation vs binaural output; coherent effects may be authored/legitimate HRTF effects. Neither semantic vocal isolation nor defect proof. No per-song production rule.',
              'currentPresetFreshRenders': current, 'stftAdditivityMaxAbs': additivity,
              'bands': band_rows}
    (OUT / 'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
    from PIL import Image, ImageDraw
    canvas = Image.new('RGB', (1100, 1150), 'white')
    draw = ImageDraw.Draw(canvas)
    for b, (band, ri, ro) in enumerate(rows):
        for k, (matrix, title) in enumerate([(ri, 'Input mono correlation'), (ro, 'Rendered binaural correlation')]):
            v = (matrix+1)/2
            rgb = np.stack([np.minimum(2*v, 1), 1-abs(2*v-1), np.minimum(2*(1-v), 1)], axis=2)
            image = Image.fromarray((rgb*255).astype('uint8')).resize((432, 432), resample=Image.Resampling.NEAREST)
            left, top = 80 + k*535, 80 + b*560
            canvas.paste(image, (left, top))
            draw.text((left, top-40), band+' Hz: '+title, fill='black')
            for i, label in enumerate(labels):
                draw.text((left-65, top+i*27+7), label, fill='black')
                draw.text((left+i*27, top+435), label.replace('Obj_', ''), fill='black')
    draw.text((80, 1120), 'Blue=-1, white=0, red=+1; diagonal=1. Not isolated voice, not acoustic ground truth.', fill='black')
    canvas.save(OUT / 'pair-coherence.png')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
