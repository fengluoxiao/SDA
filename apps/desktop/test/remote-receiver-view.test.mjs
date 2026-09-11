import {test} from 'node:test';
import assert from 'node:assert/strict';
import {receiverView} from '../remote-web/playback-sync.mjs';
test('explicit pause overrides held output loading and preserves the media position',()=>{
 const o={ready:true,hlsClock:true,syncWaiting:true,mediaWaiting:true,audio:{paused:true,currentTime:42.05,readyState:4}};
 const s={currentId:'a',playing:true,paused:true,loading:true,position:42.2};
 const v=receiverView(o,s);assert.equal(v.buffering,false);assert.equal(v.label,'已暂停');assert.equal(v.position,42.05);
 o.pendingStart=true;assert.equal(receiverView(o,s).buffering,true);
});
test('synchronized rebuffering holds the elapsed position rather than resetting to zero',()=>{
 const o={ready:true,hlsClock:true,mediaWaiting:false,audio:{paused:false,currentTime:42,readyState:4}};
 const s={currentId:'a',playing:true,paused:false,position:42};
 assert.equal(receiverView(o,s).position,42);
 o.syncWaiting=true;o.audio.paused=true;
 const v=receiverView(o,{...s,loading:true});assert.equal(v.position,42);assert.equal(v.buffering,true);
});
test('HLS waits for actual playback, freezes rebuffering and resets on track change',()=>{
 const o={ready:true,audio:{paused:false,readyState:4,seeking:false}};
 const s={currentId:'a',playing:true,position:0};
 assert.equal(receiverView(o,s).buffering,true);
 assert.equal(receiverView(o,{...s,position:3}).position,0);
 o.mediaWaiting=false;
 assert.equal(receiverView(o,{...s,position:4}).audible,true);
 o.mediaWaiting=true;
 assert.equal(receiverView(o,{...s,position:7}).position,4);
 assert.equal(receiverView(o,{...s,currentId:'b',position:0}).position,0);
 o.audio.paused=true;
 assert.equal(receiverView(o,s).buffering,false);
 assert.equal(receiverView(o,s).audible,false);
});
test('PCM holds while buffering and honors host pause',()=>{
 const o={ready:true,context:{state:'running'},buffering:true};
 const s={currentId:'a',playing:true,position:12};
 receiverView(o,s);
 assert.equal(receiverView(o,{...s,position:15}).position,12);
 o.buffering=false;
 assert.equal(receiverView(o,{...s,position:16}).position,16);
 assert.equal(receiverView(o,{...s,paused:true,position:17}).audible,false);
 assert.equal(o.displayPosition,17);
});
test('idle HLS already playing cannot open the song startup gate',()=>{
 const o={ready:true,audio:{paused:false,readyState:4,seeking:false},mediaWaiting:false};
 const s={currentId:'a',playing:true,loading:true,position:0};
 for(const position of [0,.1,5]){const v=receiverView(o,{...s,position});assert.equal(v.buffering,true);assert.equal(v.audible,false);assert.equal(v.position,0);}
 const v=receiverView(o,{...s,loading:false,position:.02});assert.equal(v.audible,true);assert.equal(v.position,.02);
});
test('local play request shows loading before host acknowledges startup',()=>{
 const o={ready:true,pendingStart:true,audio:{paused:true,readyState:4}};
 const v=receiverView(o,{playing:false,position:10});assert.equal(v.buffering,true);assert.equal(v.position,10);assert.equal(v.audible,false);
});
