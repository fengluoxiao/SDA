"""Offline gain/path audit. No production presets, source gain edits or Apple claims."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import numpy as np
from scipy import signal
import importlib.util

ROOT = Path(__file__).resolve().parents[1]
RATE = 48000

def db(power):
    return float(10*np.log10(max(float(power), 1e-30)))

def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--harness', type=Path, required=True)
    args = ap.parse_args()
    root = ROOT/'tmp/how-love-diag'
    out = root/'generic-gain-path-audit'
    out.mkdir(exist_ok=True)
    asset = ROOT/'apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json'
    manifest = json.loads(asset.read_text())
    # Preserve the directory-based matching standard-speaker fallback too.
    spec=importlib.util.spec_from_file_location('manifest_variant',ROOT/'scripts/diag-hrtf-manifest-variant.py')
    helper=importlib.util.module_from_spec(spec);spec.loader.exec_module(helper)
    bypass_path=helper.prepare_variant(asset,out/'cues-disabled-assets',{'spatialCues':False})
    frames = [json.loads(s) for s in (root/'full/frames.jsonl').read_text().splitlines()]
    original = (root/'full/sources.pcm').read_bytes()
    clean_env={k:v for k,v in os.environ.items() if not k.startswith(('SDA_ATMOS_REPLAY_','SDA_DIAGNOSTIC_'))}
    base_env = dict(clean_env, SDA_ATMOS_REPLAY_WET='0', SDA_ATMOS_REPLAY_SECONDS='11',
                    SDA_ATMOS_REPLAY_DIRECT='1', SDA_ATMOS_REPLAY_FRONT_COMMON='0',
                    SDA_ATMOS_REPLAY_SPATIAL_BALANCE='0', SDA_ATMOS_REPLAY_TAG='render',
                    SDA_ATMOS_REPLAY_LEDGER='1')
    audio, configs, ledgers = {}, {}, {}
    for name, path in [('default',asset), ('cues-disabled',bypass_path), ('ids-renumbered',asset)]:
        dest = out/name
        dest.mkdir(exist_ok=True)
        replay_frames = json.loads(json.dumps(frames))
        if name == 'ids-renumbered':
            for f in replay_frames:
                f['labels'] = [f'Obj_{int(label[4:])+100}' if label.startswith('Obj_') else label for label in f['labels']]
                for event in f['events']:
                    event['id'] += 100
                for declaration in f['objectChannels']:
                    declaration['id'] += 100
        (dest/'frames.jsonl').write_text('\n'.join(json.dumps(f) for f in replay_frames)+'\n', encoding='utf-8')
        (dest/'sources.pcm').write_bytes(original)
        env = dict(base_env, SDA_ATMOS_REPLAY_HRTF=str(path.resolve()), SDA_ATMOS_REPLAY_DIR=str(dest.resolve()))
        with (dest/'render.log').open('w') as log:
            subprocess.run([str(args.harness.resolve()), 'atmos_front_pair_probe::replay_atmos_object_stream',
                            '--exact','--ignored','--nocapture','--test-threads=1'], cwd=ROOT, env=env,
                           stdout=log, stderr=subprocess.STDOUT, check=True)
        audio[name] = np.fromfile(dest/'render.f32',dtype='<f4').reshape(-1,2).astype(float)
        configs[name] = json.loads((dest/'render.json').read_text())
        ledgers[name] = json.loads((dest/'render-gain-ledger.json').read_text())
        assert np.isfinite(audio[name]).all() and configs[name]['minimumGuard']==1
        print('Rendered',name,flush=True)
    origin = json.loads((root/'frames.jsonl').read_text().splitlines()[0])['start']
    lo,hi = 55*RATE-origin,63*RATE-origin
    baseline = audio['default'][lo:hi]
    comparisons = {}
    for name in ['cues-disabled','ids-renumbered']:
        y = audio[name][lo:hi]
        err = y-baseline
        comparisons[name] = {'maxAbsDifference':float(np.max(abs(err))),
                             'differenceRelativeDb':db(np.sum(err*err)/np.sum(baseline*baseline)),
                             'rmsChangeDb':db(np.sum(y*y)/np.sum(baseline*baseline)),
                             'perEarCorrelation':[float(np.corrcoef(y[:,ear],baseline[:,ear])[0,1]) for ear in range(2)]}
        if name == 'ids-renumbered':
            assert comparisons[name]['maxAbsDifference'] < 1e-5
    old = np.fromfile(root/'source-interaction/full/render.f32',dtype='<f4').reshape(-1,2).astype(float)
    comparisons['previous-offline-baseline'] = {'maxAbsDifference':float(np.max(abs(audio['default']-old)))}
    gains = {}
    ledger = ledgers['default']
    assert len(ledger)==len(frames)
    for key in ['outputGain','programGain','comparisonGain','monitorGain','guardGain']:
        values = [row[key] for row in ledger]
        gains[key] = {'min':min(values),'max':max(values)}
    sources = {}
    for ident in ledger[0]['sources']:
        sources[ident] = {key:{'min':min(row['sources'][ident][key] for row in ledger),
                               'max':max(row['sources'][ident][key] for row in ledger)}
                         for key in ['gain','targetGain','availability','distanceGain','continuousMix']}
    events = [e for f in frames for e in f['events']]
    metadata = {'eventCount':len(events),'gainDbValues':sorted(set(e['gainDb'] for e in events)),
                'explicitDistanceEventCount':sum(e.get('distanceM') is not None for e in events),
                'nonzeroExtentEventCount':sum(any(e.get('size',[0,0,0])) for e in events)}
    band_results = {}
    for low,high in [(150,500),(500,1500),(1500,4000),(4000,8000)]:
        sos = signal.butter(3,[low,high],fs=RATE,btype='bandpass',output='sos')
        a = signal.sosfilt(sos,audio['default'],axis=0)[lo:hi]
        b = signal.sosfilt(sos,audio['cues-disabled'],axis=0)[lo:hi]
        band_results[f'{low}-{high}Hz'] = {'cueBypassPowerChangeDb':db(np.sum(b*b)/np.sum(a*a))}
    result = {'scope':'Current-source offline replay, not iOS device capture or isolated-vocal measurement. Ledger samples block endpoints, not every sample. Cue bypass is diagnostic only, not a proposed removal.',
              'cueBypassFallbackPreserved':True,
              'cueBypassManifestSha256':hashlib.sha256(bypass_path.read_bytes()).hexdigest(),
              'referenceBoundary':'iOS Files app recording: user-confirmed ordinary stereo downmix; NOT SDA recording or Apple headphone spatial render.',
              'listeningCheckpoint':'User says C (global +2dB, Obj12..24 extra +6dB) is closest; group is not isolated vocal and is not a production rule.',
              'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
              'harnessSha256':hashlib.sha256(args.harness.read_bytes()).hexdigest(),
              'inputSha256':hashlib.sha256(original).hexdigest(), 'windowSeconds':[55,63],
              'metadata':metadata, 'globalGains':gains, 'sourceGains':sources,
              'config':configs, 'comparisons':comparisons, 'bands':band_results,
              'programEnabledObserved':sorted(set(row['programEnabled'] for row in ledger))}
    (out/'report.json').write_text(json.dumps(result,indent=2),encoding='utf-8')
    print(json.dumps({k:result[k] for k in ['metadata','globalGains','comparisons','bands','programEnabledObserved']},indent=2))

if __name__ == '__main__':
    main()
