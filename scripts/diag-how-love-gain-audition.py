"""Diagnostic gain auditions from preserved current-renderer stems; no production changes."""
import hashlib
import json
import wave
from pathlib import Path
import numpy as np
from scipy import signal

ROOT = Path(__file__).resolve().parents[1]
RATE = 48000

def main():
    root = ROOT / 'tmp/how-love-diag'
    dest = root / 'gain-audition-2026-10-08'
    dest.mkdir(exist_ok=True)
    report = json.loads((root / 'source-interaction/report.json').read_text())
    origin = report['sourceOriginSamples']
    lo, hi = 55 * RATE - origin, 63 * RATE - origin
    hashes = {}
    def load(name):
        path = root / 'source-interaction' / name / 'render.f32'
        hashes[name] = hashlib.sha256(path.read_bytes()).hexdigest()
        config = json.loads(path.with_suffix('.json').read_text())
        assert config['minimumGuard'] == 1 and not config['spatialBalanceEnabled']
        x = np.fromfile(path, dtype='<f4').reshape(-1, 2).astype(np.float64)
        assert 0 <= lo < hi <= len(x) and np.isfinite(x).all()
        return x[lo:hi]
    full = load('full')
    support = sum(load(f'Obj_{i}') for i in range(12, 25))
    # These are the user's previously recognizable E support group, NOT an isolated vocal.
    global_db = 2.0
    variants = [('A-full-plus2dB.wav', 0.0),
                ('B-full-plus2dB-support-plus3dB.wav', 3.0),
                ('C-full-plus2dB-support-plus6dB.wav', 6.0)]
    outputs = []
    for filename, extra_db in variants:
        y = (full + (10 ** (extra_db / 20) - 1) * support) * 10 ** (global_db / 20)
        peak = float(np.max(np.abs(signal.resample_poly(y, 4, 1, axis=0))))
        assert peak < 10 ** (-1 / 20), 'Not enough headroom; no clipping/limiting allowed'
        pcm = np.rint(y * 8388607).astype('<i4')
        packed = pcm.reshape(-1).view(np.uint8).reshape(-1, 4)[:, :3].copy()
        path = dest / filename
        with wave.open(str(path), 'wb') as w:
            w.setparams((2, 3, RATE, 0, 'NONE', 'not compressed'))
            w.writeframes(packed.tobytes())
        with wave.open(str(path), 'rb') as w:
            assert w.getnframes() == 8 * RATE and w.getnchannels() == 2
            data = np.frombuffer(w.readframes(w.getnframes()), dtype=np.uint8).reshape(-1, 3)
            decoded = data[:, 0].astype(np.int32) | (data[:, 1].astype(np.int32) << 8) | (data[:, 2].astype(np.int32) << 16)
            decoded = (decoded ^ 0x800000) - 0x800000
            error = float(np.max(np.abs(decoded.reshape(-1, 2) / 8388607 - y)))
            assert error <= 0.51 / 8388607
        outputs.append({'file': filename, 'globalGainDb': global_db,
                        'supportAdditionalDb': extra_db, 'samplePeakDbfs': float(20*np.log10(np.max(abs(y)))),
                        'oversampled4xPeakDbfs': float(20*np.log10(peak)),
                        'rmsDbfs': float(20*np.log10(np.sqrt(np.mean(y*y)))),
                        'roundTripMaxError': error, 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()})
    manifest = {'date': '2026-10-08', 'windowSeconds': [55, 63],
                'scope': 'User-requested diagnostic remix, not a production fix or Apple output match. Reuses existing original-position current-engine binaural renders, linear stem sum.',
                'referenceBoundary': 'User confirms iOS Files app screen recording is ordinary stereo downmix, NOT SDA capture and NOT the headphone spatial output.',
                'supportGroup': list(range(12, 25)),
                'limitation': 'Support group previously identified as containing audible target; not an isolated vocal stem. Other support-group content is boosted too.',
                'otherSourceGainDb': global_db, 'limiter': False, 'normalization': False,
                'renderHead': report['head'], 'harnessSha256': report['harnessSha256'],
                'sourceRenderSha256': hashes, 'outputs': outputs}
    (dest / 'manifest.json').write_text(json.dumps(manifest, indent=2), encoding='utf-8')
    print(json.dumps(outputs, indent=2))

if __name__ == '__main__':
    main()
