const CHANNELS=12,RATE=48000,BITS=24,ALIGN=36,MASK=0x2d63f;
// WAVE speaker-mask order puts back before side; SDA's internal bed is side first.
const ORDER=[0,1,2,3,6,7,4,5,8,9,10,11];
const LABELS=['FL','FR','FC','LFE','BL','BR','SL','SR','TFL','TFR','TBL','TBR'];
function infoChunk(){
 const comment=Buffer.from('Channels: '+LABELS.join(' ')+'; SDA stereo upmix; -2 dB matrix calibration; no headphone or room rendering.\0','ascii');
 const software=Buffer.from('SDA\0','ascii');
 const sub=(name,data)=>{const b=Buffer.alloc(8+data.length+(data.length%2));b.write(name);b.writeUInt32LE(data.length,4);data.copy(b,8);return b;};
 const body=Buffer.concat([Buffer.from('INFO'),sub('ISFT',software),sub('ICMT',comment)]),chunk=Buffer.alloc(8+body.length);chunk.write('LIST');chunk.writeUInt32LE(body.length,4);body.copy(chunk,8);return chunk;
}
const INFO=infoChunk();const HEADER_BYTES=12+36+48+INFO.length+8;
function header(frames){
 if(typeof frames!=='bigint'||frames<0n)throw Error('Invalid WAV frame count');
 const data=frames*BigInt(ALIGN),total=BigInt(HEADER_BYTES)+data,riff=total-8n,rf64=riff>0xffffffffn;
 const b=Buffer.alloc(HEADER_BYTES);b.write(rf64?'RF64':'RIFF');b.writeUInt32LE(rf64?0xffffffff:Number(riff),4);b.write('WAVE',8);
 b.write(rf64?'ds64':'JUNK',12);b.writeUInt32LE(28,16);
 if(rf64){b.writeBigUInt64LE(riff,20);b.writeBigUInt64LE(data,28);b.writeBigUInt64LE(frames,36);}
 b.write('fmt ',48);b.writeUInt32LE(40,52);b.writeUInt16LE(0xfffe,56);b.writeUInt16LE(CHANNELS,58);b.writeUInt32LE(RATE,60);b.writeUInt32LE(RATE*ALIGN,64);b.writeUInt16LE(ALIGN,68);b.writeUInt16LE(BITS,70);b.writeUInt16LE(22,72);b.writeUInt16LE(BITS,74);b.writeUInt32LE(MASK,76);
 Buffer.from('0100000000001000800000aa00389b71','hex').copy(b,80);INFO.copy(b,96);
 const at=HEADER_BYTES-8;b.write('data',at);b.writeUInt32LE(rf64?0xffffffff:Number(data),at+4);return b;
}
function encode24(planar,gain,random){
 if(planar.length!==12||!Number.isFinite(gain)||gain<=0||gain>1)throw Error('Invalid 7.1.4 PCM');
 const frames=planar[0].length;if(planar.some(c=>c.length!==frames))throw Error('Channel lengths differ');
 const b=Buffer.allocUnsafe(frames*ALIGN);
 for(let i=0;i<frames;i++)for(let out=0;out<12;out++){
  const x=planar[ORDER[out]][i]*gain;if(!Number.isFinite(x))throw Error('Non-finite export PCM');
  const dither=random?random()-random():0;
  const q=Math.round(x*8388608+dither);if(q < -8388608 || q > 8388607)throw Error('Export PCM exceeded headroom');
  b.writeIntLE(q,i*ALIGN+out*3,3);
 }
 return b;
}
module.exports={CHANNELS,RATE,BITS,ALIGN,MASK,ORDER,LABELS,HEADER_BYTES,header,encode24};
