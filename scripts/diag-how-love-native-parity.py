"""Native/WASM and independent reconstruction parity; no production gains are changed."""
import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import subprocess
import numpy as np
from scipy import signal
from scipy.io import wavfile

ROOT=Path(__file__).resolve().parents[1]
RATE=48000
FIELDS=['hasPos','pos','gainDb','size','anchor','diffuse','distanceM','distanceInfinite','screenFactor','depthFactor']

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def db(ratio):
    return float(10*np.log10(max(float(ratio),1e-30)))

def read(folder, pcm_name='sources.pcm'):
    frames=[json.loads(l) for l in (folder/'frames.jsonl').read_text().splitlines()]
    raw=np.fromfile(folder/pcm_name,dtype='<f4')
    chunks=[];offset=0
    for f in frames:
        n=f['samples'];c=len(f['labels'])
        chunks.append(raw[offset:offset+c*n].reshape(c,n).T)
        offset+=c*n
    assert offset==len(raw)
    return frames,raw,np.concatenate(chunks).astype(float)

def compact(folder):
    frames=[json.loads(l) for l in (folder/'frames.jsonl').read_text().splitlines()]
    origin=frames[0]['start'];targets={};events=[]
    for e in json.loads((folder/'initial-events.json').read_text()):
        e['samplePos']=origin;e['rampDuration']=0;e.setdefault('diffuse',0.0);targets[e['id']]=e
    for f in frames:
        for e in f['events']:
            e.setdefault('diffuse',0.0);p=targets.get(e['id'])
            if p and all(e.get(k)==p.get(k) for k in FIELDS) and p['samplePos']+(p.get('rampDuration',0) or 128)<=e['samplePos']:
                continue
            targets[e['id']]=e
            events.append({k:e.get(k) for k in ['id','samplePos','rampDuration',*FIELDS]})
    return events

def counters(events):
    # Compare numeric values, not JSON spelling of integral float values.
    return Counter(
        (e['id'],e['samplePos'],e['rampDuration'],e['hasPos'],tuple(e['pos']),e['gainDb'],tuple(e['size']),
         e['anchor'],e['diffuse'],e['distanceM'],e['distanceInfinite'],e['screenFactor'],e['depthFactor']) for e in events)

