const fs = require('node:fs');
const path = require('node:path');
const { Worker } = require('node:worker_threads');
const { spawn } = require('node:child_process');
const { createInterface } = require('node:readline');
const POLICIES = new Set(['offline-full-track-reference-v1', 'source-rate-full-track-reference-v1', 'fresh-complete-stereo-reference-v1']);

function validMeasurement(value) {
  return POLICIES.has(value?.policy) && (value.protocol === undefined || value.protocol === 1) && ['alac', 'mp3', 'pcm', 'av3a', 'flac', 'aac', 'vorbis'].includes(value.codec)
    && (value.codec === 'av3a' ? value.complete === true && Number.isSafeInteger(value.channels) && value.channels > 0 : value.channels === 2) && Number.isFinite(value.sampleRate) && value.sampleRate > 0
    && Number.isSafeInteger(value.frames) && value.frames > 0
    && Number.isSafeInteger(value.blocks) && value.blocks >= 0
    && (value.integratedLufs === null || Number.isFinite(value.integratedLufs))
    && (value.truePeakDbtp === null || Number.isFinite(value.truePeakDbtp));
}

/** A complete source reference is prepared before playback, never retargeted live. */
function createStereoLoudness({ ipcMain, workerPath, corePath, nativeExecutable }) {
  const jobs = new Map();
  function cancel(owner) {
    const current = jobs.get(owner);
    if (!current) return false;
    if (current.native) {current.cancel();return true;}
    Atomics.store(current.cancel, 0, 1);
    current.stop ??= setTimeout(() => current.worker.terminate(), 1000);
    return true;
  }
  ipcMain.handle('sda:stereo-loudness-cancel', event => cancel(event.sender));
  ipcMain.handle('sda:stereo-loudness', async (event, filePath) => {
    const owner = event.sender;
    cancel(owner);
    if (typeof filePath !== 'string' || !path.isAbsolute(filePath)) throw Error('音频路径无效');
    if (!/\.(m4a|mp4|mp3|wav|av3a|flac|aac|ogg|aif|aiff)$/i.test(filePath)) return null;
    const absolute = fs.realpathSync(filePath), stat = fs.statSync(absolute);
    if (!stat.isFile()) throw Error('音频路径不是文件');
    const executable = nativeExecutable?.();
    if (executable) {
      const child=spawn(executable,['--programme-reference',absolute],{windowsHide:true,stdio:['ignore','pipe','pipe']});
      let cancelled=false;
      const current={native:true,cancel:()=>{cancelled=true;child.kill();}};
      const onDestroyed=()=>{if(jobs.get(owner)===current)current.cancel();};
      jobs.set(owner,current);owner.once?.('destroyed',onDestroyed);
      const completion=new Promise((resolve,reject)=>{
        let result,failure,stderr='';
        const lines=createInterface({input:child.stdout});
        child.stderr.on('data',chunk=>{stderr=(stderr+chunk.toString()).slice(-4096);});
        lines.on('line',line=>{
          if(cancelled||jobs.get(owner)!==current)return;
          try {
            if(line.length>1024*1024)throw Error('Native reference message too large');
            const message=JSON.parse(line);
            if(message.type==='progress'){if(Number.isFinite(message.progress)&&!owner.isDestroyed())owner.send('sda:stereo-loudness-progress',message.progress);}
            else if(message.type==='unsupported')result=null;
            else if(message.type==='measurement'&&validMeasurement(message))result=message;
            else failure=Error(message.error||'Invalid native programme reference');
          } catch(error){failure=error;child.kill();}
        });
        child.on('error',error=>{failure=error;});
        child.on('close',code=>{
          lines.close();owner.removeListener?.('destroyed',onDestroyed);
          if(jobs.get(owner)===current)jobs.delete(owner);
          if(cancelled){reject(Error('响度分析已取消'));return;}
          if(failure||code||result===undefined){reject(failure||Error(stderr||'原生响度分析未完成'));return;}
          if(result){try{
            const now=fs.statSync(absolute);
            if(['dev','ino','size','mtimeMs','ctimeMs'].some(field=>now[field]!==stat[field]))throw Error('分析期间源文件发生变化，请重新播放');
            result={...result,size:stat.size,mtimeMs:stat.mtimeMs};
          }catch(error){reject(error);return;}}
          resolve(result);
        });
      });
      current.completion=completion;
      return await completion;
    }
    const shared = new SharedArrayBuffer(4);
    const worker = new Worker(workerPath, { workerData: {
      inputPath: absolute, outputPath: '', floatPath: '', stagingPath: '', corePath,
      cancel: shared, analysisOnly: true,
    }, trackUnmanagedFds: true });
    const current = { worker, cancel: new Int32Array(shared), stop: null };
    jobs.set(owner, current);
    const onDestroyed = () => { if (jobs.get(owner) === current) cancel(owner); };
    owner.once?.('destroyed', onDestroyed);
    return await new Promise((resolve, reject) => {
      let result, failure;
      worker.on('message', message => {
        if (jobs.get(owner) !== current || Atomics.load(current.cancel, 0)) return;
        if (message.type === 'progress') {
          if (!owner.isDestroyed()) owner.send('sda:stereo-loudness-progress', message.progress);
        } else if (message.type === 'unsupported') result = null;
        else if (message.type === 'measurement' && validMeasurement(message)) result = message;
        else failure = Error(message.error || (message.type === 'measurement'
          ? '响度分析结果与主程序不兼容，请完全退出并重新打开桌面版' : '整曲响度分析失败'));
      });
      worker.on('error', error => { failure = error; });
      worker.on('exit', code => {
        owner.removeListener?.('destroyed', onDestroyed);
        if (current.stop) clearTimeout(current.stop);
        if (jobs.get(owner) === current) jobs.delete(owner);
        if (Atomics.load(current.cancel, 0)) { reject(Error('响度分析已取消')); return; }
        if (failure || code || result === undefined) { reject(failure || Error('整曲响度分析未完成')); return; }
        if (result) {
          try {
            const now = fs.statSync(absolute);
            if (['dev', 'ino', 'size', 'mtimeMs', 'ctimeMs'].some(field => now[field] !== stat[field])) throw Error('分析期间源文件发生变化，请重新播放');
          } catch (error) { reject(error); return; }
        }
        resolve(result);
      });
    });
  });
  return { async close() {
    await Promise.allSettled([...jobs.values()].map(current => {
      if(current.native){current.cancel();return current.completion;}
      Atomics.store(current.cancel, 0, 1);
      return current.worker.terminate();
    }));
  } };
}
module.exports = { createStereoLoudness, validMeasurement };
