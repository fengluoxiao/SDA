"""Keep original metadata/HRTF; test foreground masking, never a production preset."""
import json
from pathlib import Path
import numpy as np
p=Path(__file__).resolve().parents[1]/'tmp/how-love-diag'
frames=[json.loads(l) for l in (p/'full/frames.jsonl').read_text().splitlines()]
for name,mode in [('foreground-only','solo'),('foreground-minus6','reduce')]:
 out=p/name;out.mkdir(exist_ok=True)
 a=np.fromfile(p/'full/sources.pcm',dtype='<f4');offset=0
 for f in frames:
  n=f['samples'];v=a[offset:offset+16*n].reshape(16,n);offset+=16*n
  if mode=='solo':v[3:]=0 # keep LFE and Obj10/11; complement of support-group
  else:v[1:3]*=10**(-6/20) # unchanged LFE, no change to target support group
 assert offset==len(a)
 a.tofile(out/'sources.pcm');(out/'frames.jsonl').write_text((p/'full/frames.jsonl').read_text())
 (out/'scope.json').write_text(json.dumps({'diagnosticOnly':True,'mode':mode,'positions':'unchanged','inputGainChange':{'Obj10':-6,'Obj11':-6} if mode=='reduce' else None}))
