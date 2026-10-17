"""Offline phase/delay audit against the documented spectral interpolator.

Reference is the implementation's measured-phase mixture, not Apple output.
No source gain, phase correction, or user-side analysis is installed.
"""
import json
from pathlib import Path
import numpy as np
from scipy import fft


def main():
    repo = Path(__file__).resolve().parents[1]
    out = Path.home()/'Downloads/SDA-clarity-validation/moving-filters'
    bank = repo/'apps/mobile/assets/hrtf-restored/hrtf-dense'
    rows = json.loads((out/'rows.json').read_text())
    positions = json.loads((bank/'hrtf-set.json').read_text())['positions']
    length = 2048
    freq = fft.rfftfreq(length, 1/48000)
    measured = np.array([fft.rfft(np.fromfile(bank/p['dry'], dtype='<f4').reshape(2, -1).astype(float),
                                n=length, axis=1) for p in positions])
    phases = np.unwrap(np.angle(measured), axis=2)
    power = abs(measured)**2
    h = np.fromfile(out/'filters.f32', dtype='<f4').reshape(len(rows), 2, rows[0]['tapCount'])
    actual = fft.rfft(h.astype(float), n=length, axis=2)
    reference_phase = np.zeros_like(actual.real)
    reference_power = np.zeros_like(actual.real)
    for i, row in enumerate(rows):
        assert row['support'] == 61
        for index, weight in row['weights']:
            reference_phase[i] += phases[index]*weight
            reference_power[i] += power[index]*weight
    phase_error = np.angle(actual*np.exp(-1j*reference_phase))
    delta_frequency = freq[1]-freq[0]
    # Relative group delay avoids independent unwrap branch choices. Reject
    # low-energy endpoints before interpreting any derivative.
    relative_delay = -np.angle(np.exp(1j*np.diff(phase_error, axis=2)))/(2*np.pi*delta_frequency)*48000
    frequency_mid = (freq[1:]+freq[:-1])/2
    reports = {}
    for low, high in [(500,1500),(1500,4000),(4000,8000)]:
        band = (freq>=low)&(freq<high)
        active = reference_power[:,:,band] > reference_power[:,:,band].max(axis=2,keepdims=True)*1e-3
        e = phase_error[:,:,band][active]
        pair_power = np.minimum(reference_power[:,:,1:],reference_power[:,:,:-1])
        b = (frequency_mid>=low)&(frequency_mid<high)
        use = pair_power[:,:,b] > pair_power[:,:,b].max(axis=2,keepdims=True)*1e-3
        d = relative_delay[:,:,b][use]
        reports[f'{low}-{high}'] = {
            'absolutePhaseErrorRadiansP50P99Max': np.percentile(abs(e),[50,99,100]).tolist(),
            'absoluteRelativeGroupDelaySamplesP50P99Max': np.percentile(abs(d),[50,99,100]).tolist()}
    # Left-right relative phase residual is a more direct binaural invariant
    # than treating absolute arrival independently in each ear.
    itd_error = np.angle(np.exp(1j*(phase_error[:,0]-phase_error[:,1])))
    band = (freq>=500)&(freq<4000)
    joint = np.minimum(reference_power[:,0,band],reference_power[:,1,band])
    active = joint > joint.max(axis=1,keepdims=True)*1e-3
    report = {'scope':'2498 approximated B2 ramp directions; 2048FFT matches spectral-bank grid. Reference weighted unwrapped original measured phases. Group delay difference, not absolute acoustic delay. No semantic target isolation or perceptual claim.',
              'bands':reports,
              'absoluteBinauralPhaseResidualRadiansP50P99Max':np.percentile(abs(itd_error[:,band][active]),[50,99,100]).tolist()}
    fixture = repo/'tmp/how-love-diag'
    frames = [json.loads(line) for line in (fixture/'frames.jsonl').read_text().splitlines()]
    raw = np.fromfile(fixture/'sources.f32',dtype='<f4')
    offset, chunks = 0, []
    for frame in frames:
        size = frame['samples']*len(frame['labels'])
        chunks.append(raw[offset:offset+size].reshape(len(frame['labels']),frame['samples']).T)
        offset += size
    assert offset == len(raw)
    pcm = np.concatenate(chunks)
    routes = json.loads((out/'routes.json').read_text())
    total = error_power = over01 = over003 = 0.0
    worst = []
    for object_id, track in routes.items():
        channel = frames[0]['labels'].index('Obj_'+object_id)
        indices = np.array([r['filter'] for r in track])
        centers = np.array([r['sample'] for r in track])-frames[0]['start']
        take = centers[:,None]+np.arange(-1024,1024)
        spectrum = abs(fft.rfft(pcm[take,channel]*np.hanning(2048),axis=1))**2
        weight = spectrum[:,None,band]*reference_power[indices][:,:,band]
        e = phase_error[indices][:,:,band]
        total += weight.sum()
        error_power += (weight*abs(np.exp(1j*e)-1)**2).sum()
        over01 += (weight*(abs(e)>.1)).sum()
        over003 += (weight*(abs(e)>.03)).sum()
        scores = (weight*abs(np.exp(1j*e)-1)**2).sum(axis=(1,2))
        j = int(scores.argmax())
        worst.append({'object':object_id,'timeSeconds':track[j]['sample']/48000,
                      'directionIndex':int(indices[j]),'errorPower':float(scores[j]),
                      'localPhaseOnlyErrorRelativeDb':float(10*np.log10(max(scores[j]/weight[j].sum(),1e-30)))})
    report['sourceWeighted500to4000'] = {
        'phaseOnlyComplexResponseErrorRelativeDb':float(10*np.log10(error_power/total)),
        'referenceEnergyFractionWithPhaseErrorAbove0p1Rad':float(over01/total),
        'referenceEnergyFractionWithPhaseErrorAbove0p03Rad':float(over003/total),
        'highestErrorWindows':sorted(worst,key=lambda r:-r['errorPower'])[:5]}
    (out/'phase-delay-check.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    print(json.dumps(report,indent=2))


if __name__=='__main__':
    main()
