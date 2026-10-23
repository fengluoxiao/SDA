from pathlib import Path
import os,json,subprocess,concurrent.futures,math,hashlib,argparse,shutil
import numpy as np
from scipy.signal import resample_poly
import pyloudnorm as pyln
parser=argparse.ArgumentParser(description='Export HRTFHQ auditions with one fixed gain per track/profile/mode.')
parser.add_argument('--root',type=Path,required=True)
parser.add_argument('--ffmpeg')
args=parser.parse_args()
R=args.root.resolve();site=R/'site';(site/'audio-v2').mkdir(parents=True,exist_ok=True)
catalog=json.loads((R/'catalog.json').read_text(encoding='utf-8'))
ff=args.ffmpeg or os.environ.get('FFMPEG_BINARY') or shutil.which('ffmpeg')
if not ff:
 import imageio_ffmpeg
 ff=imageio_ffmpeg.get_ffmpeg_exe()
VERSION=2
def decode_wav(path,offset=None,duration=None):
 args=[ff,'-v','error']
 if offset is not None:args+=['-ss',str(offset)]
 args+=['-i',str(path)]
 if duration is not None:args+=['-t',str(duration)]
 args+=['-ar','48000','-ac','2','-f','f32le','pipe:1'];p=subprocess.run(args,capture_output=True,check=True);x=np.frombuffer(p.stdout,dtype='<f4').reshape(-1,2).astype(float);assert np.isfinite(x).all();return x

def intrinsic_loudness(x,meter):
 peak=float(np.max(abs(x)));assert peak>1e-10,'Silent comparison excerpt'
 factor=.5/peak;v=float(meter.integrated_loudness(x*factor)-20*np.log10(factor));assert math.isfinite(v);return v

def true_peak(x):return float(np.max(abs(resample_poly(x,4,1,axis=0))))

def encode(path,x,target=None):
 z=np.asarray(x,dtype='<f4');assert np.isfinite(z).all() and float(abs(z).max())<1
 p=subprocess.run([ff,'-v','error','-y','-f','f32le','-ar','48000','-ac','2','-i','pipe:0','-c:a','pcm_s24le',str(path)],input=z.tobytes(),capture_output=True);assert p.returncode==0,p.stderr.decode(errors='replace')
 y=decode_wav(path);assert y.shape==x.shape;err=float(np.max(abs(y-z)));assert err<=1.3e-7,err
 meter=pyln.Meter(48000);level=float(meter.integrated_loudness(y));peak=true_peak(y);assert peak<.87
 if target is not None:assert abs(level-target)<.12,(str(path),level,target)
 return {'lufs':level,'truePeakDb':float(20*np.log10(max(peak,1e-30))),'pcm24MaxError':err,'sha256':hashlib.sha256(path.read_bytes()).hexdigest()}


def source_audio(t,pid,lo,hi):
 path=R/'data'/t['id']/f'{pid}.f32'
 x=np.memmap(path,dtype='<f4',mode='r').reshape(-1,2)
 result=np.asarray(x[lo*48000:hi*48000],dtype=float)
 assert len(result)==(hi-lo)*48000 and np.isfinite(result).all()
 return result

def archive_audio(a,lo,hi):
 offset=lo-a['origin'];assert offset>=0
 if a.get('format')=='f32le':
  x=np.memmap(a['source'],dtype='<f4',mode='r').reshape(-1,2)
  result=np.asarray(x[offset*48000:(hi-a['origin'])*48000],dtype=float)
 else:result=decode_wav(a['source'],offset,hi-lo)
 assert len(result)==(hi-lo)*48000 and np.isfinite(result).all()
 return result

def archives_for(t):
 items={}
 for a in catalog.get('archives',[]):
  if a['track']==t['id']:
   assert a['id'] not in items,'Archive references must share one continuous source per track/profile'
   items[a['id']]=a
 return items

