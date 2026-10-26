const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto');
const {Worker}=require('node:worker_threads');
function sameFile(input,output){
 const source=fs.realpathSync(input);if(path.resolve(input).toLowerCase()===path.resolve(output).toLowerCase())return true;
 if(!fs.existsSync(output))return false;
 const target=fs.realpathSync(output),a=fs.statSync(source),b=fs.statSync(target);
 return source.toLowerCase()===target.toLowerCase()||(a.ino!==0&&a.ino===b.ino&&a.dev===b.dev);
}
function createUpmixExport({ipcMain,dialog,BrowserWindow,shell,workerPath,corePath,rendererPath}){
 let job=null,last={state:'idle'};
 const emit=state=>{last=state;try{if(job?.owner&&!job.owner.isDestroyed())job.owner.send('sda:upmix-export-status',state);}catch{}};
 const clean=current=>{for(const file of [current.floatPath,current.stagingPath,current.balancedPath]){if(!file||!current.outputPath)continue;if(path.dirname(file)!==path.dirname(current.outputPath))continue;try{fs.unlinkSync(file);}catch{}}};
 const cancel=()=>{if(!job||job.selecting)return false;Atomics.store(job.cancel,0,1);emit({...last,state:'cancelling'});job.timeout??=setTimeout(()=>job?.worker.terminate(),5000);return true;};
 ipcMain.handle('sda:upmix-export-status',()=>last);
 ipcMain.handle('sda:upmix-export-cancel',(event)=>job?.owner===event.sender?cancel():false);
 ipcMain.handle('sda:upmix-export-reveal',()=>{if(last.state==='done'&&last.outputPath)shell.showItemInFolder(last.outputPath);});
 ipcMain.handle('sda:upmix-export-start',async(event,inputPath,options={})=>{
  if(job)throw Error('已有导出任务正在运行');
  if(typeof inputPath!=='string'||!path.isAbsolute(inputPath)||!fs.statSync(inputPath).isFile())throw Error('请选择本地立体声音频');
  if(!options||typeof options!=='object')throw Error('导出设置无效');
  const settings={volumeBalanceEnabled:options.volumeBalanceEnabled??false,spatialBalanceEnabled:options.spatialBalanceEnabled??false};
  if(Object.values(settings).some(value=>typeof value!=='boolean'))throw Error('平衡开关必须是布尔值');
  const owner=event.sender;job={owner,selecting:true};
  try{
   const selected=await dialog.showSaveDialog(BrowserWindow.fromWebContents(owner),{title:'导出 7.1.4 WAV',defaultPath:path.join(path.dirname(inputPath),path.parse(inputPath).name+'.SDA-7.1.4.wav'),filters:[{name:'7.1.4 WAV · 48 kHz / 24-bit',extensions:['wav']}],properties:['showOverwriteConfirmation','createDirectory']});
   if(selected.canceled||!selected.filePath||owner.isDestroyed()){job=null;return {cancelled:true};}
   const outputPath=path.extname(selected.filePath)?selected.filePath:selected.filePath+'.wav';if(path.extname(outputPath).toLowerCase()!=='.wav')throw Error('请使用 .wav 文件扩展名');if(sameFile(inputPath,outputPath))throw Error('导出文件不能覆盖源音频');
   const input=fs.statSync(inputPath),available=fs.statfsSync(path.dirname(outputPath));
   // A minimum sanity check; actual streaming writes report disk-full errors safely.
   if(available.bavail*available.bsize<Math.min(input.size*2,32*1024*1024))throw Error('目标磁盘可用空间不足');
   const id=crypto.randomUUID(),prefix=path.join(path.dirname(outputPath),'.sda-upmix-'+id),shared=new SharedArrayBuffer(4);
   const current={id,owner,outputPath,floatPath:prefix+'.f32.tmp',balancedPath:prefix+'.balanced.f32.tmp',stagingPath:prefix+'.wav.tmp',cancel:new Int32Array(shared),worker:null,timeout:null};job=current;
   const state={id,state:'running',stage:'decode',progress:0,inputName:path.basename(inputPath),outputPath,...settings};emit(state);
   const worker=current.worker=new Worker(workerPath,{workerData:{inputPath,outputPath,floatPath:current.floatPath,stagingPath:current.stagingPath,corePath,cancel:shared,balancedPath:current.balancedPath,rendererPath:typeof rendererPath==="function"?rendererPath():rendererPath,...settings},trackUnmanagedFds:true});
   let terminal=null;
   const onDestroyed=()=>{if(job===current)cancel();};owner.once?.('destroyed',onDestroyed);
   worker.on('message',message=>{
    if(job!==current||terminal)return;
    if(message.type==='progress'){emit({...last,stage:message.stage,progress:message.progress});return;}
    terminal=message.type==='done'?{...state,...message,type:undefined,state:'done',progress:1}:{...state,state:message.type==='cancelled'?'cancelled':'error',error:message.error};
   });
   worker.on('error',error=>{terminal={...state,state:'error',error:error.message};});
   worker.on('exit',code=>{if(job!==current)return;owner.removeListener?.('destroyed',onDestroyed);if(current.timeout)clearTimeout(current.timeout);clean(current);emit(terminal??{...state,state:Atomics.load(current.cancel,0)?'cancelled':'error',error:code?'导出进程退出 '+code:undefined});job=null;});
   return {id};
  }catch(error){if(job){clean(job);job=null;}throw error;}
 });
 return {close(){if(!job||job.selecting)return Promise.resolve();const current=job;cancel();return new Promise(resolve=>current.worker.once("exit",resolve));}};
}
module.exports={createUpmixExport,sameFile};
