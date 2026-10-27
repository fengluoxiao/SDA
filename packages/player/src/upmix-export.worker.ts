/** Offline channel-bed export; independent decoder and bounded disk spool. */
import fs from "node:fs";
import path from "node:path";
import {spawn} from "node:child_process";
import {parentPort,workerData} from "node:worker_threads";
import {createDemuxer,sniffContainer,readBwfMetadata,type DemuxedAudioPacket} from "@sda/demux";
import type {DecodedFrameData} from "@sda/core";
import {AlacStereoUpmixer} from "./alac-stereo-upmix";
import {AlacResampler} from "./alac-resampler";
import {Mp3Decoder,sniffMp3} from "./mp3-decoder";
import {TruePeakMeter,LoudnessMeter,masterBalanceGainDb} from "./bs1770";
import {readSourceMetadata} from "./source-metadata";
import {exportRenderMetadata,admBedMetadata} from "./export-scene";
const {sameFile}=require("../../../apps/desktop/upmix-export.cjs");
const delivery=require("../../../apps/desktop/sda-delivery.cjs");
const wav=require("../../../apps/desktop/upmix-wav.cjs");
const {buildMetadataChunks,chunk:riffChunk}=require("../../../apps/desktop/wav-metadata.cjs");
const job=workerData as {inputPath:string;outputPath:string;floatPath:string;stagingPath:string;corePath:string;cancel:SharedArrayBuffer;rendererPath?:string;balancedPath?:string;volumeBalanceEnabled?:boolean;spatialBalanceEnabled?:boolean;analysisOnly?:boolean;format?:"wav"|"sdar"|"m4a";codec?:"sdar-bed-channel";sourceMaster?:boolean;deliveryPath?:string;masterPath?:string;ffmpegPath?:string;sampleRate?:number;bitrateKbps?:number;renderSnapshot?:Record<string,unknown>|null;renderAssets?:{schema:string;version:number;blobs:{sha256:string;data:Uint8Array;mimeType:string;filename:string;kind:string}[];[key:string]:unknown}|null};
const balancedPath=job.balancedPath??job.floatPath+".balanced";
const volumeBalance=job.volumeBalanceEnabled===true,spatialBalance=job.spatialBalanceEnabled===true;
const cancelled=new Int32Array(job.cancel);
const check=()=>{if(Atomics.load(cancelled,0))throw Error("EXPORT_CANCELLED");};
function writeAll(fd:number,bytes:Buffer,position?:number){let offset=0;while(offset<bytes.length)offset+=fs.writeSync(fd,bytes,offset,bytes.length-offset,position===undefined?null:position+offset);}
let lastProgress=0;
function progress(stage:string,value:number,force=false){const now=Date.now();if(force||now-lastProgress>200){lastProgress=now;parentPort!.postMessage({type:"progress",stage,progress:value});}}
function frameData(frame:any):DecodedFrameData {
 const channels=Array.from({length:frame.channelCount},(_,i)=>frame.channel(i) as Float32Array);
 const data={codec:frame.codec,sampleRate:frame.sampleRate,samplePos:frame.samplePos,channels,labels:frame.labels,rawBedLabels:frame.rawBedLabels,events:JSON.parse(frame.eventsJson),objectChannels:JSON.parse(frame.objectChannelsJson),programLoudness:null,rampDuration:0};
 frame.free();return data;
}
async function renderBalances(programmeDb:number,makeupDb:number):Promise<void>{
 if(!job.rendererPath)throw Error("原生平衡渲染器不可用，请重新构建桌面版");
 await new Promise<void>((resolve,reject)=>{
  const child=spawn(job.rendererPath!,["--sda-export-upmix-bed",job.floatPath,balancedPath,String(programmeDb),String(makeupDb),String(spatialBalance)],{windowsHide:true,stdio:["pipe","pipe","pipe"]});
  let error="",pending="",ready=false,failure="";
  const readiness=setTimeout(()=>{failure="原生渲染器未确认平衡导出协议，请重新构建桌面版";child.kill();},5000);
  const timer=setInterval(()=>{if(Atomics.load(cancelled,0)&&!child.stdin.destroyed&&!child.stdin.writableEnded)child.stdin.end();},50);
  child.stdin.on("error",()=>{});
  child.stdout.on("data",bytes=>{pending+=String(bytes);let i;while((i=pending.indexOf("\n"))>=0){const line=pending.slice(0,i);pending=pending.slice(i+1);try{const state=JSON.parse(line);if(state.type==="bedExportReady"&&state.protocol===1){ready=true;clearTimeout(readiness);}else if(!ready){failure="原生渲染器不支持平衡导出，请重新构建桌面版";child.kill();}if(state.frames>0)progress("balance",.4+.25*state.processedFrames/state.frames);}catch{}}});
  child.stderr.on("data",bytes=>{error=(error+String(bytes)).slice(-3000);});
  child.on("error",failure=>{clearInterval(timer);clearTimeout(readiness);reject(failure);});
  child.on("close",code=>{clearInterval(timer);clearTimeout(readiness);if(Atomics.load(cancelled,0))reject(Error("EXPORT_CANCELLED"));else if(failure)reject(Error(failure));else if(code===0&&ready)resolve();else reject(Error(error||"原生平衡渲染失败"));});
 });
}
async function finishDelivery(master:string){
 check();const format=job.format??"wav";
 if(format==="wav"){fs.renameSync(master,job.outputPath);return;}
 const target=job.deliveryPath;if(!target)throw Error("导出临时路径缺失");
 progress("encode",.9,true);
 if(format==="sdar")await delivery.writeSdarMaster(master,target,{rendererPath:job.rendererPath,check,progress:(v:number)=>progress("encode",.9+.1*v)});
 else if(format==="m4a")await delivery.encodeSdaM4a(master,target,{codec:job.codec,sampleRate:job.sampleRate,bitrateKbps:job.bitrateKbps},{rendererPath:job.rendererPath,check,progress:(v:number)=>progress("encode",.9+.1*v)});
 else throw Error("不支持此导出格式");
 check();fs.renameSync(target,job.outputPath);
}
async function run(){
 let input:number|undefined,spool:number|undefined,output:number|undefined,processed:number|undefined,decoder:any=null;
 const resampler={current:null as AlacResampler|null};
 if(job.sourceMaster){
  try{
   check();if(sameFile(job.inputPath,job.outputPath))throw Error("导出文件不能覆盖源音频");
   const master=delivery.readWaveMaster(job.inputPath);
   // The authored bed is already processed. Conversion never runs the upmixer,
   // loudness analysis or the current listening preset over it again.
   if((job.format??"wav")==="wav"){
    const target=job.deliveryPath!;await delivery.writeWaveMaster(job.inputPath,target,{rendererPath:job.rendererPath,check,progress:(v:number)=>progress("write",v)});check();fs.renameSync(target,job.outputPath);
   }else {let input=job.inputPath;if(master.head){input=job.stagingPath;await delivery.writeWaveMaster(job.inputPath,input,{rendererPath:job.rendererPath,check});}await finishDelivery(input);}
   parentPort!.postMessage({type:"done",format:job.format??"wav",encoding:job.format!=="wav"?"SDA Bed Lossless v1":"PCM24",frames:master.frames,sampleRate:48000,channels:12,bits:24,
    volumeBalanceEnabled:master.render.baked.volumeBalance.enabled,spatialBalanceEnabled:master.render.baked.spatialBalance.enabled,
    sourceLufs:master.render.baked.volumeBalance.sourceLufs,programmeGainDb:master.render.baked.volumeBalance.programmeGainDb,
    truePeakDbtp:master.render.baked.peakProtection.truePeakDbtp,convertedMaster:true});
  }finally{for(const file of [job.deliveryPath,job.masterPath])if(file)try{fs.unlinkSync(file);}catch{}}
  return;
 }
 try{
  if(!job.analysisOnly&&sameFile(job.inputPath,job.outputPath))throw Error("导出文件不能覆盖源音频");
  input=fs.openSync(job.inputPath,"r");const initial=fs.fstatSync(input);
  if(!job.analysisOnly)progress("metadata",0,true);
  const sourceMetadata=job.analysisOnly?null:await readSourceMetadata(job.inputPath,check);
  const renderAssets=job.renderAssets?Object.fromEntries(Object.entries(job.renderAssets).filter(([key])=>key!=="blobs")):null;
  if(sourceMetadata&&job.renderAssets)for(const asset of job.renderAssets.blobs){const data=Buffer.from(asset.data);if(!sourceMetadata.blobs.some(existing=>existing.sha256===asset.sha256))sourceMetadata.blobs.push({...asset,data});}
  const metadata=sourceMetadata?buildMetadataChunks(sourceMetadata):null;
  if(!job.analysisOnly)spool=fs.openSync(job.floatPath,"wx+");const mixer=new AlacStereoUpmixer(true),meter=new TruePeakMeter(48000,12);
  const source={meter:null as LoudnessMeter|null};
  let samples=0,inputClock=0,inputRate=0,codec="";
  function store(frame:DecodedFrameData){
   check();
   const wet=mixer.liveSda(frame,true);if(wet.channels.length!==12)throw Error("仅支持 ALAC、MP3 或 WAV 的普通双声道音频");
   if(wet.samplePos!==samples)throw Error("导出音频时间轴不连续");
   const count=wet.channels[0]!.length;if(!volumeBalance&&!spatialBalance)meter.push(wet.channels);
   const bytes=Buffer.allocUnsafe(count*48);
   for(let i=0;i<count;i++)for(let ch=0;ch<12;ch++){const value=wet.channels[ch]![i]!;if(!Number.isFinite(value))throw Error("音频包含无效采样");bytes.writeFloatLE(value,(i*12+ch)*4);}
   writeAll(spool!,bytes);samples+=count;
  }
  async function accept(frame:DecodedFrameData){
   if(!["alac","mp3","pcm"].includes(frame.codec)||frame.channels.length!==2||frame.events.length||frame.objectChannels.length)throw Error(job.analysisOnly?"STEREO_ANALYSIS_UNSUPPORTED":"导出需要普通立体声输入，不能使用已有多声道或对象音轨");
   if(frame.samplePos!==inputClock||(inputRate&&inputRate!==frame.sampleRate))throw Error("输入音频格式或时间轴发生变化");
   inputRate=frame.sampleRate;codec=frame.codec;inputClock+=frame.channels[0]!.length;
   // Measure the original programme at its own rate. Analysis does not need
   // the listening SRC, HRTF, upmix or output device; export shares this reference.
   if(frame.channels.some(channel=>channel.some(value=>!Number.isFinite(value))))throw Error("音频包含无效采样");
   (source.meter??=new LoudnessMeter(frame.sampleRate,2)).push(frame.channels);
   if(job.analysisOnly){samples+=frame.channels[0]!.length;return;}
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
    else if(kind!=="mp4"&&kind!=="bwf")throw Error(job.analysisOnly?"STEREO_ANALYSIS_UNSUPPORTED":"当前 WAV 导出支持 ALAC、MP3 和普通立体声 WAV");
    const metadata=kind==="bwf"?await readBwfMetadata(async(start,length)=>{const bytes=Buffer.alloc(length);let done=0;while(done<length){const n=fs.readSync(input!,bytes,done,length-done,start+done);if(!n)throw Error("WAV 元数据不完整");done+=n;}return bytes;},initial.size):undefined;
    demuxer=createDemuxer(kind,{
     onTrack(track){if(track.channels!==2)throw Error(job.analysisOnly?"STEREO_ANALYSIS_UNSUPPORTED":"请选择双声道音频");if(track.codec==="alac"){if(!track.decoderConfig)throw Error("ALAC 缺少解码配置");const core=require(job.corePath);decoder=core.SdaDecoder.withConfig("alac",track.decoderConfig);}else if(track.codec!=="pcm")throw Error(job.analysisOnly?"STEREO_ANALYSIS_UNSUPPORTED":"该输入编码暂不支持上混导出");},
     onPacket(packet){pending.push({packet});},onPcmFrame(frame){pending.push({frame});},onError(message){throw Error(message);},
    },metadata);
   }
   demuxer.push(chunk,start);await drain();progress("decode",position/initial.size*(job.analysisOnly?1:.4));
  }
  if(prefix.length)throw Error("输入音频头不完整");demuxer?.flush();await drain();
  if(decoder){decoder.flush();let frame;while((frame=decoder.nextFrame()))await accept(codec==="mp3"?frame:frameData(frame));const errors=decoder.drainErrors();if(errors.length)throw Error(String(errors[0]));}
  const tail=resampler.current?.finish();if(tail)store(tail);if(!samples)throw Error("没有可导出的音频");
  const final=fs.fstatSync(input),current=fs.statSync(job.inputPath);
  if([final,current].some(stat=>["dev","ino","size","mtimeMs","ctimeMs"].some(key=>stat[key as keyof typeof stat]!==initial[key as keyof typeof initial])))throw Error("处理期间源文件发生变化，请重试");
  const sourceMeasurement=source.meter!.finish(),sourceTruePeak=sourceMeasurement.truePeakDbtp,programmeDb=volumeBalance&&sourceMeasurement.integratedLufs!==null?masterBalanceGainDb(sourceMeasurement.integratedLufs,sourceTruePeak):0;
  if(job.analysisOnly){
   parentPort!.postMessage({type:"measurement",protocol:1,codec,sampleRate:inputRate,channels:2,frames:Math.round(samples*48000/inputRate),sourceFrames:samples,measurementSampleRate:inputRate,blocks:sourceMeasurement.blocks,integratedLufs:sourceMeasurement.integratedLufs,truePeakDbtp:Number.isFinite(sourceTruePeak)?sourceTruePeak:null,policy:"source-rate-full-track-reference-v1",size:initial.size,mtimeMs:initial.mtimeMs});return;
  }
  const makeupDb=volumeBalance?2.5:0;
  const disk=fs.statfsSync(path.dirname(job.outputPath));if(disk.bavail*disk.bsize<samples*((volumeBalance||spatialBalance)?84:36)+wav.HEADER_BYTES+(metadata?.bytes.length??0))throw Error("目标磁盘剩余空间不足以完成平衡渲染和 WAV 输出");
  let audio=spool,inputPeak:number;
  if(volumeBalance||spatialBalance){
   progress("balance",.4,true);fs.fsyncSync(spool!);await renderBalances(programmeDb,makeupDb);check();
   processed=fs.openSync(balancedPath,"r");audio=processed;
   if(fs.fstatSync(processed).size!==samples*48)throw Error("平衡渲染输出长度不正确");
   const scan=new TruePeakMeter(48000,12);let at=0;
   while(at<samples){check();const count=Math.min(4096,samples-at),bytes=Buffer.alloc(count*48);let offset=0;while(offset<bytes.length){const n=fs.readSync(audio!,bytes,offset,bytes.length-offset,at*48+offset);if(!n)throw Error("平衡音频文件不完整");offset+=n;}const planes=Array.from({length:12},(_,ch)=>Float32Array.from({length:count},(_,i)=>bytes.readFloatLE((i*12+ch)*4)));scan.push(planes);at+=count;progress("peak",.65+.1*at/samples);}
   scan.push(Array.from({length:12},()=>new Float32Array(16)));
   // Reuse the final-stage meter result after the native source-domain balances.
   inputPeak=scan.dbtp();
  }else{meter.push(Array.from({length:12},()=>new Float32Array(16)));inputPeak=meter.dbtp();}
  const gain=Number.isFinite(inputPeak)?Math.min(1,10**((-1.01-inputPeak)/20)):1;
  progress("write",.75,true);
  output=fs.openSync(job.stagingPath,"wx");writeAll(output,wav.header(BigInt(samples)));
  const verified=new TruePeakMeter(48000,12);let read=0,seed=0x534441;
  const random=()=>{seed^=seed<<13;seed^=seed>>>17;seed^=seed<<5;return(seed>>>0)/4294967296;};
  while(read<samples){
   check();const count=Math.min(4096,samples-read),bytes=Buffer.allocUnsafe(count*48);let offset=0;
   while(offset<bytes.length){const n=fs.readSync(audio!,bytes,offset,bytes.length-offset,read*48+offset);if(!n)throw Error("临时音频文件不完整");offset+=n;}
   const planes=Array.from({length:12},(_,ch)=>Float32Array.from({length:count},(_,i)=>bytes.readFloatLE((i*12+ch)*4)));
   const pcm=wav.encode24(planes,gain,random);writeAll(output,pcm);
   const quantized=Array.from({length:12},(_,ch)=>Float32Array.from({length:count},(_,i)=>pcm.readIntLE(i*36+ch*3,3)/8388608));verified.push(quantized);
   read+=count;progress("write",.75+.25*read/samples);
  }
  verified.push(Array.from({length:12},()=>new Float32Array(16)));const peak=verified.dbtp();if(peak> -1)throw Error("导出峰值校验失败，未替换目标文件");
  const render=exportRenderMetadata(job.renderSnapshot,{frames:samples,sourceSampleRate:inputRate,sourceLufs:sourceMeasurement.integratedLufs,programmeGainDb:programmeDb,programmeMakeupDb:makeupDb,volumeBalanceEnabled:volumeBalance,spatialBalanceEnabled:spatialBalance,peakHeadroomGainDb:20*Math.log10(gain),truePeakDbtp:Number.isFinite(peak)?peak:null},renderAssets);
  const authored=admBedMetadata(render,sourceMetadata!.common.title),finalMetadata=buildMetadataChunks(sourceMetadata!,render);
  const metadataBytes=Buffer.concat([riffChunk("axml",authored.axml),riffChunk("chna",authored.chna),riffChunk("sdrm",Buffer.from(JSON.stringify(render),"utf8")),finalMetadata.bytes]);
  if(metadataBytes.length>64*1024*1024)throw Error("歌曲与渲染元数据超过 64 MiB，未替换目标文件");
  check();writeAll(output,metadataBytes);writeAll(output,wav.header(BigInt(samples),metadataBytes.length),0);
  fs.fsyncSync(output);fs.closeSync(output);output=undefined;check();await finishDelivery(job.stagingPath);
  parentPort!.postMessage({type:"done",format:job.format??"wav",encoding:job.format==="m4a"?"SDAR Bed Channel":"PCM24",metadata:{...finalMetadata.summary,renderSchemaVersion:1,bedChannels:12,objects:0},frames:samples,sampleRate:48000,channels:12,bits:24,gainDb:20*Math.log10(gain),truePeakDbtp:Number.isFinite(peak)?peak:null,codec,labels:wav.LABELS,volumeBalanceEnabled:volumeBalance,spatialBalanceEnabled:spatialBalance,programmeGainDb:programmeDb,programmeMakeupDb:makeupDb,sourceLufs:sourceMeasurement.integratedLufs,balancePolicy:"source-rate-full-track-reference-v1",spatialAlgorithm:spatialBalance?"directional-spectral-redistribution-v1":null});
 }finally{
  decoder?.free();resampler.current?.destroy();for(const fd of [input,spool,output,processed])if(fd!==undefined)try{fs.closeSync(fd);}catch{}
  if(!job.analysisOnly)for(const file of [job.floatPath,job.stagingPath,balancedPath,job.deliveryPath,job.masterPath])if(file)try{fs.unlinkSync(file);}catch{}
 }
}
run().catch(error=>parentPort!.postMessage({type:error.message==="STEREO_ANALYSIS_UNSUPPORTED"?"unsupported":error.message==="EXPORT_CANCELLED"?"cancelled":"error",error:error.message})).finally(()=>parentPort!.close());
