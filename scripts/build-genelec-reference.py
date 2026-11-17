"""Build fixed reference-room residuals; never runs during music playback.

The manufacturer's normalized angular magnitudes are combined with an assumed
reference room. Minimum phase and wall absorption are simulation assumptions.
The selected direct HRTF is preserved; this is not a measured Genelec room/GLM.
"""
import argparse,base64,csv,gzip,hashlib,itertools,json,math
from pathlib import Path
import numpy as np
def fftconvolve(a,b):
    length=len(a)+len(b)-1;size=1<<(length-1).bit_length()
    return np.fft.irfft(np.fft.rfft(a,size)*np.fft.rfft(b,size),size)[:length]

ROOT=Path(__file__).resolve().parents[1]
RATE=48000;TAPS=4608;FILTER_TAPS=1024

def sha(b):return hashlib.sha256(b).hexdigest()
def source_from_csv(path):
    groups={}
    for row in csv.DictReader(Path(path).open(encoding='utf-8')):
        groups.setdefault(int(row['response_index']),[]).append(row)
    if sorted(groups)!=list(range(107)):raise ValueError('Expected the verified 8351B 107-response source')
    frequencies=np.array([float(r['frequency_hz'])for r in groups[0]])
    if len(frequencies)!=241 or not np.all(np.diff(frequencies)>0):raise ValueError('Invalid frequency axis')
    levels=np.array([[float(r['level_db'])for r in groups[i]]for i in range(107)])
    if levels.shape!=(107,241)or not np.isfinite(levels).all()or not np.all(levels[0]==0):raise ValueError('Invalid normalized source')
    # GLL stores full first meridian, then meridians without duplicate poles.
    # The generic CSV export's row-derived coordinates do NOT account for this.
    grid=np.empty((3,37,241));grid[0]=levels[:37]
    for m in [1,2]:
        grid[m,0]=levels[0];grid[m,36]=levels[36]
        grid[m,1:36]=levels[37+(m-1)*35:37+m*35]
    return {'schema':1,'model':'Genelec 8351B','kind':'normalized-angular-magnitude',
      'sourceGllSha256':'41f01aa835cdcf03c57b81ad5eccbbfcd60eb37a21ce7ac32b770ffec67c743d',
      'sourceUrl':'https://assets.ctfassets.net/4zjnzn055a4v/1WcRAwuQu6hUCHa9UT990V/8e16d8615c0befd4d3ceded84f178aae/Genelec_8351B.gll',
      'copyright':'2023 Genelec','sourceControls':'all tone controls off',
      'validBandwidthHz':[50,16000],'frequencyHz':frequencies.tolist(),
      'meridiansDeg':[0,90,180],'parallelStepDeg':5,'symmetry':'horizontal',
      'ordering':'meridian-major, shared front/back poles; rebuilt independently of CSV angle columns',
      'phaseModel':'minimum-phase approximation from normalized magnitude; not absolute measured system phase',
      'levelsDb':grid.tolist()}

