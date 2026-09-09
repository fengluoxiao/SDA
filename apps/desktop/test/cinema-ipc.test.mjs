import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import vm from 'node:vm';
import test from 'node:test';
import cinemaProfiles from '../cinema-profiles.cjs';

test('room import, apply acknowledgement, persistence, export and removal',async()=>{
 const directory=fs.mkdtempSync(path.join(os.tmpdir(),'sda-cinema-test-'));
 try {
  const profile={version:1,name:'Synthetic IPC fixture',source:'Test only',license:'Test only',measurement:'dummy-head',sampleRate:48000,layout:'2.0',
    speakers:[['FrontLeft',30],['FrontRight',-30]].map(([name,azimuth])=>{
      const impulse=Array(512).fill(0);impulse[100]=1;
      return {name,azimuth,elevation:0,onsetSample:100,directLeft:impulse,directRight:impulse,roomLeft:impulse,roomRight:impulse};
    })};
  const input=path.join(directory,'fixture.json');fs.writeFileSync(input,JSON.stringify(profile));
  const handlers=new Map();let stored={};let accepted=true;let sent;
  const main=fs.readFileSync(new URL('../main.cjs',import.meta.url),'utf8');
  const code=main.slice(main.indexOf('const cinemaProfileDirectory ='),main.indexOf('ipcMain.handle("sda:native-renderer-object-hrtf"'));
  vm.runInNewContext(code,{fs,path,Buffer,cinemaProfiles,__dirname:directory,require:()=>({createBuiltinRooms:()=>({list:()=>[],has:()=>false})}),app:{getPath:()=>directory,on(){}},
    BrowserWindow:{fromWebContents:()=>null},
    dialog:{showOpenDialog:async()=>({filePaths:[input]}),showSaveDialog:async()=>({filePath:path.join(directory,'report.json')})},
    ipcMain:{handle:(name,handler)=>handlers.set(name,handler)},readSettings:()=>stored,writeSettings:value=>{stored={...stored,...value};},
    nativeRenderer:{stdin:{}},nativeRendererCommandAck:async command=>{sent=command;return accepted;},writeStartupLog(){}});
  const call=(name,...args)=>handlers.get(`sda:${name}`)({sender:{}},...args);
  const room=await call('cinema-import');assert.equal(room.layout,'2.0');
  assert.equal((await call('cinema-rooms')).length,1);
  const initial=await call('cinema-settings');
  assert(await call('native-renderer-cinema',{...initial.settings,enabled:true},room.id));
  assert.equal(sent.type,'setCinema');assert(fs.existsSync(sent.profile));
  accepted=false;
  assert.equal(await call('native-renderer-cinema',initial.settings,null),false);
  assert.equal((await call('cinema-settings')).profileId,room.id);
  assert.throws(()=>call('cinema-delete',room.id));
  await call('cinema-export-report',room.id);
  assert.equal(JSON.parse(fs.readFileSync(path.join(directory,'report.json'))).rows.length,2);
  accepted=true;await call('native-renderer-cinema',initial.settings,null);
  assert(await call('cinema-delete',room.id));assert.equal((await call('cinema-rooms')).length,0);
 } finally {fs.rmSync(directory,{recursive:true,force:true});}
});
