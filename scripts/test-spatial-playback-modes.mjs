import assert from 'node:assert/strict';
import {build} from 'esbuild';
import {readFileSync} from 'node:fs';
const compiled=await build({entryPoints:['apps/mobile/App.tsx'],bundle:true,platform:'node',format:'cjs',write:false,define:{__DEV__:'false'},external:['*']});
for(const platform of ['ios','android']) {
 class Component { setState(update){this.state={...this.state,...(typeof update==='function'?update(this.state):update)};} }
 const mod={exports:{}};
 new Function('require','module','exports',compiled.outputFiles[0].text)(name=>name==='react'?{Component}:name==='react-native'?{Platform:{OS:platform},AppState:{currentState:'active'}}:name.endsWith('.json')?[]:{},mod,mod.exports);
 const app=new mod.exports.default({});const calls=[];
 const engine={setBinauralEnabled:enabled=>calls.push(['binaural',enabled]),setHeadTracking:enabled=>calls.push(['tracking',enabled]),headTrackingStatus:()=>'',setObjectRendering:(...flags)=>calls.push(['objects',...flags]),stop:()=>calls.push(['stop']),playUri:()=>calls.push(['play'])};
 app.getEngine=()=>engine;
 app.state={...app.state,playing:true,paused:false,selectedUri:'file:///song',positionMs:23000};
 app.setBinauralEnabled(false);assert.equal(app.state.binauralEnabled,false);assert.equal(app.state.headTrackingEnabled,false);
 app.setObjectRendering(false,false);assert.equal(app.state.directObjects,false);assert.equal(app.state.directionalObjects,false);
 app.setBinauralEnabled(true);app.setObjectRendering(true,true);
 assert.equal(app.state.positionMs,23000);assert.equal(app.state.playing,true);assert.equal(app.state.paused,false);
 assert(!calls.some(call=>call[0]==='stop'||call[0]==='play'),'live controls must not reopen playback');
 if(platform==='ios'){app.setBinauralEnabled(false);app.setHeadTracking(true);assert.equal(app.state.binauralEnabled,true);assert.equal(app.state.headTrackingEnabled,true);}
 const before=app.state.binauralEnabled;engine.setBinauralEnabled=()=>{throw Error('rejected')};app.setBinauralEnabled(false);assert.equal(app.state.binauralEnabled,before);assert.equal(app.state.error,'rejected');
}
for(const file of ['AndroidSettings.tsx','IOSNativeSettings.tsx','IOSPlayer.tsx','RemotePlayer.tsx']) {
 const source=readFileSync('apps/mobile/src/'+file,'utf8');
 assert.doesNotMatch(source,/(?:label=|toggle\()"(?:对象渲染|逐对象渲染|实际方向)"/,'internal object modes must not remain user-facing controls');
}
const ui=readFileSync('apps/mobile/src/IOSPlayer.tsx','utf8');assert.match(ui,/text: "关闭", onPress: \(\) => p.setBinauralEnabled\?\.\(false\)/);
assert.match(readFileSync('apps/mobile/src/IOSNativeSettings.tsx','utf8'),/tag\("off"\)/);
for(const file of ['apps/desktop/main.cjs','apps/desktop/preload.cjs','apps/web/src/vite-env.d.ts'])assert.match(readFileSync(file,'utf8'),/nativeRendererObjectRendering|native-renderer-object-rendering/);
const desktop=readFileSync('apps/web/src/App.tsx','utf8');assert.doesNotMatch(desktop,/<DirectionalHrtfPanel/);assert.doesNotMatch(desktop,/onChange=\{[^}]*changeDirectObjectHrtf/);assert.match(desktop,/nativeRendererObjectRendering\?\.\(readObjectRendering\(\)\)/);
console.log('Spatial controls: real mobile setters, reversible off/fixed/tracking, hidden object controls/defaults, retained playback/clock, rejected changes and desktop bridge passed.');

// Legacy disabled flags cannot survive behind the removed settings controls.
const prefBuild=await build({entryPoints:['apps/web/src/object-rendering-preference.ts'],bundle:true,platform:'node',format:'cjs',write:false});
for (const previous of [null, 'false', 'true']) {
 const values=new Map(['sda-object-rendering-v1','sda-direct-object-hrtf','sda-directional-hrtf-v1'].map(key=>[key,previous]));
 const events=[];const pref={exports:{}};
 new Function('module','exports','localStorage','window','Event',prefBuild.outputFiles[0].text)(pref,pref.exports,{getItem:key=>values.get(key),setItem:(key,value)=>values.set(key,value)},{dispatchEvent:event=>events.push(event.type)},class Event{constructor(type){this.type=type;}});
 assert.equal(pref.exports.readObjectRendering(),true);
 assert([...values.values()].every(value=>value==='true'));
 const count=events.length;pref.exports.readObjectRendering();assert.equal(events.length,count,'already migrated preferences must stay stable');
}
for (const [file, pattern] of [
 ['apps/mobile/modules/sda-core/ios/SdaPlayer.swift', /integer\(forKey:"sda.objectRenderingVersion"\) < 2/],
 ['apps/mobile/modules/sda-core/android/src/main/java/app/sda/mobile/sda/SdaModule.kt', /getInt\("objectRenderingVersion", 0\) < 2/],
]) assert.match(readFileSync(file,'utf8'),pattern);
console.log('Legacy disabled object preferences migrate to enabled; repeat reads remain stable.');
