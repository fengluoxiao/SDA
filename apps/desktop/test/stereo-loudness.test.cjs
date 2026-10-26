const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { EventEmitter } = require('node:events');
const { execFileSync } = require('node:child_process');
const { Worker } = require('node:worker_threads');
const { buildSync } = require('esbuild');
const { createStereoLoudness, validMeasurement } = require('../stereo-loudness.cjs');
const ffmpeg = process.env.SDA_TEST_FFMPEG;
assert(ffmpeg, 'Set SDA_TEST_FFMPEG');
const root = path.resolve(__dirname, '../../..');
const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'sda-stereo-reference-'));
const workerPath = path.join(root, 'apps/desktop/upmix-export-worker.cjs');
const corePath = path.join(root, 'packages/core/pkg-node/sda_core.cjs');
function run(inputPath, cancel = false) {
  return new Promise((resolve, reject) => {
    const shared = new SharedArrayBuffer(4);
    if (cancel) Atomics.store(new Int32Array(shared), 0, 1);
    const worker = new Worker(workerPath, { workerData: {
      inputPath, outputPath: '', floatPath: '', stagingPath: '', corePath, cancel: shared, analysisOnly: true,
    } });
    const messages = [];
    const timeout = setTimeout(() => { worker.terminate(); reject(Error('Analysis timeout')); }, 60000);
    worker.on('message', message => messages.push(message));
    worker.on('error', reject);
    worker.on('exit', code => {
      clearTimeout(timeout);
      assert.equal(code, 0);
      resolve(messages.find(message => message.type !== 'progress'));
    });
  });
}
class Owner extends EventEmitter {
  isDestroyed() { return false; }
  send() { this.progressEvents = (this.progressEvents ?? 0) + 1; }
}
(async () => {
  let service;
  try {
    const wav = path.join(temp, 'quiet-intro.wav');
    const expression = 'aevalsrc=if(lt(t\\,2)\\,0.005\\,0.6)*sin(2*PI*997*t)|if(lt(t\\,2)\\,0.005\\,0.4)*sin(2*PI*3001*t):s=48000:d=12';
    execFileSync(ffmpeg, ['-v','error','-f','lavfi','-i',expression,'-c:a','pcm_f32le',wav]);
    const pcm = execFileSync(ffmpeg, ['-v','error','-i',wav,'-f','f32le','-'], { maxBuffer: 8*1024*1024 });
    buildSync({entryPoints:[path.join(root,'packages/player/src/bs1770.ts')],bundle:true,platform:'node',format:'cjs',outfile:path.join(temp,'meter.cjs')});
    const {LoudnessMeter,TruePeakMeter,masterBalanceGainDb} = require(path.join(temp,'meter.cjs'));
    for (const rate of [44100,48000,96000,192000]) {
      const tail = new TruePeakMeter(rate,2);
      tail.push([new Float32Array([1]),new Float32Array([1])]);
      assert(tail.finish()>=-0.001,'true-peak tail must include the final sample at '+rate);
    }
    const meter = new LoudnessMeter(48000, 2);
    const planes = [0,1].map(ch => Float32Array.from({length:pcm.length/8},(_,i)=>pcm.readFloatLE(i*8+ch*4)));
    meter.push(planes.map(plane=>plane.subarray(0,48000)));
    const intro = meter.integrated();
    meter.push(planes.map(plane=>plane.subarray(48000)));
    const complete = meter.integrated();
    const measurement = await run(wav);
    assert.equal(measurement.type, 'measurement');
    assert(validMeasurement(measurement));
    assert(validMeasurement({...measurement,policy:'offline-full-track-reference-v1'}),'compatible reference policies keep the same measurement protocol');
    assert(!validMeasurement({...measurement,protocol:999}),'unknown result protocols must fail explicitly');
    assert.equal(measurement.frames, 576000);
    assert.equal(measurement.policy, 'source-rate-full-track-reference-v1');
    assert(Math.abs(measurement.integratedLufs-complete.integratedLufs)<1e-8);
    assert.equal(masterBalanceGainDb(intro.integratedLufs,intro.truePeakDbtp),0);
    assert(masterBalanceGainDb(measurement.integratedLufs,measurement.truePeakDbtp)<-10,'quiet intro must not suppress full-programme attenuation');
    console.log('Quiet intro vs complete source:',{introLufs:intro.integratedLufs,fullLufs:measurement.integratedLufs,gainDb:masterBalanceGainDb(measurement.integratedLufs,measurement.truePeakDbtp)});
    for (const [name,codec,rate] of [['source.m4a','alac',44100],['source.mp3','libmp3lame',44100]]) {
      const file = path.join(temp,name);
      execFileSync(ffmpeg,['-v','error','-i',wav,'-ar',String(rate),'-c:a',codec,file]);
      const result=await run(file);
      assert.equal(result.type,'measurement',JSON.stringify(result));
      assert.equal(result.sampleRate,rate);
      assert.equal(result.measurementSampleRate,rate,'measurement uses the complete original source, not the listening SRC');
      assert.equal(result.frames,576000,'source clock after SRC');
      assert(Math.abs(result.integratedLufs-measurement.integratedLufs)<0.5,'ALAC/MP3 use the same original stereo reference');
    }
    const unsupported = path.join(temp,'unsupported.mp4');
    execFileSync(ffmpeg,['-v','error','-i',wav,'-c:a','eac3',unsupported]);
    const skipped=await run(unsupported);assert.equal(skipped.type,'unsupported',JSON.stringify(skipped));
    assert.equal((await run(wav,true)).type,'cancelled');
    const bad = path.join(temp,'broken.m4a');fs.writeFileSync(bad,Buffer.from('00000018667479704d344120000000006d70343200000000','hex'));
    const invalid=await run(bad);assert.equal(invalid.type,'error','incomplete file cannot become a reference');
    assert(!fs.existsSync(path.join(temp,'.balanced')),'measurement mode never creates an export spool');
    const handlers=new Map(),owner=new Owner(),other=new Owner();
    const options={ipcMain:{handle:(channel,handler)=>handlers.set(channel,handler)},workerPath,corePath};
    const filesBefore = fs.readdirSync(temp).sort();
    service=createStereoLoudness(options);
    const analyze=handlers.get('sda:stereo-loudness'),cancel=handlers.get('sda:stereo-loudness-cancel');
    const first=analyze({sender:owner},wav);
    assert.equal(cancel({sender:other}),false);
    const result=await first;assert.equal(result.integratedLufs,measurement.integratedLufs);
    const before=owner.progressEvents;
    const again=await analyze({sender:owner},wav);
    assert.equal(again.integratedLufs,result.integratedLufs);
    assert(owner.progressEvents>before,'replaying the same file must run analysis again');
    await service.close();
    // Even identical path/size/timestamps cannot reuse a previous result.
    service=createStereoLoudness({...options,workerPath:path.join(temp,'missing-worker.cjs')});
    await assert.rejects(handlers.get('sda:stereo-loudness')({sender:owner},wav));
    await service.close();service=createStereoLoudness(options);
    const pending=handlers.get('sda:stereo-loudness')({sender:owner},wav);
    assert.equal(handlers.get('sda:stereo-loudness-cancel')({sender:owner}),true);
    await assert.rejects(pending,/取消/);
    assert.equal(await handlers.get('sda:stereo-loudness')({sender:owner},path.join(temp,'other.ec3')),null);
    assert.deepEqual(fs.readdirSync(temp).sort(), filesBefore, 'playback analysis must not write a cache or audio spool');
    console.log('PASS full-source measurement, original stereo reference, SRC, unsupported codec, invalid file, fresh analysis on every playback, no persisted reference and owner cancellation');
  } finally {
    await service?.close();
    const absolute=path.resolve(temp);
    assert.equal(path.dirname(absolute),path.resolve(os.tmpdir()));
    assert(path.basename(absolute).startsWith('sda-stereo-reference-'));
    fs.rmSync(absolute,{recursive:true,force:true});
  }
})().catch(error=>{console.error(error);process.exitCode=1;});
