import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {createReadStream,readFileSync,writeFileSync,copyFileSync,statSync} from 'node:fs';
import {execFileSync} from 'node:child_process';

// Optional, version/tag-bound publishing for the existing mobile build jobs.
const tag=process.env.SDA_RELEASE_TAG;
if(tag){
 const platform=process.argv[2];assert(['Android','iOS'].includes(platform),'Expected Android or iOS');
 const config=JSON.parse(readFileSync('apps/mobile/app.json','utf8')).expo;
 assert.equal(tag,'v'+config.version,'Release tag must match the actual app version');
 const git=(...args)=>execFileSync('git',args,{encoding:'utf8'}).trim();
 const commit=git('rev-parse','HEAD');assert.equal(git('rev-parse',tag+'^{commit}'),commit,'Refusing to upload a different source commit');
 const dir=platform==='Android'?'android-artifacts':'ios-artifacts';
 const info=JSON.parse(readFileSync(dir+'/build-info.json','utf8'));
 assert.equal(info.sourceCommit,commit,'Native build source mismatch');assert.equal(info.appVersion,config.version,'Native build version mismatch');
 const filename=`SDA-${config.version}-${platform==='Android'?'Android-universal.apk':'iOS-arm64-unsigned.ipa'}`;
 if(platform==='iOS')copyFileSync(dir+'/SDA-unsigned-ios16.4-arm64.ipa',dir+'/'+filename);
 const hash=async file=>{const h=createHash('sha256');for await(const b of createReadStream(file))h.update(b);return h.digest('hex');};
 const binary=dir+'/'+filename;const checksum=await hash(binary);
 const manifest={schema:'sda.mobile-release',version:1,appVersion:config.version,buildNumber:Number(config.android.versionCode),sourceCommit:commit,platform,
  workflowRunId:process.env.GITHUB_RUN_ID,workflowRunAttempt:process.env.GITHUB_RUN_ATTEMPT,
  artifact:{name:filename,bytes:statSync(binary).size,sha256:checksum},signing:platform==='iOS'?'unsigned-requires-user-signing':'official-release-key',
  physicalDeviceVerifiedForThisPackage:false,buildInfo:info};
 const manifestName=`SDA-${config.version}-${platform}-manifest.json`;writeFileSync(dir+'/'+manifestName,JSON.stringify(manifest,null,2)+'\n');
 const token=process.env.GH_TOKEN;assert(token,'GH_TOKEN required for requested release upload');
 const repository=process.env.GITHUB_REPOSITORY;assert(repository,'GITHUB_REPOSITORY required');
 const headers={Authorization:'Bearer '+token,Accept:'application/vnd.github+json','X-GitHub-Api-Version':'2022-11-28'};
 const base='https://api.github.com/repos/'+repository;
 const api=async(route,options={})=>{const response=await fetch(route,{...options,headers:{...headers,...options.headers}});if(!response.ok)throw Error('Release API HTTP '+response.status);return response.status===204?null:response.json();};
 const release=await api(base+'/releases/tags/'+tag);assert(release.draft,'Refusing to replace published release assets');
 assert.equal(release.target_commitish,commit,'Draft release must be pinned to this source commit');
 for(const name of [filename,manifestName]){
  const existing=release.assets.find(a=>a.name===name);if(existing)await api(base+'/releases/assets/'+existing.id,{method:'DELETE'});
  const url=release.upload_url.replace(/\{.*$/,'')+'?name='+encodeURIComponent(name);
  const response=await fetch(url,{method:'POST',headers:{...headers,'Content-Type':'application/octet-stream','Content-Length':String(statSync(dir+'/'+name).size)},body:createReadStream(dir+'/'+name),duplex:'half'});
  if(!response.ok)throw Error('Release upload '+name+' HTTP '+response.status);
  const asset=await response.json();console.log('Uploaded',asset.name,asset.size,'bytes');
 }
}else console.log('No release tag requested; build artifacts remain in Actions.');
