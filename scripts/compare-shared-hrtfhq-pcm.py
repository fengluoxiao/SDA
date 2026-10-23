"""Fail CI if the shared engine diverges across the four actual target runtimes."""
import array,hashlib,json,math,sys
from pathlib import Path
root=Path(sys.argv[1]);names=['hrtfhq-windows-2025','hrtfhq-macos-26','hrtfhq-ios-simulator','hrtfhq-android'];rows=[]
def read(path):
 data=path.read_bytes();assert len(data)==98304*2*4
 x=array.array('f');x.frombytes(data)
 if sys.byteorder!='little':x.byteswap()
 assert all(math.isfinite(v) for v in x)
 return data,x
for layout in ['atmos','360ra','bed714','stereo']:
 reference_bytes,reference=read(root/names[0]/(layout+'.f32'));energy=math.fsum(v*v for v in reference)
 for name in names[1:]:
  data,x=read(root/name/(layout+'.f32'));differences=[a-b for a,b in zip(x,reference)];maximum=max(map(abs,differences));residual=10*math.log10(max(math.fsum(v*v for v in differences)/energy,1e-30))
  row={'layout':layout,'platform':name,'maximumSampleError':maximum,'residualDb':residual,'byteIdentical':data==reference_bytes,'sha256':hashlib.sha256(data).hexdigest(),'passed':maximum<=2e-6 and residual<=-100};rows.append(row)
report={'scope':'Same synthetic source PCM, pose/object trajectory, profile and controls; rendered inside Windows, macOS, iOS simulator and Android. Engine output before hardware effects.','maximumSampleErrorTolerance':2e-6,'residualDbTolerance':-100,'comparisons':rows,'passed':all(r['passed'] for r in rows)}
(root/'parity-report.json').write_text(json.dumps(report,indent=2),encoding='utf-8');print(json.dumps(report,indent=2));assert report['passed'],'Cross-platform PCM mismatch'
