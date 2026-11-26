import assert from "node:assert/strict";
import test from "node:test";
import {build} from "esbuild";

const compiled=await build({entryPoints:["apps/mobile/src/use-motion-artwork-selection.ts"],bundle:true,platform:"node",format:"cjs",external:["react"],write:false});
function selectionHarness(){
 const calls=[],pending=new Map();
 let state={},effect,dependencies,rendered;
 const native={hasMotionArtwork:uri=>{calls.push(uri);return new Promise(resolve=>pending.set(uri,resolve));}};
 const hooks={useState:()=>[state,value=>{state=typeof value==="function"?value(state):value;}],useCallback:callback=>callback,useEffect(run,next){
  if(!dependencies||next.some((value,index)=>value!==dependencies[index])){effect?.();dependencies=next;effect=run();}
 }};
 const module={exports:{}};
 new Function("require","module","exports",compiled.outputFiles[0].text)(()=>hooks,module,module.exports);
 return{...module.exports,calls,pending,keys:()=>Object.keys(state),render(enabled,current="file:///current",next="file:///next"){
  const previous=globalThis.expo;globalThis.expo={modules:{SdaMotionArtwork:native}};
  try{rendered=module.exports.useMotionArtworkSelection(enabled,current,next);return rendered;}finally{globalThis.expo=previous;}
 }};
}
const settle=async()=>{await Promise.resolve();await Promise.resolve();};

test("disabled motion artwork returns static immediately without inspecting any file",()=>{
 const harness=selectionHarness();
 assert.equal(harness.render(false).current,"static");
 assert.equal(harness.calls.length,0);
 assert.equal(harness.render(false,"file:///other").next,"static");
 assert.equal(harness.calls.length,0);
});

test("enabled playback decides from embedded-video presence instead of displaying a static poster while loading",async()=>{
 const harness=selectionHarness();
 assert.equal(harness.render(true).current,"pending");
 assert.deepEqual(harness.calls,["file:///current","file:///next"]);
 harness.pending.get("file:///current")(true);harness.pending.get("file:///next")(false);await settle();
 assert.equal(harness.render(true).current,"motion");
 assert.equal(harness.render(true).next,"static");
 assert.equal(harness.render(true,"file:///next","file:///third").current,"static");
 assert.equal(harness.calls.length,3);
 harness.pending.get("file:///third")(true);await settle();
 assert.equal(harness.render(true,"file:///next","file:///third").next,"motion");
 assert.deepEqual(harness.keys().sort(),["file:///next","file:///third"]);
});

test("rapid selection and disabling reject outstanding old-file results",async()=>{
 const harness=selectionHarness();
 harness.render(true);harness.render(true,"file:///third","file:///fourth");
 harness.pending.get("file:///current")(true);harness.pending.get("file:///next")(true);await settle();
 assert.equal(harness.render(true,"file:///third","file:///fourth").current,"pending");
 const calls=harness.calls.length;
 assert.equal(harness.render(false,"file:///third","file:///fourth").current,"static");
 harness.pending.get("file:///third")(true);harness.pending.get("file:///fourth")(true);await settle();
 assert.equal(harness.render(false,"file:///third","file:///fourth").current,"static");
 assert.equal(harness.calls.length,calls);assert.deepEqual(harness.keys(),[]);
});

test("video preparation failure falls back to the current static poster",async()=>{
 const harness=selectionHarness();
 harness.render(true);harness.pending.get("file:///current")(true);await settle();
 const selection=harness.render(true);assert.equal(selection.current,"motion");
 selection.onUnavailable("file:///stale");assert.equal(harness.render(true).current,"motion");
 selection.onUnavailable("file:///current");assert.equal(harness.render(true).current,"static");
});

test("a long playlist retains presence information for only the current and next songs",async()=>{
 const harness=selectionHarness();
 for(let index=0;index<50;index++){
  const current=`file:///track-${index}`,next=`file:///track-${index+1}`;
  harness.render(true,current,next);
  harness.pending.get(current)?.(true);harness.pending.get(next)?.(true);await settle();
  assert.equal(harness.render(true,current,next).current,"motion");
  assert.deepEqual(harness.keys().sort(),[current,next].sort());
 }
});
