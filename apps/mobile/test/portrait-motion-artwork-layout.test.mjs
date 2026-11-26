import assert from "node:assert/strict";
import test from "node:test";
import {build} from "esbuild";

const bundle=await build({entryPoints:["apps/mobile/src/portrait-motion-artwork-layout.ts"],bundle:true,platform:"node",format:"esm",write:false});
const {portraitMotionArtworkLayout}=await import("data:text/javascript;base64,"+Buffer.from(bundle.outputFiles[0].text).toString("base64"));
test("portrait video fills the exact width at its source aspect without square cropping or extra overscan",()=>{
 const layout=portraitMotionArtworkLayout(430,932,1080/1440);
 assert.equal(layout.frame.left,0);assert.equal(layout.frame.width,430);
 assert.equal(layout.frame.height,430*4/3);
 assert.equal(layout.featherHeight,932*.60);
 for(const width of [320,390,430,768,1024])for(const height of [700,844,932,1366])for(const aspect of [.5625,.75,1]){
  const result=portraitMotionArtworkLayout(width,height,aspect);
  assert.equal(result.frame.width/result.frame.height,aspect);
  assert(result.featherHeight<=height*.60);
  assert(result.featherHeight<=result.frame.height);
 }
});
test("invalid video dimensions cannot create oversized or non-finite artwork geometry",()=>{
 for(const value of [NaN,Infinity,0,-1])assert.deepEqual(portraitMotionArtworkLayout(390,844,value),portraitMotionArtworkLayout(390,844,.75));
 for(const value of [.0001,99999]){
  const result=portraitMotionArtworkLayout(390,844,value);
  assert(Number.isFinite(result.frame.height));assert(result.frame.height<=390*4);
 }
});
