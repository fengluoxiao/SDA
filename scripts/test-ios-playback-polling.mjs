import assert from 'node:assert/strict';
import { build } from 'esbuild';
const bundle = await build({entryPoints:['apps/mobile/App.tsx'], bundle:true, platform:'node', format:'cjs', write:false, external:['*']});
function makeApp(platform = 'ios') {
 const listeners = {}, timers = new Map(); let sequence = 0, writes = 0;
 class Component {
  setState(update, callback) { this.state = {...this.state, ...(typeof update === 'function' ? update(this.state) : update)}; writes++; callback?.(); }
 }
 const React = {Component};
 const native = {Platform:{OS:platform}, AppState:{currentState:'active', addEventListener:(_, fn) => {listeners.change = fn; return {remove(){delete listeners.change;}};}}, Linking:{addEventListener:()=>({remove(){}})}};
 const module = {exports:{}};
 const deps = name => name === 'react' ? React : name === 'react-native' ? native : name.endsWith('.json') ? [] : {};
 new Function('require','module','exports','setInterval','clearInterval',bundle.outputFiles[0].text)(deps,module,module.exports,(fn,delay)=>{const id=++sequence;timers.set(id,{fn,delay});return id;},id=>timers.delete(id));
 const app = new module.exports.default({});
 const calls = []; let done = false, error = null, next = 0;
 const engine = {renderingSettings:()=> '{}', rooms:()=> '[]', feedError:()=>{calls.push('error');return error;}, feedDone:()=>{calls.push('done');return done;}, status:()=>{calls.push('status');return '{}';}, objects:()=>{calls.push('objects');return '{}';}, hrtfStatus:()=>{calls.push('hrtf');return 'ready';}};
 app.getEngine = () => {app.engine = engine;return engine;};
 app.advancePlaylist = () => {next++;};
 app.componentDidMount(); app.state.playing = true; app.restartStatusPolling();
 return {app,calls,timers,listeners, setDone:value=>{done=value;}, setError:value=>{error=value;}, writes:()=>writes,next:()=>next};
}
const ios = makeApp();
assert.equal([...ios.timers.values()][0].delay,125);
ios.app.pollStatus(); assert.ok(ios.calls.includes('objects'));
ios.listeners.change('background');
assert.equal(ios.timers.size,1); assert.equal([...ios.timers.values()][0].delay,1000);
ios.calls.length=0; const writes=ios.writes();
ios.app.pollStatus(); assert.deepEqual(ios.calls,['error','done']); assert.equal(ios.writes(),writes);
ios.setDone(true); ios.app.pollStatus(); assert.equal(ios.next(),1); assert.equal(ios.app.state.ended,true);
ios.setDone(false); ios.app.state.playing=true; ios.setError('decode failed'); ios.app.pollStatus(); assert.equal(ios.app.state.error,'decode failed'); assert.equal(ios.next(),1);
ios.setError(null); ios.calls.length=0; ios.listeners.change('active');
assert.equal([...ios.timers.values()][0].delay,125); assert.ok(ios.calls.includes('objects'));
ios.listeners.change('inactive'); assert.equal([...ios.timers.values()][0].delay,1000);
ios.app.componentWillUnmount(); assert.equal(ios.timers.size,0); assert.equal(ios.listeners.change,undefined);
const android = makeApp('android'); android.listeners.change('background');
assert.equal([...android.timers.values()][0].delay,80); android.app.pollStatus(); assert.ok(android.calls.includes('objects'));
android.app.componentWillUnmount();
console.log('Playback polling checks passed: foreground/background cadence, no hidden UI work, end/error handling, resume, teardown, Android unchanged');
