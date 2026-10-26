const assert=require('node:assert/strict'),fs=require('node:fs'),os=require('node:os'),path=require('node:path');
const {execFileSync}=require('node:child_process'),{Worker}=require('node:worker_threads');
const wav=require('../upmix-wav.cjs');
const ffmpeg=process.env.SDA_TEST_FFMPEG;assert(ffmpeg,'Set SDA_TEST_FFMPEG');
const root=path.resolve(__dirname,'../../..'),temp=fs.mkdtempSync(path.join(os.tmpdir(),'sda-wav-tags-'));
const workerPath=path.join(root,'apps/desktop/upmix-export-worker.cjs'),corePath=path.join(root,'packages/core/pkg-node/sda_core.cjs');
function run(input,name){return new Promise((resolve,reject)=>{const output=path.join(temp,name+'.wav'),floatPath=path.join(temp,name+'.f32.tmp'),stagingPath=path.join(temp,name+'.wav.tmp');let result;const worker=new Worker(workerPath,{workerData:{inputPath:input,outputPath:output,floatPath,stagingPath,corePath,cancel:new SharedArrayBuffer(4)}});const timer=setTimeout(()=>{worker.terminate();reject(Error('Metadata export timed out'));},30000);worker.on('error',reject);worker.on('message',m=>{if(m.type!=='progress')result=m;});worker.on('exit',code=>{clearTimeout(timer);try{assert.equal(code,0);assert.equal(result.type,'done',JSON.stringify(result));assert(!fs.existsSync(floatPath)&&!fs.existsSync(stagingPath));resolve({output,result});}catch(e){reject(e);}});});}
const args=['-metadata','title=测试歌曲 🎵','-metadata','artist=歌手甲 / Artist B','-metadata','album=测试专辑','-metadata','album_artist=专辑歌手','-metadata','date=2021-03-04','-metadata','genre=Pop','-metadata','composer=作曲家','-metadata','track=3/12','-metadata','disc=1/2','-metadata','copyright=原始版权'];
const lyric='[ar:歌手甲]\r\n[00:00.20]第一句\r\n[00:00.80]第二句\r\n';
const ttml='<?xml version="1.0" encoding="UTF-16"?>\r\n<tt xmlns="http://www.w3.org/ns/ttml"><body><div><p begin="00:00:00.200" end="00:00:00.800">第一句 <span begin="00:00:00.400">逐字</span></p></div></body></tt>\r\n';
(async()=>{try{
 const {parseFile}=await import('music-metadata');
 const original=path.join(temp,'source.wav'),cover=path.join(temp,'cover.png');
 execFileSync(ffmpeg,['-v','error','-f','lavfi','-i','aevalsrc=0.2*sin(2*PI*997*t)|0.1*sin(2*PI*3001*t):s=48000:d=1.2','-c:a','pcm_f32le',original]);
 execFileSync(ffmpeg,['-v','error','-f','lavfi','-i','color=c=red:s=32x32','-frames:v','1',cover]);
 for(const [extension,codec]of [['m4a','alac'],['mp3','libmp3lame']]){
  const input=path.join(temp,'tagged-'+extension+'.'+extension);
  execFileSync(ffmpeg,['-v','error','-i',original,'-i',cover,'-map','0:a','-map','1:v','-c:a',codec,'-c:v','copy','-disposition:v','attached_pic',...args,'-metadata','lyrics='+lyric,input]);
  const source=await parseFile(input,{duration:false});assert(source.common.picture?.length);
  const stem=path.join(temp,path.parse(input).name),lrcBytes=Buffer.concat([Buffer.from([239,187,191]),Buffer.from(lyric)]),ttmlBytes=Buffer.concat([Buffer.from([255,254]),Buffer.from(ttml,'utf16le')]);
  fs.writeFileSync(stem+'.lrc',lrcBytes);fs.writeFileSync(stem+'.TTML',ttmlBytes);
  const {output,result}=await run(input,'export-'+extension),read=await parseFile(output,{duration:false});
  assert.equal(read.common.title,'测试歌曲 🎵');assert.equal(read.common.artist,'歌手甲 / Artist B');assert.equal(read.common.album,'测试专辑');assert.equal(read.common.albumartist,'专辑歌手');assert.equal(read.common.year,2021);assert.equal(read.common.date,'2021-03-04');assert.equal(read.common.track.no,3);assert.equal(read.common.track.of,12);assert.equal(read.common.disk.no,1);assert.equal(read.common.copyright,'原始版权');
  assert.deepEqual(Buffer.from(read.common.picture[0].data),Buffer.from(source.common.picture[0].data),'cover bytes must survive unchanged');
  const native=Object.values(read.native).flat(),objects=native.filter(t=>t.id==='GEOB').map(t=>t.value),texts=native.filter(t=>t.id.startsWith('USLT')).map(t=>t.value.text);
  const lrc=objects.find(o=>o.filename===path.basename(stem+'.lrc')),xml=objects.find(o=>o.filename===path.basename(stem+'.TTML'));
  assert.deepEqual(Buffer.from(lrc.data),lrcBytes,'LRC BOM, timestamps and line endings must survive byte-for-byte');assert.deepEqual(Buffer.from(xml.data),ttmlBytes,'TTML original encoding, word timings and markup must survive byte-for-byte');assert(texts.includes(lyric),'native embedded LRC timestamps must be retained');
  const schema=JSON.parse(Buffer.from(objects.find(o=>o.filename==='source-metadata.json').data).toString('utf8'));
  assert.equal(schema.schema,'sda.source-metadata');assert.equal(schema.version,1);assert.equal(schema.common.title,'测试歌曲 🎵');assert(schema.native);assert(schema.assets.length>=3);assert.equal(result.metadata.coverCount,1);assert(result.metadata.lyrics.some(l=>l.format==='ttml'));assert(result.metadata.lyrics.some(l=>l.format==='lrc'));
  const bytes=fs.readFileSync(output),dataLength=bytes.readUInt32LE(wav.HEADER_BYTES-4);assert.equal(bytes.readUInt32LE(4),bytes.length-8);assert.equal(dataLength,result.frames*36);
  const decoded=execFileSync(ffmpeg,['-v','error','-i',output,'-map','0:a:0','-f','f32le','-'],{maxBuffer:8*1024*1024});assert.equal(decoded.length,result.frames*12*4,'tag chunks must not become audible PCM');
  console.log('PASS',extension,': Unicode descriptive tags, cover, exact LRC/TTML bytes, original source schema, RIFF sizing and independent PCM decode');
 }
 const embeddedXml=ttml.replace('UTF-16','UTF-8'),xmlInput=path.join(temp,'embedded-ttml.m4a');
 execFileSync(ffmpeg,['-v','error','-i',original,'-c:a','alac','-metadata','lyrics='+embeddedXml,xmlInput]);
 const xmlExport=await run(xmlInput,'embedded-ttml-export'),xmlTags=Object.values((await parseFile(xmlExport.output)).native).flat();
 const embeddedObject=xmlTags.find(tag=>tag.id==='GEOB'&&tag.value.type==='application/ttml+xml');
 assert.equal(Buffer.from(embeddedObject.value.data).toString('utf8'),embeddedXml,'embedded TTML source text and timing markup remain intact');
 // Adding descriptive tags must not modify the rendered audio at all.
 const taggedWav=path.join(temp,'tagged-source.wav');execFileSync(ffmpeg,['-v','error','-i',original,'-c:a','copy',...args,taggedWav]);
 const dry=await run(original,'untagged-audio'),tagged=await run(taggedWav,'tagged-audio');
 const audio=file=>{const bytes=fs.readFileSync(file);return bytes.subarray(wav.HEADER_BYTES,wav.HEADER_BYTES+bytes.readUInt32LE(wav.HEADER_BYTES-4));};
 assert.deepEqual(audio(dry.output),audio(tagged.output),'metadata must not affect a single PCM byte');
 assert.equal((await parseFile(tagged.output)).common.title,'测试歌曲 🎵');
 const frames=(0xffffffffn-BigInt(wav.HEADER_BYTES)+8n)/36n;
 const normal=wav.header(frames),large=wav.header(frames,1024);
 assert.equal(normal.toString('ascii',0,4),'RIFF');assert.equal(large.toString('ascii',0,4),'RF64');assert.equal(large.readBigUInt64LE(20),BigInt(wav.HEADER_BYTES)+frames*36n+1024n-8n);assert.equal(large.readBigUInt64LE(28),frames*36n);
 assert(!fs.readdirSync(temp).some(name=>name.endsWith('.tmp')));
 console.log('PASS WAV source tags, bit-identical audio, metadata-driven RF64 threshold, exact data size and temporary cleanup');
 }finally{const absolute=path.resolve(temp);assert.equal(path.dirname(absolute),path.resolve(os.tmpdir()));assert(path.basename(absolute).startsWith('sda-wav-tags-'));fs.rmSync(absolute,{recursive:true,force:true});}})().catch(error=>{console.error(error);process.exitCode=1;});
