"""Synthetic residual-only allpass feasibility, not production DSP.

Unit-magnitude residual filters do NOT imply unit-magnitude total response.
Assess ears jointly; retain synthetic failures rather than exporting auditions.
"""
import json
from pathlib import Path
import numpy as np
from scipy import signal


def main():
    rate=48000
    nfft=32768
    frequency=np.fft.rfftfreq(nfft,1/rate)
    band=(frequency>=500)&(frequency<4000)
    # First-order allpass: H(z)=(a+z^-1)/(1+a*z^-1).
    # Same filter on both residual ears preserves residual ILD/IPD exactly,
    # but may alter their interference with the unchanged direct ears.
    filters=[('bypass',None),('allpass-a0p35',.35),('allpass-a-minus0p65',-.65)]
    scenes=[('center',1.,1.,.22,.22,0,0),
            ('lateral',.65,1.,.28,.18,14,0),
            ('different-reflection-direction',1.,.65,.18,.28,0,14)]
    reports=[]
    for name,l,r,rl,rr,dl,dr in scenes:
        for reflection_ms in [5,17,43,75]:
            direct=np.zeros((2,8192));residual=np.zeros_like(direct)
            direct[0,128+dl]=l;direct[1,128+dr]=r
            # Synthetic 5-80ms reflection, intentionally coherent. Amplitudes
            # and delays are stress cases, not measurements of the three songs.
            delay=int(reflection_ms*rate/1000)
            residual[0,128+delay]=rl;residual[1,128+delay]=rr
            original=np.fft.rfft(direct+residual,n=nfft,axis=1)
            baseline_ild=20*np.log10(abs(original[1])/abs(original[0]))
            reference=np.fft.rfft(residual,n=nfft,axis=1)
            for tag,a in filters:
                changed=residual.copy() if a is None else signal.lfilter([a,1],[1,a],residual,axis=1)
                total=np.fft.rfft(direct+changed,n=nfft,axis=1)
                wet=np.fft.rfft(changed,n=nfft,axis=1)
                magnitude=20*np.log10(abs(total)/abs(original))
                ild=20*np.log10(abs(total[1])/abs(total[0]))
                wet_error=float(np.max(abs(abs(wet)/abs(reference)-1)))
                residual_ratio_error=float(np.max(abs(wet[1]/wet[0]-reference[1]/reference[0])))
                mixed_ild_change=float(np.max(abs(ild[band]-baseline_ild[band])))
                mixed_magnitude_change=float(np.max(abs(magnitude[:,band])))
                row={'scene':name,'reflectionMs':reflection_ms,'filter':tag,
                     'residualMagnitudeRelativeMaxError':wet_error,
                     'residualComplexEarRatioMaxError':residual_ratio_error,
                     'mixedMaxAbsIldChangeDb500to4000':mixed_ild_change,
                     'mixedMaxAbsMagnitudeChangeDb500to4000':mixed_magnitude_change}
                if a is None:
                    assert mixed_ild_change==0 and mixed_magnitude_change==0
                else:
                    assert wet_error<1e-12 and residual_ratio_error<1e-12
                reports.append(row)
    assert any(r['mixedMaxAbsIldChangeDb500to4000']>1 for r in reports if r['filter']!='bypass')
    result={'scope':'Synthetic residual-only allpass safety screen; no actual-song clarity measurement. Same allpass both ears preserves isolated residual magnitude and ear ratio, but mixed output can shift. Coefficients are experimental, not literature-prescribed solution.',
            'result':'Reject blind allpass insertion as guaranteed balance-preserving fix. No production change or song audition generated.',
            'cases':reports}
    out=Path.home()/'Downloads/SDA-clarity-validation'
    (out/'residual-allpass-synthetic-safety.json').write_text(json.dumps(result,indent=2))
    for tag,_ in filters:
        rows=[r for r in reports if r['filter']==tag]
        print(tag,'maximum mixed ILD delta dB',max(r['mixedMaxAbsIldChangeDb500to4000'] for r in rows),
              'maximum mixed magnitude delta dB',max(r['mixedMaxAbsMagnitudeChangeDb500to4000'] for r in rows))


if __name__=='__main__':main()
