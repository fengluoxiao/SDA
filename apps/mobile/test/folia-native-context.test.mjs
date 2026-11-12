import assert from "node:assert/strict";
import { test } from "node:test";
import { build } from "esbuild";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { createRequire } from "node:module";
const require = createRequire(import.meta.url), THREE = require("three");
const built = await build({entryPoints:["apps/mobile/src/FoliaCompositor.ts"],bundle:true,platform:"node",format:"cjs",external:["three"],write:false});
const module={exports:{}};new Function("require","module","exports",built.outputFiles[0].text+"\n//# sourceURL=folia-native-context-bundle.cjs")(require,module,module.exports);
const {FoliaCompositor}=module.exports;
function nativeRenderer(target=null) {
 const initial={target,viewport:[7,11,320,240],scissor:[13,17,160,120],scissorTest:true,autoClear:false};
 const state={...initial,viewport:[...initial.viewport],scissor:[...initial.scissor],queries:0,rawBinds:0};
 const context={FRAMEBUFFER_BINDING:36006,FRAMEBUFFER:36160,
  getParameter(value){state.queries++;throw Error("EXGL: getParameter() doesn't support gl."+value+" yet!");},
  bindFramebuffer(){state.rawBinds++;throw Error("Browser framebuffer handles must not bypass Three's native target tracking");}};
 const renderer={autoClear:false,getContext:()=>context,getRenderTarget:()=>state.target,
  setRenderTarget(value){state.target=value;},getViewport(out){return out.fromArray(state.viewport);},getScissor(out){return out.fromArray(state.scissor);},getScissorTest:()=>state.scissorTest,
  setScissorTest(value){state.scissorTest=value;},setViewport(...args){state.viewport=args.length===1?args[0].toArray():args;},setScissor(value){state.scissor=value.toArray();}};
 return {initial,state,renderer,restored(){assert.equal(state.target,initial.target);assert.deepEqual(state.viewport,initial.viewport);assert.deepEqual(state.scissor,initial.scissor);assert.equal(state.scissorTest,initial.scissorTest);assert.equal(renderer.autoClear,initial.autoClear);assert.equal(state.queries,0);assert.equal(state.rawBinds,0);}};
}
test("Folia draws on the Expo GL contract without querying unsupported framebuffer bindings",()=>{
 const c=new FoliaCompositor(),host=nativeRenderer();let draws=0;
 c.draw(host.renderer,()=>{draws++;assert.equal(host.state.target,c.target);},430,932,3);
 assert.equal(draws,1);host.restored();assert(c.target.width*c.target.height<=1280*720+1600);c.dispose();
});
test("nested targets and native drawable state are restored even when an offscreen draw fails",()=>{
 const foreign=new THREE.WebGLRenderTarget(32,32),c=new FoliaCompositor(),host=nativeRenderer(foreign);
 assert.throws(()=>c.draw(host.renderer,()=>{throw Error("driver failure");},900,430,2),/driver failure/);
 host.restored();c.dispose();foreign.dispose();
});

test("the native shim matches the pinned Expo GL implementation rather than browser WebGL",()=>{
 const packageRoot=dirname(require.resolve("expo-gl/package.json"));const code=readFileSync(join(packageRoot,"common/EXWebGLMethods.cpp"),"utf8");
 const unsupported=code.slice(code.indexOf("// Unimplemented..."),code.indexOf("// int",code.indexOf("// Unimplemented...")));
 assert.match(unsupported,/GL_DRAW_FRAMEBUFFER_BINDING/);assert.match(unsupported,/doesn't support gl/);assert.equal(0x8ca6,36006);
});
