import {test} from 'node:test';
import assert from 'node:assert/strict';
import {PausePrefetch,pauseSegments} from '../remote-web/pause-prefetch.mjs';
const playlist='#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:40\n'+Array.from({length:10},(_,i)=>`#EXTINF:1,\n${40+i}.m4s`).join('\n');
test('paused prefetch selects upcoming media within a bounded window',()=>{
  assert.deepEqual(pauseSegments(playlist,42.5,3),['42.m4s','43.m4s','44.m4s','45.m4s']);
});
test('prefetch does not play, seek, or advance media; resume prevents further fetches',async()=>{
  const calls=[],p=new PausePrefetch(async(url)=>{calls.push(url);return {ok:true,text:async()=>playlist,arrayBuffer:async()=>new ArrayBuffer(1)};});
  const owner={audio:{currentTime:42.5,play(){assert.fail('must not play');}},lastMediaInfo:{stream:'/hls/'+'a'.repeat(32)+'/e3/index.m3u8'}};
  await p.update(owner,true);assert.equal(owner.audio.currentTime,42.5);assert.ok(calls.some(x=>x.endsWith('42.m4s')));assert.ok(!calls.some(x=>x.endsWith('40.m4s')));
  const count=calls.length;await p.update(owner,false);assert.equal(calls.length,count);
  p.cancel();assert.equal(p.done.size,0);
});
test('epoch change cancels obsolete requests',async()=>{
  let oldSignal;const p=new PausePrefetch(async(url,options)=>{oldSignal=options.signal;return new Promise(resolve=>options.signal.addEventListener('abort',()=>resolve({ok:false})));});
  const owner={audio:{currentTime:0},lastMediaInfo:{stream:'/hls/'+'a'.repeat(32)+'/e1/index.m3u8'}};
  const pending=p.update(owner,true);assert.equal(oldSignal.aborted,false);p.cancel();assert.equal(oldSignal.aborted,true);await pending;
});
