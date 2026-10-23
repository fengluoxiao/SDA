const fs=require("node:fs"),path=require("node:path"),crypto=require("node:crypto");

const SUBJECT=/^(?:ku100|h(?:[3-9]|1[0-9]|20))$/;
const safeAsset=name=>typeof name==="string"&&/^[A-Za-z0-9][A-Za-z0-9_.-]*\.f32$/.test(name);

function referenceSubject(assessment){
 const requested=assessment?.referenceSubject??assessment?.previousHead;
 return typeof requested==="string"&&SUBJECT.test(requested)?requested:"ku100";
}
function bundledRoot(){
 const roots=[path.join(__dirname,"native-renderer","hrtf-assets"),process.resourcesPath&&path.join(process.resourcesPath,"native-renderer","hrtf-assets")].filter(Boolean);
 const root=roots.find(candidate=>fs.existsSync(candidate));
 if(!root)throw new Error("找不到内置实测 HRTF 资产");
 return root;
}
function sourceDirectory(subject){return subject==="ku100"?"hrtf":`hrtf-${subject}`;}

/** A portable archive that preserves one whole bundled measured HRTF unchanged. */
async function generate(_parameters,assessment,store){
 const record=JSON.stringify(assessment??null);
 if(record.length>1024*1024)throw new Error("测试记录过大");
 const subject=referenceSubject(assessment),base=bundledRoot();
 const savedKu100=path.join(base,"hrtf-ku100-reference","hrtf-set.json");
 const directory=subject==="ku100"&&fs.existsSync(savedKu100)?"hrtf-ku100-reference":sourceDirectory(subject);
 const sourceRoot=path.join(base,directory),manifestPath=path.join(sourceRoot,"hrtf-set.json");
 if(!fs.existsSync(manifestPath))throw new Error(`内置实测 HRTF 不存在：${subject}`);
 const sourceBytes=fs.readFileSync(manifestPath),sourceManifest=JSON.parse(sourceBytes.toString("utf8"));
 if(sourceManifest?.sampleRate!==48000||sourceManifest.completeSubject!==true||!Array.isArray(sourceManifest.positions)||sourceManifest.positions.length<8)throw new Error("内置 HRTF 格式无效");
 const files=[...new Set(sourceManifest.positions.flatMap(position=>[position?.dry,position?.wet]) )];
 if(!files.length||files.some(name=>!safeAsset(name)||!fs.statSync(path.join(sourceRoot,name)).isFile()))throw new Error("内置 HRTF 资产不完整");
 const digest=crypto.createHash("sha256").update("sda-bundled-measured-proxy-v1\0").update(subject).update("\0").update(sourceBytes).update("\0").update(record).digest("hex");
 const id=`personal-${digest}`,target=path.join(store,`hrtf-${id}`);
 fs.mkdirSync(store,{recursive:true});
 const name=`实测 HRTF 代理 · ${sourceManifest.source?.name??subject.toUpperCase()}`;
 if(fs.existsSync(path.join(target,"hrtf-set.json")))return {id,name,method:"bundled-measured-proxy"};
 const staging=fs.mkdtempSync(path.join(store,"measured-proxy-"));
 try{
  for(const file of files)fs.copyFileSync(path.join(sourceRoot,file),path.join(staging,file));
  const manifest={...sourceManifest,schemaVersion:2,measuredProxyVersion:1,calibrationVersion:0,subjectId:id,completeSubject:true,
   source:{...sourceManifest.source,name,method:"bundled-measured-proxy",referenceSubject:subject,measured:true,proxyForListener:true,
    description:"完整内置实测 HRIR/BRIR 的无改动代理；它是受试者匹配结果，不是听者耳朵的物理测量"},
   processing:{...sourceManifest.processing,preserveSamples:true,normalization:false},positions:sourceManifest.positions};
  fs.writeFileSync(path.join(staging,"hrtf-set.json"),JSON.stringify(manifest,null,2));
  fs.writeFileSync(path.join(staging,"profile-info.json"),JSON.stringify({createdAt:new Date().toISOString(),createdAtSource:"recorded"},null,2));
  fs.writeFileSync(path.join(staging,"assessment.json"),record);
  fs.renameSync(staging,target);
 }finally{if(fs.existsSync(staging))fs.rmSync(staging,{recursive:true,force:true});}
 return {id,name,method:"bundled-measured-proxy"};
}
module.exports={generate,referenceSubject,sourceDirectory};
