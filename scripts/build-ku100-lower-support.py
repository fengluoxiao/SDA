"""Build supplementary -60-degree ring and nadir from the verified SADIE D1 archive.
No EQ or fabricated directions. Preserve interaural timing with common shifts/gains;
apply bilateral symmetry using a common sign/shift for mirrored measurements.
The original 61 assets are read-only. Requires numpy; outputs only the support bank.
"""
import argparse, hashlib, io, json, wave, zipfile
from pathlib import Path
import numpy as np

parser=argparse.ArgumentParser()
parser.add_argument('--archive', type=Path, required=True)
parser.add_argument('--reference', type=Path, required=True)
parser.add_argument('--out', type=Path, required=True)
a=parser.parse_args()
m=json.loads(a.reference.read_text(encoding='utf-8'))
archive_hash=hashlib.sha256(a.archive.read_bytes()).hexdigest()
assert archive_hash == m['source']['archiveSha256'], 'unverified measurement archive'
ref=[np.fromfile(a.reference.parent/p['dry'],dtype='<f4').reshape(2,-1).astype(float) for p in m['positions']]
def energy(x): return float(np.sum(x*x))
def centroid(x): return float(np.sum(np.sum(x*x,axis=0)*np.arange(x.shape[1]))/energy(x))
target_energy=float(np.median([energy(x) for x in ref]))
target_centroid=float(np.median([centroid(x) for x in ref]))
def shift(x,n):
 y=np.zeros_like(x)
 if n>=0 and n<x.shape[1]: y[:,n:]=x[:,:x.shape[1]-n]
 elif n<0 and -n<x.shape[1]: y[:,:n]=x[:,-n:]
 return y
def normalize(x):
 x=shift(x,int(np.floor(target_centroid-centroid(x)+0.5)))
 return x*np.sqrt(target_energy/energy(x))
records=[]
with zipfile.ZipFile(a.archive) as z:
 def load(az,el):
  name=f"D1/D1_HRIR_WAV/48K_24bit/azi_{az%360},0_ele_{el},0.wav"
  data=z.read(name)
  with wave.open(io.BytesIO(data)) as w:
   assert (w.getnchannels(),w.getsampwidth(),w.getframerate())==(2,3,48000)
   b=np.frombuffer(w.readframes(w.getnframes()),dtype=np.uint8).reshape(-1,3).astype(np.int32)
   v=b[:,0]|(b[:,1]<<8)|(b[:,2]<<16);v=np.where(v&0x800000,v-0x1000000,v)
   raw=v.reshape(-1,2).T/8388608.0
  x=np.zeros((2,512));x[:,:raw.shape[1]]=raw
  records.append({'azimuth':az,'elevation':el,'sourcePath':name,'sourceSha256':hashlib.sha256(data).hexdigest()})
  return normalize(x)
 def pair(x,y,center=False):
  mirrored=y[::-1];best=None
  for sign in (1,-1):
   for n in range(-8,9):
    b=shift(mirrored,n)*sign
    score=float(np.sum(x*b)/np.sqrt(energy(x)*energy(b)))
    if best is None or score>best[0]:best=(score,b)
  out=(x+best[1])*0.5
  if center:out=np.stack([out[0],out[0]])
  return normalize(out)
 bank={}
 for az in (0,30,60,90,120,150,180):
  x=load(az,-60)
  if az in (0,180):bank[(az if az<180 else -180,-60)]=pair(x,x,True)
  else:
   y=load(-az,-60);v=pair(x,y);bank[(az,-60)]=v;bank[(-az,-60)]=v[::-1]
 x=load(0,-90);bank[(0,-90)]=pair(x,x,True)
keys=[(az,-60) for az in range(-180,180,30)]+[(0,-90)]
blob=b''.join(bank[k].astype('<f4').tobytes() for k in keys)
a.out.mkdir(parents=True,exist_ok=True)
(a.out/'lower-support.f32').write_bytes(blob)
(a.out/'provenance.json').write_text(json.dumps({'source':m['source'],'referenceManifestSha256':hashlib.sha256(a.reference.read_bytes()).hexdigest(),'sampleRate':48000,'tapCountPerEar':512,'layout':'direction-major, left then right float32 little endian','positions':[{'azimuth':k[0],'elevation':k[1]} for k in keys],'processing':{'targetStereoEnergy':target_energy,'targetEnergyCentroidSamples':target_centroid,'bilateralSymmetry':'common sign and integer shift for mirror pairs; equal ears at median plane','level':'common scalar only','eq':False},'measurements':records,'sha256':hashlib.sha256(blob).hexdigest()},indent=2)+'\n',encoding='utf-8')
(a.out/'LICENSE-SADIE.txt').write_bytes((a.reference.parent/'LICENSE-SADIE.txt').read_bytes())
print('Built',len(keys),'measured lower responses;',len(blob),'bytes')
