"""Synthetic actual-renderer object latency audit; no phase compensation."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import numpy as np
from scipy import signal


def main():
    ap=argparse.ArgumentParser();ap.add_argument('--harness',type=Path,required=True);args=ap.parse_args()
    repo=Path(__file__).resolve().parents[1]
    out=Path.home()/'Downloads/SDA-clarity-validation/subarashiki-front/object-clock-audit'
    out.mkdir(exist_ok=True)
    directions=[('center',[0.,1.,0.]),('front-left',[-1.,1.,0.]),
                ('front-right',[1.,1.,0.]),('rear-height',[0.,-1.,1.])]
    requests=[]
    for _,p in directions:
        requests.append({'az':float(np.degrees(-np.arctan2(p[0],p[1]))),
                         'el':float(np.degrees(np.arctan2(p[2],np.linalg.norm(p[:2]))))})
    (out/'requests.json').write_text(json.dumps(requests))
    env={k:v for k,v in os.environ.items() if not k.startswith(('SDA_ATMOS_REPLAY_','SDA_DIAGNOSTIC_','SDA_FILTER_AUDIT_'))}
    export=dict(env,SDA_FILTER_AUDIT_REQUESTS=str(out/'requests.json'),SDA_FILTER_AUDIT_OUT=str(out))
    with (out/'filter-export.log').open('w') as log:
        subprocess.run([str(args.harness),'directional::spectral_interpolation_audit::export_actual_direction_filters',
                        '--exact','--ignored','--nocapture','--test-threads=1'],cwd=repo,env=export,stdout=log,stderr=subprocess.STDOUT,check=True)
    rows=json.loads((out/'rows.json').read_text());h=np.fromfile(out/'filters.f32',dtype='<f4').reshape(4,2,rows[0]['tapCount']).astype(float)
    # Known impulse well after source availability/filter initialization.
    pulse=48000;end=96000;hop=1536
    labels=['LFE']+['Obj_'+str(i) for i in range(10,25)]
    reports=[];outputs=[]
    for index,(name,position) in enumerate(directions):
        folder=out/name;folder.mkdir(exist_ok=True);frames=[]
        with (folder/'sources.pcm').open('wb') as file:
            for start in range(0,end,hop):
                n=min(hop,end-start);pcm=np.zeros((16,n),dtype='<f4')
                if start<=pulse<start+n:pcm[1,pulse-start]=.01
                file.write(pcm.tobytes())
                event={'id':10,'samplePos':0,'hasPos':True,'pos':position,'gainDb':0,'size':[0,0,0],'rampDuration':0}
                frames.append({'start':start,'samples':n,'labels':labels,'objectChannels':[],'events':[event] if start==0 else []})
        (folder/'frames.jsonl').write_text('\n'.join(json.dumps(f) for f in frames)+'\n')
        config=dict(env,SDA_ATMOS_REPLAY_DIR=str(folder),SDA_ATMOS_REPLAY_HRTF=str(repo/'apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json'),
                    SDA_ATMOS_REPLAY_TAG='impulse',SDA_ATMOS_REPLAY_SECONDS='2',SDA_ATMOS_REPLAY_WET='0',
                    SDA_ATMOS_REPLAY_DIRECT='1',SDA_ATMOS_REPLAY_FRONT_COMMON='0',SDA_ATMOS_REPLAY_SPATIAL_BALANCE='0',SDA_ATMOS_REPLAY_ENHANCEMENT='0')
        # Disable optional cues but preserve identical directory/fallback graph.
        import importlib.util
        spec=importlib.util.spec_from_file_location('variant',repo/'scripts/diag-hrtf-manifest-variant.py');helper=importlib.util.module_from_spec(spec);spec.loader.exec_module(helper)
        config['SDA_ATMOS_REPLAY_HRTF']=str(helper.prepare_variant(repo/'apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json',folder/'assets',{'spatialCues':False}))
        with (folder/'render.log').open('w') as log:
            subprocess.run([str(args.harness),'atmos_front_pair_probe::replay_atmos_object_stream','--exact','--ignored','--nocapture','--test-threads=1'],cwd=repo,env=config,stdout=log,stderr=subprocess.STDOUT,check=True)
        meta=json.loads((folder/'impulse.json').read_text());assert meta['minimumGuard']==1
        y=np.fromfile(folder/'impulse.f32',dtype='<f4').reshape(-1,2).astype(float)
        outputs.append(y)
        ears=[]
        for ear in [0,1]:
            cc=signal.correlate(y[:,ear],h[index,ear],mode='full',method='fft')
            lag=int(np.argmax(abs(cc)))-(len(h[index,ear])-1)
            segment=y[lag:lag+len(h[index,ear]),ear]
            gain=float(segment@h[index,ear]/(h[index,ear]@h[index,ear]))
            error=segment-gain*h[index,ear]
            residual=float(10*np.log10(max(np.sum(error*error)/np.sum(segment*segment),1e-30)))
            ears.append({'ear':ear,'bulkRendererDelaySamples':lag-pulse,'fittedScalar':gain,'filterResidualRelativeDb':residual})
        reports.append({'direction':name,'position':position,'ears':ears})
        print(json.dumps(reports[-1]),flush=True)
    # Physical HRTF impulse responses differ, but fixed renderer latency should not.
    delays=[e['bulkRendererDelaySamples'] for r in reports for e in r['ears']]
    report={'scope':'Known impulse through same single actual object path, cue-off but authored direction HRTF retained. Compare exported Grid kernel to observed impulse. Enhancement off; EAC3 presentation compensation may contribute common scalar. Not vocal identity, whole-scene phase correctness or Apple device test.',
            'directions':reports,'bulkDelaySpreadSamples':max(delays)-min(delays),'productionChanged':False}
    assert report['bulkDelaySpreadSamples']==0,'Actual routing latency discrepancy found'
    (out/'report.json').write_text(json.dumps(report,indent=2))


if __name__=='__main__':main()
