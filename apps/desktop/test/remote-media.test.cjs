const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs/promises'),path=require('node:path'),os=require('node:os');
const {createRemoteMedia}=require('../remote-media.cjs');const {validateControl}=require('../remote-session.cjs');
test('remote media exposes only saved roots, filters files, revokes removed roots and blocks junction escape',async()=>{
 const dir=await fs.mkdtemp(path.join(os.tmpdir(),'sda-remote-media-'));try{
 const root=path.join(dir,'music'),outside=path.join(dir,'outside');await fs.mkdir(root);await fs.mkdir(outside);await fs.mkdir(path.join(root,'album'));await fs.writeFile(path.join(root,'song.wav'),'');await fs.writeFile(path.join(root,'secret.txt'),'');await fs.writeFile(path.join(root,'album','two.m4a'),'');await fs.writeFile(path.join(outside,'private.wav'),'');await fs.symlink(outside,path.join(root,'escape'),'junction');
 let settings={mediaBrowserFavorites:[root],mediaBrowserRecent:[root]};const media=createRemoteMedia({readSettings:()=>settings,isMediaFile:p=>/\.(wav|m4a)$/.test(p)});
 const saved=await media.list();assert.equal(saved.favorites.length,1);assert.ok(!JSON.stringify(saved).includes(dir));const rootId=saved.favorites[0].id;const list=await media.list(rootId);assert.deepEqual(list.entries.map(e=>e.name),['album','song.wav']);assert.ok(!JSON.stringify(list).includes(dir));
 assert.deepEqual(await media.open(list.entries[1].id),[await fs.realpath(path.join(root,'song.wav'))]);assert.equal((await media.open(rootId)).length,2);
 await assert.rejects(media.list('../outside'));assert.throws(()=>validateControl({action:'mediaList',value:root}));assert.throws(()=>validateControl({action:'mediaPaths',value:[root]}));
 const album=list.entries[0].id;await fs.rm(path.join(root,'album'),{recursive:true});await fs.symlink(outside,path.join(root,'album'),'junction');await assert.rejects(media.list(album));await assert.rejects(media.open(album));
 settings={mediaBrowserFavorites:[],mediaBrowserRecent:[]};await assert.rejects(media.open(rootId));
 }finally{await fs.rm(dir,{recursive:true,force:true});}
});
