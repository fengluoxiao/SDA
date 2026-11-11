import assert from "node:assert/strict";
import { test } from "node:test";
import { createRequire } from "node:module";
import { buildSync } from "esbuild";
import { readFileSync } from "node:fs";
const require = createRequire(new URL("../package.json", import.meta.url));
const THREE = require("three");
function load(path, mocks = {}, timers = {}) {
  const result = buildSync({ entryPoints: ["apps/mobile/src/" + path], bundle: true, platform: "node", format: "cjs", write: false,
    external: ["three", "react", "react-native", "expo-sensors", "expo-screen-orientation", "expo-keep-awake"] });
  const module = { exports: {} };
  new Function("require", "module", "exports", "setInterval", "clearInterval", result.outputFiles[0].text)(name => mocks[name] ?? require(name), module, module.exports, timers.setInterval ?? setInterval, timers.clearInterval ?? clearInterval);
  return module.exports;
}
const configModule = load("cardboard-config.ts"), defaults = configModule.CARDBOARD_DEFAULTS;
const { CardboardPose } = load("cardboard-pose.ts");
const { CardboardRenderer } = load("cardboard-renderer.ts");
const tick = () => new Promise(resolve => setImmediate(resolve));
const settle = async () => { for (let i = 0; i < 8; i++) await tick(); };

