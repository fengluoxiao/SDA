"""Development-only NumPy/upstream reference generation; never shipped to iOS."""
from pathlib import Path
import sys,copy,json
import numpy as np
from mesh2hrtf.Output2HRTF.reference_hrtfs import reference_hrtfs
from mesh2hrtf.Output2HRTF.compute_hrirs import compute_hrirs
from types import SimpleNamespace
bins=256;fs=48000;n=2*bins;frequency=np.arange(1,bins+1)*fs/n;radius=1.2;areas=[2.3e-5,3.1e-5];pressures=[]
for direction in range(3):
 ears=[]
 for ear in range(2):
  gain=.6+.2*ear+.07*direction;delay=(-8 if ear else 6)+2*direction
  # Deliberately different binaural level, phase and spectral shape.
  h=gain*(1+.18*np.cos(2*np.pi*frequency/fs))*np.exp(1j*2*np.pi*frequency*delay/fs)
  ps=-1j*1.1839*2*np.pi*frequency*(.1*areas[ear])/(4*np.pi)*np.exp(1j*2*np.pi*frequency/343*radius)/radius
  ears.append(h*ps)
 pressures.append(ears)
p=np.asarray(pressures)
class SofaFixture(SimpleNamespace):
 def copy(self):return copy.copy(self)
sofa=SofaFixture(GLOBAL_SOFAConventions='SimpleFreeFieldHRTF',SourcePosition=np.array([[0,0,radius],[90,0,radius],[180,0,radius]]),SourcePosition_Type='spherical',Data_Real=p.real,Data_Imag=p.imag,N=frequency,ReceiverPosition=np.array([[0,.09,0],[0,-.09,0]]),GLOBAL_ApplicationVersion='1.3.0')
referenced=reference_hrtfs(sofa,'Both ears',areas,343,1.1839)
expected=compute_hrirs(referenced,32).Data_IR
fixture=Path(__file__).resolve().parent/'fixtures/personal-hrtf-sphere'
np.stack([p.real,p.imag],axis=-1).astype('<f8').tofile(fixture/'fft-pressure-f64.bin')
np.asarray(expected,dtype='<f4').tofile(fixture/'fft-expected-f32.bin')
(fixture/'fft-provenance.json').write_text(json.dumps({'source':'unmodified Mesh2HRTF 1.3.0 reference_hrtfs + compute_hrirs','directions':3,'bins':bins,'samplingRate':fs,'sourceArea':areas,'radius':radius,'circularShift':32,'syntheticOnly':True},indent=2))
print('Created independent upstream reference for local HRIR conversion',expected.shape)
