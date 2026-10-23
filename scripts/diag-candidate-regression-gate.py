"""Offline regression risk gate anchored to rejected candidate and fixed windows.

Passing this numerical gate is NOT listening acceptance or clarity evidence.
No playback scan/cache. Numeric budgets are conservative engineering choices.
"""
import argparse
import json
from pathlib import Path
import numpy as np
from scipy import signal


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--tag',required=True)
    args=parser.parse_args()
    assert args.tag and all(c.isalnum() or c in '-_' for c in args.tag)
    downloads=Path.home()/'Downloads'
    cases=[('How-Love',downloads/'SDA-current-render-audition/How-Do-I-Make-You-Love-Me-55-63',55,63,0),
           ('Very-First-Night',downloads/'SDA-current-render-audition/Very-First-Night-0-65',50,65,0),
           ('Lose-Yourself',downloads/'SDA-Lose-Yourself-audit/replay',115,251,5471232)]
    report={'candidateTag':args.tag,'scope':'Offline 100ms mixed-ear power gate. Fixed historical regression windows, no production song rule. A pass cannot establish clarity, voice identity, unchanged listening, or release authorization.',
            'budgets':{'maxLocalIldChangeDb':.25,'maxEarPowerIncreaseDb':.25,
                       'meaning':'Engineering screening choices, not perceptual thresholds. Never loosen automatically to pass.'},
            'songs':[],'listeningAcceptance':'not supplied by this program'}
    fail=False
    for name,folder,start,end,origin in cases:
        path=folder/(args.tag+'.f32')
        assert path.is_file(),f'Missing candidate replay: {path}'
        meta=json.loads(path.with_suffix('.json').read_text())
        assert meta['masterPreampDb']==0 and meta['spatialLayerGainDb']==1.5
        assert meta['spatialLayerMainRestoreDb']==0
        x=np.fromfile(folder/'peak-stall-fixed.f32',dtype='<f4').reshape(-1,2).astype(float)
        y=np.fromfile(path,dtype='<f4').reshape(-1,2).astype(float)
        assert x.shape==y.shape and np.isfinite(y).all()
        lo,hi=start*48000-origin,end*48000-origin
        assert 0<=lo<hi<=len(y)
        song={'name':name,'absoluteSongWindowSeconds':[start,end],'bands':{}}
        for low,high in [(100,500),(500,1500),(1500,4000),(4000,8000)]:
            sos=signal.butter(3,[low,high],fs=48000,btype='bandpass',output='sos')
            powers=[]
            for data in [x,y]:
                a=signal.sosfilt(sos,data,axis=0)[lo:hi]
                a=a[:len(a)//4800*4800].reshape(-1,4800,2)
                powers.append(np.mean(a*a,axis=1))
            a,b=powers
            active=a.min(axis=1)>max(a.max()*1e-3,1e-10)
            assert active.any()
            delta=10*np.log10(np.maximum(b,1e-30)/np.maximum(a,1e-30))
            ild=delta[:,1]-delta[:,0]
            indices=np.flatnonzero(active)
            changes=abs(ild[indices])
            maximum=float(changes.max());increase=float(delta[active].max())
            passed=maximum<=.25 and increase<=.25
            fail|=not passed
            worst=indices[np.argsort(changes)[-3:][::-1]]
            song['bands'][f'{low}-{high}']={
                'riskScreenPass':passed,'maxAbsIldChangeDb':maximum,
                'maxEarPowerIncreaseDb':increase,
                'worstWindows':[{'songTimeSeconds':float(start+i*.1),
                                  'leftPowerChangeDb':float(delta[i,0]),
                                  'rightPowerChangeDb':float(delta[i,1]),
                                  'rightRelativeChangeDb':float(ild[i])} for i in worst]}
        report['songs'].append(song)
    report['numericalRiskScreenPass']=not fail
    report['defaultPromotionAllowed']=False
    out=downloads/'SDA-clarity-validation/regression-gate'
    out.mkdir(exist_ok=True)
    (out/(args.tag+'.json')).write_text(json.dumps(report,indent=2),encoding='utf-8')
    print(json.dumps({'tag':args.tag,'numericalRiskScreenPass':not fail,
                      'defaultPromotionAllowed':False,'songs':{s['name']:{k:v['riskScreenPass'] for k,v in s['bands'].items()} for s in report['songs']}},indent=2))


if __name__=='__main__':main()