test("VR defaults off and hostile/old calibration is bounded without affecting audio settings", () => {
  assert.equal(defaults.enabled, false);
  for (const input of [null, true, [], "bad"]) assert.deepEqual(configModule.normalizeCardboardConfig(input), defaults);
  const normalized = configModule.normalizeCardboardConfig({ enabled: true, k1: NaN, k2: Infinity, fov: 10000, screenWidthMm: -1, eyeDistanceMm: 0, lensDistanceMm: 90 });
  assert.equal(normalized.enabled, true); assert.equal(normalized.k1, defaults.k1); assert.equal(normalized.k2, defaults.k2);
  assert.equal(normalized.fov, 110); assert.equal(normalized.screenWidthMm, 90); assert.equal(normalized.eyeDistanceMm, 50); assert.equal(normalized.lensDistanceMm, 80);
});
test("lens mapping keeps mirrored optical centers and horizon aligned, and zero distortion is an identity when centered", () => {
  for (const aspect of [.8, 1.05, 1.4]) {
    const config = { ...defaults, screenWidthMm: 128, lensDistanceMm: 64, k1: 0, k2: 0 };
    for (const uv of [[0,0],[.5,.5],[.9,.8]]) for (const eye of [0,1]) {
      const sample = configModule.cardboardSampleUv(...uv, eye, aspect, config);
      assert(Math.abs(sample.x - uv[0]) < 1e-12); assert(Math.abs(sample.y - uv[1]) < 1e-12);
    }
    const ratio = defaults.lensDistanceMm / defaults.screenWidthMm;
    assert.deepEqual(configModule.cardboardSampleUv(1-ratio,.5,0,aspect,defaults),{x:.5,y:.5});
    assert.deepEqual(configModule.cardboardSampleUv(ratio,.5,1,aspect,defaults),{x:.5,y:.5});
    for (const x of [.1,.3,.7,.9]) {
      const l=configModule.cardboardSampleUv(x,.65,0,aspect,defaults),r=configModule.cardboardSampleUv(1-x,.65,1,aspect,defaults);
      assert(Math.abs(l.x+r.x-1)<1e-12);assert(Math.abs(l.y-r.y)<1e-12);
    }
  }
});
function nativeSample(camera, screenAngle, platform) {
  // Known world orientation -> native attitude in a Z-up inertial frame.
  const native = new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(1,0,0), Math.PI/2).multiply(camera)
    .multiply(new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0,0,1),screenAngle));
  const e = new THREE.Euler().setFromQuaternion(native,platform === "ios" ? "ZYX" : "ZXY");
  return platform === "ios" ? {alpha:e.z,beta:e.y,gamma:e.x} : {alpha:e.z,beta:e.x,gamma:e.y};
}
test("both native Euler conventions preserve upright landscape, tilt, turn direction and 180-degree screen rotation", () => {
  for (const platform of ["ios","android"]) for (const angle of [Math.PI/2,-Math.PI/2]) {
    const pose = new CardboardPose();
    pose.update(nativeSample(new THREE.Quaternion(),angle,platform),angle,0,platform);
    assert(pose.value.angleTo(new THREE.Quaternion())<1e-7,platform+" upright");
    for (const [yaw,pitch] of [[.6,0],[-.5,.3],[.2,-.35]]) {
      const target = new THREE.Quaternion().setFromEuler(new THREE.Euler(pitch,yaw,0,"YXZ"));
      for (let i=1;i<20;i++) pose.update(nativeSample(target,angle,platform),angle,i*100,platform);
      assert(pose.value.angleTo(target)<1e-6,platform+" turn/tilt");
      pose.update(nativeSample(new THREE.Quaternion(),angle,platform),angle,2100,platform);
      pose.recenter();
    }
    const flipped=-angle;
    pose.update(nativeSample(new THREE.Quaternion(),flipped,platform),flipped,3000,platform);
    assert(pose.value.angleTo(new THREE.Quaternion())<1e-7,platform+" rotation");
  }
});
test("recenter changes heading while preserving gravity, and invalid samples cannot corrupt the camera", () => {
  const pose = new CardboardPose(),target = new THREE.Quaternion().setFromEuler(new THREE.Euler(.25,.8,0,"YXZ"));
  pose.update(nativeSample(target,Math.PI/2,"ios"),Math.PI/2,10,"ios"); pose.recenter();
  assert(pose.value.angleTo(new THREE.Quaternion().setFromEuler(new THREE.Euler(.25,0,0,"YXZ")))<1e-7);
  const before=pose.value.clone();assert.equal(pose.update({alpha:NaN,beta:0,gamma:0},0,20),false);assert.deepEqual(pose.value.toArray(),before.toArray());
  pose.reset();assert.equal(pose.ready,false);assert(pose.value.angleTo(new THREE.Quaternion())<1e-7);
});
function fakeRenderer(width=2400,height=1080) {
  let target=null,scissor=true;
  return {autoClear:false,xr:{enabled:true},getDrawingBufferSize:v=>v.set(width,height),getRenderTarget:()=>target,setRenderTarget:t=>{target=t;},getScissorTest:()=>scissor,setScissorTest:value=>{scissor=value;}};
}
test("one shared scene produces two physically separated eyes and one warped output, with bounded surfaces and restored renderer state", () => {
  const vr=new CardboardRenderer(),renderer=fakeRenderer(),scene=new THREE.Scene(),camera=new THREE.PerspectiveCamera(85,2400/1080,.02,1000);
  camera.focus=1000000;camera.updateMatrixWorld();const calls=[];
  vr.render(renderer,(s,c)=>{c.updateMatrixWorld();calls.push({s,c,target:renderer.getRenderTarget()});},scene,camera,{...defaults,enabled:true});
  assert.equal(calls.length,3);assert.equal(calls[0].s,scene);assert.equal(calls[1].s,scene);assert.equal(calls[2].s,vr.output);
  assert.equal(calls[0].target,vr.left);assert.equal(calls[1].target,vr.right);assert.equal(calls[2].target,null);
  const l=new THREE.Vector3().setFromMatrixPosition(calls[0].c.matrixWorld),r=new THREE.Vector3().setFromMatrixPosition(calls[1].c.matrixWorld);
  assert(Math.abs(l.distanceTo(r)-.064)<1e-10);assert(l.x<r.x);
  const nearPoint=new THREE.Vector3(0,0,-1);assert(nearPoint.clone().project(calls[0].c).x>0);assert(nearPoint.clone().project(calls[1].c).x<0);
  assert(Math.max(vr.left.width,vr.left.height)<=1024);assert.equal(renderer.getRenderTarget(),null);assert.equal(renderer.getScissorTest(),true);assert.equal(renderer.autoClear,false);assert.equal(renderer.xr.enabled,true);
  const resource=vr.left;vr.render(renderer,()=>{},scene,camera,defaults,true);assert.equal(vr.left,resource);assert.equal(vr.material.uniforms.calibration.value,true);
  assert.throws(()=>vr.render(renderer,()=>{throw Error("draw failed");},scene,camera,defaults),/draw failed/);
  assert.equal(renderer.getRenderTarget(),null);assert.equal(renderer.getScissorTest(),true);assert.equal(renderer.autoClear,false);assert.equal(renderer.xr.enabled,true);
  let disposed=0;for(const resource of [vr.left,vr.right,vr.geometry,vr.material])resource.addEventListener("dispose",()=>disposed++);vr.dispose();assert.equal(disposed,4);
});
test("rapid VR enter/exit serializes orientation restoration before the next lock, including denied locks", async () => {
  const { acquireCardboardOrientation }=load("cardboard-session.ts");const calls=[];let current=1,release;
  const gate=new Promise(resolve=>release=resolve),api={getOrientationLockAsync:async()=>{calls.push("get");await gate;return current;},lockAsync:async value=>{calls.push(value);current=value;}};
  const first=acquireCardboardOrientation(api,5,()=>assert.fail("unexpected failure"));await tick();first();
  const second=acquireCardboardOrientation(api,5,()=>assert.fail("unexpected failure"));release();await settle();
  assert.equal(current,5);assert.deepEqual(calls,["get",1,"get",5]);second();await settle();assert.equal(current,1);
  let failures=0;const failed=acquireCardboardOrientation({getOrientationLockAsync:async()=>1,lockAsync:async()=>{throw Error("denied");}},5,()=>failures++);await settle();assert.equal(failures,1);failed();await settle();
});
function hookHarness(options={}) {
  const cells=[],pending=[],intervals=new Map(),calls={listeners:0,removes:0,permissions:0,interval:0,locks:[],awake:new Set()};let cursor=0,callback,awakeActivations=0;
  const react={useRef:value=>{const i=cursor++;return cells[i]??(cells[i]={current:value});},useState:initial=>{const i=cursor++;if(!(i in cells))cells[i]=initial;return[cells[i],value=>{cells[i]=typeof value==="function"?value(cells[i]):value;}];},useEffect:(effect,deps)=>{const i=cursor++;const previous=cells[i];if(!previous||deps.some((v,j)=>v!==previous.deps[j])){previous?.cleanup?.();const state=cells[i]={deps};pending.push(()=>state.cleanup=effect());}}};
  const motion={isAvailableAsync:async()=>options.available!==false,requestPermissionsAsync:async()=>{calls.permissions++;return options.permissionGate?await options.permissionGate:{status:options.denied?"denied":"granted"};},setUpdateInterval:value=>calls.interval=value,addListener:fn=>{callback=fn;calls.listeners++;return{remove:()=>calls.removes++};}};
  const orientation={OrientationLock:{LANDSCAPE:5},getOrientationLockAsync:async()=>1,lockAsync:async value=>calls.locks.push(value)};
  const awake={activateKeepAwakeAsync:async tag=>{if(options.awakeGate&&++awakeActivations===1)await options.awakeGate;calls.awake.add(tag);},deactivateKeepAwake:async tag=>calls.awake.delete(tag)};
  const module=load("use-cardboard-session.ts",{react,"react-native":{Platform:{OS:options.os??"ios"}},"expo-sensors":{DeviceMotion:motion},"expo-screen-orientation":orientation,"expo-keep-awake":awake},{setInterval:fn=>{const id=intervals.size+1;intervals.set(id,fn);return id;},clearInterval:id=>intervals.delete(id)});
  const render=(enabled=true,fps=60)=>{cursor=0;const result=module.useCardboardSession(enabled,fps);for(const run of pending.splice(0))run();return result;};
  return {render,calls,intervals,event:()=>callback?.({rotation:{alpha:0,beta:0,gamma:Math.PI/2},orientation:0})};
}
test("motion starts only in VR, handles denied/unavailable sensors, removes background listeners and reacquires on resume", async () => {
  const h=hookHarness();h.render(false);await settle();assert.equal(h.calls.listeners,0);assert.equal(h.calls.permissions,0);
  h.render(true,120);await settle();assert.equal(h.calls.listeners,1);assert.equal(h.calls.interval,8);h.event();assert.equal(h.render(true,120).pose.current.ready,true);
  h.render(false,120);await settle();assert.equal(h.calls.removes,1);assert.equal(h.calls.awake.size,0);assert.equal(h.intervals.size,0);assert.equal(h.calls.locks.at(-1),1);
  h.event();assert.equal(h.render(false,120).pose.current.ready,false);
  h.render(true,60);await settle();assert.equal(h.calls.listeners,2);assert.equal(h.calls.interval,16);h.render(false);await settle();
  for(const options of [{available:false},{denied:true}]){const d=hookHarness(options);d.render();await settle();assert.equal(d.calls.listeners,0);assert(d.render().status.length>0);d.render(false);await settle();assert.equal(d.calls.awake.size,0);}
});
test("leaving while motion permission is pending cannot subscribe or leave an awake lease behind",async()=>{
  let release;const permissionGate=new Promise(resolve=>release=resolve),h=hookHarness({permissionGate});h.render();await settle();h.render(false);release({status:"granted"});await settle();assert.equal(h.calls.listeners,0);assert.equal(h.calls.awake.size,0);assert.equal(h.calls.locks.at(-1),1);
});
test("same native and Expo preview sources retain VR controls, stereo frame boundary and unchanged audio ownership",()=>{
  const source=readFileSync("apps/mobile/src/MobileObjectScene.tsx","utf8");
  assert.match(source,/gl={createSpaceRenderer}/);assert.match(source,/getSpaceRawDraw\(state.gl\)/);assert.equal((source.match(/context.endFrameEXP\?\.\(\)/g)||[]).length,1);
  assert.match(source,/vrRequested && \(displayActive \?\? active\) && foreground/);assert.match(source,/cardboard-vr|CardboardControls/);
  const pose=readFileSync("apps/mobile/src/use-cardboard-session.ts","utf8");assert.doesNotMatch(pose,/setHeadTracking|setListener|setHeadPose/);
  const app=JSON.parse(readFileSync("apps/mobile/app.json","utf8"));assert.match(app.expo.ios.infoPlist.NSMotionUsageDescription,/纸盒 VR/);assert(app.expo.plugins.some(p=>Array.isArray(p)&&p[0]==="expo-sensors"));
});

test("a delayed wake activation from an old session cannot release the new session's wake lease",async()=>{
  let release;const awakeGate=new Promise(resolve=>release=resolve),h=hookHarness({awakeGate});h.render();await settle();
  h.render(false);h.render(true);await settle();assert.equal(h.calls.awake.size,1);assert.equal(h.calls.listeners,1);
  release();await settle();assert.equal(h.calls.awake.size,1);assert.equal(h.calls.listeners,1);
  h.render(false);await settle();assert.equal(h.calls.awake.size,0);
});
