const assert=require('node:assert/strict'),fs=require('node:fs'),{scanBoxes}=require('../sda-delivery.cjs');
module.exports=function identity(file,objects=false){const fd=fs.openSync(file,'r');try{
 const top=scanBoxes(fd,0,fs.fstatSync(fd).size),moov=top.find(v=>v.id==='moov');let table=scanBoxes(fd,moov.body,moov.end).find(v=>v.id==='trak');for(const id of ['mdia','minf','stbl','stsd'])table=scanBoxes(fd,table.body,table.end).find(v=>v.id===id);
 const bytes=Buffer.alloc(16);fs.readSync(fd,bytes,0,16,table.body);assert.equal(bytes.readUInt32BE(4),1);assert.equal(bytes.toString('ascii',12,16),'SDAR');
 const atom=scanBoxes(fd,table.body+44,table.end).find(v=>v.id==='sdac'),body=Buffer.alloc(atom.end-atom.body);fs.readSync(fd,body,0,body.length,atom.body);const config=JSON.parse(body);
 const transport=top.find(v=>v.id==='sdaf'),data=Buffer.alloc(transport.end-transport.body);fs.readSync(fd,data,0,data.length,transport.body);const manifest=JSON.parse(data);
 for(const value of [config,manifest]){assert.equal(value.codecId,'SDAR');assert.equal(value.format,objects?'SDAR Bed Channel with Objects':'SDAR Bed Channel');}
 assert.equal(manifest.codecName,manifest.format);assert.equal(manifest.tracks[0].codecId,'SDAR');assert.equal(manifest.tracks[0].format,manifest.format);return {sampleEntryOffset:table.body+12,configOffset:atom.body,configBytes:body.length,config,manifest};
}finally{fs.closeSync(fd);}};
