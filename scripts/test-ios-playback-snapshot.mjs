import assert from 'node:assert/strict';
import { build } from 'esbuild';
import { trackingModule } from './ios-tracking-test-module.mjs';
const bundle=await build({entryPoints:['apps/mobile/App.tsx'],bundle:true,platform:'node',format:'cjs',write:false,define:{__DEV__:'false'},external:['*']});
function fixture() {
 const writes=[],calls=[];let next=0;
 class Component {
  setState(update,callback) { const value=typeof update==='function'?update(this.state):update;if(value){this.state={...this.state,...value};writes.push(value);}callback?.(); }
 }
 const module={exports:{}};
 const deps=name=>name.endsWith('/head-tracking-session')?trackingModule:name==='react'?{Component}:name==='react-native'?{Platform:{OS:'ios'},AppState:{currentState:'active'}}:name.endsWith('.json')?[]:{};
 new Function('require','module','exports',bundle.outputFiles[0].text)(deps,module,module.exports);
 const app=new module.exports.default({});
 let snapshot={generation:1,trackHash:'a',hasPlayback:true,feedDone:false,feedError:null,durationMs:180000,hrtfStatus:'ready',status:{positionMs:40000,decodedSamplePos:2000000,fifoFrames:12000,paused:false,hrtfReady:true},objects:{}};
 const engine={
  playbackSnapshot:includeObjects=>{calls.push(['snapshot',includeObjects]);return JSON.stringify({...snapshot,objects:includeObjects?snapshot.objects:{}});},
  feedError:()=>{calls.push(['legacyError']);return snapshot.feedError;},feedDone:()=>{calls.push(['legacyDone']);return snapshot.feedDone;},
  currentTrackHash:()=>{calls.push(['legacyHash']);return snapshot.trackHash;},status:()=>{calls.push(['legacyStatus']);return JSON.stringify(snapshot.status);},objects:()=>{calls.push(['legacyObjects']);return JSON.stringify(snapshot.objects);},hrtfStatus:()=> 'ready',configurePlaybackQueue:()=>{},
 };
 app.engine=engine;app.foreground=true;app.state={...app.state,playing:true,queueIndex:0,fileName:'A.m4a',selectedUri:'file:///a',positionMs:40000,durationMs:180000,queue:[{contentHash:'a',name:'A.m4a',uri:'file:///a',metadata:{title:'A',durationMs:180000}},{contentHash:'b',name:'B.m4a',uri:'file:///b',metadata:{title:'B',durationMs:200000}}]};
 app.realSelectTrack=app.selectTrack;
 app.selectTrack=()=>{next++;};
 return {app,engine,writes,calls,next:()=>next,set:value=>{snapshot={...snapshot,...value};}};
}
// JS was last visible on A at 40s. Native playback advances to B and ends
// while JS is in the background. Returning must show B's final clock.
const ended=fixture();ended.app.foreground=false;
ended.set({generation:2,trackHash:'b',feedDone:true,durationMs:199968,status:{positionMs:199968,decodedSamplePos:9598464,fifoFrames:0,paused:true}});
ended.app.pollStatus();ended.app.foreground=true;ended.app.pollStatus();
assert.equal(ended.app.state.fileName,'B.m4a','background EOF must reconcile the actual last track');
assert.equal(ended.app.state.positionMs,199968,'ended playback must never retain A at 40 seconds');
assert.equal(ended.app.state.durationMs,199968);assert.equal(ended.app.state.ended,true);assert.equal(ended.app.state.playing,false);assert.equal(ended.next(),0);
assert(ended.calls.every(([name])=>name==='snapshot'),'identity, EOF and clock must come from one native snapshot');
// Identity, metadata and clocks are committed together across an auto-next.
const next=fixture();next.set({generation:2,trackHash:'b',durationMs:200000,status:{positionMs:320,decodedSamplePos:48000,paused:false},objects:{x:{hasPos:true,pos:[0,0,1]}}});next.app.pollStatus();
assert.equal(next.writes.length,1);assert.equal(next.app.state.fileName,'B.m4a');assert.equal(next.app.state.positionMs,320);assert.equal(next.app.state.metadata.title,'B');
// A late snapshot from an older decoder must not replace the current track.
next.set({generation:1,trackHash:'a',status:{positionMs:170000}});next.app.pollStatus();assert.equal(next.app.state.fileName,'B.m4a');assert.equal(next.app.state.positionMs,320);
// Repeat-one has the same content hash but a new playback generation.
next.set({generation:3,trackHash:'b',status:{positionMs:0,decodedSamplePos:0,paused:false},objects:{}});next.app.pollStatus();assert.equal(next.app.state.positionMs,0);assert.equal(next.app.state.ended,false);
// Background steady playback does not rebuild hidden UI or poll object graphs.
const quiet=fixture();quiet.app.pollStatus();quiet.app.foreground=false;quiet.calls.length=0;quiet.writes.length=0;quiet.set({status:{positionMs:41000}});quiet.app.pollStatus();assert.deepEqual(quiet.calls,[['snapshot',false]]);assert.equal(quiet.writes.length,0);
// An error belongs to its snapshot and is cleared when a newer track succeeds.
const failure=fixture();failure.set({feedError:'cannot decode',status:{positionMs:45000}});failure.app.pollStatus();assert.equal(failure.app.state.error,'cannot decode');failure.set({generation:2,trackHash:'b',feedError:null,status:{positionMs:0}});failure.app.pollStatus();assert.equal(failure.app.state.error,null);
const terminal=fixture();terminal.set({generation:2,trackHash:'b',feedDone:true,feedError:'no decoded audio',status:{positionMs:0,paused:true}});terminal.app.pollStatus();
assert.equal(terminal.app.state.fileName,'B.m4a');assert.equal(terminal.app.state.playing,false);assert.equal(terminal.app.state.error,'no decoded audio');assert.equal(terminal.next(),0,'a decode failure must not silently advance the queue');
// File preparation owns the native player lock; no synchronous tracker poll
// may stall JavaScript while an asynchronous play request is still preparing.
const preparing=fixture();preparing.app.state.busy=true;
preparing.engine.headTrackingStatus=()=>{throw new Error('busy native player was synchronously queried');};
preparing.app.pollStatus();assert.equal(preparing.app.state.error,null);assert.equal(preparing.calls.length,0);


