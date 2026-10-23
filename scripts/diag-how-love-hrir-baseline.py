"""Stored HRIR baseline only; does not substitute for effective moving-filter measurements."""
import json
from pathlib import Path
import numpy as np
root=Path(__file__).resolve().parents[1];p=root/'apps/mobile/assets/hrtf-restored/hrtf-dense';m=json.loads((p/'hrtf-set.json').read_text());rows=[]
for pos in m['positions']:
 a=np.fromfile(p/pos['dry'],dtype='<f4').astype(float).reshape(2,-1)
 spec=abs(np.fft.rfft(a,n=8192,axis=1))**2;freq=np.fft.rfftfreq(8192,1/48000)
 row={'azimuth':pos['azimuth'],'elevation':pos['elevation'],'energyDb':float(10*np.log10(np.sum(a*a)))}
 for lo,hi in [(150,500),(500,1500),(1500,4000),(4000,8000)]:row[f'{lo}-{hi}Hz']=float(10*np.log10(np.mean(spec[:,(freq>=lo)&(freq<hi)])))
 rows.append(row)
summary=[]
for el in sorted(set(r['elevation'] for r in rows)):
 a=[r for r in rows if r['elevation']==el];summary.append({'elevation':el,'count':len(a),'medianEnergyDb':float(np.median([r['energyDb'] for r in a])),'minEnergyDb':min(r['energyDb'] for r in a),'maxEnergyDb':max(r['energyDb'] for r in a)})
out=root/'tmp/how-love-diag/masking-audit'
(out/'stored-hrir-audit.json').write_text(json.dumps({'scope':__doc__,'elevationSummary':summary,'directions':rows},indent=2));print(json.dumps(summary,indent=2))
