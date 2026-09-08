import assert from "node:assert/strict";
import { AlacResampler } from "../src/alac-resampler.ts";

function frame(channels, samplePos, sampleRate) {
  return {codec:"alac",channels,samplePos,sampleRate,labels:channels.map((_,i)=>i ? "R" : "L"),rawBedLabels:["L","R"],events:[],objectChannels:[],programLoudness:null,rampDuration:0};
}
async function convert(rate, length, chunk, frequency=997) {
  const converter=new AlacResampler(48000), output=[[],[]];
  let next=0;
  const collect=f=>{if(!f)return;assert.equal(f.sampleRate,48000);assert.equal(f.samplePos,next);next+=f.channels[0].length;f.channels.forEach((c,i)=>output[i].push(...c));};
  for(let at=0;at<length;at+=chunk){
    const n=Math.min(chunk,length-at);
    collect(await converter.push(frame([
      Float32Array.from({length:n},(_,i)=>.5*Math.sin(2*Math.PI*frequency*(at+i)/rate)),
      Float32Array.from({length:n},(_,i)=>.25*Math.sin(2*Math.PI*431*(at+i)/rate)),
    ],at,rate)));
  }
  collect(converter.finish());
  assert.equal(next,Math.round(length*48000/rate));
  return output;
}
function rms(a){return Math.sqrt(a.reduce((s,x)=>s+x*x,0)/a.length);}
async function main() {
for(const rate of [44100,88200,96000,192000]){
  const output=await convert(rate,rate,4096);
  for(const [ch,frequency,amplitude]of [[0,997,.5],[1,431,.25]]){
    const errors=output[ch].slice(512,-512).map((x,i)=>x-amplitude*Math.sin(2*Math.PI*frequency*(i+512)/48000));
    assert.ok(rms(errors)<1e-4,`rate ${rate} ch ${ch} tone error ${rms(errors)}`);
  }
}
const small=await convert(44100,44100,127),large=await convert(44100,44100,4096);
assert.ok(Math.max(...small[0].map((x,i)=>Math.abs(x-large[0][i])))<1e-6,"packet boundaries must not reset sinc history");
const ultrasonic=await convert(96000,96000,4096,30000);
assert.ok(rms(ultrasonic[0].slice(1024,-1024))<1e-4,"downsampling must reject ultrasonic aliasing");
await convert(44100,17,7);
const same=frame([new Float32Array([.1,.2])],0,48000);
const bypass=new AlacResampler(48000);
assert.equal(await bypass.push(same),same);
assert.equal(bypass.finish(),null);
console.log("ALAC resampling: exact duration, tone/pitch, stereo separation, streaming continuity, tail, anti-aliasing and 48 kHz bypass passed");
}
main().catch(error=>{console.error(error);process.exitCode=1;});
