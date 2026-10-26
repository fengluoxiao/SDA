const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),os=require('node:os');
const {execFileSync}=require('node:child_process');const {Worker}=require('node:worker_threads');
const wav=require('../upmix-wav.cjs'),{createUpmixExport,sameFile}=require('../upmix-export.cjs');
const ffmpeg=process.env.SDA_TEST_FFMPEG;assert(ffmpeg,'Set SDA_TEST_FFMPEG');
const root=path.resolve(__dirname,'../../..'),temp=fs.mkdtempSync(path.join(os.tmpdir(),'sda-upmix-export-'));
const workerPath=path.join(root,'apps/desktop/upmix-export-worker.cjs'),corePath=path.join(root,'packages/core/pkg-node/sda_core.cjs');
function job(input,name,cancel=false){return new Promise((resolve,reject)=>{const shared=new SharedArrayBuffer(4);if(cancel)Atomics.store(new Int32Array(shared),0,1);const output=path.join(temp,name+'.wav'),floatPath=path.join(temp,name+'.f32.tmp'),stagingPath=path.join(temp,name+'.wav.tmp');const messages=[];const worker=new Worker(workerPath,{workerData:{inputPath:input,outputPath:output,floatPath,stagingPath,corePath,cancel:shared}});const timer=setTimeout(()=>{worker.terminate();reject(Error('Export timed out'));},30000);worker.on('message',message=>messages.push(message));worker.on('error',reject);worker.on('exit',code=>{clearTimeout(timer);assert.equal(code,0);assert(!fs.existsSync(floatPath)&&!fs.existsSync(stagingPath),'temporary files must be removed');resolve({output,messages,result:messages.find(m=>m.type==='done'||m.type==='error'||m.type==='cancelled')});});});}
function generate(name,codec,rate=48000,channels=2){const file=path.join(temp,name);const source=channels===2?'aevalsrc=0.2*sin(2*PI*997*t)+0.05*sin(2*PI*80*t)|0.1*sin(2*PI*3001*t)+0.05*sin(2*PI*80*t):s='+rate+':d=0.7':'anullsrc=r='+rate+':cl=5.1';execFileSync(ffmpeg,['-hide_banner','-loglevel','error','-f','lavfi','-i',source,'-t','0.7','-c:a',codec,file]);return file;}
(async()=>{try{
 const normal=wav.header(48000n);assert.equal(normal.toString('ascii',0,4),'RIFF');assert.equal(normal.readUInt16LE(56),0xfffe);assert.equal(normal.readUInt32LE(76),0x2d63f);assert.equal(normal.readUInt16LE(58),12);
 const large=wav.header(150000000n);assert.equal(large.toString('ascii',0,4),'RF64');assert.equal(large.toString('ascii',12,16),'ds64');assert.equal(large.readBigUInt64LE(28),150000000n*36n);assert.equal(large.readBigUInt64LE(36),150000000n);
 for(const [name,encoder,rate] of [['stereo.wav','pcm_f32le',48000],['stereo.m4a','alac',44100],['stereo.mp3','libmp3lame',44100]]){
  const input=generate(name,encoder,rate),result=await job(input,name);assert.equal(result.result.type,'done',JSON.stringify(result.result));const done=result.result;assert.equal(done.channels,12);assert.equal(done.sampleRate,48000);assert.equal(done.bits,24);assert.equal(done.frames,33600);assert(done.truePeakDbtp<=-1);
  const bytes=fs.readFileSync(result.output);assert(bytes.length>=wav.HEADER_BYTES+33600*36);assert.equal(bytes.readUInt32LE(4),bytes.length-8);assert.equal(bytes.readUInt32LE(wav.HEADER_BYTES-4),33600*36);
  const decoded=execFileSync(ffmpeg,['-hide_banner','-loglevel','error','-i',result.output,'-f','f32le','-c:a','pcm_f32le','-'],{maxBuffer:8*1024*1024});assert.equal(decoded.length,33600*12*4);
  if(encoder==='pcm_f32le'){
   const original=execFileSync(ffmpeg,['-hide_banner','-loglevel','error','-i',input,'-f','f32le','-c:a','pcm_f32le','-']);const gain=Math.fround(.7943282)*10**(done.gainDb/20);
   for(let i=1000;i<32000;i+=997){const l=original.readFloatLE(i*8),r=original.readFloatLE(i*8+4),side=Math.fround(Math.fround(l-r)*.5);const values=[l*Math.fround(.8),r*Math.fround(.8),Math.fround(Math.fround(l+r)*.5)*.5,null,side*Math.fround(.2),-side*Math.fround(.2),side*Math.fround(.34),-side*Math.fround(.34),side*Math.fround(.14),-side*Math.fround(.14),side*Math.fround(.1),-side*Math.fround(.1)];values.forEach((value,ch)=>{if(value!==null)assert(Math.abs(decoded.readFloatLE((i*12+ch)*4)-value*gain)<4/8388608,'canonical WAV channel placement '+ch);});}
  }
  console.log('PASS',name,'-> 12ch PCM24 WAV, correct clock and peak ceiling');
 }
 const hot=path.join(temp,'hot.wav');execFileSync(ffmpeg,['-hide_banner','-loglevel','error','-f','lavfi','-i','aevalsrc=2*sin(2*PI*997*t)|1.7*sin(2*PI*3001*t):s=48000:d=0.7','-c:a','pcm_f32le',hot]);
 const protectedResult=await job(hot,'protected');assert.equal(protectedResult.result.type,'done');assert(protectedResult.result.gainDb< -1);assert(protectedResult.result.truePeakDbtp<=-1);
 console.log('PASS hot floating-point input receives one linked peak-safe gain');
 const invalid=generate('multichannel.wav','pcm_s16le',48000,6),existing=path.join(temp,'reject.wav');fs.writeFileSync(existing,'keep-existing');const rejected=await job(invalid,'reject');assert.equal(rejected.result.type,'error');assert.equal(fs.readFileSync(existing,'utf8'),'keep-existing');
 const cancelled=await job(path.join(temp,'stereo.wav'),'cancel',true);assert.equal(cancelled.result.type,'cancelled');assert(!fs.existsSync(cancelled.output));
 assert(sameFile(path.join(temp,'stereo.wav'),path.join(temp,'stereo.wav')));const alias=path.join(temp,'alias.wav');fs.linkSync(path.join(temp,'stereo.wav'),alias);assert(sameFile(path.join(temp,'stereo.wav'),alias));
 const same=await new Promise((resolve,reject)=>{const worker=new Worker(workerPath,{workerData:{inputPath:alias,outputPath:alias,floatPath:path.join(temp,'same.f32'),stagingPath:path.join(temp,'same.tmp'),corePath,cancel:new SharedArrayBuffer(4)}});worker.on('message',m=>{if(m.type==='error')resolve(m);});worker.on('error',reject);});assert.match(same.error,/覆盖源/);
 const handlers=new Map(),events=[];let target=path.join(temp,'ipc.wav');
 const owner={isDestroyed:()=>false,send:(_channel,value)=>events.push(value)};
 createUpmixExport({ipcMain:{handle:(name,fn)=>handlers.set(name,fn)},dialog:{showSaveDialog:async()=>({filePath:target})},BrowserWindow:{fromWebContents:()=>null},shell:{showItemInFolder:()=>{}},workerPath,corePath});
 const start=handlers.get('sda:upmix-export-start'),status=handlers.get('sda:upmix-export-status');
 const first=await start({sender:owner},path.join(temp,'stereo.wav'));assert(first.id);
 await assert.rejects(()=>start({sender:owner},path.join(temp,'stereo.wav')),/已有导出/);
 async function finish(){const until=Date.now()+20000;while(['running','cancelling'].includes(status().state)){if(Date.now()>until)throw Error('IPC export timeout');await new Promise(resolve=>setTimeout(resolve,10));}return status();}
 assert.equal((await finish()).state,'done');assert(fs.existsSync(target));assert(events.some(event=>event.stage==='write'));
 target=path.join(temp,'ipc-cancel.wav');await start({sender:owner},path.join(temp,'stereo.wav'));
 assert.equal(handlers.get('sda:upmix-export-cancel')({sender:{}}),false,'other windows cannot cancel this export');
 assert.equal(handlers.get('sda:upmix-export-cancel')({sender:owner}),true);assert.equal((await finish()).state,'cancelled');assert(!fs.existsSync(target));
 console.log('PASS IPC save, single-job isolation, progress, owner cancellation and final file');
 console.log('PASS RF64 sizes, existing-file preservation, source/hard-link protection and cancellation cleanup');
 }finally{assert.equal(path.dirname(temp),path.resolve(os.tmpdir()));assert(path.basename(temp).startsWith('sda-upmix-export-'));fs.rmSync(temp,{recursive:true,force:true});}})().catch(error=>{console.error(error);process.exitCode=1;});
