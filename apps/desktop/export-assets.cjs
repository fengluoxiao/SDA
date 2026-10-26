const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto');
/** Capture immutable, self-contained authored renderer assets, never pairing/network settings. */
function captureExportAssets({snapshot,selectedHrtf,hrtfRoot,personalRoot,readRoom,readHeadphone}){
 if(!snapshot)return null;
 const blobs=new Map();let total=0;
 const add=(data,mimeType,filename,kind)=>{data=Buffer.from(data);const sha256=crypto.createHash('sha256').update(data).digest('hex');if(!blobs.has(sha256)){total+=data.length;if(total>56*1024*1024)throw Error('当前渲染资源超过母版元数据容量');blobs.set(sha256,{sha256,data,mimeType,filename,kind});}return sha256;};
 let hrtf=null,room=null,headphone=null;
 const set=selectedHrtf;if(!set||!/^hrtf(?:-[a-z0-9-]+)?$/.test(set))throw Error('当前 HRTF 资源尚未就绪');
 const captureHrtf=name=>{
  const root=name.startsWith('hrtf-personal-')?path.join(personalRoot,name):path.join(hrtfRoot,name),manifestBytes=fs.readFileSync(path.join(root,'hrtf-set.json')),manifest=JSON.parse(manifestBytes);
  const names=new Set();for(const position of manifest.positions??[]){for(const key of ['dry','wet'])if(position[key])names.add(position[key]);}
  if(manifest.triangleSupportFile)names.add(manifest.triangleSupportFile);
  if(!names.size)throw Error('当前 HRTF 资源缺少方向数据');
  const files={};for(const file of names){if(typeof file!=='string'||path.basename(file)!==file||!/^[a-zA-Z0-9_.-]+\.(f32|json)$/.test(file))throw Error('当前 HRTF 资源路径不可用于母版');const data=fs.readFileSync(path.join(root,file));files[file]=add(data,file.endsWith('.json')?'application/json':'application/octet-stream',file,'hrtf-resource');}
  return {set:name,manifest:add(manifestBytes,'application/json','hrtf-set.json','hrtf-manifest'),files,sampleRate:manifest.sampleRate,subjectId:manifest.subjectId};
 };
 hrtf={...captureHrtf(set),wetWeight:.04};
 const companion=set==='hrtf-dense'?'hrtf':set==='hrtf-ku100-reference-dense'?'hrtf-ku100-reference':set==='hrtf-dense-raw'?'hrtf-raw':set.endsWith('-dense')&&(set.startsWith('hrtf-h')||set==='hrtf-d2-dense')?set.slice(0,-6):null;
 if(companion)hrtf.companion=captureHrtf(companion);
 const cinema=snapshot.cinema;if(cinema?.settings?.enabled){if(!cinema.profileId)throw Error('房间仿真已开启但没有有效房间档案');const profile=readRoom(cinema.profileId).profile;room={id:cinema.profileId,layout:profile.layout,profile:add(Buffer.from(JSON.stringify(profile)),'application/json','room.json','room-profile')};}
 const id=snapshot.equalizer?.headphoneProfileId;
 if(id){const profile=readHeadphone(id);headphone={id,preamp:profile.preamp??1,manifest:profile.manifest??profile.profile??null,left:add(profile.leftFir,'application/octet-stream','headphone-left.f32','headphone-fir'),right:add(profile.rightFir,'application/octet-stream','headphone-right.f32','headphone-fir')};}
 return {schema:'sda.render-assets',version:1,hrtf,room,headphone,blobs:[...blobs.values()]};
}
module.exports={captureExportAssets};
