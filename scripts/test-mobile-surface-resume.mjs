import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {build} from 'esbuild';
const pure=await build({entryPoints:['apps/mobile/src/surface-layout-state.ts'],bundle:true,format:'esm',platform:'node',write:false});
const {createSurfaceLayoutState,surfaceLayoutSize}=await import('data:text/javascript;base64,'+Buffer.from(pure.outputFiles[0].text).toString('base64'));
const portrait={width:430,height:932},landscape={width:932,height:430};
for(const [before,after] of [[portrait,landscape],[landscape,portrait],[portrait,portrait],[landscape,landscape]]) {
 const model=createSurfaceLayoutState(before,true,()=>{});
 model.measured(before,0);assert.deepEqual(surfaceLayoutSize(model.get()),before);
 const oldRevision=model.get().revision;
 model.activityChanged(false,before);assert.equal(model.get().measured,null);assert.deepEqual(surfaceLayoutSize(model.get()),before);
 model.windowChanged(after);model.activityChanged(true,after);
 assert.deepEqual(surfaceLayoutSize(model.get()),before,'retain snapshot layout until one stable native layout is ready');
 if(before.width!==after.width){model.measured(before,model.get().revision);assert.deepEqual(surfaceLayoutSize(model.get()),before);}
 model.measured(before,oldRevision);assert.equal(model.get().measured,null,'late old callback is discarded');
 assert.equal(model.measured(after,model.get().revision),false);
 assert.equal(model.measured(after,model.get().revision),true);assert.deepEqual(model.get().measured,after);
}
// Actual view measurement can be fresher than the Dimensions event.
const delayed=createSurfaceLayoutState(landscape,true,()=>{});
delayed.measured(landscape,0);delayed.activityChanged(false,landscape);delayed.activityChanged(true,landscape);
delayed.measured(portrait,delayed.get().revision);assert.deepEqual(surfaceLayoutSize(delayed.get()),landscape);
 delayed.measured(portrait,delayed.get().revision);assert.deepEqual(surfaceLayoutSize(delayed.get()),portrait);
delayed.windowChanged(portrait);assert.deepEqual(surfaceLayoutSize(delayed.get()),portrait);
const previous=delayed.get();delayed.measured({width:0,height:0},previous.revision);assert.equal(delayed.get(),previous);
// Foreground rotation now settles before switching the presentation, too.
const normal=createSurfaceLayoutState(portrait,true,()=>{});normal.measured(portrait,0);normal.windowChanged(landscape);
assert.deepEqual(surfaceLayoutSize(normal.get()),portrait);assert.equal(normal.get().measured,null);
 normal.measured(landscape,normal.get().revision);assert.deepEqual(surfaceLayoutSize(normal.get()),portrait);
 normal.measured(landscape,normal.get().revision);assert.deepEqual(surfaceLayoutSize(normal.get()),landscape);

