"""Audit actual SADIE archive vs shipped directional HRIRs; no asset changes."""
import io
import json
import zipfile
from pathlib import Path
import numpy as np
from scipy import signal
from scipy.io import wavfile


def main():
    repo=Path(__file__).resolve().parents[1]
    asset=repo/'apps/mobile/assets/hrtf-restored/hrtf-dense'
    m=json.loads((asset/'hrtf-set.json').read_text())
    archive=zipfile.ZipFile(repo/'tmp/sadie-source/D1.zip')
    report={'scope':'Original measured HRIR vs stored calibrated HRIR at measured anchors. Correlation/phase descriptive, no perceptual proof or production changes.',
            'productionChanged':False,'directions':[]}
    for p in m['positions']:
        if p['elevation'] != 0 or p['azimuth'] not in (-90,-50,-40,0,40,50,90):continue
        path=p['measurement']['dry']['sourcePath']
        matches=[n for n in archive.namelist() if n.endswith(path)]
        assert len(matches)==1
        rate,raw=wavfile.read(io.BytesIO(archive.read(matches[0])))
        assert rate==48000 and raw.shape[1]==2
        raw=raw.astype(float)/(2**31)
        h=np.fromfile(asset/p['dry'],dtype='<f4').reshape(2,-1).T.astype(float)
        ears=[]
        for e in range(2):
            cc=signal.correlate(h[:,e],raw[:,e],method='fft')
            lag=int(np.argmax(abs(cc)))-(len(raw)-1)
            start=max(0,lag);rs=max(0,-lag);count=min(len(h)-start,len(raw)-rs)
            a=raw[rs:rs+count,e];b=h[start:start+count,e]
            scale=float(a@b/max(a@a,1e-30))
            corr=float(a@b/max(np.sqrt((a@a)*(b@b)),1e-30))
            ears.append({'lagSamples':lag,'fittedScalar':scale,'waveformCorrelation':corr})
        R=np.fft.rfft(raw,n=4096,axis=0);H=np.fft.rfft(h,n=4096,axis=0)
        f=np.fft.rfftfreq(4096,1/48000)
        def itd(response,lo=300,hi=1500):
            phase=np.unwrap(np.angle(response[:,0]*np.conj(response[:,1])))
            mask=(f>=lo)&(f<=hi)
            weights=abs(response[mask,0])*abs(response[mask,1])
            slope=np.polyfit(f[mask],phase[mask],1,w=np.sqrt(weights))[0]
            return float(-slope/(2*np.pi)*48000)
        bands={}
        R*=np.sqrt(np.sum(abs(H)**2)/np.sum(abs(R)**2))
        for lo,hi in [(700,1500),(1500,4000),(4000,8000)]:
            mask=(f>=lo)&(f<hi)
            bands[f'{lo}-{hi}']={'storedVsRawBroadbandMatchedDb':float(10*np.log10(np.sum(abs(H[mask])**2)/np.sum(abs(R[mask])**2)))}
        report['directions'].append({'azimuth':p['azimuth'],'ears':ears,'rawLowBandItdSamples':itd(R),
                                      'storedLowBandItdSamples':itd(H),'bands':bands,
                                      'calibrationCommonDelay':p['processing']['dry']['commonDelaySamples']})
    out=Path.home()/'Downloads/SDA-clarity-validation/subarashiki-front'
    (out/'direction-calibration-audit.json').write_text(json.dumps(report,indent=2))
    print(json.dumps(report,indent=2))


if __name__=='__main__':main()
