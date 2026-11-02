import assert from "node:assert/strict";
import {buildSync} from "esbuild";
import {fileURLToPath} from "node:url";
function load(relative){
 const bundle=buildSync({entryPoints:[fileURLToPath(new URL(relative,import.meta.url))],bundle:true,platform:"node",format:"cjs",write:false});
 const module={exports:{}};new Function("module","exports",bundle.outputFiles[0].text)(module,module.exports);return module.exports;
}
const {MobileObjectActivity}=load("../src/mobile-object-activity.ts");
const {ObjectActivityFilter}=load("../../../packages/player/src/object-activity.ts");
const fixed={id:11,pos:[-.5,1,0],gainDb:0,hasPos:true},moving={id:12,pos:[.5,1,.4],gainDb:0,hasPos:true};
const mobile=new MobileObjectActivity(),desktop=new ObjectActivityFilter();
let objects=[fixed,moving];
for(const [clock,stream,sounding,paused] of [[0,0,[],false],[.1,.1,[11],false],[.2,.2,[11,12],false],[.3,.3,[],false],[.5,.5,[],false],[.7,.7,[],false],[.8,.8,[11],false],[.9,.8,[11],true],[1.3,.8,[11],true],[1.4,.8,[11],false]]){
 const actual=mobile.project(objects,sounding,clock,stream,paused,false);
 const expected=desktop.project(objects,new Set(paused?[]:sounding),clock);
 assert.deepEqual(actual,expected,'desktop/mobile signal policy differs at '+clock);
 if(!paused&&sounding.includes(11))assert.equal(actual.find(o=>o.id===11).sceneOpacity,1,'a fixed sounding source is full brightness');
}
assert.equal(objects.length,2,'the full inventory remains available');assert.equal(fixed.sceneOpacity,undefined);
const all=mobile.project(objects,[],1.5,.9,false,true);assert.equal(all.length,2);assert(all.every(o=>o.sceneOpacity===.12));
assert.equal(mobile.project(objects,[],1.6,0,false,false).length,0,'seek retires the previous activity trail');
const unknown=new MobileObjectActivity();assert.equal(unknown.project(objects,undefined,0,0,false,false).length,2,'older native/system output does not invent signal activity');
assert.equal(unknown.project(objects,undefined,.1,0,true,false).length,0,'pause hides unknown activity');
console.log('PASS: shared desktop/mobile fixed+moving signal visibility, brightness, silence fade, pause, resume, seek, all-view and full inventory');

const afterSeek=new MobileObjectActivity();
assert.equal(afterSeek.project(objects,[11],10,10,false,false).find(o=>o.id===11)?.sceneOpacity,1);
assert.equal(afterSeek.project(objects,[11,12],10.1,100,false,false).length,2,'forward seek shows current sounding sources without pausing');
assert.equal(afterSeek.project(objects,[12],10.2,2,false,false).length,1,'backward seek drops previous activity and immediately shows current source');
assert.equal(afterSeek.project(objects,[12],10.3,2.1,false,false)[0].id,12);
console.log('PASS: forward/backward seek visibility recovers from live activity with no user pause/resume');
