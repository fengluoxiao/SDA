const fs=require('node:fs');const path=require('node:path');const assert=require('node:assert/strict');const {test}=require('node:test');const ts=require('typescript');
const file=path.resolve(__dirname,'../App.tsx');const source=ts.createSourceFile(file,fs.readFileSync(file,'utf8'),ts.ScriptTarget.Latest,true,ts.ScriptKind.TSX);
const player=source.statements.find(n=>ts.isClassDeclaration(n)&&n.members.some(m=>m.name?.getText(source)==='pollStatus'));
assert.ok(player,'Actual player polling class missing');
const methods=['pollStatus','restartStatusPolling'].map(name=>player.members.find(m=>m.name?.getText(source)===name).getText(source));
function harness(os){
 const intervals=[];const module={exports:{}};
 const js=ts.transpileModule(`class Harness {${methods.join('\n')}};module.exports=Harness`,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
 new Function('Platform','setInterval','clearInterval','module',js)({OS:os},(fn,ms)=>{intervals.push(ms);return 1},()=>{},module);
 const p=new module.exports();p.foreground=false;p.changingTrack=false;p.state={playing:true,busy:false,queue:[],queueIndex:0,headTrackingEnabled:false,headYaw:0};p.updates=0;p.advances=0;
 p.setState=(value,callback)=>{Object.assign(p.state,value);p.updates++;callback?.()};p.advancePlaylist=()=>p.advances++;
 return {p,intervals};
}
test('background playback never asks native code for scene JSON or rebuilds hidden UI',()=>{
 for(const os of ['android','ios']){
  const {p}=harness(os);let calls=0;p.engine={feedError:()=>null,feedDone:()=>false,status:()=>{calls++;throw Error('hidden status')},objects:()=>{calls++;throw Error('hidden scene')}};
  for(let i=0;i<20;i++)p.pollStatus();assert.equal(calls,0);assert.equal(p.updates,0);
 }
});
test('background EOS still advances once, and errors are retained',()=>{
 const {p}=harness('android');p.engine={feedError:()=>null,feedDone:()=>true};p.pollStatus();p.pollStatus();assert.equal(p.advances,1);assert.equal(p.state.ended,true);
 const {p:q}=harness('android');q.engine={feedError:()=> 'device error',feedDone:()=>true};q.pollStatus();assert.equal(q.state.error,'device error');assert.equal(q.advances,0);
});
test('foreground keeps live scene updates while background only paces playback control',()=>{
 for(const [os,foreground,expected] of [['android',true,80],['android',false,250],['ios',true,125],['ios',false,1000]]){
  const {p,intervals}=harness(os);p.foreground=foreground;p.restartStatusPolling();assert.equal(intervals.at(-1),expected);
 }
 const {p}=harness('android');p.foreground=true;let calls=0;p.engine={feedError:()=>null,feedDone:()=>false,status:()=>{calls++;return JSON.stringify({positionMs:1000,hrtfReady:true,binauralEnabled:true})},objects:()=>{calls++;return JSON.stringify({one:{hasPos:true,pos:[0,1,0]}})},hrtfStatus:()=> 'loaded'};
 p.pollStatus();assert.equal(calls,2);assert.equal(p.state.positionMs,1000);assert.equal(p.state.objects.length,1);
});
