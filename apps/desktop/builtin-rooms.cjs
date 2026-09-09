const fs=require('node:fs');
const path=require('node:path');
const {gunzipSync}=require('node:zlib');
const profiles=require('./cinema-profiles.cjs');

function createBuiltinRooms(root,cacheRoot) {
  let catalog;
  const entries=()=>{
    if(catalog)return catalog;
    const value=JSON.parse(fs.readFileSync(path.join(root,'catalog.json'),'utf8'));
    if(value.version!==1||!Array.isArray(value.profiles))throw new Error('Invalid built-in room catalog');
    const seen=new Set();
    for(const e of value.profiles){
      if(!/^[a-f0-9]{64}$/.test(e.id)||e.file!==`${e.id}.json.gz`||seen.has(e.id)
        ||!Number.isInteger(e.bytes)||e.bytes<1||e.bytes>64*1024*1024||!/^[a-f0-9]{64}$/.test(e.compressedSha256)
        ||e.summary?.id!==e.id)throw new Error('Invalid built-in room entry');
      seen.add(e.id);
    }
    catalog=value.profiles;return catalog;
  };
  return {
    list:()=>entries().map(e=>({...e.summary,builtin:true})),
    has:id=>typeof id==='string'&&/^[a-f0-9]{64}$/.test(id)&&(entries().some(e=>e.id===id)||fs.existsSync(path.join(root,`${id}.json.gz`))),
    read(id){
      if(typeof id!=='string'||!/^[a-f0-9]{64}$/.test(id))throw new Error('Unknown built-in room');
      const entry=entries().find(e=>e.id===id);
      // Keep an explicitly selected older bundled response readable after a
      // catalog upgrade. Verify its content hash; never rewrite it as new data.
      if(!entry){
        const archive=path.join(root,`${id}.json.gz`);
        if(!fs.existsSync(archive)||fs.statSync(archive).size>64*1024*1024)throw new Error('Unknown built-in room');
        const bytes=gunzipSync(fs.readFileSync(archive),{maxOutputLength:64*1024*1024});
        if(profiles.roomId(bytes)!==id)throw new Error('Legacy built-in room checksum mismatch');
        const profile=profiles.validateRoom(JSON.parse(bytes));
        fs.mkdirSync(cacheRoot,{recursive:true});const filePath=path.join(cacheRoot,`${id}.json`);
        fs.writeFileSync(filePath,bytes);
        return {filePath,profile,builtin:true};
      }
      const filePath=path.join(cacheRoot,`${id}.json`);
      let bytes;
      if(fs.existsSync(filePath)&&fs.statSync(filePath).size===entry.bytes){
        const cached=fs.readFileSync(filePath);if(profiles.roomId(cached)===id)bytes=cached;
      }
      if(!bytes){
        const compressed=fs.readFileSync(path.join(root,entry.file));
        if(profiles.roomId(compressed)!==entry.compressedSha256)throw new Error('Built-in room archive checksum mismatch');
        bytes=gunzipSync(compressed,{maxOutputLength:64*1024*1024});
        if(bytes.length!==entry.bytes||profiles.roomId(bytes)!==id)throw new Error('Built-in room checksum mismatch');
        profiles.validateRoom(JSON.parse(bytes));
        fs.mkdirSync(cacheRoot,{recursive:true});
        fs.writeFileSync(filePath,bytes);
      }
      return {filePath,profile:profiles.validateRoom(JSON.parse(bytes)),builtin:true};
    }
  };
}
module.exports={createBuiltinRooms};
