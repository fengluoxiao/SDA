const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),os=require('node:os'),zlib=require('node:zlib');
const lib=require('../../apps/desktop/personal-hrtf-library.cjs'),{importInWorker,listPersonal}=require('../../apps/desktop/personal-hrtf.cjs');
(async()=>{const root=fs.mkdtempSync(path.join(os.tmpdir(),'sda-hrtf-library-')),first=path.join(root,'first'),second=path.join(root,'second');try{
 const {generateParameters}=await import('../../apps/desktop/parametric-hrtf.mjs');
 const generated=await importInWorker(null,first,{parameters:generateParameters(),assessment:{test:'archive QA'}});
 lib.rename(first,generated.id,'我的耳机');assert.equal(listPersonal(first)[0].name,'我的耳机');
 const one=await importInWorker(null,first,{kind:'archive',action:'export',id:generated.id,directory:root});
 const two=lib.exportArchive(first,generated.id,root);assert.notEqual(one.path,two.path);assert(fs.existsSync(one.path));
 const imported=await importInWorker(one.path,second);assert.equal(imported.id,generated.id);assert.equal(imported.name,'我的耳机');
 const source=path.join(first,'hrtf-'+generated.id),destination=path.join(second,'hrtf-'+imported.id);
 for(const file of fs.readdirSync(source))assert.deepEqual(fs.readFileSync(path.join(source,file)),fs.readFileSync(path.join(destination,file)),file);
 assert.equal((await importInWorker(one.path,second)).id,generated.id);assert.equal(listPersonal(second).length,1);
 const copy=await importInWorker(null,second,{kind:'archive',action:'copy',id:imported.id,name:'另一份'});assert.notEqual(copy.id,imported.id);assert.equal(listPersonal(second).length,2);assert.equal(lib.info(second,copy.id).name,'另一份');
 const payload=JSON.parse(zlib.gunzipSync(fs.readFileSync(one.path)));payload.files[0].name='../escape.f32';const bad=path.join(root,'bad.phrtf');fs.writeFileSync(bad,zlib.gzipSync(Buffer.from(JSON.stringify(payload))));await assert.rejects(()=>importInWorker(bad,second));assert.equal(listPersonal(second).length,2);
 const corrupt=JSON.parse(zlib.gunzipSync(fs.readFileSync(one.path)));corrupt.files[0].sha256='0'.repeat(64);fs.writeFileSync(bad,zlib.gzipSync(Buffer.from(JSON.stringify(corrupt))));assert.throws(()=>lib.importArchive(second,bad));
 assert.throws(()=>lib.rename(second,'../bad','abc'));assert.throws(()=>lib.rename(second,imported.id,''));
 console.log('Personal library: name/save-as, worker export/import, byte-identical responses, duplicate import, no overwrite and malformed archive rejection passed.');
}finally{fs.rmSync(root,{recursive:true,force:true});}})().catch(e=>{console.error(e);process.exitCode=1});
