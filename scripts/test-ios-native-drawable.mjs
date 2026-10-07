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
assert.ok(sceneSource.indexOf('renderFrame(scene, camera)') < sceneSource.indexOf('context.readPixels'));
console.log('Scene readiness: first real draw is separate from one-shot CI pixel capture');
