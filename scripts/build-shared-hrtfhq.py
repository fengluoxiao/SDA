"""Build the shared response bank from synthetic public HRTFHQ measurements.
No song audio, fitted source gains, per-direction normalization, or Apple runtime dependency.
"""
import argparse,hashlib,json,math,struct
from pathlib import Path

def sha(data):return hashlib.sha256(data).hexdigest()
def build():
 ap=argparse.ArgumentParser(description=__doc__);ap.add_argument('--capture',type=Path,required=True);ap.add_argument('--calibration',type=Path,required=True);ap.add_argument('--out',type=Path);a=ap.parse_args()
 root=Path(__file__).resolve().parents[1];out=a.out or root/'assets/hrtf/apple-hrtfhq-v1'
 report=json.loads((a.capture/'report.json').read_text());cal=json.loads(a.calibration.read_text())
 assert report['sampleRate']==48000 and report['outputType']=='headphones' and report['reverbEnabled'] is False
 assert report['stationarityPassed'] and report['linearityPassed'] and report['sourceCountPassed']
 assert cal['commonGain']>0 and math.isfinite(cal['commonGain']);delay=cal['commonDelaySamples'];assert 0<=delay<256
 measurements={(r['azimuth'],r['elevation']):r for r in report['directions']};assert len(measurements)==len(report['directions'])
 reference=root/'apps/mobile/ios-experiments/fullheight-combined/hrtf-dense'
 geometries={'hrtf-dense':json.loads((reference/'hrtf-set.json').read_text())['positions'],'hrtf':json.loads((root/'apps/mobile/assets/hrtf-restored/hrtf/hrtf-set.json').read_text())['positions']}
 source_records={};provenance={'scope':'Public macOS HRTFHQ synthetic transfer measurements, replayed by shared SDA convolution on all platforms. Not Apple Music, personalized Spatial Audio or a live Apple API.','captureOs':report['os'],'captureArchitecture':report['architecture'],'captureReportSha256':sha((a.capture/'report.json').read_bytes()),'calibration':cal,'responses':source_records}
 for family,positions in geometries.items():
  dest=out/family;dest.mkdir(parents=True,exist_ok=True);rows=[]
  for i,p in enumerate(positions):
   row=measurements[(p['azimuth'],p['elevation'])];assert row['algorithmRequested']==row['algorithmReadback']==6
   assert row['repeatResidualDb']<=-100 and row['tailEnergyDb']<=-100
   original=(a.capture/row['file']).read_bytes();values=struct.unpack('<'+'f'*(len(original)//4),original)
   assert len(values)==report['kernelFrames']*2 and all(math.isfinite(v) for v in values)
   # An exact zero-tail truncation, not an approximate shorter HRTF.
   assert all(v==0 for v in values[(512-delay)*2:]),'Response exceeds512tap storage'
   ears=[[0.0]*delay+[v*cal['commonGain'] for v in values[ear:(512-delay)*2:2]] for ear in [0,1]]
   packed=struct.pack('<1024f',*(ears[0]+ears[1]));name=f'direction-{i:03d}.f32';(dest/name).write_bytes(packed)
   rows.append({'azimuth':p['azimuth'],'elevation':p['elevation'],'dry':name,'wet':name,'assets':{'dry':{'sha256':sha(packed)},'wet':{'sha256':sha(packed)}},'measurement':{'sourceFile':row['file'],'sourceSha256':sha(original)}})
   source_records[row['file']]={'azimuth':row['azimuth'],'elevation':row['elevation'],'sha256':sha(original)}
  manifest={'schemaVersion':2,'measuredResponseVersion':2,'sharedDepthVersion':1,'sampleRate':48000,'subjectId':'apple-public-hrtfhq','completeSubject':True,'azimuthConvention':'positive azimuth left, positive elevation up','processing':{'calibrated':True,'preserveMeasurements':True,'historicalInterpolation':False,'mobileDirectOnly':False,'spatialCues':False,'frontEarIsolation':False,'dryTapLimit':512,'wetTapLimit':512,'runtimeEnergyNormalization':False,'note':'Common front-impulse calibration only. Preserve measured direction levels, ear ratios, phase and waveform; no KU100-specific processing.'},'levelMatchDb':0,'positions':rows,'calibration':cal,'source':{'name':'Apple public AVAudioEnvironmentNode HRTFHQ response measurements','os':report['os'],'reportSha256':provenance['captureReportSha256']},'sharedRenderProfile':'apple-hrtfhq-542-v1'}
  if family=='hrtf-dense':
   data=(reference/'triangle-grid.json').read_bytes();grid=json.loads(data);assert grid['positions']==[{k:p[k] for k in ['azimuth','elevation']} for p in rows]
   (dest/'triangle-grid.json').write_bytes(data);manifest['triangleSupportFile']='triangle-grid.json'
  (dest/'hrtf-set.json').write_text(json.dumps(manifest,indent=2)+'\n',encoding='utf-8')
 (out/'provenance.json').write_text(json.dumps(provenance,indent=2)+'\n',encoding='utf-8')
 print(json.dumps({'out':str(out),'dense':len(geometries['hrtf-dense']),'fallback':len(geometries['hrtf']),'commonGain':cal['commonGain'],'commonDelaySamples':delay},indent=2))
if __name__=='__main__':build()
