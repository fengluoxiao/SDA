import assert from "node:assert/strict";
import {test} from "node:test";
import {createRequire} from "node:module";
import {readFileSync} from "node:fs";
import {buildSync} from "esbuild";
const require=createRequire(import.meta.url);
function load(path,mocks={}){
 const result=buildSync({entryPoints:[path],bundle:true,platform:"node",format:"cjs",write:false,external:["react","three","@react-three/fiber/native"]});
 const module={exports:{}};new Function("require","module","exports",result.outputFiles[0].text)(name=>mocks[name]??require(name),module,module.exports);return module.exports;
}
const{SceneObjectMotion,SceneFrameClock}=load("apps/mobile/src/scene-motion.ts");
const point=(id,x)=>({id,pos:[x,.25,.5],gainDb:0,hasPos:true});
test("8Hz metadata produces intermediate positions at 60 and 120Hz with no overshoot",()=>{
 for(const fps of [60,120]){
  const motion=new SceneObjectMotion(),output=new Float32Array(3);
  motion.update([point(7,0)],0,0,false);motion.update([point(7,1)],.125,.125,false);
  let previous=-1,changes=0;
  for(let t=.125;t<=.25;t+=1/fps){motion.write(7,t,output,0);assert(output[0]>=previous);assert(output[0]<=2);if(output[0]>previous)changes++;previous=output[0];assert.equal(output[1],1);assert.equal(output[2],-.5);}
  assert(changes>=fps/10);
 }
});
test("seek, pause, track replacement, invalid objects and changing ADM sizes cannot retain old positions",()=>{
 const motion=new SceneObjectMotion(),out=new Float32Array(3);
 for(const count of [1,15,128,256,2,0]){motion.update(Array.from({length:count},(_,id)=>point(id,.1)),count,count,false);assert.equal(motion.size,count);}
 motion.update([point(0,0)],20,20,false);motion.update([point(0,1)],2,20.1,false);motion.write(0,20.1,out,0);assert.equal(out[0],2);
 motion.update([point(0,-1)],2,20.2,true);motion.write(0,20.2,out,0);assert.equal(out[0],-2);
 motion.update([{...point(0,1),hasPos:false},point(1,NaN)],2,20.3,false);assert.equal(motion.size,0);assert.equal(motion.write(0,20.3,out,0),false);
 const replacement=new SceneObjectMotion();replacement.update([point(0,.5)],0,21,false);replacement.write(0,21,out,0);assert.equal(out[0],1);
});
test("frame pacing preserves rounded 60/120Hz callbacks, caps faster displays, and recovers from pauses",()=>{
 for(const [display,target]of [[60,60],[120,60],[120,120],[144,60],[144,120]]){
  const clock=new SceneFrameClock();let rendered=0;
  for(let frame=0;frame<display*5;frame++)if(clock.due(Math.round(frame/display*100000)/100000,target))rendered++;
  assert(Math.abs(rendered-target*5)<=1,JSON.stringify({display,target,rendered}));assert(clock.due(100,target));assert(clock.due(1,target));
 }
});
test("display policy rejects unsupported 120Hz and restores requests when the scene leaves",()=>{
 for(const maximumFps of [60,90,120,144,NaN]){
  const before=globalThis.expo,calls=[],cleanups=[];globalThis.expo={modules:{SdaEngine:{sceneDisplayCapabilities:()=>({maximumFps}),configureSceneDisplay:(...args)=>calls.push(args)}}};
  try{
   const api=load("apps/mobile/src/scene-display.ts",{react:{useEffect:fn=>cleanups.push(fn())}});
   const rate=api.useSceneDisplayPolicy(true,120,true);assert.equal(rate,maximumFps>=119.5?120:60);assert.deepEqual(calls[0],[true,rate,true]);cleanups[0]();assert.deepEqual(calls[1],[false,60,false]);
  }finally{globalThis.expo=before;}
 }
});
test("the native iOS GL presenter and JS animation display link are both patched idempotently",()=>{
 const{patchSource}=require("../../../scripts/patch-ios-scene-frame-rate.cjs");
 for(const[kind,file]of [["gl","expo-gl/ios/GLView.swift"],["raf","react-native/React/Base/RCTDisplayLink.m"]]){
  const before=readFileSync("apps/mobile/node_modules/"+file,"utf8"),after=patchSource(before,kind);
  assert(after.includes("sda.scene.preferredFrameRate"));assert(after.includes("preferredFrameRateRange"));assert.equal(patchSource(after,kind),after);assert.equal(after.split("SDA_SCENE_FRAME_RATE").length,2);
 }
 assert.throws(()=>patchSource("unknown upgraded source","gl"),/Unsupported/);
 const config=JSON.parse(readFileSync("apps/mobile/app.json","utf8"));assert.equal(config.expo.ios.infoPlist.CADisableMinimumFrameDurationOnPhone,true);
});
test("batched object geometry retains every object with two GPU passes independent of ADM size",()=>{
 for(const count of [0,1,15,128,256]){
  const frames=[],cleanup=[];
  const react={createElement:(type,props,...children)=>({type,props:{...props,children}}),useMemo:fn=>fn(),useEffect:fn=>cleanup.push(fn())};
  const{BatchedSpaceObjects}=load("apps/mobile/src/BatchedSpaceObjects.tsx",{react,"@react-three/fiber/native":{useFrame:fn=>frames.push(fn)}});
  const objects=Array.from({length:count},(_,id)=>point(id,id/Math.max(1,count)));
  const tree=BatchedSpaceObjects({objects,visibility:new Map(objects.map(o=>[o.id,1])),immersive:true,phase:{current:0},reduceMotion:false,sourceKey:"test",streamTimeSec:0,paused:false});
  assert.equal(tree.props.children.length,2);frames[0]();
  for(const mesh of tree.props.children){assert.equal(mesh.props.geometry.instanceCount,count);const opacity=mesh.props.geometry.getAttribute("instanceOpacity");for(let id=0;id<count;id++)assert(opacity.array[id]>0);}
  for(const dispose of cleanup)dispose();
 }
});


test("Expo Go fullscreen hides and restores its navigation bar, including an interrupted async read",async()=>{
 const before=globalThis.expo;
 try{
  for(const interrupt of [false,true]){
   const calls=[],cleanup=[];let release;
   globalThis.expo={modules:{ExpoNavigationBar:{getVisibilityAsync:()=>new Promise(resolve=>{release=resolve;}),setHidden:hidden=>{calls.push(hidden);return Promise.resolve();}}}};
   const api=load("apps/mobile/src/scene-display.ts",{react:{useEffect:fn=>cleanup.push(fn())}});api.useSceneDisplayPolicy(true,60,true);
   if(interrupt)cleanup[0]();release("visible");await Promise.resolve();await Promise.resolve();
   if(interrupt)assert.deepEqual(calls,[]);else{assert.deepEqual(calls,[true]);cleanup[0]();assert.deepEqual(calls,[true,false]);}
  }
 }finally{globalThis.expo=before;}
});
