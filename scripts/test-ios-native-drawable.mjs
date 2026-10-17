import assert from 'node:assert/strict';
import { build } from 'esbuild';
const result = await build({entryPoints:['apps/mobile/src/nativeDrawable.ts'],bundle:true,platform:'node',format:'cjs',write:false});
const module={exports:{}};
new Function('module','exports',result.outputFiles[0].text)(module,module.exports);
const {resetNativeDrawable}=module.exports;
const canvas={width:1062,height:1062};
const calls=[];
const gl={FRAMEBUFFER:36160,bindFramebuffer:(target,buffer)=>calls.push(['bind',target,buffer])};
const renderer={domElement:canvas,resetState:()=>calls.push(['reset',gl.canvas.width,gl.canvas.height]),setViewport:(...args)=>calls.push(['viewport',...args])};
resetNativeDrawable(renderer,gl,354,354);
assert.equal(gl.canvas,canvas);
assert.deepEqual(calls,[['reset',1062,1062],['bind',36160,null],['viewport',0,0,354,354]]);
canvas.width=1200;calls.length=0;
resetNativeDrawable(renderer,gl,400,354);
assert.equal(calls[0][1],1200,'native resize must not retain stale copied dimensions');
const existing={width:60,height:90};gl.canvas=existing;
resetNativeDrawable(renderer,gl,20,30);assert.equal(gl.canvas,existing);
console.log('Native drawable reset: Expo missing canvas, reset ordering, resized canvas, existing context canvas passed');

// A slow CI pixel sample must not keep the first-frame watchdog armed.
const {readFileSync}=await import('node:fs');
const sceneSource=readFileSync('apps/mobile/src/MobileObjectScene.tsx','utf8');
assert.match(sceneSource,/if \(!firstFrame\.current\) \{\s*firstFrame\.current = true;/);
assert.ok(sceneSource.indexOf('firstFrame.current = true;',sceneSource.indexOf('state.gl.render =')) < sceneSource.indexOf('if (smoke && frames >= 30'));
assert.match(sceneSource,/frames >= 30 && !smokeReported/);
assert.doesNotMatch(sceneSource,/context\.endFrameEXP\s*=/);
assert.ok(sceneSource.indexOf('renderFrame(scene, camera)') < sceneSource.indexOf('state.gl.readRenderTargetPixels'));
console.log('Scene readiness: first real draw is separate from one-shot CI pixel capture');

assert.match(sceneSource,/new THREE.WebGLRenderTarget/);
assert.match(sceneSource,/state.gl.setRenderTarget\(previousTarget\)/);
assert.match(sceneSource,/target.dispose\(\)/);

// Native tab layout / rotation replaces Fiber state instead of mutating the
// original onCreated snapshot. Rendering must use the new viewport every time.
const {resetNativeSceneDrawable}=module.exports;
const initialState={gl:renderer,size:{width:354,height:354}};
let currentState=initialState;
const getState=()=>currentState;
for(const [width,height] of [[354,354],[844,220],[1194,410],[390,410],[844,180]]) {
  currentState={...initialState,size:{width,height}};
  calls.length=0;resetNativeSceneDrawable(getState,gl);
  assert.deepEqual(calls.at(-1),['viewport',0,0,width,height]);
  // A perspective camera resized by Fiber must use a matching GL viewport.
  // Otherwise a circle becomes an ellipse and moves toward the left edge.
  const aspect=width/height;
  const viewport=calls.at(-1);
  const horizontalRadius=viewport[3]/aspect;
  const verticalRadius=viewport[4];
  assert(Math.abs(horizontalRadius-verticalRadius)<1e-9);
  assert.equal(viewport[3]/2,width/2);
}
assert.deepEqual(initialState.size,{width:354,height:354},'test must retain the stale creation snapshot');
assert.match(sceneSource,/resetNativeSceneDrawable\(state\.get, context\)/);
assert.doesNotMatch(sceneSource,/resetNativeDrawable\(state\.gl, context, state\.size/);
console.log('Native rotation regression: replaced Fiber snapshots keep viewport, projection scale and center in sync.');
