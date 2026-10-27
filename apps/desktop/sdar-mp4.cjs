/** One logical SDAR Bed Channel track, with synchronized bed components per access unit. */
const fs=require('node:fs');
const MAPS=[[0,1,2,3,6,7],[4,5],[8,9],[10,11]];
const RENDER_LAYOUTS=['5.1','5.1.2','5.1.4','7.1','7.1.2','7.1.4','9.1','9.1.2','9.1.4','9.1.6','11.1.8','22.2','360RA-13'];
const CODEC='sdar-bed-channel',NAME='SDAR Bed Channel',MAX_SAMPLES=1000000;
function exact(fd,at,size){const b=Buffer.alloc(size);let n=0;while(n<size){const got=fs.readSync(fd,b,n,size-n,at+n);if(!got)throw Error('SDAR 组件数据提前结束');n+=got;}return b;}
function box(id,body){const h=Buffer.alloc(8);h.writeUInt32BE(body.length+8);h.write(id,4,4,'ascii');return Buffer.concat([h,body]);}
function atoms(fd,start,end){const out=[];for(let at=start;at<end;){if(at+8>end)throw Error('MP4 区块头不完整');const h=exact(fd,at,8);let size=h.readUInt32BE(),head=8;if(size===1){size=Number(exact(fd,at+8,8).readBigUInt64BE());head=16;}else if(!size)size=end-at;if(!Number.isSafeInteger(size)||size<head||at+size>end)throw Error('MP4 区块长度无效');out.push({id:h.toString('ascii',4,8),start:at,body:at+head,end:at+size});at+=size;}return out;}
function child(fd,a,id){const b=atoms(fd,a.body,a.end).find(v=>v.id===id);if(!b)throw Error('MP4 缺少 '+id);return b;}
function bytes(fd,a){return exact(fd,a.start,a.end-a.start);}
function body(fd,a){return exact(fd,a.body,a.end-a.body);}
function fullTable(id,rows){return box(id,Buffer.concat([Buffer.alloc(4),u32(rows.length),...rows]));}
function u32(n){const b=Buffer.alloc(4);b.writeUInt32BE(n);return b;}
function descriptor(b,at){const id=b[at++];let size=0,ended=false;for(let i=0;i<4;i++){if(at>=b.length)throw Error('AAC 配置截断');const n=b[at++];size=(size<<7)|(n&127);if(!(n&128)){ended=true;break;}}if(!ended||at+size>b.length)throw Error('AAC 配置长度无效');return {id,start:at,end:at+size};}
function aacConfig(b){const es=descriptor(b,4);if(es.id!==3)throw Error('AAC ES 配置无效');const flags=b[es.start+2];let at=es.start+3;if(flags&128)at+=2;if(flags&64)at+=1+b[at];if(flags&32)at+=2;const dc=descriptor(b,at);if(dc.id!==4||b[dc.start]!==64)throw Error('需要 AAC-LC 组件');at=dc.start+13;while(at<dc.end){const d=descriptor(b,at);if(d.id===5)return [...b.subarray(d.start,d.end)];at=d.end;}throw Error('缺少 AAC 解码配置');}
function sourceTrack(fd,trak){
 const mdia=child(fd,trak,'mdia'),minf=child(fd,mdia,'minf'),stbl=child(fd,minf,'stbl'),stsd=child(fd,stbl,'stsd'),entry=stsd.body+8;
 if(exact(fd,stsd.body+4,4).readUInt32BE()!==1||exact(fd,entry+4,4).toString('ascii')!=='mp4a')throw Error('编码器返回了非 AAC 组件');
 const size=exact(fd,entry,4).readUInt32BE(),configAtom=atoms(fd,entry+36,entry+size).find(a=>a.id==='esds');if(!configAtom)throw Error('缺少组件配置');
 const sz=body(fd,child(fd,stbl,'stsz')),count=sz.readUInt32BE(8),fixed=sz.readUInt32BE(4);if(!count||count>MAX_SAMPLES||(!fixed&&sz.length!==12+count*4))throw Error('组件采样表无效');
 const sizes=Array.from({length:count},(_,i)=>fixed||sz.readUInt32BE(12+i*4));if(sizes.some(n=>!n||n>1048576))throw Error('组件包长度无效');
 const sc=body(fd,child(fd,stbl,'stsc')),runs=Array.from({length:sc.readUInt32BE(4)},(_,i)=>[sc.readUInt32BE(8+i*12),sc.readUInt32BE(12+i*12),sc.readUInt32BE(16+i*12)]);if(!runs.length||runs[0][0]!==1||runs.some((r,i)=>!r[1]||r[2]!==1||(i&&r[0]<=runs[i-1][0])))throw Error('组件分块表无效');
 const children=atoms(fd,stbl.body,stbl.end),wide=children.find(a=>a.id==='co64'),offsetTable=body(fd,wide||child(fd,stbl,'stco')),offsets=[];let run=0;
 for(let c=0;c<offsetTable.readUInt32BE(4);c++){while(run+1<runs.length&&runs[run+1][0]<=c+1)run++;let at=wide?Number(offsetTable.readBigUInt64BE(8+c*8)):offsetTable.readUInt32BE(8+c*4);for(let n=0;n<runs[run][1];n++){if(offsets.length>=count)throw Error('组件包数量错误');offsets.push(at);at+=sizes[offsets.length-1];}}
 if(offsets.length!==count)throw Error('组件包数量不完整');
 return {trak,mdia,minf,stbl,config:aacConfig(body(fd,configAtom)),entryHeader:exact(fd,entry+8,28),sizes,offsets,timing:bytes(fd,child(fd,stbl,'stts')),edit:atoms(fd,trak.body,trak.end).find(a=>a.id==='edts')};
}
function repackSdarBedM4a(inputPath,outputPath,master,options,{check,progress}){
 const input=fs.openSync(inputPath,'r');let output;
 try{
  const top=atoms(input,0,fs.fstatSync(input).size),moov=top.find(a=>a.id==='moov');if(!moov)throw Error('AAC 组件容器缺少 moov');
  const components=atoms(input,moov.body,moov.end).filter(a=>a.id==='trak').filter(a=>body(input,child(input,child(input,a,'mdia'),'hdlr')).toString('ascii',8,12)==='soun').map(t=>sourceTrack(input,t));
  if(components.length!==4)throw Error('SDAR 床层组件不完整');const base=components[0],count=base.sizes.length;
  for(const t of components){if(t.sizes.length!==count||!t.timing.equals(base.timing)||(!!t.edit!==!!base.edit)||(t.edit&&!bytes(input,t.edit).equals(bytes(input,base.edit))))throw Error('SDAR 组件时间轴不一致');}
  const config={schema:'sda.bed-codec',version:1,name:NAME,codec:CODEC,componentCodec:'aac-lc',baseChannels:6,bedChannels:12,frameSamples:1024,packetSignature:'SBP1',components:components.map((c,slot)=>({slot,channels:MAPS[slot],decoderConfig:c.config}))};
  const sizes=Array.from({length:count},(_,i)=>24+components.reduce((sum,c)=>sum+c.sizes[i],0));let payload=0;for(const n of sizes)payload+=n;
  const ftyp=box('ftyp',Buffer.concat([Buffer.from('sda1'),u32(0),Buffer.from('sda1isommp42')]));
  function makeMoov(offset){
   const header=Buffer.from(base.entryHeader);header.writeUInt16BE(6,16);
   const stsd=fullTable('stsd',[box('sdar',Buffer.concat([header,box('sdac',Buffer.from(JSON.stringify(config)))]))]);
   const co=Buffer.alloc(8);co.writeBigUInt64BE(BigInt(offset));
   const stbl=box('stbl',Buffer.concat([stsd,base.timing,fullTable('stsc',[Buffer.concat([u32(1),u32(count),u32(1)])]),box('stsz',Buffer.concat([Buffer.alloc(8),u32(count),...sizes.map(u32)])),fullTable('co64',[co])]));
   const minf=box('minf',Buffer.concat([...atoms(input,base.minf.body,base.minf.end).filter(a=>a.id!=='stbl').map(a=>bytes(input,a)),stbl]));
   const handler=body(input,child(input,base.mdia,'hdlr'));const hdlr=box('hdlr',Buffer.concat([handler.subarray(0,24),Buffer.from(NAME+'\0')]));
   const mdia=box('mdia',Buffer.concat([bytes(input,child(input,base.mdia,'mdhd')),hdlr,minf]));
   const trak=box('trak',Buffer.concat([...atoms(input,base.trak.body,base.trak.end).filter(a=>a.id!=='mdia').map(a=>bytes(input,a)),mdia]));
   const mvhd=Buffer.from(bytes(input,child(input,moov,'mvhd')));mvhd.writeUInt32BE(2,mvhd.length-4);
   return box('moov',Buffer.concat([mvhd,trak]));
  }
  const first=makeMoov(0),movie=makeMoov(ftyp.length+first.length+16);if(first.length!==movie.length)throw Error('SDAR 分块偏移不稳定');
  output=fs.openSync(outputPath,'wx');const write=b=>{let at=0;while(at<b.length)at+=fs.writeSync(output,b,at,b.length-at);};write(ftyp);write(movie);
  const mdat=Buffer.alloc(16);mdat.writeUInt32BE(1);mdat.write('mdat',4);mdat.writeBigUInt64BE(BigInt(payload+16),8);write(mdat);
  for(let i=0;i<count;i++){check();const h=Buffer.alloc(24);h.write('SBP1');h[4]=4;components.forEach((c,slot)=>h.writeUInt32BE(c.sizes[i],8+slot*4));write(h);for(const c of components)write(exact(input,c.offsets[i],c.sizes[i]));if(i%128===0)progress?.(i/count);}
  const manifest={schema:'sda.delivery',version:2,profile:NAME,codec:CODEC,codecName:NAME,componentCodec:'aac-lc',baseLayout:'5.1',bedLayout:'7.1.4',mobileDefaultLayout:'7.1.4',compatibleRenderLayouts:RENDER_LAYOUTS,sampleRate:options.sampleRate,frames:Math.round(master.frames*options.sampleRate/48000),masterFrames:master.frames,bitrateKbps:options.bitrateKbps,tracks:[{trackId:1,role:'bed',channels:Array.from({length:12},(_,i)=>i),codec:CODEC}]};
  write(box('sdaf',Buffer.from(JSON.stringify(manifest))));write(box('sdrm',Buffer.from(JSON.stringify(master.render))));if(master.id3)write(box('sdid',master.id3));fs.fsyncSync(output);progress?.(1);return manifest;
 }finally{if(output!==undefined)fs.closeSync(output);fs.closeSync(input);}
}
module.exports={CODEC,NAME,MAPS,RENDER_LAYOUTS,repackSdarBedM4a};