class Source:
    def __init__(self,data):
        self.data=data;self.freq=np.array(data['frequencyHz']);self.level=np.array(data['levelsDb'])
        if self.level.shape!=(3,37,241)or not np.isfinite(self.level).all():raise ValueError('Invalid source grid')
    def db(self,forward,emission):
        forward=np.asarray(forward,dtype=float);forward/=np.linalg.norm(forward)
        emission=np.asarray(emission,dtype=float);emission/=np.linalg.norm(emission)
        theta=math.degrees(math.acos(np.clip(np.dot(forward,emission),-1,1)))/5
        up=np.array([0.,0.,1.]);up-=np.dot(up,forward)*forward
        if np.linalg.norm(up)<1e-8:up=np.array([0.,1.,0.]);up-=np.dot(up,forward)*forward
        up/=np.linalg.norm(up);lateral=np.cross(up,forward)
        psi=math.degrees(math.atan2(np.dot(emission,up),np.dot(emission,lateral)))%360
        # Mirror across horizontal source plane: published horizontal symmetry.
        psi=(psi-90)%360;psi=360-psi if psi>180 else psi
        mi=min(1,int(psi//90));mt=(psi-mi*90)/90;pi=min(35,int(theta));pt=theta-pi
        a=(1-pt)*self.level[mi,pi]+pt*self.level[mi,pi+1]
        b=(1-pt)*self.level[mi+1,pi]+pt*self.level[mi+1,pi+1]
        return (1-mt)*a+mt*b

def magnitude_fir(frequency,db):
    size=4096;f=np.fft.rfftfreq(size,1/RATE)
    # Outside nominal validity hold the nearest valid magnitude. No claim of
    # an absolute 8351B low-frequency rolloff is manufactured from this file.
    query=np.clip(f,50,16000)
    log_amplitude=np.interp(np.log(query),np.log(frequency),db)*math.log(10)/20
    cep=np.fft.irfft(log_amplitude,n=size);minimum=np.zeros(size);minimum[0]=cep[0]
    minimum[1:size//2]=2*cep[1:size//2];minimum[size//2]=cep[size//2]
    impulse=np.fft.irfft(np.exp(np.fft.rfft(minimum)),n=size)[:FILTER_TAPS]
    impulse[-128:]*=0.5+0.5*np.cos(np.linspace(0,np.pi,128))
    target=math.exp(float(log_amplitude[0]));actual=float(impulse.sum())
    if abs(actual)>1e-12:impulse*=target/actual
    if not np.isfinite(impulse).all():raise ValueError('Invalid minimum-phase filter')
    return impulse

def source_position(az,el,listener,radius=1.2):
    a,e=np.deg2rad([az,el]);return listener+radius*np.array([-np.sin(a)*np.cos(e),np.cos(a)*np.cos(e),np.sin(e)])

def receiver_direction(v):
    v=v/np.linalg.norm(v);return np.array([v[0],v[1],v[2]])

def build(args):
    directory=Path(args.output);directory.mkdir(parents=True,exist_ok=True)
    source_path=directory/'8351b-directivity.json.gz'
    if args.source_csv:
        source_data=source_from_csv(args.source_csv);source_path.write_bytes(gzip.compress(json.dumps(source_data,separators=(',',':')).encode(),mtime=0))
    else:source_data=json.loads(gzip.decompress(source_path.read_bytes()))
    source=Source(source_data)
    packed=json.loads(gzip.decompress(Path(args.receiver).read_bytes()));receiver=packed['manifest']
    directions=[];responses=[]
    for p in receiver['positions']:
        a,e=np.deg2rad([p['azimuth'],p['elevation']]);directions.append([-np.sin(a)*np.cos(e),np.cos(a)*np.cos(e),np.sin(e)])
        responses.append(np.frombuffer(base64.b64decode(packed['files'][p['dry']]),dtype='<f4').reshape(2,-1).astype(float))
    directions=np.array(directions)
    layouts=json.loads(Path(args.layouts).read_text())['layouts'];direct_bank=np.fromfile(args.direct_bank,dtype='<f4')
    size=np.array([6.,5.,3.2]);listener=np.array([3.,2.5,1.2]);freq=source.freq
    # Explicit reference-room absorption assumptions at low/mid/high bands.
    bands=np.array([50,125,250,500,1000,2000,4000,8000,16000])
    wall=np.array([.15,.24,.45,.68,.78,.83,.85,.86,.86]);floor=np.array([.05,.08,.16,.3,.5,.62,.7,.75,.75]);ceiling=np.array([.2,.3,.55,.75,.84,.87,.88,.9,.9])
    surfaces=[wall,wall,wall,wall,floor,ceiling]
    unique={};mapped={}
    for layout,speakers in layouts.items():
        mapped[layout]=[]
        for s in speakers:
            n=s['samplesPerEar'];o=s['offsetFloats'];direct=direct_bank[o:o+2*n].reshape(2,n).astype(float)
            key=f"{s['azimuth']:.4f},{s['elevation']:.4f}:"+sha(direct.astype('<f4').tobytes())[:12]
            mapped[layout].append({'name':s['name'],'key':key})
            if key in unique:continue
            position=source_position(s['azimuth'],s['elevation'],listener);forward=listener-position;radius=float(np.linalg.norm(forward));residual=np.zeros((2,TAPS))
            for index in itertools.product(range(-2,3),repeat=3):
                order=sum(abs(i)for i in index)
                if not 1<=order<=2:continue
                image=np.array([i*length+(p if i%2==0 else length-p)for i,length,p in zip(index,size,position)])
                ray=image-listener;distance=float(np.linalg.norm(ray));fold=np.array([1 if i%2==0 else-1 for i in index]);emission=(-ray)*fold
                db=source.db(forward,emission).copy()
                for axis,i in enumerate(index):
                    if not i:continue
                    high=(abs(i)+1)//2 if i>0 else abs(i)//2;low=abs(i)-high
                    for side,count in [(0,low),(1,high)]:
                        if not count:continue
                        absorption=surfaces[axis*2+side] if axis<2 else surfaces[4+side]
                        alpha=np.interp(np.log(freq),np.log(bands),absorption)
                        db+=count*10*np.log10(1-alpha)
                ir=magnitude_fir(freq,db);h=responses[int(np.argmax(directions@receiver_direction(ray)))]
                offset=(distance-radius)/343*RATE;integer=int(math.floor(offset));fraction=offset-integer
                k=np.arange(-16,17);sinc=np.sinc(k-fraction)*np.hanning(33);sinc/=sinc.sum()
                for ear in range(2):
                    v=fftconvolve(fftconvolve(h[ear],ir),sinc)*(radius/distance);start=integer-16
                    if start<0:raise ValueError('Noncausal reflection')
                    end=start+len(v)
                    if end>TAPS and np.max(np.abs(v[TAPS-start:]))>1e-7:raise ValueError('Reflection tail exceeds fixed budget')
                    count=min(len(v),TAPS-start)
                    if count>0:residual[ear,start:start+count]+=v[:count]
            unique[key]={'azimuth':s['azimuth'],'elevation':s['elevation'],'direct':direct,'residual':residual}
    # One common scale for the entire bank; no ear/source/direction normalization.
    scale=.35;probes=np.geomspace(20,500,128);worst={}
    response=lambda x: x@np.exp(-2j*np.pi*np.arange(x.shape[1])[:,None]*probes/RATE)
    pairs=[(response(v['direct']),response(v['residual']))for v in unique.values()]
    for _ in range(160):
        changes=np.array([10*np.log10(np.maximum(np.sum(abs(d+scale*r)**2,axis=0),1e-30)/np.maximum(np.sum(abs(d)**2,axis=0),1e-30))for d,r in pairs])
        energy=[scale**2*np.sum(v['residual']**2)/max(np.sum(v['direct']**2),1e-30)for v in unique.values()]
        # A room contains real interference. Calibrate octave-band energy,
        # rather than flattening every narrow bin/HRTF notch into an EQ target.
        band_changes=[]
        for d,r in pairs:
            for lo,hi in [(20,45),(45,90),(90,180),(180,360),(360,500)]:
                keep=(probes>=lo)&(probes<hi)
                band_changes.append(10*np.log10(np.sum(abs(d[:,keep]+scale*r[:,keep])**2)/np.sum(abs(d[:,keep])**2)))
        if min(band_changes)>=-1 and max(band_changes)<=3 and max(energy)<=.1:break
        scale*=.95
    else:raise ValueError('Reference-room calibration failed')
    manifest={'version':1,'sampleRate':RATE,'taps':TAPS,'model':'Genelec 8351B reference room','sourceModel':'genelec-8351b-normalized-directivity','preserveSelectedDirect':True,'directivitySha256':sha(source_path.read_bytes()),'receiverSha256':receiver['receiverSha256'],'phaseModel':source_data['phaseModel'],'room':{'metres':size.tolist(),'listenerMetres':listener.tolist(),'speakerRadiusMetres':1.2,'imageOrder':2,'absorptionFrequencyHz':bands.tolist(),'wallAbsorption':wall.tolist(),'floorAbsorption':floor.tolist(),'ceilingAbsorption':ceiling.tolist(),'waveModes':False},'commonResidualGain':scale,'calibration':{'scope':'native direct plus reference residual, 20-500 Hz, stereo energy','minimumNarrowbandChangeDb':float(changes.min()),'maximumNarrowbandChangeDb':float(changes.max()),'minimumOctaveChangeDb':float(min(band_changes)),'maximumOctaveChangeDb':float(max(band_changes)),'maximumResidualEnergyRatio':float(max(energy))},'layouts':mapped,'responses':[]}
    parts=[];offset=0
    for key,v in unique.items():
        d=v['direct'].astype('<f4');r=(v['residual']*scale).astype('<f4');manifest['responses'].append({'key':key,'azimuth':v['azimuth'],'elevation':v['elevation'],'offsetFloats':offset,'directTaps':d.shape[1],'residualTaps':TAPS});parts.extend([d.tobytes(),r.tobytes()]);offset+=d.size+r.size
    data=b''.join(parts);manifest['coefficientsSha256']=sha(data);(directory/'coefficients.f32').write_bytes(data);(directory/'manifest.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n')
    print(json.dumps({'responses':len(unique),'layouts':len(mapped),'coefficientBytes':len(data),'gain':scale,'calibration':manifest['calibration']},indent=2))

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--source-csv');p.add_argument('--layouts',required=True);p.add_argument('--direct-bank',required=True);p.add_argument('--receiver',default=str(ROOT/'assets/hrtf/apple-hrtfhq-v1/room-receiver.json.gz'));p.add_argument('--output',default=str(ROOT/'apps/native-renderer/assets/genelec-reference'));build(p.parse_args())
