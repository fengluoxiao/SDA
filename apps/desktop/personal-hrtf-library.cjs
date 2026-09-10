const fs=require("node:fs"),path=require("node:path"),crypto=require("node:crypto"),zlib=require("node:zlib");
const ID=/^personal-[a-f0-9]{64}$/,ASSET=/^azm?\d+_elm?\d+_(?:dry|wet)\.f32$/;
const hash=b=>crypto.createHash("sha256").update(b).digest("hex");
function rootFor(store,id){if(typeof id!=="string"||!ID.test(id))throw new Error("无效档案 ID");return path.join(store,'hrtf-'+id);}
function cleanName(value){if(typeof value!=="string"||!value.trim()||value.trim().length>80||/[\x00-\x1f]/.test(value))throw new Error("档案名称需为 1–80 个字符");return value.trim();}
function info(store,id){const root=rootFor(store,id),manifest=JSON.parse(fs.readFileSync(path.join(root,'hrtf-set.json'),'utf8'));let metadata={};try{metadata=JSON.parse(fs.readFileSync(path.join(root,'profile-info.json'),'utf8'))}catch{}
 return {id,name:metadata.name||manifest.source?.name||'个人 HRTF',directions:manifest.positions.length,method:manifest.source?.method||'imported-sofa'};}
function rename(store,id,name){const root=rootFor(store,id);info(store,id);const target=path.join(root,'profile-info.json'),temp=target+'.'+crypto.randomUUID()+'.tmp';
 fs.writeFileSync(temp,JSON.stringify({name:cleanName(name)},null,2));fs.renameSync(temp,target);return info(store,id);}
function readPayload(store,id){const root=rootFor(store,id),manifest=JSON.parse(fs.readFileSync(path.join(root,'hrtf-set.json'),'utf8'));
 const names=[...new Set(manifest.positions.flatMap(p=>[p.dry,p.wet]))].sort(),files=[];let total=0;
 for(const name of names){if(!ASSET.test(name))throw new Error('档案资产路径无效');const b=fs.readFileSync(path.join(root,name));total+=b.length;if(total>64*1024*1024)throw new Error('档案超过 64 MB');files.push({name,sha256:hash(b),data:b.toString('base64')});}
 let assessment=null;try{assessment=JSON.parse(fs.readFileSync(path.join(root,'assessment.json'),'utf8'))}catch(e){if(e.code!=='ENOENT')throw e;}
 return {format:'sda-phrtf',version:1,name:info(store,id).name,manifest,assessment,files};}
function validate(payload){const m=payload?.manifest;
 if(payload?.format!=='sda-phrtf'||payload.version!==1||!m||!ID.test(m.subjectId)||m.sampleRate!==48000||m.completeSubject!==true
  ||!Array.isArray(m.positions)||m.positions.length<1||m.positions.length>4096||!Array.isArray(payload.files)||payload.files.length>8192)throw new Error('不支持的 pHRTF 档案');
 cleanName(payload.name);
 if(!((m.parametricHrtfVersion===1&&m.processing?.preserveSamples===true)||(m.personalSofaVersion===1&&m.processing?.preserveMeasurements===true)))throw new Error('未知个人 HRTF 处理格式');
 const files=new Map();let total=0;
 for(const f of payload.files){if(typeof f.name!=='string'||!ASSET.test(f.name)||files.has(f.name)||typeof f.data!=='string'||f.data.length>174764||f.data.length%4||!/^[A-Za-z0-9+/]*={0,2}$/.test(f.data))throw new Error('档案资产无效');
  const b=Buffer.from(f.data,'base64');total+=b.length;if(b.toString('base64')!==f.data||b.length<32||b.length%8||b.length>131072||total>64*1024*1024||hash(b)!==f.sha256)throw new Error('响应数据损坏或校验失败');
  for(let i=0;i<b.length;i+=4)if(!Number.isFinite(b.readFloatLE(i))||Math.abs(b.readFloatLE(i))>100)throw new Error('响应包含无效采样');files.set(f.name,b);}
 const used=new Set();for(const p of m.positions){if(!Number.isFinite(p.azimuth)||Math.abs(p.azimuth)>360||!Number.isFinite(p.elevation)||Math.abs(p.elevation)>90||!files.has(p.dry)||!files.has(p.wet))throw new Error('方向或响应文件缺失');used.add(p.dry);used.add(p.wet);}
 if(used.size!==files.size)throw new Error('档案包含未引用资产');return files;
}
function install(store,payload){const files=validate(payload),id=payload.manifest.subjectId,target=rootFor(store,id);fs.mkdirSync(store,{recursive:true});
 if(fs.existsSync(target)){const old=readPayload(store,id);if(JSON.stringify(old.manifest)!==JSON.stringify(payload.manifest)||JSON.stringify(old.assessment)!==JSON.stringify(payload.assessment)||JSON.stringify(old.files)!==JSON.stringify(payload.files))throw new Error('同 ID 档案内容不同，未覆盖现有档案');return info(store,id);}
 const temp=fs.mkdtempSync(path.join(store,'archive-'));try{for(const [name,b] of files)fs.writeFileSync(path.join(temp,name),b);
 fs.writeFileSync(path.join(temp,'hrtf-set.json'),JSON.stringify(payload.manifest,null,2));fs.writeFileSync(path.join(temp,'assessment.json'),JSON.stringify(payload.assessment));
 fs.writeFileSync(path.join(temp,'profile-info.json'),JSON.stringify({name:payload.name},null,2));fs.renameSync(temp,target);
 }finally{if(fs.existsSync(temp))fs.rmSync(temp,{recursive:true,force:true});}return info(store,id);}
function exportArchive(store,id,directory){if(typeof directory!=='string'||!path.isAbsolute(directory)||!fs.statSync(directory).isDirectory())throw new Error('请选择导出目录');
 const payload=readPayload(store,id);validate(payload);const data=zlib.gzipSync(Buffer.from(JSON.stringify(payload)),{level:6});
 const stem=payload.name.replace(/[<>:"/\\|?*\x00-\x1f]/g,'_').replace(/[. ]+$/,'').slice(0,70)||'personal-hrtf';
 for(let i=0;i<10000;i++){const file=path.join(directory,`SDA-${stem}${i?' ('+i+')':''}.phrtf`);try{fs.writeFileSync(file,data,{flag:'wx'});return {path:file};}catch(e){if(e.code!=='EEXIST')throw e;}}
 throw new Error('目录中同名档案过多');}
function importArchive(store,file){if(typeof file!=='string'||!path.isAbsolute(file)||path.extname(file).toLowerCase()!=='.phrtf')throw new Error('请选择 .phrtf 文件');
 const stat=fs.statSync(file);if(!stat.isFile()||stat.size>128*1024*1024)throw new Error('档案文件过大');
 const plain=zlib.gunzipSync(fs.readFileSync(file),{maxOutputLength:128*1024*1024});return install(store,JSON.parse(plain.toString('utf8')));}
function copy(store,id,name){const payload=readPayload(store,id);payload.name=cleanName(name);payload.manifest.subjectId='personal-'+hash(crypto.randomUUID());return install(store,payload);}
module.exports={info,rename,copy,exportArchive,importArchive,validate};
