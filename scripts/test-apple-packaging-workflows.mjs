import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
const require=createRequire(import.meta.url), {parse}=require('yaml');
const workflow=name=>parse(readFileSync('.github/workflows/'+name,'utf8'));
const ios=workflow('ios-port-ci.yml').jobs['device-ios'];
const mac=workflow('release.yml').jobs['build-macos'];
assert.equal(ios['runs-on'],'xcode-27');
assert.equal(ios.strategy,undefined,'device packaging has no simulator/test matrix');
assert.equal(ios.env.SDA_PACKAGE_SCOPE,'package-only');
assert.equal(ios.env.SDA_PACKAGE_TESTS,'0');
assert(!ios.steps.some(step=>/cargo test|node --test|check-embedded-motion-artwork|test-ios-ui/.test(step.run??'')));
assert.deepEqual(Object.keys(workflow('ios-port-ci.yml').jobs).sort(),['device-ios','shared-cache-entry']);
assert.deepEqual(mac.strategy.matrix.include.map(x=>[x.arch,x.runner,x.target,x.probe_arch]),[
 ['x64','xcode-27','x86_64-apple-darwin','x86_64'],
 ['arm64','xcode-27','aarch64-apple-darwin','arm64'],
]);
for(const job of [mac]){
 const stage=job.steps.findIndex(x=>x.name==='Stage pinned Apple toolchain selector');
 const select=job.steps.findIndex(x=>x.run?.includes('$SDA_APPLE_TOOLCHAIN_SELECTOR'));
 assert(stage>=0&&select>stage);
 assert(job.steps[stage].with.script.includes('ref:process.env.GITHUB_WORKFLOW_SHA'),'pinned old runtime sources must still receive the workflow tool selector');
 assert(job.steps[select].run.includes('--host-major 27'));
 assert(job.steps[select].run.includes('--record-dir'));
 assert(!job.steps[select]['continue-on-error']);
}
assert(ios.steps.find(x=>x.name==='Select Xcode and verify iOS SDK').run.includes('--ios-sdk-version'));
assert(ios.steps.some(x=>x.run?.includes('IPHONEOS_DEPLOYMENT_TARGET=16.4 archive')));
assert(mac.steps.find(x=>x.id==='apple_tools').run.includes('--probe-macos-arch'));
assert(mac.steps.find(x=>x.uses==='Swatinem/rust-cache@v2').with.key.includes('steps.apple_tools.outputs.fingerprint'));
assert.equal(mac.steps.find(x=>x.name==='Ensure Intel cross-build tools can execute').if,"matrix.arch == 'x64'");
console.log('Apple package runner, toolchain pinning, SDK/architecture probes and old-source staging checks passed.');

const focused=ios;
const compileCache=focused.steps.find(step=>step.name==='Restore Xcode compilation cache');
assert.equal(compileCache.with.path,'ios-build/CompilationCache.noindex');
assert(compileCache.with.key.includes('github.run_id'),'successful runs must save newly compiled entries under a new key');
assert(compileCache.with['restore-keys'].includes('ios-compile-v1-'),'focused packages must reuse the original compilation cache');
assert(!focused.steps.some(step=>step.with?.path==='ios-build/device'));
const archive=focused.steps.find(step=>step.name==='Archive unsigned device app');
assert(archive.run.includes('COMPILATION_CACHE_CAS_PATH="$PWD/'+compileCache.with.path+'"'));
const fusion=focused.steps.find(step=>step.id==='fusion-cache');
assert(!fusion.with['restore-keys'],'finished native frameworks require exact input matches');
for(const input of ['SdaHrtfMesh.h','PersonalHrtfNative/*.hpp','stage-faceframe-scan.py','patch-ios-personal-scan-fusion.py'])assert(fusion.with.key.includes(input));
for(const output of ['StandardCyborgFusion.xcframework','PersonalHrtfFusion/FaceFrame','Licenses/StandardCyborg','Licenses/FaceFrame','personal-scan-framework.json'])assert(fusion.with.path.includes(output));
assert.equal(focused.steps.find(step=>step.name==='Embed on-device TrueDepth framework').if,"steps.fusion-cache.outputs.cache-hit != 'true'");
console.log('Focused iOS builds restore the actual compiler cache and reuse complete frameworks only for identical inputs.');

