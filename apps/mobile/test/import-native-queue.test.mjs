import assert from 'node:assert/strict';
import {test} from 'node:test';
import {build} from 'esbuild';
const bundle=await build({entryPoints:['apps/mobile/App.tsx'],bundle:true,platform:'node',format:'cjs',write:false,define:{__DEV__:'false'},external:['*']});
const a='a'.repeat(64),b='b'.repeat(64);
function fixture(os='android',options={}) {
 const calls={plays:[],analyses:[],queues:[],skips:[],discarded:[],rolledBack:[],demux:0};
 const assets=[{uri:'file:///picker/a.m4a',name:'a.m4a'}];
 class Component {setState(update,done){const value=typeof update==='function'?update(this.state):update;if(value)Object.assign(this.state,value);done?.();}}
 const engine={contentHash:async()=>a,metadata:async()=>JSON.stringify({title:'First'}),preparePlayback:async()=>{},
  prepareStereoTrack:async(...args)=>{calls.analyses.push(args);await options.analysis;return true;},
  retainQueueTrack:async(_uri,name,hash)=>'file:///files/playback-library/'+hash+'.'+name.split('.').at(-1),
  discardPickerCopy:async uri=>{calls.discarded.push(uri);},
  discardUnqueuedTrack:async uri=>{calls.rolledBack.push(uri);},
  configurePlaybackQueue:(text,mode)=>calls.queues.push({tracks:JSON.parse(text),mode}),
  playUri:async(...args)=>{calls.plays.push(args);},setNowPlayingMetadata(){},setVolume(){},
  renderingSettings:()=>JSON.stringify({volumeBalanceEnabled:true,layout:'7.1.4'}),status:()=>JSON.stringify({paused:false}),
  hrtfStatus:()=> 'ready',feedError:()=>null,feedDone:()=>false,objects:()=> '{}'};
 const deps=name=>name==='react'?{Component}:name==='react-native'?{Platform:{OS:os},AppState:{currentState:'active'}}:
  name==='expo-document-picker'?{getDocumentAsync:async()=>({canceled:false,assets})}:
  name.endsWith('/mpeghMp4')?{prepare360RaMp4:async()=>{calls.demux++;return options.prepared??null;}}:
  name.endsWith('/lyric-files')?{readFileLyrics:async()=>[],readExternalLyric:async(uri,name)=>({text:'[0:1]歌词',name,source:'external'}),releaseLyricPickerCopy:async()=>{}}:
  name.endsWith('/mobile-lyrics')?{lyricSources:value=>Array.isArray(value)?value:[]}:
  name.endsWith('/SystemVolumeSlider')?{hasSystemVolumeSlider:false}:
  name.endsWith('/artwork-color')?{primeArtworkColor:async()=>{}}:
  name.endsWith('/playbackOrder')?{nextPlaylistItemId:()=>{throw Error('native EOF must never ask JS to advance');}}:
  name.endsWith('.json')?[]:{};
 const module={exports:{}};new Function('require','module','exports',bundle.outputFiles[0].text)(deps,module,module.exports);
 const app=new module.exports.default({});app.engine=engine;app.getEngine=()=>engine;app.poller=1;
 app.restartStatusPolling=()=>{};return {app,engine,calls,assets};
}
for(const os of ['android','ios'])test(os+': import computes before ready; playing never repeats the JS import step',async()=>{
 let finish;const analysis=new Promise(resolve=>finish=resolve);const {app,calls}=fixture(os,{analysis});
 const task=app.chooseFile();for(let i=0;i<8;i++)await new Promise(resolve=>setImmediate(resolve));
 assert.equal(calls.analyses.length,1);assert.equal(app.state.queue[0].preparation.state,'loading');assert.equal(app.state.queueIndex,-1);assert.equal(calls.plays.length,0);
 finish();await task;assert(!app.state.queue[0].preparation);assert.equal(app.state.queueIndex,0);
 await app.selectTrack(0);await app.selectTrack(0);assert.equal(app.state.error,null);assert.equal(app.state.playing,true);
 assert.equal(calls.analyses.length,1);assert.equal(calls.plays.length,2);
 if(os==='android')assert(calls.plays.every(args=>args[0].startsWith('file:///files/playback-library/')));
});
test('live native playback reattaches its real filename without importing again',async()=>{
 const {app,engine,calls}=fixture();engine.playbackQueueSnapshot=()=>JSON.stringify({queue:[{hash:a,uri:'file:///files/a.mhas',name:'a.m4a.mhas',originalName:'a.m4a',metadata:{title:'First'}},{hash:b,uri:'file:///files/b.mhas',name:'b.m4a.mhas',originalName:'b.m4a',metadata:{title:'Second'}}],currentHash:b,mode:'repeat-all'});
 engine.playbackSnapshot=()=>JSON.stringify({generation:3,trackHash:b,hasPlayback:true,feedDone:false,feedError:null,status:{},objects:{},durationMs:1000,hrtfStatus:'ready'});
 app.attachLiveNativeQueue(engine);assert.equal(app.state.queue.length,2);assert.equal(app.state.queueIndex,1);assert.equal(app.state.fileName,'b.m4a');assert.equal(app.state.playbackMode,'repeat-all');assert.equal(app.state.playing,true);
 await app.selectTrack(1);assert.equal(app.state.error,null);assert.equal(calls.demux,0);assert.equal(calls.plays[0][0],'file:///files/b.mhas');assert.equal(calls.plays[0][1],'b.m4a.mhas');
 app.syncNativeQueue();assert.equal(calls.queues.at(-1).tracks[1].name,'b.m4a.mhas');
});
for(const os of ['android','ios'])test(os+': cold launch ignores a legacy playlist without active playback',()=>{
 const {app,engine}=fixture(os);engine.playbackQueueSnapshot=()=>JSON.stringify({queue:[{hash:a,uri:'file:///old.mhas',name:'old.mhas',metadata:{}}],currentHash:a,mode:'repeat-all'});
 engine.playbackSnapshot=()=>JSON.stringify({generation:0,trackHash:'',hasPlayback:false,feedDone:false,status:{},objects:{}});
 app.attachLiveNativeQueue(engine);assert.equal(app.state.queue.length,0);assert.equal(app.state.queueIndex,-1);assert.equal(app.state.selectedUri,'');
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

for(const os of ['android','ios'])test(os+': prepared import transfers to one durable file and releases temporary ownership',async()=>{
 let released=0;const prepared={uri:'file:///tmp/raw.mhas',name:'a.m4a.mhas',durationMs:12345,
  release:async()=>{released++;}};
 const {app,calls}=fixture(os,{prepared});await app.chooseFile();
 assert.equal(released,1);assert.equal(app.preparedTracks.size,0);
 assert.deepEqual(calls.discarded,['file:///picker/a.m4a']);
 assert.equal(app.state.queue[0].playbackName,'a.m4a.mhas');
 assert(calls.queues.at(-1).tracks[0].uri.startsWith('file:///files/playback-library/'));
 await app.selectTrack(0);assert.equal(calls.demux,1,'playing again must not create another raw file');
 assert.equal(calls.plays[0][0],app.state.queue[0].uri);
});
for(const os of ['android','ios'])test(os+': failed import releases temporary session and rolls back unqueued file',async()=>{
 let released=0;const prepared={uri:'file:///tmp/raw.mhas',name:'a.m4a.mhas',durationMs:1,release:async()=>{released++;}};
 const {app,engine,calls}=fixture(os,{prepared});engine.prepareStereoTrack=async()=>{throw Error('interrupted import');};
 await app.chooseFile();assert.equal(released,1);assert.equal(app.preparedTracks.size,0);
 assert.equal(calls.rolledBack.length,1);assert.equal(calls.discarded.length,1);
 assert.equal(app.state.queue[0].preparation.state,'failed');assert.equal(calls.plays.length,0);
});
for(const os of ['android','ios'])test(os+': duplicate picker copy is discarded without touching library audio',async()=>{
 const {app,calls}=fixture(os);await app.chooseFile();await app.chooseFile();
 assert.equal(app.state.queue.length,1);assert.equal(calls.rolledBack.length,0);
 assert.equal(calls.discarded.length,2);assert.equal(calls.analyses.length,1);
});
for(const os of ['android','ios'])test(os+': seek uses native transport without reimport, playback restart or fake UI time',async()=>{
 const {app,engine,calls}=fixture(os);await app.chooseFile();app.state.durationMs=200000;app.state.positionMs=1000;app.state.playing=true;
 const seeks=[];engine.seekTo=async(...args)=>{seeks.push(args);return true;};
 const imports=calls.demux,analyses=calls.analyses.length;
 await app.seekTo(100013);assert.deepEqual(seeks,[[100013,a]]);
 assert.equal(app.state.positionMs,1000,'only the actual native output clock may move playback time');
 assert.equal(calls.plays.length,0);assert.equal(calls.demux,imports);assert.equal(calls.analyses.length,analyses);
 app.state.paused=true;await app.seekTo(-100);assert.equal(seeks[1][0],0);assert.equal(app.state.paused,true);
 await app.seekTo(1e9);assert.equal(seeks[2][0],199999);
});
for(const os of ['android','ios'])test(os+': superseded seek failure cannot overwrite a newer successful click',async()=>{
 const {app,engine}=fixture(os);await app.chooseFile();app.state.durationMs=200000;app.state.playing=true;
 let rejectOld;engine.seekTo=()=>new Promise((_resolve,reject)=>{rejectOld=reject;});const old=app.seekTo(30000);
 engine.seekTo=async()=>true;await app.seekTo(90000);rejectOld(Error('abandoned seek'));await old;assert.equal(app.state.error,null);
});
for(const os of ['android','ios'])test(os+': queue save failure is surfaced without leaving a ready track or import session',async()=>{
 let released=0;const prepared={uri:'file:///tmp/raw.mhas',name:'a.m4a.mhas',durationMs:1000,release:async()=>{released++;}};
 const {app,engine,calls}=fixture(os,{prepared});engine.configurePlaybackQueue=()=>{throw Error('storage full');};
 await app.chooseFile();assert.equal(app.state.queue[0].preparation.state,'failed');assert.equal(released,1);
 assert.equal(calls.rolledBack.length,1);assert.equal(app.state.error,'storage full');
});
test('older iOS native host keeps temporary ownership until durable retain is available',async()=>{
 let released=0;const prepared={uri:'file:///tmp/raw.mhas',name:'a.m4a.mhas',durationMs:1000,release:async()=>{released++;}};
 const {app,engine,calls}=fixture('ios',{prepared});delete engine.retainQueueTrack;await app.chooseFile();
 assert.equal(app.state.error,null);assert.equal(released,0);assert.equal(calls.discarded.length,0);
 assert.equal(app.preparedTracks.size,1);await app.selectTrack(0);assert.equal(calls.plays[0][0],'file:///tmp/raw.mhas');
});
test('retrying a failed import replaces its failed row without leaving a second loading entry',async()=>{
 const {app,engine}=fixture('ios');engine.prepareStereoTrack=async()=>{throw Error('temporary failure');};await app.chooseFile();assert.equal(app.state.queue.length,1);
 engine.prepareStereoTrack=async()=>true;await app.chooseFile();assert.equal(app.state.queue.length,1);assert(!app.state.queue[0].preparation);assert.equal(app.state.error,null);
});

test('external lyrics selected with a song attach to its metadata, never enter the decoder queue',async()=>{
 const {app,calls,assets}=fixture();assets.push({uri:'file:///picker/a.lrc',name:'a.lrc'});await app.chooseFile();
 assert.equal(app.state.queue.length,1);assert.equal(calls.analyses.length,1);assert.equal(app.state.metadata.lyrics[0].text,'[0:1]歌词');
});
test('attaching lyrics preserves the existing native queue and playback state',()=>{
 const {app,calls}=fixture();app.state.queue=[{contentHash:a,uri:'file:///song',name:'song',metadata:{}}];app.state.queueIndex=0;app.state.selectedUri='file:///song';app.state.playing=true;
 app.attachLyrics('file:///song',{text:'[0:1]一',source:'external'});assert.equal(app.state.playing,true);assert.equal(app.state.queue[0].metadata.lyrics[0].text,'[0:1]一');assert.equal(calls.plays.length,0);assert.equal(calls.analyses.length,0);
});
