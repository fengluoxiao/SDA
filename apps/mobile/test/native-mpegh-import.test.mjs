import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
const require=createRequire(import.meta.url),{buildSync}=require('esbuild');
const dir=mkdtempSync(join(tmpdir(),'sda-native-mp4-'));
buildSync({entryPoints:['apps/mobile/src/mpeghMp4.ts'],bundle:true,platform:'node',format:'cjs',outfile:join(dir,'import.cjs')});
const {prepare360RaMp4}=require(join(dir,'import.cjs'));
process.on('exit',()=>rmSync(dir,{recursive:true,force:true}));
function host(result){const calls=[];return {calls,beginMp4Import:async()=>{calls.push('begin');return JSON.stringify({token:'t',size:5000})},extractMp4ImportNative:async()=>{calls.push('native');if(result instanceof Error)throw result;return JSON.stringify(result)},readMp4Import:async()=>{calls.push('read');throw Error('fallback read')},appendMp4Import:async()=>{throw Error('unexpected bridge payload')},finishMp4Import:async()=>{calls.push('finish');return 'file:///ready.mhas'},adoptMp4Import:async()=>{calls.push('adopt')},discardMp4Import:async()=>{calls.push('discard')}};}
for(const codec of ['mha1','mhm1'])test(`${codec}: one native extraction, no JS audio bytes, explicit ownership`,async()=>{const h=host({codec,durationMs:248000,packetCount:10000,bytes:20000000}),progress=[];const r=await prepare360RaMp4(h,'file:///input','music.m4a',p=>progress.push(p));assert.equal(r.name,'music.m4a.mhas');assert.equal(r.durationMs,248000);assert.deepEqual(h.calls,['begin','native','finish']);assert.equal(progress.at(-1).fraction,1);await r.adopt();await r.release();assert.deepEqual(h.calls,['begin','native','finish','adopt','discard']);});
test('unsupported native container uses established bounded JS parser',async()=>{const h=host(null);await assert.rejects(prepare360RaMp4(h,'file:///input','music.mp4'),/fallback read/);assert.deepEqual(h.calls,['begin','native','read','discard']);});
test('native error retires session and does not retry corrupt audio in JS',async()=>{const h=host(new Error('truncated MPEG-H sample'));await assert.rejects(prepare360RaMp4(h,'file:///input','music.m4a'),/truncated/);assert.deepEqual(h.calls,['begin','native','discard']);});
test('invalid native response is rejected and cleaned',async()=>{for(const row of [{codec:'aac',packetCount:2,bytes:3,durationMs:5},{codec:'mhm1',packetCount:0,bytes:3,durationMs:5},{codec:'mhm1',packetCount:2,bytes:3,durationMs:-1}]){const h=host(row);await assert.rejects(prepare360RaMp4(h,'file:///input','music.m4a'),/原生音轨提取结果无效/);assert.equal(h.calls.at(-1),'discard');}});
test('non MP4 input does not create import session',async()=>{const h=host(null);assert.equal(await prepare360RaMp4(h,'input','music.mhas'),null);assert.deepEqual(h.calls,[]);});
