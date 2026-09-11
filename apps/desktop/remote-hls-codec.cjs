"use strict";
// FLAC verbatim frames: lossless coding of explicitly quantized signed PCM24.
// No third-party encoder runtime, resampling, normalization, or lossy fallback.
const u32=n=>{const b=Buffer.alloc(4);b.writeUInt32BE(n>>>0);return b;};
const u16=n=>{const b=Buffer.alloc(2);b.writeUInt16BE(n);return b;};
const box=(name,...parts)=>{const body=Buffer.concat(parts);return Buffer.concat([u32(body.length+8),Buffer.from(name),body]);};
const full=(name,version,flags,...parts)=>box(name,Buffer.from([version,(flags>>16)&255,(flags>>8)&255,flags&255]),...parts);
function crc(data,bits,polynomial){let n=0;for(const byte of data){n^=byte<<(bits-8);for(let i=0;i<8;i++)n=((n<<1)^((n&(1<<(bits-1)))?polynomial:0))&((1<<bits)-1);}return n;}
function utf8Integer(n){if(n<128)return Buffer.from([n]);let count=2;while(n>=2**(5*count+1)&&count<6)count++;const out=Buffer.alloc(count);for(let i=count-1;i>0;i--){out[i]=128|(n&63);n=Math.floor(n/64);}out[0]=((255<<(8-count))&255)|n;return out;}
function streamInfo(){const b=Buffer.alloc(34);b.writeUInt16BE(480,0);b.writeUInt16BE(480,2);const word=(48000n<<44n)|(1n<<41n)|(23n<<36n);b.writeBigUInt64BE(word,10);return b;}
function encodeFrame(pcm,index){
  if(pcm.length!==3840)throw Error("HLS PCM packet length mismatch");
  const header=Buffer.concat([Buffer.from([0xff,0xf8,0x7a,0x1c]),utf8Integer(index),u16(479)]);
  const channels=[];let clipped=0;
  for(let c=0;c<2;c++){const b=Buffer.alloc(1441);b[0]=2;for(let i=0;i<480;i++){const v=pcm.readFloatLE((i*2+c)*4);if(!Number.isFinite(v))throw Error("HLS PCM contains non-finite samples");if(v>1||v< -1)clipped++;
      let sample=Math.max(-8388608,Math.min(8388607,Math.round(v*8388608)));if(sample<0)sample+=16777216;b.writeUIntBE(sample,1+i*3,3);}channels.push(b);}
  const data=Buffer.concat([header,Buffer.from([crc(header,8,7)]),...channels]);return {data:Buffer.concat([data,u16(crc(data,16,0x8005))]),clipped};
}
function initSegment(){
  const matrix=Buffer.concat([u32(65536),u32(0),u32(0),u32(0),u32(65536),u32(0),u32(0),u32(0),u32(0x40000000)]);
  const mvhd=full("mvhd",0,0,u32(0),u32(0),u32(48000),u32(0),u32(65536),u16(256),Buffer.alloc(10),matrix,Buffer.alloc(24),u32(2));
  const tkhd=full("tkhd",0,7,u32(0),u32(0),u32(1),u32(0),u32(0),Buffer.alloc(8),u16(0),u16(0),u16(256),u16(0),matrix,u32(0),u32(0));
  const mdhd=full("mdhd",0,0,u32(0),u32(0),u32(48000),u32(0),u16(0x55c4),u16(0));
  const hdlr=full("hdlr",0,0,u32(0),Buffer.from("soun"),Buffer.alloc(12),Buffer.from("SDA Audio\0"));
  const entry=box("fLaC",Buffer.alloc(6),u16(1),Buffer.alloc(8),u16(2),u16(24),u16(0),u16(0),u32(48000*65536),full("dfLa",0,0,Buffer.from([0x80,0,0,34]),streamInfo()));
  const stbl=box("stbl",full("stsd",0,0,u32(1),entry),full("stts",0,0,u32(0)),full("stsc",0,0,u32(0)),full("stsz",0,0,u32(0),u32(0)),full("stco",0,0,u32(0)));
  const dinf=box("dinf",full("dref",0,0,u32(1),full("url ",0,1)));
  const minf=box("minf",full("smhd",0,0,u16(0),u16(0)),dinf,stbl);
  const moov=box("moov",mvhd,box("trak",tkhd,box("mdia",mdhd,hdlr,minf)),box("mvex",full("trex",0,0,u32(1),u32(1),u32(480),u32(0),u32(0))));
  return Buffer.concat([box("ftyp",Buffer.from("iso6"),u32(1),Buffer.from("iso6mp41")),moov]);
}
function mediaSegment(frames,sequence,baseTime){
  const time=Buffer.alloc(8);time.writeBigUInt64BE(BigInt(baseTime));
  const entries=Buffer.concat(frames.map(frame=>Buffer.concat([u32(480),u32(frame.length)])));
  const make=offset=>box("moof",full("mfhd",0,0,u32(sequence+1)),box("traf",full("tfhd",0,0x020000,u32(1)),full("tfdt",1,0,time),full("trun",0,0x301,u32(frames.length),u32(offset),entries)));
  const moof=make(0);return Buffer.concat([make(moof.length+8),box("mdat",...frames)]);
}
module.exports={encodeFrame,initSegment,mediaSegment,streamInfo};
