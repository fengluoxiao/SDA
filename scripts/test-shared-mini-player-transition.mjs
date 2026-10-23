import assert from 'node:assert/strict';
import {build} from 'esbuild';
const bundle=async file=>(await build({entryPoints:[file],bundle:true,platform:'node',format:'cjs',write:false,external:['*']})).outputFiles[0].text;
function load(code,require){const m={exports:{}};new Function('require','module','exports',code)(require,m,m.exports);return m.exports;}
const geometry=load(await bundle('apps/mobile/src/mini-player-motion.ts'),()=>({}));
const frames={artwork:{x:0,y:0,width:390,height:390},progress:{x:450,y:220,width:280,height:5},time:{x:450,y:240,width:24,height:14},mini:{x:780,y:42,width:48,height:54}};
for(const fraction of [0,.01,.25,.5,1,2,NaN]) {
 const parts=geometry.progressMotionSegments(frames,fraction);const ratio=Number.isFinite(fraction)?Math.max(0,Math.min(1,fraction)):0;
 if(!ratio){assert.equal(parts.length,0);continue;}
 assert.equal(parts.length,24);assert.equal(parts[0].ax[0],frames.progress.x);
 assert(Math.abs(parts.at(-1).bx[0]-(frames.progress.x+280*ratio))<1e-8);
 for(const part of parts)for(let i=0;i<geometry.MINI_MOTION_SAMPLES.length;i++) {
  assert([part.x[i],part.y[i],part.width[i],part.height[i]].every(Number.isFinite));assert(part.width[i]>0);assert(part.height[i]>=2);
 }
 for(const part of parts)for(const [x,y] of [[part.ax.at(-1),part.ay.at(-1)],[part.bx.at(-1),part.by.at(-1)]])assert(Math.abs(Math.hypot(x-804,y-66)-23)<1e-8,'destination remains a circle, never a pill');
 for(let i=1;i<parts.length;i++)for(let step=0;step<21;step++) {assert(Math.abs(parts[i-1].bx[step]-parts[i].ax[step])<1e-8);assert(Math.abs(parts[i-1].by[step]-parts[i].ay[step])<1e-8);}
}
assert.deepEqual(geometry.miniArtworkRect(frames.mini),{x:787,y:49,width:34,height:34});assert.deepEqual(geometry.miniTimeCenter(frames.mini),{x:804,y:90.5});
// Each component has its own straight route in either direction and on either
// side of the screen. A compact ring straightens before a long track expands.
const onSegment=(point,from,to)=>{
  const dx=to.x-from.x,dy=to.y-from.y;
  assert(Math.abs((point.x-from.x)*dy-(point.y-from.y)*dx)<1e-6,'motion must remain on the straight segment');
  assert(point.x>=Math.min(from.x,to.x)-1e-8&&point.x<=Math.max(from.x,to.x)+1e-8);
  assert(point.y>=Math.min(from.y,to.y)-1e-8&&point.y<=Math.max(from.y,to.y)+1e-8);
};
for(const miniX of [20,780]){
  const layout={...frames,mini:{...frames.mini,x:miniX}};
  const coverStart={x:layout.artwork.x+layout.artwork.width/2,y:layout.artwork.y+layout.artwork.height/2};
  const progressStart={x:layout.progress.x+layout.progress.width/2,y:layout.progress.y+layout.progress.height/2};
  const timeStart={x:layout.time.x+layout.time.width/2,y:layout.time.y+layout.time.height/2};
  const dock={x:layout.mini.x+24,y:layout.mini.y+24},timeEnd=geometry.miniTimeCenter(layout.mini);
  for(const phase of geometry.MINI_MOTION_SAMPLES){
    const pose=geometry.miniMotionPose(layout,phase);
    onSegment(pose.center,coverStart,dock);onSegment(pose.ringCenter,progressStart,dock);onSegment(pose.timeCenter,timeStart,timeEnd);
    assert([pose.width,pose.height,pose.timeScale].every(Number.isFinite));
    if(phase<=.65)assert.equal(pose.bend,0,'a long bar stays straight');
  }
  const halfway=geometry.miniMotionPose(layout,.5);
  assert.notDeepEqual(halfway.center,halfway.ringCenter,'progress travels separately from the cover');
  assert.notEqual(halfway.center.x,halfway.timeCenter.x,'time returns to its own position');
  const initial=geometry.miniMotionPose(layout,0),final=geometry.miniMotionPose(layout,1);
  assert.deepEqual(initial.center,coverStart);assert.deepEqual(initial.ringCenter,progressStart);assert.deepEqual(initial.timeCenter,timeStart);
  assert.equal(initial.width,layout.artwork.width);assert.equal(final.width,34);
  assert.deepEqual(final.center,dock);assert.deepEqual(final.ringCenter,dock);assert.deepEqual(final.timeCenter,timeEnd);
}
const track=geometry.progressMotionSegments(frames,1);
for(let step=0;step<geometry.MINI_MOTION_SAMPLES.length;step++)if(geometry.MINI_MOTION_SAMPLES[step]<=.65){
  const y=track[0].ay[step];for(const part of track){assert(Math.abs(part.ay[step]-y)<1e-8);assert(Math.abs(part.by[step]-y)<1e-8);}
}
const slots=[],pending=[],raf=new Map(),events={},animations=[],timers=new Map();let cursor=0,dirty=false,frameId=0,timerId=0;
const realTimeout=globalThis.setTimeout,realClearTimeout=globalThis.clearTimeout;globalThis.setTimeout=fn=>{timers.set(++timerId,fn);return timerId;};globalThis.clearTimeout=id=>timers.delete(id);
const same=(a,b)=>a&&b&&a.length===b.length&&a.every((x,i)=>Object.is(x,b[i]));
const hooks={
 useRef(value){const i=cursor++;return slots[i]??(slots[i]={current:value});},
 useState(value){const i=cursor++;if(!slots[i])slots[i]={state:typeof value==='function'?value():value};return [slots[i].state,next=>{const v=typeof next==='function'?next(slots[i].state):next;if(!Object.is(v,slots[i].state)){slots[i].state=v;dirty=true;}}];},
 useMemo(fn,deps){const i=cursor++;if(!slots[i]||!same(slots[i].deps,deps))slots[i]={deps,value:fn()};return slots[i].value;},
 useCallback(fn,deps){return hooks.useMemo(()=>fn,deps);},
};
function effect(fn,deps,layout){const i=cursor++;if(!slots[i]||!same(slots[i].deps,deps)){const previous=slots[i];slots[i]={deps,cleanup:previous?.cleanup};pending.push({i,fn,layout});}}
hooks.useEffect=(fn,deps)=>effect(fn,deps,false);hooks.useLayoutEffect=(fn,deps)=>effect(fn,deps,true);
class Value{constructor(v){this.v=v;this.native=v;}setValue(v){this.v=v;this.native=v;}stopAnimation(cb){if(cb){if(this.deferStop)this.pendingStop=cb;else cb(this.native);}}interpolate(config){return {value:this,config};}}
const animated={Value,timing(value,config){const animation={value,config,from:value.v,start(callback){animation.callback=callback;animations.push(animation);},stop(){animation.stopped=true;animation.callback?.({finished:false});}};return animation;}};
const state={currentState:'active',addEventListener(name,cb){events[name]=cb;return {remove(){delete events[name];}};}};
const accessibility={isReduceMotionEnabled:async()=>false,addEventListener(name,cb){events[name]=cb;return {remove(){delete events[name];}};}};
globalThis.requestAnimationFrame=cb=>{raf.set(++frameId,cb);return frameId;};globalThis.cancelAnimationFrame=id=>raf.delete(id);
const api=load(await bundle('apps/mobile/src/use-shared-mini-player-transition.ts'),name=>name==='react'?hooks:name==='react-native'?{Animated:animated,AppState:state,AccessibilityInfo:accessibility,Easing:{bezier:()=> 'curve'}}:name==='./mini-player-motion'?geometry:{});
const node=(x,y,width,height)=>({measureInWindow:cb=>cb(x,y,width,height)});
let props={enabled:true,page:0,layoutKey:'844:390:right',mediaKey:'track',artworkUri:'file:///cover.jpg',rootRef:{current:node(10,20,844,390)}},result;
function render(){let guard=0;do{assert(guard++<20);dirty=false;cursor=0;result=api.useSharedMiniPlayerTransition(props);const batch=pending.splice(0).sort((a,b)=>Number(b.layout)-Number(a.layout));for(const item of batch){slots[item.i].cleanup?.();slots[item.i].cleanup=item.fn();}}while(dirty);return result;}
function runFrames(){const batch=[...raf.values()];raf.clear();for(const cb of batch)cb();render();}
async function measure(){runFrames();await Promise.resolve();await Promise.resolve();render();}
render();result.artworkRef.current=node(10,20,390,390);result.progressRef.current=node(460,240,280,5);result.timeRef.current=node(460,260,24,14);result.miniRef.current=node(790,62,48,54);
await measure();assert(result.frames);assert.equal(result.mainOpacity,1);assert.equal(result.miniOpacity,0);
// A missing image must never hide the original player or leave a blank transfer.
props={...props,page:1};render();runFrames();assert.equal(result.active,false);assert.equal(result.miniOpacity,1);assert.equal(animations.length,0);
props={...props,page:0};render();await measure();result.onArtworkReady(props.artworkUri);render();
props={...props,page:1};render();assert.equal(result.active,true);assert.equal(result.mainOpacity,0,'a single opaque moving image prevents double-alpha dimming');assert.equal(result.miniOpacity,0);assert.equal(animations.length,0,'wait for committed overlay before starting');runFrames();assert.equal(animations.at(-1).config.toValue,1);assert.equal(animations.at(-1).config.useNativeDriver,true);
const frozen=result.frames,oldAnimation=animations.at(-1),count=animations.length;
result.travel.native=.37;result.travel.v=0;
props={...props,page:2};render();runFrames();assert.equal(animations.length,count,'secondary pages keep the same transfer');assert.equal(result.frames,frozen);
// A late layout callback during transfer must not move its endpoints.
result.miniRef.current=node(40,62,48,54);result.capture();await Promise.resolve();render();assert.equal(result.frames,frozen);result.miniRef.current=node(790,62,48,54);
props={...props,page:0};render();runFrames();assert.equal(animations.at(-1).from,.37,'reverse from native position, not stale JS zero');assert.equal(animations.at(-1).config.toValue,0);
oldAnimation.callback({finished:true});render();assert.equal(result.active,true,'obsolete completion must not hide the new overlay');
animations.at(-1).callback({finished:true});render();assert.equal(result.active,false);assert.equal(result.travel.v,0);assert.equal(result.mainOpacity,1,'return restores real artwork with ordinary opacity');assert.equal(result.miniOpacity,0);
// Native getValue is asynchronous after the driver has taken ownership.
await measure();props={...props,page:1};render();runFrames();animations.at(-1).callback({finished:true});render();
result.travel.deferStop=true;const beforeAsync=animations.length;props={...props,page:0};render();runFrames();assert.equal(result.active,true);assert.notEqual(result.mainOpacity,1,'do not flash the full artwork before the reverse starts');assert.equal(animations.length,beforeAsync);
result.travel.deferStop=false;result.travel.pendingStop(1);render();runFrames();assert.equal(animations.at(-1).from,1);animations.at(-1).callback({finished:true});render();assert.equal(result.mainOpacity,1);
// Lost native completion callback: the bounded deadline still restores stable UI.
await measure();props={...props,page:1};render();runFrames();assert.equal(result.active,true);result.travel.native=.82;
for(const callback of [...timers.values()])callback();render();assert.equal(result.active,false);assert.equal(result.miniOpacity,1);assert.equal(result.mainOpacity,1);assert.equal(result.travel.v,1);
props={...props,page:0};render();runFrames();assert.equal(result.active,true);animations.at(-1).callback({finished:false});render();assert.equal(result.active,false);assert.equal(result.mainOpacity,1,'interrupted native animation restores the real player');
await measure();const beforeReduced=animations.length;events.reduceMotionChanged(true);render();props={...props,page:1};render();runFrames();assert.equal(animations.length,beforeReduced);assert.equal(result.miniOpacity,1);
events.reduceMotionChanged(false);render();props={...props,page:0};render();runFrames();state.currentState='background';events.change('background');render();assert.equal(result.active,false);assert.equal(result.mainOpacity,1);
state.currentState='active';props={...props,layoutKey:'667:375:left'};render();assert.equal(result.frames,null,'rotation discards old coordinates');assert.equal(result.mainOpacity,1);
for(const slot of slots)slot?.cleanup?.();assert.deepEqual(Object.keys(events),[]);assert.equal(timers.size,0);globalThis.setTimeout=realTimeout;globalThis.clearTimeout=realClearTimeout;
console.log('Shared transition passed: separate straight routes, straight track expansion, connected ring geometry, image readiness, committed-node start, native-position reversal, frozen endpoints, stale-callback rejection, timeout recovery, cancellation, reduced motion, background and rotation.');
