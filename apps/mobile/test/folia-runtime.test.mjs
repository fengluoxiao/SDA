import assert from "node:assert/strict";
import { test } from "node:test";
import { readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { build } from "esbuild";
const requireModule = async path => { const built=await build({entryPoints:[path],bundle:true,platform:"node",format:"esm",write:false});return import("data:text/javascript;base64,"+Buffer.from(built.outputFiles[0].text).toString("base64")); };
const upstream=await requireModule("vendor/folia/latent.generated.ts"), native=await requireModule("apps/mobile/src/folia-runtime.ts"), shader=await requireModule("apps/mobile/src/folia-shaders.ts");
test("Folia/Paper runtime is the pinned upstream source, not another hand-written visualizer",()=>{
 const record=JSON.parse(readFileSync("vendor/folia/upstream.json","utf8"));assert.equal(record.folia.commit,"a44758bbc02929225ca7304b5a4c4580d7b4e87b");assert.equal(record.paperShaders.version,"0.0.80");
 for(const [name,hash] of Object.entries(record.folia.files))assert.equal(createHash("sha256").update(readFileSync("vendor/folia/"+name)).digest("hex"),hash,name);
 for(const [name,hash] of Object.entries(record.paperShaders.files))assert.equal(createHash("sha256").update(readFileSync("vendor/folia/paper-shaders/"+name)).digest("hex"),hash,name);
 const checked=spawnSync(process.execPath,["scripts/sync-folia-runtime.mjs","--check"],{encoding:"utf8",windowsHide:true});assert.equal(checked.status,0,checked.stderr);
});
test("the native host passes theme colors through Folia's original palette resolver",()=>{
 const theme={backgroundColor:"#282533",primaryColor:"#eeeaf5",secondaryColor:"#c8bfd3",accentColor:"#aa83b4",colors:["#987c9d","#62969c","#a68170"]};assert.deepEqual(native.foliaColors(theme),upstream.resolveLatentShaderColors(theme.colors,theme,"cover-theme"));
});
test("GL compatibility transforms syntax while preserving the upstream shader's equations",()=>{
 assert.match(shader.meshGradientFragmentShader,/#version 300 es/);assert.doesNotMatch(shader.FOLIA_MESH_FRAGMENT,/#version|\bout vec4|\bin vec2/);
 assert.match(shader.FOLIA_MESH_FRAGMENT,/dist = pow\(dist, 3\.5\)/);assert.match(shader.FOLIA_MESH_FRAGMENT,/firstFrameOffset = 41\.5/);
 assert.match(shader.FOLIA_DITHER_FRAGMENT,/bayer4x4At\(index\)/);assert.doesNotMatch(shader.FOLIA_DITHER_FRAGMENT,/switch \(type\)|const int bayer/);
 assert.match(shader.FOLIA_DITHER_FRAGMENT,/u_sdaOpacity/);
});
test("measured audio changes slow gradient drift without patterned texture, swirls or brightness pulses",()=>{
 const silent=new native.FoliaLatentDriver(), loud=new native.FoliaLatentDriver();let a,b;
 for(let i=0;i<120;i++){a=silent.step(1/60,undefined,false,false);b=loud.step(1/60,[180,170,160,160,140,100],false,false);}
 assert(b.meshTime>a.meshTime);assert(b.meshTime<=2*.075);assert.equal(a.power,0);
 for(const v of [a,b]){assert(v.distortion<=.2);assert.equal(v.swirl,0);assert.equal(v.meshScale,1);assert.equal(v.brightness,1);assert.equal(v.saturation,1);}
 const d=new native.FoliaLatentDriver();let previous=0;
 for(let i=0;i<600;i++){const v=d.step(1/60,Array(6).fill(i%2?255:0),false,false);assert(v.meshTime-previous<=.075/60+1e-9);assert.equal(v.swirl,0);assert.equal(v.brightness,1);previous=v.meshTime;}
});
test("Folia's pause easing and reduced-motion policy stay bounded without touching audio playback",()=>{
 const d=new native.FoliaLatentDriver();for(let i=0;i<60;i++)d.step(1/60,[200,200,200,200,200,200],false,false);
 const before=d.meshTime;for(let i=0;i<120;i++)d.step(1/60,undefined,true,false);assert(d.meshTime>before);assert(d.playing<.01);
 const frozen=d.meshTime;for(let i=0;i<30;i++)d.step(1/60,[255,255,255,255,255,255],false,true);assert.equal(d.meshTime,frozen);
});
test("the AI no longer supplies geometry and the local engine is available without an AI result",()=>{
 const scene=readFileSync("apps/mobile/src/MobileObjectScene.tsx","utf8");assert.match(scene,/aiScene \|\| immersive \? <AiSceneEnvironment/);
 const env=readFileSync("apps/mobile/src/AiSceneEnvironment.tsx","utf8");assert.match(env,/FoliaCompositor/);assert.doesNotMatch(env,/AI_FIELD_FRAGMENT|AI_PARTICLE_FRAGMENT|description\.elements|fetch\(/);
 assert.match(env,/getSpaceRawDraw/);assert.doesNotMatch(env,/renderer\.render\(/);
 const app=readFileSync("apps/mobile/App.tsx","utf8");assert.match(app,/value.visualizerBands/);
});

test("the live AI request uses Folia's upstream theme prompt and accepts its native dual-theme JSON",async()=>{
 const model=await requireModule("apps/mobile/src/ai-scene.ts"), prompt=await requireModule("vendor/folia/theme-prompt.generated.ts");assert(model.AI_SCENE_PROMPT.endsWith(prompt.FOLIA_THEME_PROMPT));
 const scene=model.parseAiScene(JSON.stringify({light:{name:"晨雾",backgroundColor:"#e9e2df",wordColors:[{word:"雾",color:"#ba839d"}]},dark:{name:"夜雾",backgroundColor:"#292432",wordColors:[{word:"雾",color:"#8b6486"}]}}),5);
 assert.equal(scene.version,2);assert.equal(scene.name,"夜雾");assert.equal("elements" in scene,false);
});

test("reduced motion freezes reactive deformation as well as shader time",()=>{
 const d=new native.FoliaLatentDriver();for(let i=0;i<30;i++)d.step(1/60,[200,200,200,200,200,200],false,false);
 const a=d.step(1/60,[255,255,255,255,255,255],false,true),b=d.step(1/60,[0,0,0,0,0,0],false,true);assert.deepEqual(a,b);assert.equal(a.power,0);
});

test("a native render-loop exception is reported once and does not escape to React Native fatal handling",()=>{
 const guard=new native.FoliaFrameGuard(),reports=[];let calls=0;
 const error=new Error("EXGL: getParameter() doesn't support gl.36006 yet!");
 assert.equal(guard.run(()=>{calls++;throw error;},e=>reports.push(e)),false);
 assert.equal(guard.run(()=>{calls++;},e=>reports.push(e)),false);
 assert.equal(calls,1);assert.deepEqual(reports,[error]);assert.equal(guard.failed,true);
 const world=new native.FoliaFrameGuard(),offscreen=new native.FoliaFrameGuard();let worldDraws=0;
 offscreen.run(()=>{throw error;},e=>world.fail(e));world.run(()=>worldDraws++);assert.equal(worldDraws,0,"the main pass must not continue after an offscreen failure");
 const retry=new native.FoliaFrameGuard();assert.equal(retry.run(()=>{calls++;}),true);assert.equal(calls,2);
 const scene=readFileSync("apps/mobile/src/MobileObjectScene.tsx","utf8");assert.match(scene,/active && foreground && !failure/);assert.match(scene,/onRenderError=\{reportRenderError\}/);
});
test("Folia license metadata matches the actual upstream AGPL license shipped with the app",()=>{
 const info=JSON.parse(readFileSync("vendor/folia/upstream.json","utf8")),notices=JSON.parse(readFileSync("vendor/folia/notices.json","utf8"));
 assert.equal(info.folia.license,"AGPL-3.0");assert.match(notices.foliaLicense,/GNU AFFERO GENERAL PUBLIC LICENSE/);assert.equal(notices.foliaLicense,readFileSync("vendor/folia/LICENSE","utf8"));
});
