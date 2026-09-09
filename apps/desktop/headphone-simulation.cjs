// Target colour is inverse correction; playback correction is applied first.
// Published EQs may use different rigs/targets, so this is a timbre approximation.
const FFT=require('fft.js');
const SIZE=16384,TAPS=8192;
function spectrum(bytes,fft) {
  const input=new Float64Array(SIZE);
  if(!bytes)input[0]=1;
  else {
    if(bytes.length%4||bytes.length/4>TAPS)throw new Error('Invalid headphone FIR size');
    for(let i=0;i<bytes.length/4;i++){input[i]=bytes.readFloatLE(i*4);if(!Number.isFinite(input[i]))throw new Error('Invalid headphone FIR sample');}
  }
  const output=fft.createComplexArray();fft.realTransform(output,input);fft.completeSpectrum(output);
  return Array.from({length:SIZE/2+1},(_,i)=>20*Math.log10(Math.max(1e-6,Math.hypot(output[i*2],output[i*2+1]))));
}
function build(source,target) {
  if(source&&source.equals(target)){const bytes=Buffer.alloc(TAPS*4);bytes.writeFloatLE(1,0);return {bytes,preamp:1,peakDb:0};}
  const fft=new FFT(SIZE),src=spectrum(source,fft),dst=spectrum(target,fft);
  const db=src.map((v,i)=>v-dst[i]);
  const sample=f=>{const index=Math.max(0,Math.min(SIZE/2,f*SIZE/48000));const lo=Math.floor(index),hi=Math.min(SIZE/2,lo+1);return db[lo]+(db[hi]-db[lo])*(index-lo);};
  const reference=sample(1000),log=fft.createComplexArray();
  for(let i=0;i<=SIZE/2;i++) {
    const f=i*48000/SIZE;
    let value=0;for(let k=-4;k<=4;k++)value+=sample(f*2**(k/48));
    value=Math.max(-12,Math.min(12,value/9-reference));
    // Do not extrapolate unreliable sub-bass or high-treble correction.
    value*=Math.min(1,f/20)*Math.max(0,Math.min(1,(20000-f)/8000));
    log[i*2]=value*Math.LN10/20;
    if(i>0&&i<SIZE/2)log[(SIZE-i)*2]=log[i*2];
  }
  const cepstrum=fft.createComplexArray();fft.inverseTransform(cepstrum,log);
  for(let i=1;i<SIZE;i++){cepstrum[2*i]*=i<SIZE/2?2:i===SIZE/2?1:0;cepstrum[2*i+1]=0;}
  const minimum=fft.createComplexArray();fft.transform(minimum,cepstrum);
  for(let i=0;i<SIZE;i++){const mag=Math.exp(minimum[2*i]),phase=minimum[2*i+1];minimum[2*i]=mag*Math.cos(phase);minimum[2*i+1]=mag*Math.sin(phase);}
  const impulse=fft.createComplexArray();fft.inverseTransform(impulse,minimum);
  const bytes=Buffer.alloc(TAPS*4);
  for(let i=0;i<TAPS;i++){
    const fade=i<TAPS-512?1:.5*(1+Math.cos(Math.PI*(i-(TAPS-512))/511));
    const v=impulse[2*i]*fade;if(!Number.isFinite(v))throw new Error('Headphone simulation synthesis failed');bytes.writeFloatLE(v,i*4);
  }
  const peakDb=Math.max(...spectrum(bytes,fft));
  return {bytes,peakDb,preamp:10**(-Math.max(0,peakDb+.2)/20)};
}
function simulate(sourceLeft,sourceRight,targetLeft,targetRight) {
  const left=build(sourceLeft,targetLeft),right=build(sourceRight,targetRight);
  // Match 20 Hz–20 kHz pink-noise energy rather than attenuating the whole
  // programme by the largest narrow-band boost. The native final linked
  // look-ahead peak guard still protects actual programme peaks.
  const fft=new FFT(SIZE),l=spectrum(left.bytes,fft),r=spectrum(right.bytes,fft);
  let energy=0,weight=0;
  for(let i=1;i<=SIZE/2;i++) {
    const frequency=i*48000/SIZE;if(frequency<20||frequency>20000)continue;
    const w=1/frequency;energy+=w*(10**(l[i]/10)+10**(r[i]/10))*.5;weight+=w;
  }
  const levelDb=Math.max(-12,Math.min(6,-10*Math.log10(energy/weight)));
  return {left:left.bytes,right:right.bytes,preamp:10**(levelDb/20)};
}
module.exports={simulate};
