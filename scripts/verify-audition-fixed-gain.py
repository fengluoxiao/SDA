from pathlib import Path
import json,hashlib,warnings,argparse
import numpy as np
from scipy.io import wavfile
parser=argparse.ArgumentParser(description='Verify fixed-gain audition metadata, hashes and exact Very First Night overlaps.')
parser.add_argument('--root',type=Path,required=True)
args=parser.parse_args()
R=args.root.resolve();d=json.loads((R/'site/delivery.json').read_text(encoding='utf-8'));assert d['normalizationVersion']==2 and d['gainScope']=='track'
checked=[];hashes=0;max_peak=-999.;gains={}
for w in d['windows']:
 assert w['normalizationVersion']==2 and w['gainScope']=='track'
 plan=d['gainPlans'][w['track']];assert w['normalizationPlanKey']==plan['key']
 for p in w['profiles']:
  for mode,rel in p['files'].items():
   file=R/'site'/rel;assert hashlib.sha256(file.read_bytes()).hexdigest()==p['measurements'][mode]['sha256'];hashes+=1
   key=(w['track'],p['id'],mode)
   if key in gains:assert gains[key]==p['gainDb'][mode]
   gains[key]=p['gainDb'][mode];max_peak=max(max_peak,p['measurements'][mode]['truePeakDb'])
windows=[w for w in d['windows'] if w['track']=='very-first'];long=next(w for w in windows if w['start']==0 and w['end']==65)
for short in windows:
 if short is long:continue
 for lp in long['profiles']:
  sp=next((p for p in short['profiles'] if p['id']==lp['id']),None)
  if sp is None:continue
  for mode,rel in sp['files'].items():
   if mode not in lp['files']:continue
   with warnings.catch_warnings():
    warnings.simplefilter('ignore');rate,a=wavfile.read(R/'site'/lp['files'][mode]);rate2,b=wavfile.read(R/'site'/rel)
   assert rate==rate2==48000
   part=a[short['start']*rate:short['end']*rate]
   assert part.shape==b.shape and np.array_equal(part,b),(sp['id'],mode,short['start'],short['end'])
   checked.append({'profile':sp['id'],'mode':mode,'shortWindow':[short['start'],short['end']],'longWindow':[0,65],'sampleIdentical':True,'maxPcmDifference':0})
assert len(checked)==31 and hashes==235
error=max(max(p['referenceLevelErrorLU'].values()) for p in d['gainPlans'].values());assert error<.001 and max_peak<=-1.49999
report={'normalizationVersion':2,'gainScope':'track','verifiedWavHashes':hashes,'overlapComparisons':checked,'allOverlapsSampleIdentical':True,'sameTrackGainsIdentical':True,'maxReferenceLoudnessErrorLU':error,'max4xPeakDb':max_peak,'veryFirstAnchor':d['gainPlans']['very-first']['anchor'],'acceptedTracks':list(d['confirmedSelections']),'pendingTracks':d['pendingSelections']}
(R/'fixed-gain-verification.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n',encoding='utf-8');print(json.dumps({k:v for k,v in report.items() if k!='overlapComparisons'},ensure_ascii=False,indent=2))
