import assert from 'node:assert/strict';
import {test} from 'node:test';
import {build} from 'esbuild';
const bundle=await build({entryPoints:['apps/mobile/App.tsx'],bundle:true,platform:'node',format:'cjs',write:false,define:{__DEV__:'false'},external:['*']});
const a='a'.repeat(64),b='b'.repeat(64);
function fixture(os='android',options={}) {
 const calls={plays:[],analyses:[],queues:[],skips:[],demux:0};
 const assets=[{uri:'file:///picker/a.m4a',name:'a.m4a'}];
 class Component {setState(update,done){const value=typeof update==='function'?update(this.state):update;if(value)Object.assign(this.state,value);done?.();}}
 const engine={contentHash:async()=>a,metadata:async()=>JSON.stringify({title:'First'}),preparePlayback:async()=>{},
  prepareStereoTrack:async(...args)=>{calls.analyses.push(args);await options.analysis;return true;},
  retainQueueTrack:async(_uri,name,hash)=>'file:///files/playback-library/'+hash+'.'+name.split('.').at(-1),
  configurePlaybackQueue:(text,mode)=>calls.queues.push({tracks:JSON.parse(text),mode}),
  playUri:async(...args)=>{calls.plays.push(args);},setNowPlayingMetadata(){},setVolume(){},
  renderingSettings:()=>JSON.stringify({volumeBalanceEnabled:true,layout:'7.1.4'}),status:()=>JSON.stringify({paused:false}),
  hrtfStatus:()=> 'ready',feedError:()=>null,feedDone:()=>false,objects:()=> '{}'};
 const deps=name=>name==='react'?{Component}:name==='react-native'?{Platform:{OS:os},AppState:{currentState:'active'}}:
  name==='expo-document-picker'?{getDocumentAsync:async()=>({canceled:false,assets})}:
  name.endsWith('/mpeghMp4')?{prepare360RaMp4:async()=>{calls.demux++;return options.prepared??null;}}:
  name.endsWith('/SystemVolumeSlider')?{hasSystemVolumeSlider:false}:
  name.endsWith('/artwork-color')?{primeArtworkColor:async()=>{}}:
  name.endsWith('/playbackOrder')?{nextPlaylistItemId:()=>{throw Error('native EOF must never ask JS to advance');}}:
  name.endsWith('.json')?[]:{};
 const module={exports:{}};new Function('require','module','exports',bundle.outputFiles[0].text)(deps,module,module.exports);
 const app=new module.exports.default({});app.engine=engine;app.getEngine=()=>engine;app.poller=1;
 app.restartStatusPolling=()=>{};return {app,engine,calls,assets};
}
for(const os of ['android','ios'])test(os+': import finishes full stereo measurement before ready; repeated play never reanalyses',async()=>{
 let finish;const analysis=new Promise(resolve=>finish=resolve);const {app,calls}=fixture(os,{analysis});
 const task=app.chooseFile();for(let i=0;i<8;i++)await new Promise(resolve=>setImmediate(resolve));
 assert.equal(calls.analyses.length,1);assert.equal(app.state.queue[0].preparation.state,'loading');assert.equal(app.state.queueIndex,-1);assert.equal(calls.plays.length,0);
 finish();await task;assert(!app.state.queue[0].preparation);assert.equal(app.state.queueIndex,0);
 await app.selectTrack(0);await app.selectTrack(0);assert.equal(app.state.error,null);assert.equal(app.state.playing,true);
 assert.equal(calls.analyses.length,1);assert.equal(calls.plays.length,2);
 if(os==='android')assert(calls.plays.every(args=>args[0].startsWith('file:///files/playback-library/')));
});
test('Android restored compressed queue keeps its real MHAS filename and durable URI',async()=>{
 const {app,engine,calls}=fixture();engine.playbackQueueSnapshot=()=>JSON.stringify({queue:[{hash:a,uri:'file:///files/a.mhas',name:'a.m4a.mhas',originalName:'a.m4a',metadata:{title:'First'}},{hash:b,uri:'file:///files/b.mhas',name:'b.m4a.mhas',originalName:'b.m4a',metadata:{title:'Second'}}],currentHash:b,mode:'repeat-all'});
 engine.playbackSnapshot=()=>JSON.stringify({generation:3,trackHash:b,hasPlayback:false,feedDone:false,feedError:null,status:{},objects:{},durationMs:1000,hrtfStatus:'ready'});
 app.restoreNativeQueue(engine);assert.equal(app.state.queue.length,2);assert.equal(app.state.queueIndex,1);assert.equal(app.state.fileName,'b.m4a');assert.equal(app.state.playbackMode,'repeat-all');assert.equal(app.state.playing,false);
 await app.selectTrack(1);assert.equal(app.state.error,null);assert.equal(calls.demux,0);assert.equal(calls.plays[0][0],'file:///files/b.mhas');assert.equal(calls.plays[0][1],'b.m4a.mhas');
 app.syncNativeQueue();assert.equal(calls.queues.at(-1).tracks[1].name,'b.m4a.mhas');
});
for(const os of ['android','ios'])test(os+': suspended JS reconciles the native current track without a second advancement',()=>{
 const {app,engine,calls}=fixture(os);app.foreground=false;app.state.queue=[{contentHash:a,uri:'file:///a',name:'First',metadata:{}},{contentHash:b,uri:'file:///b',name:'Second',metadata:{title:'Second'}}];app.state.queueIndex=0;app.state.playing=true;
 engine.playbackSnapshot=()=>JSON.stringify({generation:4,trackHash:b,hasPlayback:true,feedDone:false,feedError:null,durationMs:10000,status:{positionMs:1800},objects:{},hrtfStatus:'ready'});
 app.pollStatus();assert.equal(app.state.queueIndex,1);assert.equal(app.state.positionMs,1800);assert.equal(app.state.metadata.title,'Second');assert.equal(app.state.queue.length,2);app.advancePlaylist();assert.equal(calls.plays.length,0);
 // An obsolete source cannot overwrite the current clock or discard the queue.
 engine.playbackSnapshot=()=>JSON.stringify({generation:2,trackHash:a,hasPlayback:true,feedDone:false,status:{positionMs:99},objects:{}});app.pollStatus();assert.equal(app.state.queueIndex,1);assert.equal(app.state.positionMs,1800);
});
test('Android transport sends explicit skips to native instead of scheduling JS playback',()=>{
 const {app,engine,calls}=fixture();app.state.playing=true;engine.skipPlayback=direction=>{calls.skips.push(direction);return true;};
 app.skipTrack(1);app.skipTrack(-1);assert.deepEqual(calls.skips,[1,-1]);assert.equal(calls.plays.length,0);
});
test('failed stereo preparation leaves an unready row and never starts playback',async()=>{
 const {app,engine,calls}=fixture();engine.prepareStereoTrack=async()=>{throw Error('measurement failed');};await app.chooseFile();
 assert.equal(app.state.queue[0].preparation.state,'failed');assert.equal(app.state.queueIndex,-1);assert.equal(calls.plays.length,0);
});