def track_plan(t):
 lo=min(w[0] for w in t['windows']);hi=max(w[1] for w in t['windows'])
 meter=pyln.Meter(48000);archives=archives_for(t);stats={}
 for pid in [*t['profiles'],*archives]:
  x=archive_audio(archives[pid],lo,hi) if pid in archives else source_audio(t,pid,lo,hi)
  windows=archives[pid]['windows'] if pid in archives else t['windows']
  peak=max(true_peak(x[(a-lo)*48000:(b-lo)*48000]) for a,b in windows)
  stats[pid]={'level':intrinsic_loudness(x,meter),'peakDb':20*np.log10(max(peak,1e-30))}
 target=min(-23.,min(s['level']-s['peakDb']-1.5 for s in stats.values()))
 # One calibration range per track/profile. Never normalize individual excerpts.
 gains={};errors={}
 for iteration in range(4):
  for pid in stats:
   x=archive_audio(archives[pid],lo,hi) if pid in archives else source_audio(t,pid,lo,hi)
   db=target-stats[pid]['level']
   for _ in range(4):
    actual=float(meter.integrated_loudness(x*10**(db/20)));error=target-actual
    if abs(error)<1e-5:break
    db+=error
   gains[pid]=db;errors[pid]=abs(target-float(meter.integrated_loudness(x*10**(db/20))))
  excess=max(stats[pid]['peakDb']+gains[pid]+1.5 for pid in stats)
  if excess<=1e-6:break
  target-=excess+1e-5
 assert max(errors.values())<.001 and excess<=1e-6
 reference=catalog.get('acceptedDefaultProfile','current')
 common=min(gains[reference],-1.5-max(stats[pid]['peakDb'] for pid in t['profiles']))
 plan={'version':VERSION,'gainScope':'track','track':t['id'],'anchor':[lo,hi],'targetReferenceLufs':target,'commonGainDb':common,'matchedGainDb':gains,'referenceLevelErrorLU':errors,'stats':stats}
 plan['key']=hashlib.sha256(json.dumps(plan,sort_keys=True).encode()).hexdigest()
 print('GAIN PLAN',t['id'],f'{lo}-{hi}',flush=True)
 return plan

with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
 plans={p['track']:p for p in pool.map(track_plan,catalog['tracks'])}
(site/'gain-plans.json').write_text(json.dumps(plans,ensure_ascii=False,indent=2),encoding='utf-8')

def export_window(task):
 t,start,end=task;plan=plans[t['id']];key=f'{start}-{end}'
 dest=site/'audio-v2'/t['id']/key;dest.mkdir(parents=True,exist_ok=True);receipt=dest/'verification.json'
 if receipt.exists():
  old=json.loads(receipt.read_text(encoding='utf-8'))
  if old.get('normalizationPlanKey')==plan['key']:return old
 clips={};metadata={}
 for pid in t['profiles']:
  clips[pid]=source_audio(t,pid,start,end);metadata[pid]=dict(catalog['profiles'][pid],archival=False)
 for pid,a in archives_for(t).items():
  if [start,end] in a['windows']:
   clips[pid]=archive_audio(a,start,end);metadata[pid]=dict(a,archival=True)
 rows=[]
 for pid,x in clips.items():
  gains={'matched':plan['matchedGainDb'][pid]}
  if not metadata[pid]['archival']:gains['common']=plan['commonGainDb']
  row={'id':pid,'label':metadata[pid]['label'],'status':metadata[pid]['status'],'note':metadata[pid].get('note',''),'archival':metadata[pid]['archival'],'files':{},'measurements':{},'gainDb':gains}
  for mode,db in gains.items():
   path=dest/f'{pid}-{mode}.wav';metrics=encode(path,x*10**(db/20))
   assert metrics['truePeakDb']<=-1.49999
   row['files'][mode]=path.relative_to(site).as_posix();row['measurements'][mode]=metrics
  rows.append(row)
 report={'track':t['id'],'start':start,'end':end,'normalizationVersion':VERSION,'normalizationPlanKey':plan['key'],'gainScope':'track','normalizationAnchor':plan['anchor'],'referenceTargetLufs':plan['targetReferenceLufs'],'commonGainDb':plan['commonGainDb'],'profiles':rows,'skippedArchives':[],'scope':'One fixed gain for every excerpt of the same track/profile/mode; matched mode calibrates the common reference range, common mode shares one gain among all current-engine profiles. Overlapping excerpts must be sample-identical.'}
 receipt.write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8');print('EXPORTED',t['id'],key,flush=True);return report

tasks=[(t,a,b) for t in catalog['tracks'] for a,b in t['windows']]
with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:results=list(pool.map(export_window,tasks))
results.sort(key=lambda q:(next(i for i,t in enumerate(catalog['tracks']) if t['id']==q['track']),q['start'],q['end']))
delivery={'date':catalog['date'],'gainClarification':catalog['gainClarification'],'limits':catalog['limits'],'tracks':catalog['tracks'],'windows':results,'freshEngineReplays':sum(len(t['profiles']) for t in catalog['tracks']),'derivedOptions':0,'wavFiles':sum(len(p['files']) for w in results for p in w['profiles']),'normalizationVersion':VERSION,'gainScope':'track','gainPlans':plans,'acceptedDefaultProfile':catalog['acceptedDefaultProfile'],'confirmedSelections':catalog['confirmedSelections'],'pendingSelections':catalog['pendingSelections']}
(site/'delivery.json').write_text(json.dumps(delivery,ensure_ascii=False,indent=2),encoding='utf-8')
print('DELIVERY',len(delivery['tracks']),'tracks',len(results),'windows',delivery['wavFiles'],'WAV files; fixed gains by track',flush=True)
