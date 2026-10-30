const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),os=require('node:os');
const {Worker}=require('node:worker_threads'),{execFileSync}=require('node:child_process');
const {HEADER_BYTES}=require('../upmix-wav.cjs'),{createUpmixExport}=require('../upmix-export.cjs');
const ffmpeg=process.env.SDA_TEST_FFMPEG,rendererPath=process.env.SDA_BED_RENDERER;assert(ffmpeg&&rendererPath,'Set SDA_TEST_FFMPEG and SDA_BED_RENDERER to current built tools');
const root=path.resolve(__dirname,'../../..'),temp=fs.mkdtempSync(path.join(os.tmpdir(),'sda-export-balances-'));
const workerPath=path.join(root,'apps/desktop/upmix-export-worker.cjs'),corePath=path.join(root,'packages/core/pkg-node/sda_core.cjs');
function run(input,name,volume,spatial,cancelDuringRender=false){return new Promise((resolve,reject)=>{
 const output=path.join(temp,name+'.wav'),floatPath=path.join(temp,name+'.f32'),balancedPath=path.join(temp,name+'.balanced'),stagingPath=path.join(temp,name+'.stage'),cancel=new SharedArrayBuffer(4);let result;
 const worker=new Worker(workerPath,{workerData:{inputPath:input,outputPath:output,floatPath,balancedPath,stagingPath,corePath,rendererPath,volumeBalanceEnabled:volume,spatialBalanceEnabled:spatial,cancel}});
 const timeout=setTimeout(()=>{worker.terminate();reject(Error('Balanced export timeout'));},60000);
 worker.on('error',reject);worker.on('message',m=>{if(cancelDuringRender&&m.stage==='balance')Atomics.store(new Int32Array(cancel),0,1);if(['done','error','cancelled'].includes(m.type))result=m;});
 worker.on('exit',code=>{clearTimeout(timeout);try{assert.equal(code,0);assert(result);assert(!fs.existsSync(floatPath)&&!fs.existsSync(balancedPath)&&!fs.existsSync(stagingPath));resolve({output,result});}catch(e){reject(e);}});
});}
function pcm(file){const bytes=fs.readFileSync(file),frames=bytes.readUInt32LE(HEADER_BYTES-4)/36;return {frames,at:(i,ch)=>bytes.readIntLE(HEADER_BYTES+i*36+ch*3,3)/8388608};}
function amplitude(data,ch,hz){let re=0,im=0;for(let i=96000;i<144000;i++){const phase=2*Math.PI*hz*i/48000,x=data.at(i,ch);re+=x*Math.cos(phase);im+=x*Math.sin(phase);}return Math.hypot(re,im)*2/48000;}
(async()=>{try{
 // The production weak-detail stimulus has a stronger body than presence;
 // spectral redistribution is not a uniform high-frequency gain.
 const input=path.join(temp,'stereo.wav');const mid='0.3*sin(2*PI*500*t)+0.08*sin(2*PI*80*t)',side='0.01*sin(2*PI*80*t)+0.005*sin(2*PI*400*t)+0.00125*sin(2*PI*3000*t)+0.000625*sin(2*PI*8000*t)';
 execFileSync(ffmpeg,['-hide_banner','-loglevel','error','-f','lavfi','-i',`aevalsrc=(${mid})+(${side})|(${mid})-(${side}):s=48000:d=3`,'-c:a','pcm_f32le',input]);
 const outputs=[];
 for(const [name,v,s]of [['off',false,false],['volume',true,false],['spatial',false,true],['both',true,true]]){
  const job=await run(input,name,v,s);assert.equal(job.result.type,'done',JSON.stringify(job.result));assert.equal(job.result.frames,144000);assert.equal(job.result.volumeBalanceEnabled,v);assert.equal(job.result.spatialBalanceEnabled,s);assert(job.result.truePeakDbtp<=-1);assert.equal(job.result.programmeMakeupDb,0);if(v){assert(job.result.programmeGainDb<0);assert(Math.abs(job.result.programmeGainDb-(-18-job.result.sourceLufs))<1e-9);}outputs.push({...job,data:pcm(job.output)});
 }
 const [off,volume,spatial,both]=outputs;assert.equal(volume.result.programmeGainDb,both.result.programmeGainDb);
 const ratio=10**((volume.result.programmeGainDb+volume.result.gainDb-off.result.gainDb)/20);
 for(let i=1009;i<144000;i+=1009)for(let ch=0;ch<12;ch++)assert(Math.abs(volume.data.at(i,ch)-off.data.at(i,ch)*ratio)<5/8388608,'volume balance must be one linked gain from sample zero');
 const db=(a,b)=>20*Math.log10(a/b);
 console.log('Measured spatial differences dB:',{rearDetail:db(amplitude(spatial.data,4,3000),amplitude(off.data,4,3000)),rearBass:db(amplitude(spatial.data,4,80),amplitude(off.data,4,80)),center:db(amplitude(spatial.data,2,500),amplitude(off.data,2,500))});
 assert(db(amplitude(spatial.data,4,3000),amplitude(off.data,4,3000))>1,'spatial balance must alter actual rear-channel detail samples');
 assert(Math.abs(db(amplitude(spatial.data,4,80),amplitude(off.data,4,80)))<0.2,'spatial balance preserves rear bass');
 assert(Math.abs(db(amplitude(spatial.data,2,500),amplitude(off.data,2,500)))<0.05,'front center remains protected');
 for(let i=1009;i<144000;i+=1009)for(let ch=0;ch<12;ch++)assert(Math.abs(both.data.at(i,ch)-spatial.data.at(i,ch)*ratio)<6/8388608,'both balances compose without duplicated programme gain');
 const existing=path.join(temp,'cancel.wav');fs.writeFileSync(existing,'keep-target');const cancelled=await run(input,'cancel',true,true,true);assert.equal(cancelled.result.type,'cancelled');assert.equal(fs.readFileSync(existing,'utf8'),'keep-target');
 const handlers=new Map();let done;const owner={isDestroyed:()=>false,send:(_name,value)=>{if(['done','error'].includes(value.state))done=value;}};
 createUpmixExport({ipcMain:{handle:(name,fn)=>handlers.set(name,fn)},dialog:{showSaveDialog:async()=>({filePath:path.join(temp,'snapshot.wav')})},BrowserWindow:{fromWebContents:()=>null},shell:{showItemInFolder(){}},workerPath,corePath,rendererPath});
 const options={volumeBalanceEnabled:true,spatialBalanceEnabled:true},start=handlers.get('sda:upmix-export-start')({sender:owner},input,options);options.volumeBalanceEnabled=false;options.spatialBalanceEnabled=false;await start;
 const until=Date.now()+60000;while(!done){if(Date.now()>until)throw Error('Snapshot export timeout');await new Promise(r=>setTimeout(r,10));}assert.equal(done.state,'done');assert.equal(done.volumeBalanceEnabled,true);assert.equal(done.spatialBalanceEnabled,true);
 console.log('PASS actual PCM: all four balance combinations, linked volume gain, native rear detail, protected center/bass, final peaks, cancellation and settings snapshot');
 }finally{assert.equal(path.dirname(temp),path.resolve(os.tmpdir()));assert(path.basename(temp).startsWith('sda-export-balances-'));fs.rmSync(temp,{recursive:true,force:true});}})().catch(e=>{console.error(e);process.exitCode=1;});
