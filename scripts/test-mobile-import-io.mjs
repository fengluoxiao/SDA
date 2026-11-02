import assert from 'node:assert/strict';
import {createRequire} from 'node:module';import {build} from 'esbuild';import {mkdirSync} from 'node:fs';import {resolve} from 'node:path';
const require=createRequire(import.meta.url),mp4=require('mp4box'),dest=resolve('tmp/import-io');mkdirSync(dest,{recursive:true});
await build({entryPoints:['apps/mobile/src/mpeghMp4.ts'],bundle:true,platform:'node',format:'cjs',outfile:dest+'/import.cjs'});
const {prepare360RaMp4}=require(dest+'/import.cjs');
mp4.BoxParser.createSampleEntryCtor(mp4.BoxParser.SAMPLE_ENTRY_TYPE_AUDIO,'mhm1');
const file=mp4.createFile(),id=file.addTrack({type:'mhm1',hdlr:'soun',timescale:48000,samplerate:48000,channel_count:2,samplesize:16});
const payloads=Array.from({length:256},(_,i)=>Buffer.alloc(16384,(i*13)&255));
for(let i=0;i<payloads.length;i++)file.addSample(id,Uint8Array.from(payloads[i]).buffer,{duration:1024,dts:i*1024,cts:i*1024,is_sync:true});
const original=Buffer.from(file.getBuffer());
function boxes(data,start=0,end=data.length){const out=[];for(let at=start;at<end;){const size=data.readUInt32BE(at);assert(size>=8&&at+size<=end);out.push({at,size,type:data.toString('ascii',at+4,at+8)});at+=size;}return out;}
const top=boxes(original),ftyp=top.find(b=>b.type==='ftyp'),moov=top.find(b=>b.type==='moov'),mdats=top.filter(b=>b.type==='mdat');assert(ftyp&&moov&&mdats.length);
const ftypBytes=original.subarray(ftyp.at,ftyp.at+ftyp.size),index=Buffer.from(original.subarray(moov.at,moov.at+moov.size));
function atom(type,payload){const head=Buffer.alloc(8);head.writeUInt32BE(payload.length+8);head.write(type,4);return Buffer.concat([head,payload]);}
function words(...values){const data=Buffer.alloc(values.length*4);values.forEach((v,i)=>data.writeUInt32BE(v,i*4));return data;}
const containers=new Set(['moov','trak','mdia','minf']);
function rebuild(data){return Buffer.concat(boxes(data).flatMap(box=>{
 if(box.type==='mvex')return [];
 let payload=Buffer.from(data.subarray(box.at+8,box.at+box.size));
 if(box.type==='stbl'){
  const stsd=boxes(payload).find(b=>b.type==='stsd');assert(stsd);
  payload=Buffer.concat([payload.subarray(stsd.at,stsd.at+stsd.size),atom('stts',words(0,1,payloads.length,1024)),atom('stsc',words(0,1,1,payloads.length,1)),atom('stsz',words(0,payloads[0].length,payloads.length)),atom('stco',words(0,1,ftypBytes.length+8))]);
 }else if(containers.has(box.type))payload=rebuild(payload);
 if(box.type==='mdhd')payload.writeUInt32BE(payloads.length*1024,16);
 return [atom(box.type,payload)];
}));}
const ordinaryIndex=rebuild(index),body=Buffer.concat(payloads),header=Buffer.alloc(8);header.writeUInt32BE(body.length+8);header.write('mdat',4);
const input=Buffer.concat([ftypBytes,header,body,ordinaryIndex]),output=[],reads=[];let firstWriteAt=0;
const host={mp4ImportChunkBytes:()=>1024*1024,beginMp4Import:async()=>JSON.stringify({token:'t',size:input.length}),readMp4Import:async(_t,offset,count)=>{reads.push({offset,count});return input.subarray(offset,offset+count).toString('base64');},appendMp4Import:async(_t,data)=>{if(!firstWriteAt)firstWriteAt=reads.length;const bytes=Buffer.from(data,'base64');assert(bytes.length<=1024*1024);output.push(bytes);},finishMp4Import:async()=> 'file:///ready.mhas',discardMp4Import:async()=>{}};
try { await prepare360RaMp4(host,'fixture','tail-index.m4a',undefined); } catch(error) {console.log({reads,top:top.map(b=>({type:b.type,at:b.at,size:b.size})).slice(0,4),body:body.length,index:index.length});throw error;}assert.deepEqual(Buffer.concat(output),body);
assert(firstWriteAt<=2,'tail moov must be read before scanning the entire mdat');
assert(reads.length<=8,'bounded random-access extraction must avoid fine-grained bridge calls');
console.log(JSON.stringify({mp4Bytes:input.length,firstWriteAfterReads:firstWriteAt,totalReads:reads.length,writes:output.length,readOffsets:reads.map(r=>r.offset)}));
