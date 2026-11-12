import assert from "node:assert/strict";
import {test} from "node:test";
import {buildSync} from "esbuild";
import {readFileSync,existsSync} from "node:fs";
const code=buildSync({entryPoints:["apps/mobile/src/retired-ai-cleanup.ts"],bundle:true,platform:"node",format:"cjs",write:false,external:["*"]}).outputFiles[0].text;
function create(fail=false){
 const calls=[];let reject=fail;const module={exports:{}};
 const storage={deleteItemAsync(key,options){calls.push({kind:"key",key,options});if(reject)throw Error("locked");return Promise.resolve();},getItemAsync(){throw Error("Removed credentials must never be read");}};
 const fs={documentDirectory:"file:///documents/",deleteAsync(path,options){calls.push({kind:"file",path,options});return Promise.resolve();},readAsStringAsync(){throw Error("Retired preferences must not be loaded");}};
 new Function("require","module","exports",code)(name=>name==="react-native"?{Platform:{OS:"ios"}}:name==="expo-secure-store"?storage:fs,module,module.exports);
 return{run:module.exports.cleanupRetiredAiSettings,calls,unlock:()=>{reject=false;}};
}
test("upgrade deletes only the retired preference file and key, without reading either",async()=>{
 const h=create();const a=h.run(),b=h.run();assert.equal(a,b);assert.equal(await a,true);assert.deepEqual(h.calls,[{kind:"key",key:"sda.ai-scene.api-key.v1",options:{keychainService:"app.sda.ai-scene"}},{kind:"file",path:"file:///documents/sda-ai-scene-settings.json",options:{idempotent:true}}]);
 await h.run();assert.equal(h.calls.length,2,"completed cleanup must not repeat");
});
test("a locked or unavailable keychain never interrupts startup and cleanup retries on foreground",async()=>{
 const h=create(true);assert.equal(await h.run(),false);h.unlock();assert.equal(await h.run(),true);assert.equal(h.calls.filter(c=>c.kind==="key").length,2);
});
test("production navigation and rendering have no remaining AI scene entry point",()=>{
 for(const file of ["IOSPlayer.tsx","IOSNativeSettings.tsx","AndroidSettings.tsx","MobileObjectScene.tsx","SpaceVisualPage.tsx"]){const source=readFileSync("apps/mobile/src/"+file,"utf8");assert.doesNotMatch(source,/useAiScene|AiSceneProvider|AiSceneSettings|AiSceneEnvironment|ai-scene-settings-link/);}
 for(const file of ["ai-scene.ts","ai-scene-context.tsx","ai-scene-session.ts","AiSceneSettings.tsx","AiSceneEnvironment.tsx","FoliaCompositor.ts"]){assert.equal(existsSync("apps/mobile/src/"+file),false);}
 assert.equal(existsSync("vendor/folia"),false);
 const app=readFileSync("apps/mobile/App.tsx","utf8");assert.match(app,/componentDidMount\(\) \{\s*void cleanupRetiredAiSettings/);assert.match(app,/if \(this.foreground\) void cleanupRetiredAiSettings/);
});
