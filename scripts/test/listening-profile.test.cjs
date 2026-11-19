const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),path=require('node:path');
const {transformSync}=require('esbuild');
const root=path.resolve(__dirname,'../..'), cache=new Map();let dialogs=[];
function load(name){if(cache.has(name))return cache.get(name);const code=transformSync(fs.readFileSync(path.join(root,'apps/mobile/src',name+'.ts'),'utf8'),{loader:'ts',format:'cjs'}).code;const module={exports:{}};vm.runInNewContext(code,{module,exports:module.exports,require:n=>n==='react-native'?{Alert:{alert:(...args)=>dialogs.push(args)}}:load(n.replace('./','')),console,Object,Number,JSON,Error});cache.set(name,module.exports);return module.exports;}
const {listeningProfile,DEFAULT_LISTENING_PROFILE}=load('listening-profile');
const native=require('../../apps/desktop/listening-profile.cjs');
for(const value of [null,{},[],{rear:NaN,height:1,bassDb:0,trebleDb:0},{rear:1,height:2,bassDb:0,trebleDb:0},{rear:true,height:1,bassDb:0,trebleDb:0}]){
 assert.deepEqual(listeningProfile(value),DEFAULT_LISTENING_PROFILE);assert.throws(()=>native.validate(value));
}
assert.deepEqual(native.validate({rear:.5,height:1.5,bassDb:-3,trebleDb:3}),{rear:.5,height:1.5,bassDb:-3,trebleDb:3});
const {showListeningCalibration}=load('listening-calibration');
const tick=()=>new Promise(resolve=>setImmediate(resolve));
async function click(text){const d=dialogs.pop();assert(d,'missing sheet '+text);const b=d[2].find(x=>x.text===text);assert(b,`missing ${text} in ${d[0]}`);b.onPress?.();await tick();}
(async()=>{
 const saved=[];const p={playing:true,paused:false,spatialEnhancementEnabled:true,listeningProfile:{rear:1,height:1,bassDb:0,trebleDb:0},setListeningProfile:async s=>saved.push({...s})};
 showListeningCalibration(p);await click('开始试听');
 for(const [choice]of [['清晰'],['柔和'],['厚实 +2 dB'],['柔和 −2 dB']]){await click('调整并试听');await click(choice);await click('下一步');}
 assert.deepEqual(saved.at(-1),{rear:1.5,height:.5,bassDb:2,trebleDb:-2});
 await click('听进入前设置');assert.deepEqual(saved.at(-1),p.listeningProfile);
 await click('听本次选择');assert.equal(saved.at(-1).rear,1.5);
 await click('保存本次选择');assert.equal(saved.at(-1).trebleDb,-2);
 dialogs=[];showListeningCalibration(p);await click('开始试听');await click('调整并试听');await click('清晰');await click('取消并恢复');assert.deepEqual(saved.at(-1),p.listeningProfile);
 dialogs=[];showListeningCalibration(p);await click('恢复默认');assert.deepEqual(saved.at(-1),{...DEFAULT_LISTENING_PROFILE});
 dialogs=[];showListeningCalibration({...p,paused:true});await click('开始试听');assert.equal(dialogs.at(-1)[0],'先准备试听');
 dialogs=[];showListeningCalibration({...p,setListeningProfile:async()=>{throw Error('native rejected');}});await click('开始试听');await click('调整并试听');await click('清晰');assert.equal(dialogs.at(-1)[0],'试听设置未应用');
 for(const file of ['IOSNativeSettings.tsx','AndroidSettings.tsx','IOSPlayer.tsx'])assert.match(fs.readFileSync(path.join(root,'apps/mobile/src',file),'utf8'),/showListeningCalibration\(p\)/);
 const swift=fs.readFileSync(path.join(root,'apps/mobile/modules/sda-core/ios/SdaPlayer.swift'),'utf8');assert.match(swift,/command\("listeningProfile", \["json":s\["listeningProfile"\]!\]\)/);
 const android=fs.readFileSync(path.join(root,'apps/mobile/modules/sda-core/android/src/main/java/app/sda/mobile/sda/SdaModule.kt'),'utf8');assert.match(android,/nativeSetListeningProfile\(ptr, renderingSettings\(\).getString\("listeningProfile"\)\)/);
 console.log('PASS: bounded shared settings, four-step real callbacks, A/B, save, cancel, reset, playback prerequisite, native failure and both mobile startup restores.');
})().catch(e=>{console.error(e);process.exitCode=1;});
