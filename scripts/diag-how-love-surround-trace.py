"""Diagnostic only: cross-validated linear reconstruction of core surrounds from JOC PCM."""
import json, wave
from pathlib import Path
import numpy as np
from scipy import signal
root=Path(__file__).resolve().parents[1]/'tmp/how-love-diag'
out=root/'surround-trace';out.mkdir(exist_ok=True)
f=[json.loads(l) for l in (root/'frames.jsonl').read_text().splitlines()]
r=np.fromfile(root/'sources.f32',dtype='<f4'); blocks=[];off=0
for a in f:
 n=a['samples'];blocks.append(r[off:off+n*16].reshape(16,n));off+=n*16
x=np.concatenate(blocks,axis=1).T.astype(float)
core=np.memmap(root/'core-reference/core-0-70.f32',dtype='<f4',mode='r').reshape(-1,6)
base=f[0]['start']; start=55*48000-base;end=63*48000-base
# LFE alignment found independently. JOC filterbank may introduce an additional delay.
y=np.asarray(core[base-3217:base-3217+len(x),4:6],dtype=float)
# Determine broadband lag using total non-LFE JOC vs sum of core surrounds.
sos=signal.butter(3,[300,6000],btype='bandpass',fs=48000,output='sos')
xx=signal.sosfilt(sos,x[:,1:],axis=0);yy=signal.sosfilt(sos,y,axis=0)
search=[]
for j in range(15):
 for c in range(2):
  cc=signal.correlate(xx[start:end,j],yy[start:end,c],mode='full',method='fft')
  middle=end-start-1; sl=cc[middle-2048:middle+2049];k=int(np.argmax(abs(sl)))-2048
  search.append({'obj':j+10,'side':c,'lag':k,'strength':float(abs(sl[k+2048])/np.sqrt(np.sum(xx[start:end,j]**2)*np.sum(yy[start:end,c]**2)))})
search.sort(key=lambda a:a['strength'],reverse=True)
# Choose common lag by training-set reconstruction error, not one row's peak.
Y=y[start:end]
train=(np.arange(len(Y))//12000)%2==0
lag_trials=[]
for candidate in range(-8,9):
 Xc=x[start+candidate:end+candidate,1:]
 cc=np.linalg.lstsq(Xc[train],Y[train],rcond=None)[0]
 err=float(np.mean((Xc[train]@cc-Y[train])**2))
 lag_trials.append((err,candidate))
lag=min(lag_trials)[1]
X=x[start+lag:end+lag,1:]
coef=np.linalg.lstsq(X[train],Y[train],rcond=None)[0];pred=X@coef
metrics=[]
for c in range(2):
 t=Y[~train,c];p=pred[~train,c]
 metrics.append({'side':['SL','SR'][c],'heldout_correlation':float(np.corrcoef(t,p)[0,1]),'heldout_residual_db_relative':float(20*np.log10(np.linalg.norm(t-p)/np.linalg.norm(t))), 'coefficients':{str(i+10):float(coef[i,c]) for i in range(15)}})
# One common audition gain, no individual normalization of reconstruction/residual.
gain=min(10**(12/20),.9/max(np.max(abs(Y)),np.max(abs(pred)),np.max(abs(Y-pred))))
def wav(name,a):
 a=a*gain; b=np.rint(a.reshape(-1)*8388607).astype('<i4').view(np.uint8).reshape(-1,4)[:,:3].copy()
 with wave.open(str(out/name),'wb') as w:
  w.setparams((2,3,48000,0,'NONE','not compressed'));w.writeframes(b.tobytes())
wav('A-core-surround-reference.wav',Y);wav('B-JOC-linear-reconstruction.wav',pred);wav('C-reconstruction-residual.wav',Y-pred)
report={'scope':'Diagnostic fit, not production downmix or vocal extraction. Broadband results do not establish target vocal identity. Alternating 250ms held-out chunks; shared audition gain.', 'core_alignment_samples':-3217,'additional_joc_lag_samples':lag,'lag_candidates':search[:16],'audition_gain_db':float(20*np.log10(gain)),'metrics':metrics}
(out/'report.json').write_text(json.dumps(report,indent=2))
print(json.dumps(report,indent=2))
