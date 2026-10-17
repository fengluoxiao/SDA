import assert from 'node:assert/strict';
import {build} from 'esbuild';
const bundle=await build({entryPoints:['apps/mobile/src/use-ios-cutout-side.ts'],bundle:true,platform:'node',format:'cjs',write:false,external:['react','react-native','expo-screen-orientation']});
function fixture(os='ios',available=true) {
 const effects=[],values=[],queries=[];let event,foreground,removed=0,loads=0;
 const native={getOrientationAsync:()=>new Promise(resolve=>queries.push(resolve)),addOrientationChangeListener:fn=>{event=fn;return{remove(){removed++;}};}};
 const mod={exports:{}};
 new Function('require','module','exports',bundle.outputFiles[0].text)(name=>{
  if(name==='react')return {useState:()=>[null,value=>values.push(value)],useEffect:fn=>effects.push(fn)};
  if(name==='react-native')return {Platform:{OS:os},AppState:{addEventListener:(_,fn)=>{foreground=fn;return{remove(){removed++;}};}}};
  if(name==='expo-screen-orientation'){loads++;if(!available)throw Error('old native binary');return native;}
  throw Error(name);
 },mod,mod.exports);
 assert.equal(mod.exports.useIOSCutoutSide(),null);
 const cleanup=effects[0]();
 return {values,queries,change:value=>event({orientationInfo:{orientation:value}}),foreground:state=>foreground(state),cleanup,removed:()=>removed,loads:()=>loads};
}
const f=fixture();
f.change(3);assert.equal(f.values.at(-1),'right','UIInterfaceOrientation landscapeLeft puts the portrait top/cutout on the right');
f.queries.shift()(4);await Promise.resolve();assert.equal(f.values.at(-1),'right','stale initial query must not undo a rotation');
f.change(4);assert.equal(f.values.at(-1),'left','180 degree flip is handled even when size and insets are identical');
f.change(3);assert.equal(f.values.at(-1),'right');
f.change(1);assert.equal(f.values.at(-1),null);
f.foreground('background');assert.equal(f.queries.length,0);
f.foreground('active');f.queries.shift()(4);await Promise.resolve();assert.equal(f.values.at(-1),'left');
f.foreground('active');const late=f.queries.shift();f.cleanup();late(3);await Promise.resolve();assert.equal(f.values.at(-1),'left');assert.equal(f.removed(),2);
const first=fixture();first.queries.shift()(3);await Promise.resolve();assert.equal(first.values.at(-1),'right');first.cleanup();
const android=fixture('android');assert.equal(android.loads(),0);
const missing=fixture('ios',false);assert.equal(missing.cleanup,undefined);assert.equal(missing.queries.length,0);
console.log('Cutout orientation: initial landscape, both 180-degree rotations, stale reads, foreground refresh, cleanup and old-binary fallback passed.');