def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--harness',required=True,type=Path)
    ap.add_argument('--decoder',required=True,type=Path)
    ap.add_argument('--openjoc',required=True,type=Path)
    args=ap.parse_args()
    root=ROOT/'tmp/how-love-diag';out=root/'native-parity-2026-10-08'
    fa,raw,a=read(out/'chunk8192');fb,rawb,b=read(out/'chunk65537')
    fw,raww,w=read(root,'sources.f32')
    assert len(fa)==len(fb)==len(fw) and a.shape==b.shape==w.shape and np.isfinite(a).all()
    assert np.array_equal(a,b) and fa==fb
    assert all(all(f[k]==g[k] for k in ['start','samples','labels','objectChannels']) for f,g in zip(fa,fw))
    delta=a-w
    wasm={'maxAbs':float(np.max(abs(delta))), 'relativeErrorDb':db(np.sum(delta*delta)/np.sum(w*w)),
          'rmsChangeDb':db(np.sum(a*a)/np.sum(w*w))}
    native_events=compact(out/'chunk8192');old_events=compact(root)
    ne=counters([e for e in native_events if 55*RATE<=e['samplePos']<63*RATE])
    oe=counters([e for e in old_events if 55*RATE<=e['samplePos']<63*RATE])
    assert ne==oe,'Compacted target-window metadata differs'
    origin=fa[0]['start'];lo,hi=55*RATE-origin,63*RATE-origin
    open_summary=json.loads((out/'openjoc-codec-core/debug/streaming_summary.json').read_text())
    assert open_summary['max_reconstruction_rows']==len(fa[0]['labels'])-1
    independent=[]
    for i in range(15):
        rate,y=wavfile.read(out/f'openjoc-codec-core/diagnostics/reconstruction_rows/row_{i:03}.wav',mmap=True)
        assert rate==RATE and y.ndim==1 and np.issubdtype(y.dtype,np.floating)
        x=a[lo:hi,i+1]; ref=np.asarray(y[55*RATE:63*RATE],dtype=float)
        cc=signal.correlate(x,ref,mode='full',method='fft');middle=len(x)-1
        lag=int(np.argmax(abs(cc[middle-1024:middle+1025])))-1024
        ref=np.asarray(y[55*RATE-lag:63*RATE-lag],dtype=float)
        fit=float(x@ref/(ref@ref));res=x-ref
        band={}
        for low,high in [(500,1500),(1500,4000),(4000,8000)]:
            sos=signal.butter(3,[low,high],fs=RATE,btype='bandpass',output='sos')
            xx=signal.sosfilt(sos,x);yy=signal.sosfilt(sos,ref)
            band[f'{low}-{high}Hz']=db(np.sum(xx*xx)/np.sum(yy*yy))
        independent.append({'source':fa[0]['labels'][i+1],'row':i,'lagSamples':lag,
                            'correlation':float(np.corrcoef(x,ref)[0,1]),
                            'unscaledDifferenceRelativeDb':db(np.sum(res*res)/np.sum(x*x)),
                            'nativeVsReferenceRmsDb':db(np.sum(x*x)/np.sum(ref*ref)),
                            'diagnosticFitGainDb':float(20*np.log10(abs(fit))), 'bandRmsChangeDb':band})
    dest=out/'native-replay';dest.mkdir(exist_ok=True)
    targets=json.loads((out/'chunk8192/initial-events.json').read_text())
    for e in targets:e['samplePos']=0;e['rampDuration']=0
    replay=json.loads(json.dumps(fa))
    for f in replay:
        f['start']-=origin
        for e in f['events']:e['samplePos']-=origin
    replay[0]['events']=targets+replay[0]['events']
    (dest/'frames.jsonl').write_text('\n'.join(json.dumps(f) for f in replay)+'\n',encoding='utf-8')
    (dest/'sources.pcm').write_bytes(raw.tobytes())
    env=dict(os.environ,SDA_ATMOS_REPLAY_DIR=str(dest.resolve()),SDA_ATMOS_REPLAY_TAG='render',
             SDA_ATMOS_REPLAY_HRTF=str((ROOT/'apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json').resolve()),
             SDA_ATMOS_REPLAY_WET='0',SDA_ATMOS_REPLAY_SECONDS='11',SDA_ATMOS_REPLAY_DIRECT='1',
             SDA_ATMOS_REPLAY_FRONT_COMMON='0',SDA_ATMOS_REPLAY_SPATIAL_BALANCE='0',SDA_ATMOS_REPLAY_LEDGER='1')
    with (dest/'render.log').open('w') as log:
        subprocess.run([str(args.harness.resolve()),'atmos_front_pair_probe::replay_atmos_object_stream',
                        '--exact','--ignored','--nocapture','--test-threads=1'],cwd=ROOT,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
    z=np.fromfile(dest/'render.f32',dtype='<f4').reshape(-1,2).astype(float)
    old=np.fromfile(root/'generic-gain-path-audit/default/render.f32',dtype='<f4').reshape(-1,2).astype(float)
    err=z[lo:hi]-old[lo:hi]
    render={'wholeReplayMaxAbs':float(np.max(abs(z-old))),'targetWindowMaxAbs':float(np.max(abs(err))),
            'targetDifferenceRelativeDb':db(np.sum(err*err)/np.sum(old[lo:hi]**2)),
            'minimumGuard':json.loads((dest/'render.json').read_text())['minimumGuard']}
    assert render['targetWindowMaxAbs']<1e-5 and render['minimumGuard']==1
    result={'scope':'Current native window from stream start, two byte chunkings, old WASM cache, and local independent OpenJOC reconstruction. NOT Apple output or authored-vocal isolation.',
            'nativeInputChunkingBitIdentical':True,'nativeMetadataChunkingEqual':True,
            'wasmComparison':wasm, 'targetWindowCompactedMetadataEqual':True,
            'targetWindowEventCount':sum(ne.values()),'rawEventCounts':{'native':sum(len(f['events']) for f in fa),'oldWasm':sum(len(f['events']) for f in fw)},
            'initialStateBoundaryNote':'Old cache snapshots included future OAMD events crossing the first PCM frame. Two initial motion targets differ; before 55s only. Target-window compacted events agree and replay comparison verifies preroll settlement.',
            'nativeDecodeErrors':{name:json.loads((out/name/'decode-summary.json').read_text())['errors'] for name in ['chunk8192','chunk65537']},
            'windowSeconds':[55,63], 'nativeReplay':render,
            'openjocPolicy':'Unmodified local 0.16.0; codec-core internal base. First65s raw prefix, no explicit trim override. Reconstruction rows matched by ordinal; semantic binding remains unresolved according to reference summary.',
            'openjocSummary':open_summary,'independentRows':independent,
            'hashes':{'decoder':sha(args.decoder),'harness':sha(args.harness),'openjoc':sha(args.openjoc),
                       'sourceM4a':sha(Path(json.loads((root/'analysis.json').read_text())['file'])),
                       'sourceEac3':sha(out/'source.eac3'),'sourcePrefix':sha(out/'source-first65.eac3'),
                       'nativePcm':sha(out/'chunk8192/sources.pcm'),'oldWasmPcm':sha(root/'sources.f32')}}
    (out/'report.json').write_text(json.dumps(result,indent=2),encoding='utf-8')
    print(json.dumps({'wasm':wasm,'metadataEqual':ne==oe,'nativeReplay':render,
                      'independentMinCorrelation':min(row['correlation'] for row in independent),
                      'independentMaxAbsGainDb':max(abs(row['nativeVsReferenceRmsDb']) for row in independent)},indent=2))

if __name__=='__main__':main()
