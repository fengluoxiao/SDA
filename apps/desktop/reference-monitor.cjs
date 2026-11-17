const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto');
// Export-only materialization of the same fixed native coefficients. No song cache.
function readReferenceProfile(root,layout,strength=1){
 const directory=path.join(root,'genelec-reference'),manifest=JSON.parse(fs.readFileSync(path.join(directory,'manifest.json'))),bytes=fs.readFileSync(path.join(directory,'coefficients.f32'));
 if(crypto.createHash('sha256').update(bytes).digest('hex')!==manifest.coefficientsSha256)throw Error('参考监听资源校验失败');
 if(!Number.isFinite(strength)||strength<0||strength>2||!manifest.layouts[layout])throw Error('参考监听布局或强度无效');
 const read=(offset,n)=>Array.from({length:n},(_,i)=>bytes.readFloatLE((offset+i)*4));
 const speakers=manifest.layouts[layout].map(entry=>{
  const r=manifest.responses.find(r=>r.key===entry.key);if(!r)throw Error('Missing reference response');
  const directLeft=read(r.offsetFloats,r.directTaps),directRight=read(r.offsetFloats+r.directTaps,r.directTaps);
  while(directLeft.length<manifest.taps)directLeft.push(0);while(directRight.length<manifest.taps)directRight.push(0);
  const residual=r.offsetFloats+r.directTaps*2,roomLeft=read(residual,manifest.taps).map((v,i)=>directLeft[i]+v*strength),roomRight=read(residual+manifest.taps,manifest.taps).map((v,i)=>directRight[i]+v*strength);
  const peak=a=>a.reduce((best,v,i)=>Math.abs(v)>=Math.abs(a[best])?i:best,0);
  return {name:entry.name,azimuth:r.azimuth,elevation:r.elevation,onsetSample:Math.min(peak(directLeft),peak(directRight)),directLeft,directRight,roomLeft,roomRight};
 });
 return {version:1,name:'8351B 参考监听室',source:'Genelec 8351B normalized directivity + SDA assumed reference room',license:'See GENELEC-REFERENCE-NOTICE.md and original HRTF notices',measurement:'simulated',sampleRate:48000,layout,speakers,simulation:{referenceMonitor:'genelec-8351b',referenceAlgorithm:'genelec8351b-reference-room-v1',sourceModel:manifest.sourceModel,phaseModel:manifest.phaseModel,preserveSelectedDirect:true,referenceStrength:strength,commonResidualGain:manifest.commonResidualGain,roomMetres:[6,5,3.2],imageOrder:2,notPersonalMeasurement:true}};
}
module.exports={readReferenceProfile};
