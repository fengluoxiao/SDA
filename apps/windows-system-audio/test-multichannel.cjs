'use strict';
const assert = require('node:assert/strict');
const {SystemDecoder} = require('./decoder.cjs');
const {pcmFormat} = require('./iec61937.cjs');
for (const [count,mask,labels] of [[6,0x3f,['L','R','C','LFE','Lb','Rb']], [8,0x63f,['L','R','C','LFE','Lb','Rb','Ls','Rs']], [12,0x2d63f,['L','R','C','LFE','Lb','Rb','Ls','Rs','TopFrontLeft','TopFrontRight','TopRearLeft','TopRearRight']]]) {
  for (const [bits,float] of [[16,false],[24,false],[32,false],[32,true]]) {
    const wave = Buffer.alloc(40), stride=count*bits/8;
    wave.writeUInt16LE(0xfffe);wave.writeUInt16LE(count,2);wave.writeUInt32LE(48000,4);wave.writeUInt32LE(48000*stride,8);
    wave.writeUInt16LE(stride,12);wave.writeUInt16LE(bits,14);wave.writeUInt16LE(22,16);wave.writeUInt16LE(bits,18);wave.writeUInt32LE(mask,20);
    wave.writeUInt32LE(float?3:1,24);Buffer.from('00001000800000aa00389b71','hex').copy(wave,28);
    assert.deepEqual(pcmFormat(wave).labels,labels);
    // Each channel has its own isolated impulse, including LFE and heights.
    const bytes=Buffer.alloc(count*stride);
    for(let c=0;c<count;c++) { const at=c*stride+c*bits/8; if(float)bytes.writeFloatLE(0.5,at);else bytes.writeIntLE(2**(bits-2),at,bits/8); }
    const got=[];const decoder=new SystemDecoder({onFrame:f=>got.push(f)});
    for(let at=0;at<bytes.length;at+=7){const payload=bytes.subarray(at,at+7);decoder.accept({epoch:1n,state:3,offset:BigInt(at),produced:BigInt(at+payload.length),overflows:0n,format:wave,payload});}
    for(let c=0;c<count;c++)assert.deepEqual(got.flatMap(f=>Array.from(f.channels[c])),Array.from({length:count},(_,i)=>i===c?0.5:0));
    assert(got.every(f=>f.objectChannels.length===0)); decoder.close();
    const invalid=Buffer.from(wave);invalid.writeUInt32LE(3,20);assert.throws(()=>pcmFormat(invalid),/mask/);
  }
}
console.log('PASS 5.1 / 7.1 / 7.1.4: 16/24/32-bit integer and float, fragmented samples, every channel isolated, masks validated');
