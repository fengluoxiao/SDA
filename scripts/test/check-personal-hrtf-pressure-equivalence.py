"""Compare actual NumCalc pEvalGrid files, including nulls and phase, by node ID."""
from pathlib import Path
import cmath,json,math,sys

def load(path):
    header,groups,group,*lines=Path(path).read_text().splitlines()
    if header.strip()!='Mesh2HRTF 1.0.0' or int(groups)!=1:raise ValueError('Unexpected pressure header')
    g,n=map(int,group.split());out={}
    for line in lines:
        if not line.strip():continue
        k,a,b=line.split();i=int(k);z=complex(float(a),float(b))
        if i in out or not (math.isfinite(z.real) and math.isfinite(z.imag)):raise ValueError('Repeated or nonfinite pressure')
        out[i]=z
    if g!=1 or not out or len(out)!=n:raise ValueError('Missing pressure nodes')
    return out

ref,actual=map(load,sys.argv[1:]);db=phase=squared=energy=0.
if set(ref)!=set(actual):raise ValueError('Evaluation grids differ')
for k,z in ref.items():
    if not z:
        if actual[k]:raise ValueError('Exact reference null was lost')
        continue
    ratio=actual[k]/z
    db=max(db,abs(20*math.log10(abs(ratio))));phase=max(phase,abs(math.degrees(cmath.phase(ratio))))
    squared+=abs(actual[k]-z)**2;energy+=abs(z)**2
rms=math.sqrt(squared/energy)
print(json.dumps(dict(directions=len(ref),maxMagnitudeDB=db,maxPhaseDegrees=phase,relativeRMS=rms)))
if db>.02 or phase>.1 or rms>1e-5:sys.exit('Complex pressure mismatch')
