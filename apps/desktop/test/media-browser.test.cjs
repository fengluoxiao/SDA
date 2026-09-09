const {test}=require("node:test");
const assert=require("node:assert/strict");
const fs=require("node:fs/promises");
const path=require("node:path");
const os=require("node:os");
const {createMediaBrowser}=require("../media-browser.cjs");
test("media browser listing, recursive selection, validation and saved locations",async()=>{
  const root=await fs.mkdtemp(path.join(os.tmpdir(),"sda-browser-"));
  try {
    await fs.mkdir(path.join(root,"album"));
    await fs.writeFile(path.join(root,"track.wav"),"");
    await fs.writeFile(path.join(root,"ignore.txt"),"");
    await fs.writeFile(path.join(root,"album","song.m4a"),"");
    let settings={unrelated:true};
    const create=()=>createMediaBrowser({app:{getPath:()=>root},readSettings:()=>settings,writeSettings:value=>{settings={...settings,...value};},isMediaFile:file=>/\.(wav|m4a)$/i.test(file)});
    const browser=create();
    const listing=await browser("list",root);
    assert.deepEqual(listing.entries.map(e=>e.name),["album","track.wav"]);
    assert.equal(listing.parent,path.dirname(root));
    await browser("favorite",root);await browser("favorite",root);
    assert.deepEqual((await create()("places")).favorites,[root]);
    const files=await browser("folder",root);
    assert.equal(files.length,2);
    assert.deepEqual((await create()("places")).recent,[root]);
    assert.deepEqual(await browser("files",[files[0],files[0]]),[files[0]]);
    await browser("unfavorite",root);assert.deepEqual((await browser("places")).favorites,[]);
    await browser("forget",path.dirname(files[0]));
    assert(!(await browser("places")).recent.includes(path.dirname(files[0])));
    await assert.rejects(()=>browser("list","relative"));
    await assert.rejects(()=>browser("files",[path.join(root,"ignore.txt")]));
    await assert.rejects(()=>browser("files",[]));
    await assert.rejects(()=>browser("favorite",path.join(root,"track.wav")));
    await assert.rejects(()=>browser("list",path.join(root,"missing")));
    assert.equal(settings.unrelated,true);
  }finally{await fs.rm(root,{recursive:true,force:true});}
});
