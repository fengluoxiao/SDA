import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import {spawnSync} from 'node:child_process';
const require=createRequire(import.meta.url),{parse}=require('yaml');
const json=path=>JSON.parse(readFileSync(path,'utf8'));
const app=json('apps/mobile/app.json').expo;
assert.match(app.version,/^\d+\.\d+\.\d+$/);
for(const file of ['package.json','apps/desktop/package.json','apps/mobile/package.json'])assert.equal(json(file).version,app.version,file+' version must match the release');
assert(Number.isInteger(Number(app.ios.buildNumber)) && Number(app.ios.buildNumber)>1);
const androidVersion=app.extra?.androidReleaseVersion??app.version;
assert.match(androidVersion,/^\d+\.\d+\.\d+[A-Za-z]?$/);
if(app.extra?.androidReleaseVersion)assert(androidVersion.startsWith(app.version),'clear the Android-only suffix override when advancing the base app version');
assert(Number.isInteger(app.android.versionCode)&&app.android.versionCode>1);
const gradle=readFileSync('apps/mobile/android/app/build.gradle','utf8');
assert(gradle.includes('versionName "'+androidVersion+'"'));
assert(gradle.includes('versionCode '+app.android.versionCode));
assert.match(gradle,/sdaOfficialRelease[^]*!sdaHasReleaseSigning/);
for(const name of ['SDA_ANDROID_RELEASE_STORE_FILE','SDA_ANDROID_RELEASE_STORE_PASSWORD','SDA_ANDROID_RELEASE_KEY_ALIAS','SDA_ANDROID_RELEASE_KEY_PASSWORD'])assert(gradle.includes("System.getenv('"+name+"')"));
const workflow=parse(readFileSync('.github/workflows/release.yml','utf8'));
assert(workflow.on.workflow_dispatch);assert.equal(workflow.on.push,undefined,'publishing a mobile tag must not build desktop packages');
assert.deepEqual(workflow.on.workflow_dispatch.inputs.platform.options,['both','windows','macos']);
for(const name of ['build-windows','build-macos']){assert(workflow.jobs[name].if.includes('inputs.platform'));const checkout=workflow.jobs[name].steps.find(step=>step.uses?.startsWith('actions/checkout@'));assert.equal(checkout.with.ref,'$'+'{{ inputs.source_ref || inputs.tag }}');}
assert(workflow.jobs['build-web']);
for(const [name,platform] of [['build-windows','Windows'],['build-macos','macOS']]){
 const job=workflow.jobs[name];assert.equal(job.needs,'build-web');
 assert.deepEqual(job.strategy.matrix.include.map(m=>m.arch),['x64','arm64']);
 assert(job.steps.some(step=>step.run?.includes('upload-desktop-release.mjs') && step.run.endsWith(platform+' $'+'{{ matrix.arch }}')));
}
assert(!readFileSync('.github/workflows/release.yml','utf8').includes('-universal'));
const desktop=json('apps/desktop/package.json').build;
assert.equal(desktop.win.artifactName,'SDA-${version}-Windows-${arch}.${ext}');
assert.equal(desktop.mac.artifactName,'SDA-${version}-macOS-${arch}.${ext}');
assert(readFileSync('scripts/upload-mobile-release.mjs','utf8').includes('for(const name of [filename])'),'Release assets must exclude internal build manifests');
const android=parse(readFileSync('.github/workflows/android-release-windows.yml','utf8'));
assert(android.on.workflow_dispatch === null || android.on.workflow_dispatch);assert.equal(android.on.push,undefined);
assert.equal(android.jobs.android['runs-on'],'windows-2022');
const build=android.jobs.android.steps.find(step=>step.name==='Build and verify official APK');
assert.equal(build.run,'./scripts/build-android-release-windows.ps1');
for(const name of ['SDA_ANDROID_KEYSTORE_BASE64','SDA_ANDROID_RELEASE_STORE_PASSWORD','SDA_ANDROID_RELEASE_KEY_ALIAS','SDA_ANDROID_RELEASE_KEY_PASSWORD']) assert.equal(build.env[name],String.fromCharCode(36)+'{{ secrets.'+name+' }}');
console.log('Mobile release version, build numbers, private signing guard, Windows Android and manual-only desktop publishing checks passed.');

for(const target of [android,parse(readFileSync('.github/workflows/ios-port-ci.yml','utf8'))]){
 assert(target.on.workflow_dispatch.inputs.release_tag);
 assert.equal(target.permissions.contents,'write');
 const publish=Object.values(target.jobs).flatMap(job=>job.steps??[]).find(step=>step.name==='Upload versioned package to requested draft release');
 assert(publish?.if.includes('inputs.release_tag'));
 assert(publish.run.includes('scripts/upload-mobile-release.mjs'));
}
const rejected=spawnSync(process.execPath,['scripts/upload-mobile-release.mjs','Android'],{encoding:'utf8',env:{...process.env,SDA_RELEASE_TAG:'v0.0.0',GH_TOKEN:''}});
assert.notEqual(rejected.status,0);
assert(rejected.stderr.includes('Release tag must match the actual app version'),'wrong-version packages must fail before contacting GitHub');
console.log('Release upload is optional, tag-bound and rejects incorrect versions before network access.');
