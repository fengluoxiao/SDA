"""Offline causal common-cue controller feasibility, not production DSP.

Past-block moments constrain the next block, no song prescan or future input.
This deliberately measures violations on actual output, not controller intent.
One coefficient both ears; no source IDs. Retaining balance is not clarity.
"""
import json
from pathlib import Path
import numpy as np
from scipy import signal


def band_powers(x, low, high):
    sos=signal.butter(3,[low,high],fs=48000,btype='bandpass',output='sos')
    return signal.sosfilt(sos,x,axis=0)


def controller(direct,wet):
    hop=480 #10ms control updates using only completed prior blocks
    bands=[(100,500),(500,1500),(1500,4000),(4000,8000)]
    filtered=[(band_powers(direct,*b),band_powers(wet,*b)) for b in bands]
    moments=np.zeros((len(bands),3,2))
    coefficient=1.;target=1.
    gains=np.ones(len(direct));states=[]
    grid=np.linspace(1.,10**(-6/20),65)
    smoothing=1-np.exp(-hop/(.08*48000))
    envelope_alpha=1-np.exp(-hop/(.04*48000))
    for start in range(0,len(direct),hop):
        end=min(start+hop,len(direct))
        # moments are from preceding audio only. Require activity in both ears;
        # incomplete/low energy -> preserve baseline rather than extrapolate.
        target=1.
        pd,pw,cross=moments[:,0],moments[:,1],moments[:,2]
        baseline=np.maximum(pd+pw+cross,1e-30)
        active=np.min(baseline,axis=1)>1e-10
        if active.any():
            for candidate in grid:
                power=np.maximum(pd+candidate*candidate*pw+candidate*cross,1e-30)
                delta=10*np.log10(power/baseline)
                lr=delta[:,1]-delta[:,0]
                if np.all(abs(lr[active])<=.1) and np.all(abs(delta[active])<=.15):
                    target=float(candidate)
                else:
                    # Only connected safe interval from unity. No jumping
                    # across a nonlinear/cancellation region on gain grid.
                    break
        previous=coefficient
        coefficient+=envelope_alpha*(target-coefficient)
        gains[start:end]=np.linspace(previous,coefficient,end-start,endpoint=False)
        for band,(d,w) in enumerate(filtered):
            a,b=d[start:end],w[start:end]
            observation=np.array([np.mean(a*a,axis=0),np.mean(b*b,axis=0),2*np.mean(a*b,axis=0)])
            moments[band]+=smoothing*(observation-moments[band])
        states.append([start/48000,coefficient,target])
    return direct+wet*gains[:,None],gains,np.array(states)


def main():
    downloads=Path.home()/'Downloads'
    out=downloads/'SDA-clarity-validation/causal-linked-cue-controller'
    out.mkdir(exist_ok=True)
    cases=[('How-Love',downloads/'SDA-current-render-audition/How-Do-I-Make-You-Love-Me-55-63',55,63,0),
           ('Very-First-Night',downloads/'SDA-current-render-audition/Very-First-Night-0-65',0,65,0),
           ('Lose-Yourself',downloads/'SDA-Lose-Yourself-audit/replay',115,251,5471232)]
    result={'scope':'Offline controller feasibility. No future input/prescan, common residual coefficient, past10ms block moments with80ms smoothing,40ms coefficient smoothing. Models mixed residual bus, not actual object engine integration. Engineering limits are not hearing thresholds. No default or audition release.', 'songs':[]}
    ratio=10**(-6/20)
    for name,folder,start,end,origin in cases:
        pair=[]
        for k in [0,1]:
            tag='cue-attribution-safe-'+str(k)
            meta=json.loads((folder/(tag+'.json')).read_text());assert meta['minimumGuard']==1
            pair.append(np.fromfile(folder/(tag+'.f32'),dtype='<f4').reshape(-1,2).astype(float)*10**(12/20))
        a,b=pair;direct=(b-ratio*a)/(1-ratio);wet=a-direct
        candidate,gains,states=controller(direct,wet)
        lo,hi=start*48000-origin,end*48000-origin
        row={'name':name,'gainPercentilesP0P10P50P90P100':np.percentile(gains[lo:hi],[0,10,50,90,100]).tolist(),
             'fractionNearAcceptedMinus12Coefficient':float(np.mean(gains[lo:hi]<ratio+.02)),
             'bands':{},'actualOutputRiskGatePass':True}
        for low,high in [(100,500),(500,1500),(1500,4000),(4000,8000)]:
            x=band_powers(a,low,high)[lo:hi];y=band_powers(candidate,low,high)[lo:hi]
            n=len(x)//4800;x=x[:n*4800].reshape(n,4800,2);y=y[:n*4800].reshape(n,4800,2)
            pa=np.mean(x*x,axis=1);pb=np.mean(y*y,axis=1)
            active=pa.min(axis=1)>max(pa.max()*1e-3,1e-10)
            delta=10*np.log10(np.maximum(pb,1e-30)/np.maximum(pa,1e-30));ild=delta[:,1]-delta[:,0]
            maximum=float(abs(ild[active]).max());increase=float(delta[active].max())
            passed=maximum<=.25 and increase<=.25
            row['actualOutputRiskGatePass'] &= passed
            i=int(np.flatnonzero(active)[np.argmax(abs(ild[active]))])
            row['bands'][f'{low}-{high}']={'maxAbsLocalIldChangeDb':maximum,'maxEarPowerIncreaseDb':increase,
                                           'riskScreenPass':passed,'worstIldSongTime':start+i*.1}
        # Prefix invariance test proves future samples do not change earlier gain.
        prefix=min(len(direct),2*48000)
        short,short_gain,_=controller(direct[:prefix],wet[:prefix])
        assert np.array_equal(short_gain,gains[:prefix])
        assert np.array_equal(short,candidate[:prefix])
        row['prefixCausalityVerified']=True
        np.savez_compressed(out/(name+'-gain-trace.npz'),states=states)
        result['songs'].append(row)
        (out/'report.json').write_text(json.dumps(result,indent=2),encoding='utf-8')
        print(json.dumps(row,indent=2),flush=True)
    result['candidatePromotable']=False
    result['reason']='Actual output risk and clarity evidence must both pass; no perceptual improvement proven. Do not loosen constraints to force acceptance.'
    (out/'report.json').write_text(json.dumps(result,indent=2),encoding='utf-8')


if __name__=='__main__':main()
