const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),os=require('node:os');
const {execFileSync}=require('node:child_process'),{createHash}=require('node:crypto');
const {inspect,mux}=require('../sdar-lossless.cjs'),{scanBoxes}=require('../sda-delivery.cjs');
const codec=process.env.SDA_FORMAT_CODEC;assert(codec,'Set SDA_FORMAT_CODEC');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sda-wave-archive-'));
const frames=1024,channels=6,pcm=Buffer.alloc(frames*channels*3);
for(let i=0;i<pcm.length;i++)pcm[i]=(i*37+i%7)&255;
const render={schema:'sda.render-metadata',version:1,scene:{kind:'bed',layout:'5.1',sampleRate:48000,durationSamples:frames,objects:[],automation:[],channels:['FrontLeft','FrontRight','Center','LFE','SurroundLeft','SurroundRight'].map((name,i)=>({id:'c'+i,pcmChannel:i+1,kind:'bed',name,label:name,position:{azimuth:0,elevation:0,distance:1},cartesian:[0,1,0],gainDb:0}))},baked:{volumeBalance:{applied:false,enabled:false},spatialBalance:{applied:false,enabled:false}},runtimeRecipe:{operations:[]},playbackPolicy:{authority:'file-authored',applyBakedProcessingAgain:false,overrideUserEnhancementPresets:true}};
function chunk(fd,id,b){const h=Buffer.alloc(8);h.write(id);h.writeUInt32LE(b.length,4);fs.writeSync(fd,h);fs.writeSync(fd,b);if(b.length&1)fs.writeSync(fd,Buffer.alloc(1));}
function adm(fd,size){if(!size)return;const a=Buffer.from('<audioFormatExtended><!--'),b=Buffer.from('--></audioFormatExtended>');const h=Buffer.alloc(8);h.write('axml');h.writeUInt32LE(size,4);fs.writeSync(fd,h);fs.writeSync(fd,a);let left=size-a.length-b.length;const block=Buffer.alloc(65536,120);while(left){const n=Math.min(left,block.length);fs.writeSync(fd,block,0,n);left-=n;}fs.writeSync(fd,b);if(size&1)fs.writeSync(fd,Buffer.alloc(1));}
function wave(file,before,after){const fd=fs.openSync(file,'wx');try{const h=Buffer.alloc(12);h.write('RIFF');h.write('WAVE',8);fs.writeSync(fd,h);adm(fd,before);const fmt=Buffer.alloc(16);fmt.writeUInt16LE(1);fmt.writeUInt16LE(channels,2);fmt.writeUInt32LE(48000,4);fmt.writeUInt32LE(48000*channels*3,8);fmt.writeUInt16LE(channels*3,12);fmt.writeUInt16LE(24,14);chunk(fd,'fmt ',fmt);chunk(fd,'data',pcm);adm(fd,after);chunk(fd,'sdrm',Buffer.from(JSON.stringify(render)));const n=Buffer.alloc(4);n.writeUInt32LE(fs.fstatSync(fd).size-8);fs.writeSync(fd,n,0,4,4);}finally{fs.closeSync(fd);}}
async function hash(file){const h=createHash('sha256');for await(const b of fs.createReadStream(file))h.update(b);return h.digest('hex');}
(async()=>{try{
 for(const [name,before,after]of [['small',1024,512],['over64',0,66*1024*1024],['over256',129*1024*1024,141*1024*1024]]){
  const source=path.join(dir,name+'.wav'),raw=path.join(dir,name+'.tmp'),sdar=path.join(dir,name+'.sdar'),m4a=path.join(dir,name+'.m4a'),restored=path.join(dir,name+'-restored.wav');wave(source,before,after);
  execFileSync(codec,['encode-bed',source,raw,'48000'],{stdio:'pipe'});
  const info=inspect(raw);assert(info.waveArchive);assert.equal(info.waveHeader,undefined);assert.deepEqual(info.render,render);
  const fd=fs.openSync(raw,'r');let at=info.waveArchive.offset,count=0;
  while(at<info.waveArchive.offset+info.waveArchive.bytes){const h=Buffer.alloc(21);fs.readSync(fd,h,0,21,at);const n=parseInt(h.toString('ascii',4,20),16);assert(n<=2*1024*1024+65536);at+=21+n;count++;}fs.closeSync(fd);
  mux(raw,sdar,{container:'sdar'});mux(raw,m4a,{container:'m4a'});
  assert(inspect(sdar).waveArchive);
  for(const file of [sdar,m4a]){const result=JSON.parse(execFileSync(codec,['inspect',file],{encoding:'utf8',maxBuffer:8*1024*1024}));assert.deepEqual(result.render,render);const fd=fs.openSync(file,'r'),top=scanBoxes(fd,0,fs.fstatSync(fd).size);assert.equal(top.some(a=>a.id==='sdwe'),file===sdar);fs.closeSync(fd);}
  execFileSync(codec,['restore-master',sdar,restored]);assert.equal(await hash(source),await hash(restored),'exact original WAV including ADM before/after PCM');
  if(name==='small'){
    const intermediate=path.join(dir,'intermediate-restored.wav');execFileSync(codec,['restore-master',raw,intermediate]);assert.equal(await hash(source),await hash(intermediate));
    const bad=path.join(dir,'bad.sdar');fs.copyFileSync(sdar,bad);const fd=fs.openSync(bad,'r+'),archive=inspect(bad).waveArchive,byte=Buffer.alloc(1);fs.readSync(fd,byte,0,1,archive.offset+24);byte[0]^=1;fs.writeSync(fd,byte,0,1,archive.offset+24);fs.closeSync(fd);
    assert.throws(()=>execFileSync(codec,['restore-master',bad,path.join(dir,'bad.wav')],{stdio:'pipe'}),/Command failed/,'corrupt preserved bytes must fail restoration');
  }
  console.log('PASS '+name+': '+((before+after)/1024/1024).toFixed(2)+' MiB ADM, '+count+' bounded records, SDAR/M4A inspection, exact master restore');
 }
}finally{assert.equal(path.dirname(dir),path.resolve(os.tmpdir()));assert(path.basename(dir).startsWith('sda-wave-archive-'));fs.rmSync(dir,{recursive:true,force:true});}})().catch(e=>{console.error(e);process.exitCode=1});
