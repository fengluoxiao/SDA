const fs=require('node:fs/promises'),path=require('node:path'),crypto=require('node:crypto');
function createRemoteMedia({readSettings,isMediaFile}){
 const secret=crypto.randomBytes(32),entries=new Map();
 const saved=()=>{const s=readSettings();return {favorites:(s.mediaBrowserFavorites||[]).filter(p=>typeof p==='string'&&path.isAbsolute(p)).slice(0,100),recent:(s.mediaBrowserRecent||[]).filter(p=>typeof p==='string'&&path.isAbsolute(p)).slice(0,12)};};
 const inside=(root,file)=>{const rel=path.relative(root,file);return !rel||(!rel.startsWith('..'+path.sep)&&rel!=='..'&&!path.isAbsolute(rel));};
 const item=(root,file,directory)=>{const id=crypto.createHmac('sha256',secret).update(root+'\0'+file).digest('hex');if(entries.size>=20000&&!entries.has(id))throw Error('浏览记录过多，请重启主机后重试');entries.set(id,{root,file,directory});return {id,name:path.basename(file)||file,directory};};
 async function resolve(id){const entry=entries.get(id);if(!entry)throw Error('此项目已失效，请重新打开');const roots=saved();if(![...roots.favorites,...roots.recent].includes(entry.root))throw Error('目录已从收藏或最近记录移除');const [root,file]=await Promise.all([fs.realpath(entry.root),fs.realpath(entry.file)]);if(!inside(root,file))throw Error('无法访问收藏目录以外的项目');return {...entry,realRoot:root,realFile:file};}
 async function list(id){if(!id){const s=saved();return {favorites:s.favorites.map(p=>item(p,p,true)),recent:s.recent.map(p=>item(p,p,true))};}
 const e=await resolve(id);if(!e.directory)throw Error('请选择目录');const files=await fs.readdir(e.realFile,{withFileTypes:true});const visible=[];
 for(const f of files){if(!(f.isDirectory()||f.isFile()&&isMediaFile(f.name)))continue;const target=path.join(e.file,f.name);try{if(!inside(e.realRoot,await fs.realpath(target)))continue;}catch{continue;}visible.push(item(e.root,target,f.isDirectory()));if(visible.length>1500)throw Error('目录项目过多，请在主机收藏更具体的子目录');}
 visible.sort((a,b)=>Number(b.directory)-Number(a.directory)||a.name.localeCompare(b.name,undefined,{numeric:true}));const result={name:path.basename(e.file),entries:visible};if(Buffer.byteLength(JSON.stringify(result))>180000)throw Error("目录项目过多，请选择子目录");return result;}
 async function open(id){const e=await resolve(id);if(!e.directory){if(!isMediaFile(e.file)||!(await fs.stat(e.realFile)).isFile())throw Error('文件不可用');return [e.realFile];}
 const pending=[e.realFile],seen=new Set(),files=[];while(pending.length){const current=await fs.realpath(pending.pop());if(!inside(e.realRoot,current)||seen.has(current))continue;seen.add(current);if(seen.size>20000)throw Error('目录过大，请选择子目录');for(const f of await fs.readdir(current,{withFileTypes:true})){const target=path.join(current,f.name);if(f.isDirectory())pending.push(target);else if(f.isFile()&&isMediaFile(f.name)){const real=await fs.realpath(target);if(inside(e.realRoot,real))files.push(real);if(files.length>10000)throw Error('歌曲过多，请选择子目录');}}}if(!files.length)throw Error('目录中没有支持的媒体文件');return files.sort((a,b)=>a.localeCompare(b,undefined,{numeric:true}));}
 return {list,open};
}
module.exports={createRemoteMedia};