// A new song must never inherit the old codec while preparing, even when
// initialization fails before there is a new native playback snapshot.
for (const stage of ['prepare', 'native-open']) {
 const change=fixture();change.app.state.sourceCodec='truehd';change.app.state.alacUpmixActive=true;change.app.state.systemSpatial360RAActive=true;change.app.state.outputChannels=12;
 change.app.getEngine=()=>change.engine;change.app.poller=1;change.app.syncNativeQueue=()=>{};
 let reject;
 const pending=new Promise((_,fail)=>{reject=fail;});
 change.app.prepareTrack=stage==='prepare'?()=>pending:async()=>null;
 change.engine.playUri=()=>pending;
 const operation=change.app.realSelectTrack(1);
 assert.equal(change.app.state.fileName,'B.m4a');
 assert.equal(change.app.state.sourceCodec,'','new source must clear the previous TrueHD label before awaiting work');
 assert.equal(change.app.state.alacUpmixActive,false);assert.equal(change.app.state.systemSpatial360RAActive,false);assert.equal(change.app.state.outputChannels,2);
 await Promise.resolve();reject(new Error('fixture load failure'));await operation;
 assert.equal(change.app.state.sourceCodec,'','failed preparation must not display the previous decoder');
 assert.equal(change.app.state.error,'fixture load failure');
}

console.log('iOS playback snapshots: atomic EOF, generations, repeat-one, loading and failed-load codec isolation passed.');