const hook=await build({entryPoints:['apps/mobile/src/use-player-surface-layout.ts'],bundle:true,format:'cjs',platform:'node',write:false,external:['react','react-native']});
function fixture(initial=portrait) {
 let current=initial,active='active',nextFrame=0,windowListener,activityListener,removed=0;
 const frames=new Map(),measurements=[],snapshots=[],effects=[];
 const mod={exports:{}};
 const react={useState:fn=>[fn(),value=>snapshots.push(value)],useRef:value=>({current:value}),useEffect:fn=>effects.push(fn),useCallback:fn=>fn};
 const native={View:'View',Dimensions:{get:()=>current,addEventListener:(_event,fn)=>{windowListener=fn;return{remove(){removed++;}};}},AppState:{get currentState(){return active;},addEventListener:(_event,fn)=>{activityListener=fn;return{remove(){removed++;}};}}};
 new Function('require','module','exports','requestAnimationFrame','cancelAnimationFrame',hook.outputFiles[0].text)(name=>name==='react'?react:native,mod,mod.exports,fn=>{frames.set(++nextFrame,fn);return nextFrame;},id=>frames.delete(id));
 const result=mod.exports.usePlayerSurfaceLayout();result.surfaceRef.current={measure(){throw Error('transformed measure() must not be used');},measureLayout:(relative,fn)=>{assert.equal(relative,result.surfaceRef.current);measurements.push(fn);}};
 const cleanup=effects[0]();
 return {result,snapshots,measurements,frames,cleanup,removed:()=>removed,
  frame(){const [id,fn]=frames.entries().next().value;frames.delete(id);fn();},
  window(size){current=size;windowListener({window:size});},
  activity(value,size=current){active=value;current=size;activityListener(value);},
 };
}
for(const [before,after] of [[portrait,landscape],[landscape,portrait],[portrait,portrait]]) {
 const f=fixture(before);f.frame();const stale=f.measurements.shift();
 f.activity('inactive');f.activity('background');f.window(after);assert.equal(f.frames.size,0,'never measure the background view');
 f.activity('active',after);assert.deepEqual(f.snapshots.at(-1).window,after);assert.equal(f.snapshots.at(-1).measured,null);
 stale(0,0,before.width,before.height);assert.equal(f.snapshots.at(-1).measured,null);
 // Simulate an old queued onLayout event. Ignore its payload and measure live.
 f.result.onSurfaceLayout({nativeEvent:{layout:before}});assert.equal(f.frames.size,1);
 f.frame();f.measurements.shift()(0,0,after.width,after.height);assert.equal(f.snapshots.at(-1).measured,null);
 f.frame();f.measurements.shift()(0,0,after.width,after.height);assert.deepEqual(f.snapshots.at(-1).measured,after);
 f.result.onSurfaceLayout();f.cleanup();assert.equal(f.frames.size,0);assert.equal(f.removed(),2);
}
const late=fixture();late.frame();const callback=late.measurements.shift();late.cleanup();const count=late.snapshots.length;callback(0,0,932,430);assert.equal(late.snapshots.length,count);
const ui=readFileSync('apps/mobile/src/IOSPlayer.tsx','utf8'),scene=readFileSync('apps/mobile/src/MobileObjectScene.tsx','utf8');
assert.match(ui,/ref=\{surfaceRef\}/);assert.match(ui,/onLayout=\{onSurfaceLayout\}/);assert.match(ui,/active=\{page === 2 && !settings && layoutReady\}/);
assert.doesNotMatch(ui,/surfaceSize \|\| initialWindow/);
assert.match(scene,/<Canvas[^>]+style=\{\{ flex: 1 \}\}/);
assert.doesNotMatch(scene,/<Canvas[^>]+style=\{\{ width: viewport.width/);
console.log('Surface resume: both orientation directions, same-orientation return, stale native callbacks, delayed Dimensions, live measurement, cleanup and flex GL viewport passed.');

const racing=fixture(portrait);racing.frame();racing.measurements.shift()(0,0,430,932);
racing.activity('inactive');racing.window(landscape);assert.deepEqual(surfaceLayoutSize(racing.snapshots.at(-1)),portrait);
racing.activity('active',landscape);racing.frame();racing.measurements.shift()(0,0,430,932);assert.deepEqual(surfaceLayoutSize(racing.snapshots.at(-1)),portrait);
racing.frame();racing.measurements.shift()(0,0,932,430);racing.frame();racing.measurements.shift()(0,0,932,430);
assert.deepEqual(surfaceLayoutSize(racing.snapshots.at(-1)),landscape);
const layouts=racing.snapshots.map(surfaceLayoutSize).map(x=>x.width>x.height?'landscape':'portrait').filter((x,i,a)=>i===0||x!==a[i-1]);
assert.deepEqual(layouts,['portrait','landscape'],'resume commits one orientation change without bouncing back');racing.cleanup();
const fabric=fixture();fabric.result.surfaceRef.current={offsetWidth:430,offsetHeight:932,measure(){throw Error('do not use presentation bounds');}};
fabric.frame();assert.deepEqual(fabric.snapshots.at(-1).measured,portrait);fabric.cleanup();
const reversed=fixture();reversed.frame();const older=reversed.measurements.shift();reversed.result.onSurfaceLayout();reversed.frame();const newer=reversed.measurements.shift();newer(0,0,430,932);const updatedCount=reversed.snapshots.length;older(0,0,932,430);assert.equal(reversed.snapshots.length,updatedCount);reversed.cleanup();

// Both event orders occur on-device while the app remains active.
for(const [before,after] of [[portrait,landscape],[landscape,portrait]]) {
 const nativeFirst=createSurfaceLayoutState(before,true,()=>{});nativeFirst.measured(before,0);
 assert.equal(nativeFirst.measured(after,0),false);assert.equal(nativeFirst.get().settling,true);
 assert.deepEqual(surfaceLayoutSize(nativeFirst.get()),before);
 nativeFirst.windowChanged(after);const revision=nativeFirst.get().revision;
 nativeFirst.measured(before,revision);assert.deepEqual(surfaceLayoutSize(nativeFirst.get()),before);
 assert.equal(nativeFirst.measured(after,revision),false);assert.equal(nativeFirst.measured(after,revision),true);
 assert.deepEqual(surfaceLayoutSize(nativeFirst.get()),after);
 const f=fixture(before);f.frame();f.measurements.shift()(0,0,before.width,before.height);
 f.window(after);assert.equal(f.snapshots.at(-1).settling,true);assert.equal(f.snapshots.at(-1).measured,null);
 f.frame();f.measurements.shift()(0,0,before.width,before.height);
 f.frame();f.measurements.shift()(0,0,after.width,after.height);
 f.frame();f.measurements.shift()(0,0,after.width,after.height);
 const shown=f.snapshots.map(surfaceLayoutSize).map(x=>x.width>x.height?'landscape':'portrait').filter((x,i,a)=>i===0||x!==a[i-1]);
 assert.deepEqual(shown,[before.width>before.height?'landscape':'portrait',after.width>after.height?'landscape':'portrait']);f.cleanup();
}
const quick=createSurfaceLayoutState(portrait,true,()=>{});quick.measured(portrait,0);quick.windowChanged(landscape);const abandoned=quick.get().revision;
quick.measured(landscape,abandoned);quick.windowChanged(portrait);quick.measured(landscape,abandoned);assert.deepEqual(surfaceLayoutSize(quick.get()),portrait);
quick.measured(portrait,quick.get().revision);quick.measured(portrait,quick.get().revision);assert.equal(quick.get().settling,false);
console.log('Foreground rotation: both directions/event orders, stale intermediate sizes and quick reversal settle without extra page or orientation flips.');
