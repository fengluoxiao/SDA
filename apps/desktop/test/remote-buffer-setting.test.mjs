import {test} from 'node:test';
import assert from 'node:assert/strict';
import {StereoPcmBuffer} from '../remote-web/pcm-buffer.mjs';
test('selected buffer survives a 300ms delivery gap without starving or changing samples',()=>{
 const b=new StereoPcmBuffer();b.configure(1000);
 b.push(new Float32Array(480*2).fill(.125));
 const l=new Float32Array(480),r=new Float32Array(480);
 assert.equal(b.fill(l,r),0);
 for(let i=1;i<100;i++)b.push(new Float32Array(480*2).fill(.125));
 for(let i=0;i<30;i++){assert.equal(b.fill(l,r),480);assert.ok(l.every(v=>v===.125));}
 assert.equal(b.buffering,false);
 b.reset();assert.equal(b.fill(l,r),0);assert.equal(b.refill,46080);
});
test('all offered buffer sizes fit credit windows and default safely',()=>{
 for(const ms of [100,300,600,1000]){const b=new StereoPcmBuffer();b.configure(ms);assert.ok(b.refill<=ms*48);assert.ok(b.refill>0);}
 const b=new StereoPcmBuffer();b.configure(NaN);assert.equal(b.refill,12480);
});
