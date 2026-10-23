const assert=require('node:assert/strict');
const fs=require('node:fs');
const {transformSync}=require('esbuild');
const app=fs.readFileSync('apps/mobile/App.tsx','utf8').replace(/\r\n/g,'\n');
const method=app.match(/  private setSpatialEnhancement = \(spatialEnhancementEnabled: boolean\) => \{[\s\S]*?\n  };/)[0];
for(const platform of ['ios','android']) {
 const module={exports:{}};
 new Function('module','exports','Platform',transformSync(`export class Harness {${method}}`,{loader:'ts',format:'cjs'}).code)(module,module.exports,{OS:platform});
 for(const enabled of [false,true])for(const fails of [false,true]) {
  const h=new module.exports.Harness(),calls=[];
  h.state={spatialEnhancementEnabled:false,busy:false,systemSpatial360RAActive:false,playing:true,paused:true,positionMs:54321};
  h.setState=x=>Object.assign(h.state,x);
  h.getEngine=()=>({setSpatialEnhancement(value){calls.push(value);if(fails)throw Error('native failure')},play(){throw Error('transport changed')},stop(){throw Error('transport changed')}});
  h.setSpatialEnhancement(enabled);
  assert.deepEqual(calls,[enabled]);assert.equal(h.state.spatialEnhancementEnabled,fails?false:enabled);
  assert.equal(h.state.positionMs,54321);assert.equal(h.state.playing,true);assert.equal(h.state.paused,true);
  if(fails)assert.equal(h.state.error,'native failure');
  for(const field of ['busy','systemSpatial360RAActive']) {h.state[field]=true;h.setSpatialEnhancement(!enabled);h.state[field]=false;assert.deepEqual(calls,[enabled]);}
 }
}
console.log('Shared enhancement controls: both platforms, failure handling, busy/system-route guards and transport preservation passed.');
