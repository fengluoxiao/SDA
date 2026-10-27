const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const {Worker} = require('node:worker_threads');
const {SUPPORTED_RATES, SUPPORTED_BITRATES, readWaveMaster} = require('./sda-delivery.cjs');
function sameFile(a,b) {
  if(path.resolve(a).toLowerCase()===path.resolve(b).toLowerCase())return true;
  try {const x=fs.statSync(a),y=fs.statSync(b);return x.dev===y.dev&&x.ino===y.ino;}catch{return false;}
}
function resolveEncoder(resourceRoot) {
  const override=process.env.SDA_FFMPEG;
  const name=process.platform==='win32'?'ffmpeg.exe':'ffmpeg';
  const staged=path.join(resourceRoot,'audio-encoder',process.platform+'-'+process.arch,name);
  const file=override||(fs.existsSync(staged)?staged:path.join(resourceRoot,'audio-encoder',name));
  return fs.existsSync(file)?file:null;
}
function createUpmixExport({ipcMain,dialog,BrowserWindow,shell,workerPath,corePath,rendererPath,captureAssets,encoderPath}) {
  let job=null,last={state:'idle'};
  const encoder=()=>typeof encoderPath==='function'?encoderPath():encoderPath;
  const singleTrackReady=()=>{try{const file=typeof rendererPath==='function'?rendererPath():rendererPath;return !!file&&fs.readFileSync(file).includes(Buffer.from('sdar-bed-channel-v2'));}catch{return false;}};
  const emit=state=>{last=state;if(job?.owner&&!job.owner.isDestroyed())job.owner.send('sda:upmix-export-status',state);};
  const clean=current=>{for(const file of [current.floatPath,current.stagingPath,current.balancedPath,current.deliveryPath,current.deliveryPath&&current.deliveryPath+'.aac-components.tmp']) {
    if(!file||!current.outputPath||path.dirname(file)!==path.dirname(current.outputPath))continue;
    try{fs.unlinkSync(file);}catch{}
  }};
  const cancel=()=>{if(!job||job.selecting)return false;Atomics.store(job.cancel,0,1);emit({...last,state:'cancelling'});const current=job;current.timeout??=setTimeout(()=>current.worker.terminate(),5000);return true;};
  ipcMain.handle('sda:upmix-export-capabilities',()=>{const ready=singleTrackReady();return ({version:2,sourceMetadata:1,renderMetadata:1,authoredAssets:!!captureAssets,formats:['wav','sdar',...(encoder()&&ready?['m4a']:[])],codecs:ready?['sdar-bed-channel']:[],sampleRates:[...SUPPORTED_RATES],bitratesKbps:[...SUPPORTED_BITRATES]});});
  ipcMain.handle('sda:upmix-export-status',()=>last);
  ipcMain.handle('sda:upmix-export-cancel',event=>job?.owner===event.sender?cancel():false);
  ipcMain.handle('sda:upmix-export-reveal',()=>{if(last.state==='done'&&last.outputPath)shell.showItemInFolder(last.outputPath);});
  ipcMain.handle('sda:upmix-export-start',async(event,inputPath,options={})=>{
    if(job)throw Error('已有导出任务正在运行');
    if(typeof inputPath!=='string'||!path.isAbsolute(inputPath)||!fs.statSync(inputPath).isFile())throw Error('请选择本地音频');
    if(!options||typeof options!=='object')throw Error('导出设置无效');
    const format=options.format??'wav';
    if(format==='m4a'&&options.codec!=='sdar-bed-channel')throw Error('请选择 SDAR Bed Channel 编码');if(!['wav','sdar','m4a'].includes(format))throw Error('导出格式无效');
    if(format==='m4a'&&(!SUPPORTED_RATES.has(options.sampleRate)||!SUPPORTED_BITRATES.has(options.bitrateKbps)))throw Error('M4A 采样率或比特率不受支持');
    if(format==='m4a'&&!singleTrackReady())throw Error('请更新并重启支持单音轨 SDAR Bed Channel 的原生渲染器');
    if(format==='m4a'&&!encoder())throw Error('SDA AAC 编码器未安装，请重新构建桌面版');
    const kind=require('./authored-playback.cjs').probeAuthoredFile(inputPath);
    if(kind?.kind==='m4a')throw Error('有损 M4A 不能转换成无损母版，请使用原始 WAV / SDAR');
    const sourceMaster=!!kind;
    const settings={volumeBalanceEnabled:options.volumeBalanceEnabled??false,spatialBalanceEnabled:options.spatialBalanceEnabled??false};
    if(Object.values(settings).some(value=>typeof value!=='boolean'))throw Error('平衡开关必须是布尔值');
    let renderSnapshot=null;
    if(!sourceMaster&&options.renderSnapshot!==undefined){
      const text=JSON.stringify(options.renderSnapshot);if(!text||Buffer.byteLength(text)>8*1024*1024)throw Error('渲染参数快照过大');
      renderSnapshot=JSON.parse(text);if(!renderSnapshot||typeof renderSnapshot!=='object'||Array.isArray(renderSnapshot))throw Error('渲染参数快照无效');
    }
    const owner=event.sender;job={owner,selecting:true};
    try {
      const renderAssets=renderSnapshot&&captureAssets?await captureAssets(renderSnapshot):null;
      if(sourceMaster){const m=readWaveMaster(inputPath);settings.volumeBalanceEnabled=m.render.baked.volumeBalance.enabled;settings.spatialBalanceEnabled=m.render.baked.spatialBalance.enabled;}
      const description=format==='m4a'?'SDAR Bed Channel · 单音轨 / 5.1 基础':format==='sdar'?'SDAR Bed Channel · 无损':'7.1.4 WAV 母版 · 48 kHz / 24-bit';
      const selected=await dialog.showSaveDialog(BrowserWindow.fromWebContents(owner),{title:'导出 '+description,defaultPath:path.join(path.dirname(inputPath),path.parse(inputPath).name+'.SDA-7.1.4.'+format),filters:[{name:description,extensions:[format]}],properties:['showOverwriteConfirmation','createDirectory']});
      if(selected.canceled||!selected.filePath||owner.isDestroyed()){job=null;return {cancelled:true};}
      const outputPath=path.extname(selected.filePath)?selected.filePath:selected.filePath+'.'+format;
      if(path.extname(outputPath).toLowerCase()!=='.'+format)throw Error('请使用 .'+format+' 文件扩展名');
      if(sameFile(inputPath,outputPath))throw Error('导出文件不能覆盖源音频');
      const input=fs.statSync(inputPath),available=fs.statfsSync(path.dirname(outputPath));
      if(available.bavail*available.bsize<Math.min(input.size*2,32*1024*1024))throw Error('目标磁盘可用空间不足');
      const id=crypto.randomUUID(),prefix=path.join(path.dirname(outputPath),'.sda-upmix-'+id),shared=new SharedArrayBuffer(4);
      const current={id,owner,outputPath,floatPath:prefix+'.f32.tmp',balancedPath:prefix+'.balanced.f32.tmp',stagingPath:prefix+'.wav.tmp',deliveryPath:prefix+'.delivery.tmp',cancel:new Int32Array(shared),worker:null,timeout:null};job=current;
      const state={id,state:'running',stage:sourceMaster?'encode':'decode',progress:0,inputName:path.basename(inputPath),outputPath,format,...settings};emit(state);
      const worker=current.worker=new Worker(workerPath,{workerData:{...current,owner:undefined,worker:undefined,cancel:shared,inputPath,corePath,sourceMaster,format,codec:options.codec,sampleRate:options.sampleRate,bitrateKbps:options.bitrateKbps,ffmpegPath:encoder(),rendererPath:typeof rendererPath==='function'?rendererPath():rendererPath,renderSnapshot,renderAssets,...settings},trackUnmanagedFds:true});
      let terminal=null;const onDestroyed=()=>{if(job===current)cancel();};owner.once?.('destroyed',onDestroyed);
      worker.on('message',message=>{
        if(job!==current||terminal)return;
        if(message.type==='progress'){emit({...last,stage:message.stage,progress:message.progress});return;}
        terminal=message.type==='done'?{...state,...message,type:undefined,state:'done',progress:1}:{...state,state:message.type==='cancelled'?'cancelled':'error',error:message.error};
      });
      worker.on('error',error=>{terminal={...state,state:'error',error:error.message};});
      worker.on('exit',code=>{if(job!==current)return;owner.removeListener?.('destroyed',onDestroyed);if(current.timeout)clearTimeout(current.timeout);clean(current);emit(terminal??{...state,state:Atomics.load(current.cancel,0)?'cancelled':'error',error:code?'导出进程退出 '+code:undefined});job=null;});
      return {id};
    } catch(error){if(job){clean(job);job=null;}throw error;}
  });
  return {close(){if(!job||job.selecting)return Promise.resolve();const current=job;cancel();return new Promise(resolve=>current.worker.once('exit',resolve));}};
}
module.exports={createUpmixExport,sameFile,resolveEncoder};