const iosWorkflow=workflow('ios-port-ci.yml');
assert.equal(iosWorkflow.on.workflow_call.inputs.source_ref.required,true);
assert.equal(iosWorkflow.permissions.actions,'write');
const route=iosWorkflow.jobs['shared-cache-entry'];
assert(route.if.includes("github.ref != format('refs/heads/{0}', github.event.repository.default_branch)"));
for(const job of [ios]){
 assert(job.if.includes("github.ref == format('refs/heads/{0}', github.event.repository.default_branch)"));
 const rust=job.steps.find(step=>step.uses==='Swatinem/rust-cache@v2');
 assert.equal(rust.with['shared-key'],'ios-device-release');
 assert.equal(rust.with.key,'ios-device-${{ steps.xcode.outputs.fingerprint }}');
 const checkout=job.steps.find(step=>step.uses==='actions/checkout@v4');
 assert(checkout.with.ref.includes('inputs.source_ref'));
 for(const step of job.steps.filter(step=>step.uses==='actions/cache@v4')){
  for(const value of Object.values(step.with))assert(!String(value).includes('github.ref'),'cache keys must be independent of the requested tag or branch');
 }
 assert(!job.steps.some(step=>step.run?.includes('git show "$GITHUB_WORKFLOW_SHA:scripts/record-ios-build.mjs"')),'provenance must describe the selected source checkout, not replace it from the entry branch');
}
const dispatchScript=route.steps[0].with.script;
const AsyncFunction=Object.getPrototypeOf(async function(){}).constructor;
const savedEnvironment={SOURCE_REF:process.env.SOURCE_REF,PACKAGE_SCOPE:process.env.PACKAGE_SCOPE,RELEASE_TAG:process.env.RELEASE_TAG};
try{
 for(const requested of ['feature/lyrics','v0.9.40']){
  process.env.SOURCE_REF=requested;process.env.PACKAGE_SCOPE='motion-artwork';process.env.RELEASE_TAG='';
  const dispatched=[];
  const sourceSha='a'.repeat(40);
  const github={rest:{repos:{getCommit:async request=>{assert.equal(request.ref,requested);return{data:{sha:sourceSha}};}},actions:{createWorkflowDispatch:async request=>dispatched.push(request)}}};
  const context={repo:{owner:'test',repo:'SDA'},payload:{repository:{default_branch:'master'}},serverUrl:'https://github.com'};
  const summary={addHeading(){return this;},addRaw(){return this;},addLink(){return this;},async write(){}};
  await new AsyncFunction('github','context','core',dispatchScript)(github,context,{summary});
  assert.equal(dispatched.length,1);
  assert.equal(dispatched[0].ref,'master');
  assert.equal(dispatched[0].inputs.source_ref,sourceSha);
  assert.equal(dispatched[0].workflow_id,'ios-shared-cache.yml');
 }
}finally{
 for(const [name,value]of Object.entries(savedEnvironment)){if(value===undefined)delete process.env[name];else process.env[name]=value;}
}
console.log('Branch and tag requests pin source commits and route to the same default-branch cache scope.');

const shared=workflow('ios-shared-cache.yml');
assert.equal(shared.on.workflow_dispatch.inputs.source_ref.required,true);
assert.match(shared.jobs.package.uses,/^fengluoxiao\/SDA\/\.github\/workflows\/ios-port-ci\.yml@[a-f0-9]{40}$/);
assert.equal(shared.jobs.package.with.source_ref,'${{ needs.source.outputs.commit }}');
const resolveScript=shared.jobs.source.steps[0].with.script;
try{
 process.env.SOURCE_REF='v0.9.40';process.env.PACKAGE_SCOPE='motion-artwork';process.env.RELEASE_TAG='';
 for(const ref of ['refs/heads/master','refs/heads/feature/artwork','refs/tags/v0.9.40']){
  const outputs={},dispatched=[];
  const sourceSha='b'.repeat(40);
  const github={rest:{repos:{getCommit:async()=>({data:{sha:sourceSha}})},actions:{createWorkflowDispatch:async request=>dispatched.push(request)}}};
  const context={repo:{owner:'test',repo:'SDA'},ref,payload:{repository:{default_branch:'master'}}};
  const summary={addHeading(){return this;},addRaw(){return this;},async write(){}};
  await new AsyncFunction('github','context','core',resolveScript)(github,context,{summary,notice(){},setOutput(name,value){outputs[name]=value;}});
  if(ref==='refs/heads/master'){
   assert.equal(outputs.commit,sourceSha);assert.equal(dispatched.length,0);
  }else{
   assert.equal(outputs.commit,undefined);assert.equal(dispatched.length,1);
   assert.equal(dispatched[0].ref,'master');assert.equal(dispatched[0].inputs.source_ref,sourceSha);
  }
 }
}finally{
 for(const [name,value]of Object.entries(savedEnvironment)){if(value===undefined)delete process.env[name];else process.env[name]=value;}
}
console.log('Shared entry builds only in the default-branch scope; non-default entries redirect without a duplicate build.');

const policy=readFileSync('AGENTS.md','utf8');
assert(policy.includes('ios-shared-cache.yml --ref master'));
assert(policy.includes('A completed forwarding job is not a completed IPA build'));
if(process.argv[2]){
 const legacy=parse(readFileSync(process.argv[2],'utf8'));
 assert.equal(legacy.jobs['shared-cache-entry'].if,"inputs.source_ref != ''");
 assert.equal(legacy.jobs.ios.if,"inputs.source_ref == ''");
 assert.equal(legacy.permissions.actions,'write');
 const rust=legacy.jobs.ios.steps.find(step=>step.uses==='Swatinem/rust-cache@v2');
 assert.equal(rust.with['shared-key'],'ios-device-release');
 assert.equal(rust.with.key,focused.steps.find(step=>step.uses==='Swatinem/rust-cache@v2').with.key);
 const script=legacy.jobs['shared-cache-entry'].steps[0].with.script;
 try{
  process.env.SOURCE_REF='other-agent/source';process.env.PACKAGE_SCOPE='motion-artwork';process.env.RELEASE_TAG='';
  const dispatched=[];
  const sha='c'.repeat(40);
  const github={rest:{repos:{getCommit:async()=>({data:{sha}})},actions:{createWorkflowDispatch:async request=>dispatched.push(request)}}};
  const context={repo:{owner:'test',repo:'SDA'},payload:{repository:{default_branch:'master'}},serverUrl:'https://github.com'};
  const summary={addHeading(){return this;},addRaw(){return this;},addLink(){return this;},async write(){}};
  await new AsyncFunction('github','context','core',script)(github,context,{summary});
  assert.equal(dispatched.length,1);assert.equal(dispatched[0].ref,'master');
  assert.equal(dispatched[0].workflow_id,'ios-shared-cache.yml');assert.equal(dispatched[0].inputs.source_ref,sha);
 }finally{
  for(const [name,value]of Object.entries(savedEnvironment)){if(value===undefined)delete process.env[name];else process.env[name]=value;}
 }
 console.log('Legacy master entry redirects other-agent source builds once, retaining the original unpinned master path.');
}
