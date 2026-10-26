/** Offline channel-bed export; independent decoder and bounded disk spool. */
import fs from "node:fs";
import {parentPort,workerData} from "node:worker_threads";
import {createDemuxer,sniffContainer,readBwfMetadata,type DemuxedAudioPacket} from "@sda/demux";
import type {DecodedFrameData} from "@sda/core";
import {AlacStereoUpmixer} from "./alac-stereo-upmix";
import {AlacResampler} from "./alac-resampler";
import {Mp3Decoder,sniffMp3} from "./mp3-decoder";
import {TruePeakMeter} from "./bs1770";
const {sameFile}=require("../../../apps/desktop/upmix-export.cjs");
const wav=require("../../../apps/desktop/upmix-wav.cjs");
const job=workerData as {inputPath:string;outputPath:string;floatPath:string;stagingPath:string;corePath:string;cancel:SharedArrayBuffer};
const cancelled=new Int32Array(job.cancel);
const check=()=>{if(Atomics.load(cancelled,0))throw Error("EXPORT_CANCELLED");};
function writeAll(fd:number,bytes:Buffer){let offset=0;while(offset<bytes.length)offset+=fs.writeSync(fd,bytes,offset,bytes.length-offset);}
let lastProgress=0;
function progress(stage:string,value:number,force=false){const now=Date.now();if(force||now-lastProgress>200){lastProgress=now;parentPort!.postMessage({type:"progress",stage,progress:value});}}
function frameData(frame:any):DecodedFrameData {
 const channels=Array.from({length:frame.channelCount},(_,i)=>frame.channel(i) as Float32Array);
 const data={codec:frame.codec,sampleRate:frame.sampleRate,samplePos:frame.samplePos,channels,labels:frame.labels,rawBedLabels:frame.rawBedLabels,events:JSON.parse(frame.eventsJson),objectChannels:JSON.parse(frame.objectChannelsJson),programLoudness:null,rampDuration:0};
 frame.free();return data;
}
async function run(){
 let input:number|undefined,spool:number|undefined,output:number|undefined,decoder:any=null;
 const resampler={current:null as AlacResampler|null};
 try{
  if(sameFile(job.inputPath,job.outputPath))throw Error("导出文件不能覆盖源音频");
  input=fs.openSync(job.inputPath,"r");const initial=fs.fstatSync(input);
  spool=fs.openSync(job.floatPath,"wx+");const mixer=new AlacStereoUpmixer(true),meter=new TruePeakMeter(48000,12);
  let samples=0,inputClock=0,inputRate=0,codec="";
  function store(frame:DecodedFrameData){
   check();const wet=mixer.liveSda(frame,true);if(wet.channels.length!==12)throw Error("仅支持 ALAC、MP3 或 WAV 的普通双声道音频");
   if(wet.samplePos!==samples)throw Error("导出音频时间轴不连续");
   const count=wet.channels[0]!.length;meter.push(wet.channels);
   const bytes=Buffer.allocUnsafe(count*48);
   for(let i=0;i<count;i++)for(let ch=0;ch<12;ch++){const value=wet.channels[ch]![i]!;if(!Number.isFinite(value))throw Error("音频包含无效采样");bytes.writeFloatLE(value,(i*12+ch)*4);}
   writeAll(spool!,bytes);samples+=count;
  }
  async function accept(frame:DecodedFrameData){
   if(!["alac","mp3","pcm"].includes(frame.codec)||frame.channels.length!==2||frame.events.length||frame.objectChannels.length)throw Error("导出需要普通立体声输入，不能使用已有多声道或对象音轨");
   if(frame.samplePos!==inputClock||(inputRate&&inputRate!==frame.sampleRate))throw Error("输入音频格式或时间轴发生变化");
   inputRate=frame.sampleRate;codec=frame.codec;inputClock+=frame.channels[0]!.length;
   if(frame.sampleRate!==48000){resampler.current??=new AlacResampler(48000);const converted=await resampler.current.push(frame);if(converted)store(converted);}else store(frame);
  }
  const pending:({packet:DemuxedAudioPacket}|{frame:DecodedFrameData})[]=[];
  async function drain(){
   while(pending.length){check();const item=pending.shift()!;if("frame"in item){await accept(item.frame);continue;}
    if(!decoder)throw Error("导出解码器未就绪");
    for(const bytes of item.packet.frames){decoder.push(bytes);let frame;while((frame=decoder.nextFrame()))await accept(codec==="mp3"?frame:frameData(frame));const errors=decoder.drainErrors();if(errors.length)throw Error(String(errors[0]));}
   }
  }
  let prefix=Buffer.alloc(0),position=0,demuxer:ReturnType<typeof createDemuxer>|null=null;
  while(position<initial.size){
   check();const bytes=Buffer.allocUnsafe(Math.min(16384,initial.size-position));const count=fs.readSync(input,bytes,0,bytes.length,position);if(!count)throw Error("输入文件提前结束");
   const at=position;position+=count;let chunk=new Uint8Array(bytes.buffer,bytes.byteOffset,count),start=at;
   if(!demuxer){
    if(prefix.length){chunk=Buffer.concat([prefix,chunk]);start=0;prefix=Buffer.alloc(0);}
    const kind=sniffContainer(chunk),mp3=kind==="raw"?sniffMp3(chunk):null;
    if(mp3==="need-more"||chunk.length<16){prefix=Buffer.from(chunk);continue;}
    if(mp3==="mp3"){codec="mp3";decoder=await Mp3Decoder.create(()=>{});}
    else if(kind!=="mp4"&&kind!=="bwf")throw Error("当前 WAV 导出支持 ALAC、MP3 和普通立体声 WAV");
    const metadata=kind==="bwf"?await readBwfMetadata(async(start,length)=>{const bytes=Buffer.alloc(length);let done=0;while(done<length){const n=fs.readSync(input!,bytes,done,length-done,start+done);if(!n)throw Error("WAV 元数据不完整");done+=n;}return bytes;},initial.size):undefined;
    demuxer=createDemuxer(kind,{
     onTrack(track){if(track.channels!==2)throw Error("请选择双声道音频");if(track.codec==="alac"){if(!track.decoderConfig)throw Error("ALAC 缺少解码配置");const core=require(job.corePath);decoder=core.SdaDecoder.withConfig("alac",track.decoderConfig);}else if(track.codec!=="pcm")throw Error("该输入编码暂不支持上混导出");},
     onPacket(packet){pending.push({packet});},onPcmFrame(frame){pending.push({frame});},onError(message){throw Error(message);},
    },metadata);
   }
   demuxer.push(chunk,start);await drain();progress("decode",position/initial.size*.65);
  }
  if(prefix.length)throw Error("输入音频头不完整");demuxer?.flush();await drain();
  if(decoder){decoder.flush();let frame;while((frame=decoder.nextFrame()))await accept(codec==="mp3"?frame:frameData(frame));const errors=decoder.drainErrors();if(errors.length)throw Error(String(errors[0]));}
  const tail=resampler.current?.finish();if(tail)store(tail);if(!samples)throw Error("没有可导出的音频");
  const final=fs.fstatSync(input);if(final.size!==initial.size||final.mtimeMs!==initial.mtimeMs)throw Error("导出期间源文件发生变化，请重试");
  meter.push(Array.from({length:12},()=>new Float32Array(16)));const inputPeak=meter.dbtp();
  const gain=Number.isFinite(inputPeak)?Math.min(1,10**((-1.01-inputPeak)/20)):1;
  progress("write",.65,true);
  output=fs.openSync(job.stagingPath,"wx");writeAll(output,wav.header(BigInt(samples)));
  const verified=new TruePeakMeter(48000,12);let read=0,seed=0x534441;
  const random=()=>{seed^=seed<<13;seed^=seed>>>17;seed^=seed<<5;return(seed>>>0)/4294967296;};
  while(read<samples){
   check();const count=Math.min(4096,samples-read),bytes=Buffer.allocUnsafe(count*48);let offset=0;
   while(offset<bytes.length){const n=fs.readSync(spool,bytes,offset,bytes.length-offset,read*48+offset);if(!n)throw Error("临时音频文件不完整");offset+=n;}
   const planes=Array.from({length:12},(_,ch)=>Float32Array.from({length:count},(_,i)=>bytes.readFloatLE((i*12+ch)*4)));
   const pcm=wav.encode24(planes,gain,random);writeAll(output,pcm);
   const quantized=Array.from({length:12},(_,ch)=>Float32Array.from({length:count},(_,i)=>pcm.readIntLE(i*36+ch*3,3)/8388608));verified.push(quantized);
   read+=count;progress("write",.65+.35*read/samples);
  }
  verified.push(Array.from({length:12},()=>new Float32Array(16)));const peak=verified.dbtp();if(peak> -1)throw Error("导出峰值校验失败，未替换目标文件");
  fs.fsyncSync(output);fs.closeSync(output);output=undefined;check();fs.renameSync(job.stagingPath,job.outputPath);
  parentPort!.postMessage({type:"done",frames:samples,sampleRate:48000,channels:12,bits:24,gainDb:20*Math.log10(gain),truePeakDbtp:Number.isFinite(peak)?peak:null,codec,labels:wav.LABELS});
 }finally{
  decoder?.free();resampler.current?.destroy();for(const fd of [input,spool,output])if(fd!==undefined)try{fs.closeSync(fd);}catch{}
  for(const file of [job.floatPath,job.stagingPath])try{fs.unlinkSync(file);}catch{}
 }
}
run().catch(error=>parentPort!.postMessage({type:error.message==="EXPORT_CANCELLED"?"cancelled":"error",error:error.message})).finally(()=>parentPort!.close());
