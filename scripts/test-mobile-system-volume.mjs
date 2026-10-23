import assert from 'node:assert/strict';
import { build } from 'esbuild';
const compile = async file => (await build({entryPoints:[file],bundle:true,platform:'node',format:'cjs',write:false,external:['*'],define:{__DEV__:'false'}})).outputFiles[0].text;
function load(code, require) { const module={exports:{}};new Function('require','module','exports',code)(require,module,module.exports);return module.exports; }
const react={createElement:(type,props,...children)=>({type,props:{...props,children}})};
const code=await compile('apps/mobile/src/SystemVolumeSlider.tsx');
for(const platform of ['ios','android'])for(const native of [false,true]) {
 globalThis.expo={modules:native?{SdaSystemVolume:{}}:{}};
 let requested=0;
 const slider=load(code,name=>name==='react'?react:name==='react-native'?{Platform:{OS:platform},processColor:c=>c==='active'?-1:0x3dffffff}:name==='expo'?{requireNativeView:name=>{assert.equal(name,'SdaSystemVolume');requested++;return 'NativeSystemVolume';}}:{});
 assert.equal(slider.hasSystemVolumeSlider,native);assert.equal(requested,native?1:0);
 const changes=[],tracking=[];
 const element=slider.SystemVolumeSlider({activeColor:'active',inactiveColor:'inactive',onChange:v=>changes.push(v),onTracking:v=>tracking.push(v)});
 if(!native){assert.equal(element,null);continue;}
 assert.equal(element.type,'NativeSystemVolume');assert.equal(element.props.activeColorArgb,0xffffffff);
 assert(!Object.hasOwn(element.props,'value'),'mounting must not send a saved app volume to the system');
 assert.deepEqual(changes,[],'mounting must not produce a volume write');
 for(const value of [0,.37,1,-.5,1.2,NaN,Infinity])element.props.onChange({nativeEvent:{value}});
 assert.deepEqual(changes,[0,.37,1,0,1]);
 element.props.onTracking({nativeEvent:{active:true}});element.props.onTracking({nativeEvent:{active:false}});assert.deepEqual(tracking,[true,false]);
}
class Component { setState(update,callback){const value=typeof update==='function'?update(this.state):update;if(value)this.state={...this.state,...value};callback?.();} }
const appCode=await compile('apps/mobile/App.tsx');
for(const platform of ['ios','android'])for(const native of [false,true]) {
 const module=load(appCode,name=>name==='react'?{Component}:name==='react-native'?{Platform:{OS:platform},AppState:{currentState:'active'}}:name.endsWith('/SystemVolumeSlider')?{hasSystemVolumeSlider:native}:name.endsWith('.json')?[]:{});
 const app=new module.default({});const gains=[];
 const engine={setVolume:v=>gains.push(v),hrtfStatus:()=> 'ready',playUri:async()=> '{}',renderingSettings:()=>JSON.stringify({layout:'7.1.4'}),status:()=>JSON.stringify({paused:false})};
 app.getEngine=()=>engine;app.poller=1;app.prepareTrack=async()=>null;app.syncNativeQueue=()=>{};
 app.state={...app.state,queue:[{contentHash:'song',name:'song.m4a',uri:'file:///song.m4a',metadata:{durationMs:1000}}],queueIndex:0,busy:false};
 app.setVolume(.37);assert.equal(app.state.volume,.37);assert.deepEqual(gains,native?[]:[.37],'hardware volume events must not be applied again as DSP gain');
 gains.length=0;await app.selectTrack(0);assert.equal(app.state.error,null);assert.deepEqual(gains,[native?1:.37],'track changes keep DSP gain at unity when the system owns volume');
 assert.equal(app.state.volume,.37,'track changes must not reset the visible system volume');
}
console.log('System volume: iOS/Android capability gates, no mount-time writes, hardware event synchronization, no double attenuation, and track-switch gain passed.');
