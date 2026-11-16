import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
const require=createRequire(import.meta.url), {parse}=require('yaml');
const workflow=name=>parse(readFileSync('.github/workflows/'+name,'utf8'));
const ios=workflow('ios-port-ci.yml').jobs.ios;
const mac=workflow('release.yml').jobs['build-macos'];
assert.equal(ios['runs-on'],'xcode-27');
assert.deepEqual(ios.strategy.matrix.sdk,['27.1']);
assert.deepEqual(mac.strategy.matrix.include.map(x=>[x.arch,x.runner,x.target,x.probe_arch]),[
 ['x64','xcode-27','x86_64-apple-darwin','x86_64'],
 ['arm64','xcode-27','aarch64-apple-darwin','arm64'],
]);
for(const job of [ios,mac]){
 const stage=job.steps.findIndex(x=>x.name==='Stage pinned Apple toolchain selector');
 const select=job.steps.findIndex(x=>x.run?.includes('$SDA_APPLE_TOOLCHAIN_SELECTOR'));
 assert(stage>=0&&select>stage);
 assert(job.steps[stage].with.script.includes('ref:process.env.GITHUB_WORKFLOW_SHA'),'pinned old runtime sources must still receive the workflow tool selector');
 assert(job.steps[select].run.includes('--host-major 27'));
 assert(job.steps[select].run.includes('--record-dir'));
 assert(!job.steps[select]['continue-on-error']);
}
assert(ios.steps.find(x=>x.id==='sdk').run.includes('--ios-sdk-version'));
assert(ios.steps.some(x=>x.run?.includes('IPHONEOS_DEPLOYMENT_TARGET=16.4 archive')));
assert(mac.steps.find(x=>x.id==='apple_tools').run.includes('--probe-macos-arch'));
assert(mac.steps.find(x=>x.uses==='Swatinem/rust-cache@v2').with.key.includes('steps.apple_tools.outputs.fingerprint'));
assert.equal(mac.steps.find(x=>x.name==='Ensure Intel cross-build tools can execute').if,"matrix.arch == 'x64'");
console.log('Apple package runner, toolchain pinning, SDK/architecture probes and old-source staging checks passed.');
