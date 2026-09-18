'use strict';
const assert = require('node:assert/strict');
const {Readable} = require('node:stream');
const {SystemAudio} = require('./system-audio.cjs');
const {CaptureRecords} = require('../windows-system-audio/iec61937.cjs');
const {SystemDecoder} = require('../windows-system-audio/decoder.cjs');

function record(epoch, channels, mask) {
  const frames = 32, payload = frames * channels * 4;
  const b = Buffer.alloc(120 + payload);
  b.write('SDAC'); b.writeUInt32LE(1,4); b.writeBigUInt64LE(BigInt(epoch),8);
  b.writeBigUInt64LE(BigInt(payload),24); b.writeUInt32LE(3,40);
  b.writeUInt32LE(payload,44); b.writeUInt32LE(40,48); b.writeUInt32LE(2,52);
  const w = b.subarray(56,96);
  w.writeUInt16LE(0xfffe); w.writeUInt16LE(channels,2); w.writeUInt32LE(48000,4);
  w.writeUInt32LE(48000*channels*4,8); w.writeUInt16LE(channels*4,12);
  w.writeUInt16LE(32,14); w.writeUInt16LE(22,16); w.writeUInt16LE(32,18);
  w.writeUInt32LE(mask,20); w.writeUInt32LE(3,24);
  Buffer.from('00001000800000aa00389b71','hex').copy(w,28);
  for (let i=120; i<b.length; i+=4) b.writeFloatLE(0.25,i);
  return b;
}

(async () => {
  const commands=[], counts=[];
  const service = new SystemAudio({
    command: async c => { commands.push(c); return true; },
    batch: async (_sample, channels) => {
      counts.push(channels.length);
      // Simulate cancelling Solo before the format changes back to stereo.
      if (channels.length===8) service.setSpeakerMonitor([],[]);
      return {accepted:true};
    }, publish: () => {},
  });
  const names=['FrontRight']; service.setSpeakerMonitor(names,[]); names.push('Center');
  const session={}; service.session=session;
  const bytes=Buffer.concat([record(1,2,3),record(2,8,0x63f),record(3,2,3)]);
  const fragmented=[]; for(let i=0;i<bytes.length;i+=97)fragmented.push(bytes.subarray(i,i+97));
  await service.consume(session,Readable.from(fragmented),CaptureRecords,SystemDecoder,class {});
  assert.deepEqual(counts,[2,8,2]);
  assert.deepEqual(commands.filter(c=>c.type==='setSpeakerMutes').map(c=>c.names),[['FrontRight'],['FrontRight'],[]]);
  for(let i=0;i<commands.length;i++) if(commands[i].type==='reset') assert.equal(commands[i+1].type,'setSpeakerMutes');
  assert.equal(service.status.returnStreams,2);
  console.log('PASS stereo -> 7.1 -> stereo: fresh sources, durable/current speaker controls, fragmented capture, loopback diagnostics');
})().catch(error => {console.error(error); process.exitCode=1;});
