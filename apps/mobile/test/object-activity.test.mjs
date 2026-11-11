import assert from "node:assert/strict";
import { test } from "node:test";
import { buildSync } from "esbuild";
import { fileURLToPath } from "node:url";
const bundle = buildSync({ entryPoints: [fileURLToPath(new URL("../src/mobile-object-activity.ts", import.meta.url))], bundle: true, platform: "node", format: "cjs", write: false });
const module = { exports: {} }; new Function("module", "exports", bundle.outputFiles[0].text)(module, module.exports);
const { MobileObjectActivity } = module.exports;
const fixed = { id: 11, pos: [-.5,1,0], gainDb: 0, hasPos: true }, moving = { id: 12, pos: [.5,1,.4], gainDb: 0, hasPos: true };
const objects = [fixed, moving];
test("sounding-only mode hides silent fixed/moving objects immediately instead of retaining faded lights", () => {
 const activity = new MobileObjectActivity();
 assert.deepEqual(activity.project(objects,[],0,0,false,false),[]);
 assert.deepEqual(activity.project(objects,[11],.1,.1,false,false).map(p=>p.id),[11]);
 assert.equal(activity.project(objects,[11],.2,.2,false,false)[0].sceneOpacity,1);
 assert.deepEqual(activity.project(objects,[],.201,.201,false,false),[]);
 assert.deepEqual(activity.project(objects,[12],.3,.3,false,false).map(p=>p.id),[12]);
 assert.equal(fixed.sceneOpacity,undefined);assert.equal(objects.length,2);
});
test("show-silent is a real display toggle; toggling off, pausing, seeking and unknown activity leave no dim fallback", () => {
 const activity = new MobileObjectActivity();
 assert(activity.project(objects,[],1,1,false,true).every(p=>p.sceneOpacity===1));
 assert.deepEqual(activity.project(objects,[],1.001,1,false,false),[]);
 assert.equal(activity.project(objects,[11],2,100,false,false)[0].id,11);
 assert.equal(activity.project(objects,[12],2.1,2,false,false)[0].id,12);
 assert.deepEqual(activity.project(objects,[12],2.2,2,true,false),[]);
 assert.deepEqual(activity.project(objects,undefined,2.3,2,false,false),[]);
 assert.equal(activity.project(objects,undefined,2.4,2,false,true).length,2);
});
test("invalid positions and missing inventory never create ghost markers for stale activity ids", () => {
 const activity = new MobileObjectActivity();
 assert.deepEqual(activity.project([{...fixed,hasPos:false},{...moving,pos:[NaN,1,0]}],[11,12],0,0,false,true),[]);
 assert.deepEqual(activity.project(objects,[99],.1,.1,false,false),[]);
 activity.project(objects,[11,12],.2,.2,false,true);
 assert.deepEqual(activity.project([], [11,12],.3,.3,false,false),[]);
 assert.deepEqual(activity.project(objects,[],.4,.4,false,false),[]);
});
