'use strict';
const {Worker,isMainThread,parentPort,workerData}=require('node:worker_threads');
const fs=require('node:fs');
if(!isMainThread&&workerData?.sdaBwfMetadata){
 (async()=>{const file=await fs.promises.open(workerData.path,'r');try{const before=await file.stat();const {readBwfMetadata}=require('./bwf-metadata.cjs');const metadata=await readBwfMetadata(async(offset,length)=>{const bytes=Buffer.allocUnsafe(length);const {bytesRead}=await file.read(bytes,0,length,offset);if(bytesRead!==length)throw Error('BWF metadata read was truncated');return bytes;},before.size);const after=await file.stat();if(before.size!==after.size||before.mtimeMs!==after.mtimeMs)throw Error('BWF changed during metadata parsing');parentPort.postMessage({description:{format:metadata.format,dataOffset:metadata.dataOffset,dataSize:metadata.dataSize,labels:metadata.labels,rawBedLabels:metadata.adm?.rawBedLabels??metadata.labels,objectChannels:metadata.adm?.objectChannels??[],adm:!!metadata.adm},events:metadata.adm?.events??[],title:metadata.adm?.title,binaural:metadata.binaural});}finally{await file.close();}})().catch(error=>{parentPort.postMessage({error:String(error.message||error)});});
}
function readBwfFileMetadata(path,{signal}={}){return new Promise((resolve,reject)=>{
 if(signal?.aborted){reject(Error('BWF metadata parsing cancelled'));return;}
 const worker=new Worker(__filename,{workerData:{sdaBwfMetadata:true,path},trackUnmanagedFds:true});let result,settled=false;
 const finish=(error,value)=>{if(settled)return;settled=true;signal?.removeEventListener('abort',abort);if(error)reject(error);else resolve(value);};
 const abort=()=>{worker.terminate();finish(Error('BWF metadata parsing cancelled'));};signal?.addEventListener('abort',abort,{once:true});
 worker.on('message',value=>{result=value;});worker.on('error',error=>finish(error));worker.on('exit',code=>{if(code||!result||result.error)finish(Error(result?.error||'BWF metadata parser failed'));else finish(null,result);});
});}

module.exports={readBwfFileMetadata};
