import assert from 'node:assert/strict';
import { build } from 'esbuild';
const bundle = await build({entryPoints:['apps/mobile/App.tsx'],bundle:true,platform:'node',format:'cjs',write:false,external:['*']});
function fixture(platform='ios') {
 let preparations=0, releases=0, stops=0, plays=0, configured, currentHash='a';
 class Component { setState(update, callback) {this.state={...this.state,...update};callback?.();} }
 const module={exports:{}};
 const engine={
  configurePlaybackQueue:(text,mode)=>{configured={queue:JSON.parse(text),mode};},
  currentTrackHash:()=>currentHash,
  playUri:async()=>{plays++;}, stop:()=>{stops++;},
  hrtfStatus:()=>'',renderingSettings:()=> '{}',setVolume:()=>{},
  feedError:()=>null,feedDone:()=>false,status:()=> '{}',objects:()=> '{}',
 };
 const deps=name=> name==='react'?{Component}:name==='react-native'?{Platform:{OS:platform},AppState:{currentState:'active'}}:
  name.endsWith('/mpeghMp4')?{prepare360RaMp4:async(_host,uri,name)=>{preparations++;return {uri:uri+'.mhas',name:name+'.mhas',durationMs:42000,release:async()=>{releases++;}};}}:
  name.endsWith('.json')?[]:{};
 new Function('require','module','exports',bundle.outputFiles[0].text)(deps,module,module.exports);
 const app=new module.exports.default({});
 app.engine=engine;app.getEngine=()=>engine;app.poller=1;
 app.state.queue=[{contentHash:'a',uri:'file:///a.m4a',name:'a.m4a',metadata:{}},{contentHash:'b',uri:'file:///b.m4a',name:'b.m4a',metadata:{title:'Second'}}];
 return {app,engine,counts:()=>({preparations,releases,stops,plays}),configured:()=>configured,setHash:hash=>{currentHash=hash;}};
}
const ios=fixture();
for(const track of ios.app.state.queue) await ios.app.prepareTrack(track);
await ios.app.selectTrack(0);
await ios.app.selectTrack(0);
assert.deepEqual(ios.counts(),{preparations:2,releases:0,stops:0,plays:2},'repeat must reuse owned extraction; never stop audio before prep');
assert.equal(ios.configured().queue[1].uri,'file:///b.m4a.mhas');
assert.equal(ios.configured().queue[1].metadata.durationMs,42000);
ios.app.setPlaybackMode('repeat-one');assert.equal(ios.configured().mode,'repeat-one');
// Simulate native EOF while JS did not execute any callback.
ios.setHash('b');ios.app.foreground=true;ios.app.pollStatus();
assert.equal(ios.app.state.queueIndex,1);assert.equal(ios.app.state.fileName,'b.m4a');
assert.equal(ios.app.state.durationMs,42000);assert.equal(ios.app.state.metadata.title,'Second');
let duplicates=0;ios.app.selectTrack=()=>{duplicates++;};ios.app.advancePlaylist();assert.equal(duplicates,0);
const android=fixture('android');
await android.app.selectTrack(0);await android.app.selectTrack(0);
assert.deepEqual(android.counts(),{preparations:2,releases:2,stops:0,plays:2});
assert.equal(android.configured(),undefined);
console.log('Native queue bridge: cached preparation, owned lifetime, no pre-stop, foreground reconciliation, repeat mode, no duplicate EOF, Android lifetime passed');

