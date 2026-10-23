"""Offline phase attribution for two stationary observed frontal sources.

IDs are fixture-only evidence. Phase-neutral output is a mathematical counterfactual,
NOT a proposed spatial renderer (it would remove physical binaural timing).
"""
import argparse
import json
import os
import subprocess
from pathlib import Path

import numpy as np
from scipy import signal
from PIL import Image, ImageDraw


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--harness', type=Path, required=True)
    ap.add_argument('--pair', type=int, nargs=2, default=[16,20],
                    help='Diagnostic fixture ordinals only: 12/16/20/23')
    a = ap.parse_args()
    repo = Path(__file__).resolve().parents[1]
    root = Path.home()/'Downloads/SDA-clarity-validation/subarashiki-front'
    geometry = {12:90.,16:45.,20:-45.,23:-90.}
    if any(i not in geometry for i in a.pair) or a.pair[0] == a.pair[1]:
        raise ValueError('Pair lacks audited dominant horizontal geometry')
    suffix = '' if a.pair == [16,20] else '-'+ '-'.join(map(str,a.pair))
    out = root/('transfer-phase-audit'+suffix); out.mkdir(exist_ok=True)
    requests = [{'az': geometry[i], 'el': 0.} for i in a.pair] + [{'az':0., 'el':0.}]
    (out/'requests.json').write_text(json.dumps(requests))
    env = {k:v for k,v in os.environ.items() if not k.startswith(('SDA_FILTER_AUDIT_', 'SDA_ATMOS_REPLAY_', 'SDA_DIAGNOSTIC_'))}
    env.update(SDA_FILTER_AUDIT_REQUESTS=str(out/'requests.json'), SDA_FILTER_AUDIT_OUT=str(out))
    with (out/'export.log').open('w') as log:
        subprocess.run([str(a.harness), 'directional::spectral_interpolation_audit::export_actual_direction_filters',
                        '--exact', '--ignored', '--nocapture', '--test-threads=1'], cwd=repo,
                       env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
    rows = json.loads((out/'rows.json').read_text())
    all_h = np.fromfile(out/'filters.f32', dtype='<f4').reshape(3,2,rows[0]['tapCount']).astype(float)
    h = all_h[:2]
    frames = [json.loads(s) for s in (root/'replay/frames.jsonl').read_text().splitlines()]
    raw = np.memmap(root/'replay/sources.pcm', dtype='<f4', mode='r')
    # Exported kernel clock proven by known impulse: account for renderer delay.
    lo, hi = 140*48000, 180*48000
    delay = 2288
    inputs = [[], []]; offset = 0
    for frame in frames:
        n = frame['samples']; c = len(frame['labels'])
        block = raw[offset:offset+n*c].reshape(c,n); offset += n*c
        l = max(0, lo-delay-frame['start']); r = min(n, hi-delay-frame['start'])
        if r > l:
            for i, ident in enumerate(a.pair):
                inputs[i].append(block[frame['labels'].index('Obj_'+str(ident)),l:r].copy())
    x = np.stack([np.concatenate(v) for v in inputs])
    ys = []
    for ident in a.pair:
        meta = json.loads((root/f'forward-pair-audit/Obj_{ident}.json').read_text())
        assert meta['minimumGuard'] == 1 and not meta['frontCommonActive']
        y = np.memmap(root/f'forward-pair-audit/Obj_{ident}.f32', dtype='<f4', mode='r').reshape(-1,2)
        ys.append(np.array(y[lo:hi], dtype=float))
    nfft = 4096
    f, t, X = signal.stft(x, fs=48000, nperseg=nfft, noverlap=3072, boundary=None)
    Y = np.stack([signal.stft(y.T, fs=48000, nperseg=nfft, noverlap=3072, boundary=None)[2] for y in ys])
    H = np.fft.rfft(h, n=nfft, axis=-1)
    # Both ears receive the same output shelf. Estimate its common magnitude
    # from solos for weighting only; do not fit directional phase to outputs.
    pred = H[:,:,:,None] * X[:,None,:,:]
    px = np.sum(np.abs(pred)**2, axis=(0,1,3))
    py = np.sum(np.abs(Y)**2, axis=(0,1,3))
    common = np.sqrt(py/np.maximum(px,1e-30))
    pred *= common[None,None,:,None]
    neutral = np.abs(H[:,:,:,None])*X[:,None,:,:]*common[None,None,:,None]
    # Remove ONLY each direction's common ear phase. Keep the complex L/R
    # transfer ratio exactly: this isolates monaural-common phase from ITD.
    phase = np.unwrap(np.angle(H), axis=-1)
    # Keep the common frontal-reference clock/coloration, rather than removing
    # the ~170-sample bulk delay only from a subset of the scene.
    centre_H = np.fft.rfft(all_h[2], n=nfft, axis=-1)
    reference_phase = np.unwrap(np.angle(centre_H),axis=-1).mean(axis=0)
    common_phase = phase.mean(axis=1)-reference_phase[None,:]
    interaural_only_H = H*np.exp(-1j*common_phase[:,None,:])
    interaural_only = interaural_only_H[:,:,:,None]*X[:,None,:,:]*common[None,None,:,None]
    ratio_error = float(np.max(np.abs(interaural_only_H[:,0]*H[:,1]-interaural_only_H[:,1]*H[:,0])))
    assert ratio_error < 1e-10, 'Counterfactual changed complex interaural ratio'
    # Embed this pair back into the actual same-graph whole scene. This is a
    # diagnostic ablation, not an ID-conditioned production candidate.
    full = np.memmap(root/'replay/reflection-direct-only.f32',dtype='<f4',mode='r').reshape(-1,2)
    _, _, full_stft = signal.stft(np.array(full[lo:hi].T,dtype=float), fs=48000,
                                 nperseg=nfft,noverlap=3072,boundary=None)
    # Use actual output times a unit common phase factor: do not replace sources
    # with the approximate input/kernel prediction.
    phase_factor = np.exp(-1j*common_phase[:,None,:,None])
    whole_counterfactual = full_stft + np.sum(Y*(phase_factor-1),axis=0)
    # Crossfade phase (not complex output amplitudes) to isolate high-band risk.
    high_weight = np.clip((f-2500)/2000,0,1)
    high_weight = high_weight*high_weight*(3-2*high_weight)
    high_factor = np.exp(-1j*common_phase[:,None,:,None]*high_weight[None,None,:,None])
    high_counterfactual = full_stft + np.sum(Y*(high_factor-1),axis=0)
    risk={}
    for low,high in [(300,700),(700,1500),(1500,4000),(4000,8000)]:
        mask=(f>=low)&(f<high)
        baseline_power=np.sum(abs(full_stft[:,mask,:])**2,axis=1)
        active=baseline_power.min(axis=0)>baseline_power.max()*1e-4
        baseline_ild=10*np.log10(np.maximum(baseline_power[0],1e-30)/np.maximum(baseline_power[1],1e-30))
        row={}
        for name,counter in [('commonPhaseAllBands',whole_counterfactual),('commonPhaseHighBand',high_counterfactual)]:
            power=np.sum(abs(counter[:,mask,:])**2,axis=1)
            ild=10*np.log10(np.maximum(power[0],1e-30)/np.maximum(power[1],1e-30))
            diff=abs(ild[active]-baseline_ild[active])
            row[name]={'wholeBandPowerChangeDb':float(10*np.log10(power.sum()/baseline_power.sum())),
                       'maxActiveFrameIldChangeDb':float(diff.max()),
                       'p95ActiveFrameIldChangeDb':float(np.percentile(diff,95))}
        risk[f'{low}-{high}']=row
    def cross(v):
        return 2*np.real(v[0]*np.conj(v[1]))
    bands = {}
    for low, high in [(300,700),(700,1500),(1500,4000),(4000,8000)]:
        mask = (f>=low)&(f<high)
        def metric(v):
            a,b = v[0,...,mask,:], v[1,...,mask,:]
            pa,pb = np.sum(abs(a)**2),np.sum(abs(b)**2)
            cr = 2*np.real(np.sum(a*np.conj(b)))
            return {'normalizedCorrelation':float(cr/(2*np.sqrt(pa*pb))),
                    'coherentVsIndependentDb':float(10*np.log10(max((pa+pb+cr)/(pa+pb),1e-30)))}
        bands[f'{low}-{high}'] = {'input':metric(X), 'actualOutput':metric(Y),
                                  'kernelPrediction':metric(pred), 'phaseNeutralCounterfactual':metric(neutral),
                                  'commonPhaseRemovedInterauralPreserved':metric(interaural_only)}
    # Window leakage makes the prediction approximate; expose its discrepancy.
    cm = cross(Y); cp = cross(pred)
    independent = abs(Y[0])**2+abs(Y[1])**2
    rows_img = []
    mask = (f>=300)&(f<=8000)
    active = independent > independent.max()*1e-5
    for ear in range(2):
        for v in (cm,cp):
            ratio = np.clip(v[ear]/np.maximum(independent[ear],1e-30),-.9,.9)[mask]
            c = np.ones((*ratio.shape,3))*255
            amount = abs(ratio)/.9
            c[:,:,0] *= 1-np.maximum(-ratio,0)/.9
            c[:,:,1] *= 1-amount
            c[:,:,2] *= 1-np.maximum(ratio,0)/.9
            c[~active[ear][mask]] = 190
            rows_img.append(Image.fromarray(c[::-1].astype('uint8')).resize((1000,170)))
    canvas = Image.new('RGB',(1120,840),'white'); draw = ImageDraw.Draw(canvas)
    titles = ['Left actual cross/independent','Left measured-kernel prediction',
              'Right actual cross/independent','Right measured-kernel prediction']
    for i,img in enumerate(rows_img):
        y=30+i*195;canvas.paste(img,(80,y+20));draw.text((80,y),titles[i],fill='black')
    draw.text((80,817),'140s -> 180s; 300-8000Hz; blue negative/red positive. No vocal isolation or correction.',fill='black')
    canvas.save(out/'actual-vs-kernel-cross.png')
    report = {'scope':'Two fixture objects at dominant geometry; side objects have slight motion, so prediction gate is required. Common-phase removal is relative to center-reference phase, preserving its bulk clock. Common magnitude fit cannot alter predicted phase. Neutral phase is NOT a candidate.',
              'pair':a.pair,
              'bands':bands, 'interauralComplexRatioCrossProductError':ratio_error,
              'actualWholeSceneRisk':risk,
              'productionChanged':False}
    report['maxBandCrossPredictionErrorDb'] = max(abs(v['actualOutput']['coherentVsIndependentDb']-
                                                   v['kernelPrediction']['coherentVsIndependentDb']) for v in bands.values())
    report['predictionCheckPassed'] = report['maxBandCrossPredictionErrorDb'] < .02
    (out/'report.json').write_text(json.dumps(report,indent=2))
    print(json.dumps(report,indent=2))
    if not report['predictionCheckPassed']:
        raise RuntimeError('Dominant-geometry kernel does not meet prediction gate; do not treat counterfactual as verified')


if __name__=='__main__':
    main()
