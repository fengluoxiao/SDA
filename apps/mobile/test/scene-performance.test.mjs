import assert from "node:assert/strict";
import {test} from "node:test";
import {createRequire} from "node:module";
import {readFileSync} from "node:fs";
import {dirname,join} from "node:path";
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
 for(const[kind,packageName,relative]of [["gl","expo-gl","ios/GLView.swift"],["raf","react-native","React/Base/RCTDisplayLink.m"]]){
  const file=join(dirname(require.resolve(packageName+"/package.json")),relative);
  const before=readFileSync(file,"utf8"),after=patchSource(before,kind);
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


test("rotation/layout settling stops GL work without restoring fullscreen bars or resetting its refresh request",()=>{
 const ts=require("typescript"),text=readFileSync("apps/mobile/src/MobileObjectScene.tsx","utf8");
 const source=ts.createSourceFile("MobileObjectScene.tsx",text,ts.ScriptTarget.Latest,true,ts.ScriptKind.TSX);
 let expression;const visit=node=>{if(ts.isVariableDeclaration(node)&&node.name.getText(source)==="effectiveFrameRate")expression=node.initializer.getText(source);ts.forEachChild(node,visit);};visit(source);assert(expression);
 const apply=new Function("displayActive","active","foreground","frameRate","fullscreen","useSceneDisplayPolicy","return "+expression);
 const calls=[];const policy=(...args)=>{calls.push(args);return args[1];};
 // A layout-induced render pause must preserve the native display policy.
 apply(true,false,true,120,true,policy);assert.deepEqual(calls.at(-1),[true,120,true]);
 apply(false,false,true,120,false,policy);assert.deepEqual(calls.at(-1),[false,120,false]);
 apply(true,true,false,120,true,policy);assert.deepEqual(calls.at(-1),[false,120,true]);
 apply(undefined,true,true,60,false,policy);assert.deepEqual(calls.at(-1),[true,60,false]);
});


test("fullscreen camera uses the desktop listener origin, forward axis and 85-degree field of view",()=>{
 const THREE=require("three"),camera=new THREE.PerspectiveCamera(50,430/932,.1,1000),size={width:430,height:932};
 const slots=[];let index=0,frames=[];
 const react={useRef:value=>slots[index++]??(slots[index-1]={current:value}),useMemo:fn=>slots[index++]??(slots[index-1]=fn())};
 const{MobileSceneCamera}=load("apps/mobile/src/MobileSceneCamera.tsx",{react,"@react-three/fiber/native":{useThree:pick=>pick({camera,size}),useFrame:fn=>frames.push(fn)}});
 const input={rotation:{x:.72,y:.25},distance:7,look:{yaw:0,pitch:0},fov:85};
 const render=firstPerson=>{index=0;frames=[];MobileSceneCamera({input,firstPerson,immersive:true,layout:"7.1.4"});for(const f of frames)f();};
 render(false);const position=camera.position.clone(),orientation=camera.quaternion.clone();const orbit=JSON.stringify({rotation:input.rotation,distance:input.distance});
 render(true);assert.deepEqual(camera.position.toArray(),[0,0,0]);assert.equal(camera.fov,85);assert.equal(camera.near,.02);
 assert(camera.getWorldDirection(new THREE.Vector3()).distanceTo(new THREE.Vector3(0,0,-1))<1e-10);
 input.look={yaw:-Math.PI/2,pitch:.3};input.fov=45;render(true);assert.deepEqual(camera.position.toArray(),[0,0,0]);assert.equal(camera.fov,45);assert(camera.getWorldDirection(new THREE.Vector3()).x>.9);
 assert.equal(JSON.stringify({rotation:input.rotation,distance:input.distance}),orbit);
 render(false);assert(camera.position.distanceTo(position)<1e-10);assert(camera.quaternion.angleTo(orientation)<1e-7);assert.equal(camera.fov,50);assert.equal(camera.near,.1);
 render(true);size.width=932;size.height=430;camera.aspect=size.width/size.height;camera.updateProjectionMatrix();render(true);assert.deepEqual(camera.position.toArray(),[0,0,0]);assert.equal(camera.fov,45);
});

test("first-person touch look stays finite, clamps pitch, and pinch changes field of view instead of the orbit",()=>{
 const{rotateListenerView,listenerViewFov,LISTENER_VIEW_FOV}=load("apps/mobile/src/scene-camera-input.ts");
 assert.equal(LISTENER_VIEW_FOV,85);
 let look={yaw:0,pitch:0};look=rotateListenerView(look,100,0,932);assert(look.yaw<0);assert.equal(look.pitch,0);
 assert.equal(rotateListenerView(look,0,-10000,932).pitch,1.48);assert.equal(rotateListenerView(look,0,10000,932).pitch,-1.48);
 assert.deepEqual(rotateListenerView(look,NaN,10,932),look);
 assert.equal(listenerViewFov(85,.5),42.5);assert.equal(listenerViewFov(85,.01),35);assert.equal(listenerViewFov(85,10),105);assert.equal(listenerViewFov(NaN,1),85);
});

test("silent objects submit zero instances for both dots and glows; packing activity changes preserves source colors",()=>{
 const frames=[],cleanup=[];const react={createElement:(type,props,...children)=>({type,props:{...props,children}}),useMemo:fn=>fn(),useEffect:fn=>cleanup.push(fn())};
 const {BatchedSpaceObjects}=load("apps/mobile/src/BatchedSpaceObjects.tsx",{react,"@react-three/fiber/native":{useFrame:fn=>frames.push(fn)}});
 const objects=[point(7,0),point(11,.3),point(20,.6)],visibility=new Map();
 const tree=BatchedSpaceObjects({objects,visibility,immersive:true,phase:{current:0},reduceMotion:false,sourceKey:"activity",streamTimeSec:0,paused:false});
 const geometries=tree.props.children.map(m=>m.props.geometry);const run=()=>frames[0]();run();for(const g of geometries)assert.equal(g.instanceCount,0);
 visibility.set(7,1);visibility.set(20,1);run();for(const g of geometries)assert.equal(g.instanceCount,2);
 const firstColor=geometries[0].getAttribute("instanceColor").array.slice(0,3);
 visibility.delete(7);run();for(const g of geometries){assert.equal(g.instanceCount,1);assert.equal(g.getAttribute("instanceOpacity").array[0]>0,true);}
 const nextColor=geometries[0].getAttribute("instanceColor").array.slice(0,3);assert.notDeepEqual(nextColor,firstColor);
 visibility.clear();run();for(const g of geometries)assert.equal(g.instanceCount,0);
 for(const object of objects)visibility.set(object.id,1);run();for(const g of geometries)assert.equal(g.instanceCount,3);
 for(const dispose of cleanup)dispose();
});
test("fullscreen intent ignores hidden-tab resume callbacks until the user explicitly exits",()=>{
 const ts=require("typescript"),text=readFileSync("apps/mobile/src/IOSPlayer.tsx","utf8"),source=ts.createSourceFile("IOSPlayer.tsx",text,ts.ScriptTarget.Latest,true,ts.ScriptKind.TSX);
 const initializers={};const visit=node=>{if(ts.isVariableDeclaration(node)&&["navigate","setSpaceFullscreen"].includes(node.name.getText(source)))initializers[node.name.getText(source)]=node.initializer.getText(source);ts.forEachChild(node,visit);};visit(source);
 let page=2,requested=false;const intent={current:false},callback=fn=>fn,visited=[];
 const navigate=new Function("useCallback","spaceFullscreenIntent","setPage","setSceneVisited",ts.transpileModule("var extracted = "+initializers.navigate,{compilerOptions:{target:ts.ScriptTarget.ES2020}}).outputText+";return extracted")(callback,intent,value=>page=value,value=>visited.push(value));
 const setFullscreen=new Function("useCallback","spaceFullscreenIntent","setRequestedSpaceFullscreen",ts.transpileModule("var extracted = "+initializers.setSpaceFullscreen,{compilerOptions:{target:ts.ScriptTarget.ES2020}}).outputText+";return extracted")(callback,intent,value=>requested=value);
 setFullscreen(true);for(const index of [0,1,0,2,1])navigate(index);assert.equal(page,2);assert.equal(requested,true);
 setFullscreen(false);navigate(1);assert.equal(page,1);assert.equal(requested,false);navigate(2);assert.equal(page,2);
});

test("the rendered instance positions follow changing native snapshots at 60/120Hz and stay still for a stationary source",()=>{
 const descriptor=Object.getOwnPropertyDescriptor(performance,"now");let now=0;
 Object.defineProperty(performance,"now",{configurable:true,value:()=>now*1000});
 try{for(const fps of [60,120]){
  const memos=[],cleanup=[];let cursor=0,frame;
  const react={createElement:(type,props,...children)=>({type,props:{...props,children}}),
   useMemo:(fn,deps)=>{const index=cursor++,old=memos[index];if(old&&deps.every((d,i)=>d===old.deps[i]))return old.value;const value=fn();memos[index]={deps,value};return value;},useEffect:fn=>cleanup.push(fn())};
  const{BatchedSpaceObjects}=load("apps/mobile/src/BatchedSpaceObjects.tsx",{react,"@react-three/fiber/native":{useFrame:fn=>{frame=fn;}}});
  const visibility=new Map([[7,1]]),render=(x,stream,paused=false)=>{cursor=0;return BatchedSpaceObjects({objects:[point(7,x)],visibility,immersive:true,phase:{current:0},reduceMotion:false,sourceKey:"native-song",streamTimeSec:stream,paused});};
  now=0;let tree=render(0,0);frame();const geometry=tree.props.children[1].props.geometry;
  now=.125;tree=render(1,.125);assert.equal(tree.props.children[1].props.geometry,geometry);
  let changes=0,previous=0;
  for(let i=0;i<=fps/8;i++){now=.125+i/fps;frame();const x=geometry.getAttribute("instancePosition").array[0];assert(x>=previous&&x<=2);if(x>previous)changes++;previous=x;}
  assert(changes>=fps/10,"GPU instance positions must change between metadata polls");
  now=.5;render(1,.5);frame();const fixed=Array.from(geometry.getAttribute("instancePosition").array);
  for(let i=1;i<=fps;i++){now=.5+i/fps;frame();assert.deepEqual(Array.from(geometry.getAttribute("instancePosition").array),fixed,"A fixed metadata position must not acquire decorative motion");}
  now=2;render(-1,.1);frame();assert.equal(geometry.getAttribute("instancePosition").array[0],-2,"seek replaces the previous coordinates immediately");
  visibility.clear();frame();for(const mesh of tree.props.children)assert.equal(mesh.props.geometry.instanceCount,0);
  for(const dispose of cleanup)dispose();
 }}finally{if(descriptor)Object.defineProperty(performance,"now",descriptor);else delete performance.now;}
});

test("a scene render failure is contained, reported once and reset by a fresh retry",()=>{
 const{SceneFrameGuard}=load("apps/mobile/src/scene-frame-guard.ts"),guard=new SceneFrameGuard(),reports=[];let frames=0;const error=new Error("native draw failed");
 assert.equal(guard.run(()=>{frames++;throw error;},e=>reports.push(e)),false);assert.equal(guard.run(()=>frames++),false);assert.equal(frames,1);assert.deepEqual(reports,[error]);
 assert.equal(new SceneFrameGuard().run(()=>frames++),true);assert.equal(frames,2);
});
