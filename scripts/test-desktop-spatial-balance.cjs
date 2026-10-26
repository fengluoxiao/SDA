const assert = require("node:assert/strict");
const {readFileSync} = require("node:fs");
const vm = require("node:vm");
const source = readFileSync("apps/desktop/main.cjs", "utf8");
const handlers = new Map(), commands = [], writes = [];
let saved = false, accepted = true;
const context = {
  ipcMain: {handle: (name, fn) => handlers.set(name, fn), on: (name, fn) => handlers.set(name, fn)},
  NATIVE_HRTF_SET_PATTERN: /^hrtf(?:-dense)?$/,
  personalHrtf: {PERSONAL_SET: /^hrtf-personal-[a-f0-9]{64}$/},
  NATIVE_RENDERER_COMMAND_ACK_TIMEOUT_MS: 3000,
  nativeRendererCommandAck: async (command) => {commands.push(command); return accepted;},
  nativeRendererStatus: {running:true},
  readSpatialBalanceEnabled: () => saved,
  writeSettings: (update) => {writes.push(update);saved=update.spatialBalanceEnabled;},
  writeStartupLog: () => {},
};
vm.createContext(context);
for (const [start,end] of [
  ['ipcMain.handle("sda:native-renderer-hrtf",','ipcMain.handle("sda:native-renderer-layout",'],
  ['ipcMain.on("sda:get-spatial-balance-enabled",','ipcMain.handle("sda:native-renderer-object-rendering",'],
]) vm.runInContext(source.slice(source.indexOf(start),source.indexOf(end,source.indexOf(start))), context);
(async()=>{
  const hrtf=handlers.get("sda:native-renderer-hrtf"), balance=handlers.get("sda:native-renderer-spatial-balance");
  assert(await hrtf(null,"hrtf-dense",.04));
  assert.equal(commands.at(-1).type,"setSpatialEnhancement");assert.equal(commands.at(-1).enabled,false,"resource replacement must restore an explicit off preference");
  const count=commands.length;assert(await balance(null,true));
  assert.equal(commands.length,count+1);assert.equal(commands.at(-1).type,"setSpatialEnhancement");assert.equal(saved,true);
  accepted=false;assert.equal(await balance(null,false),false);assert.equal(saved,true,"rejected toggle must not overwrite the saved preference");
  accepted=true;commands.length=0;assert(await hrtf(null,"hrtf-personal-"+"a".repeat(64),.04));assert.equal(commands.length,1,"personal heads retain their own presentation policy");
  context.nativeRendererStatus.running=false;commands.length=0;assert(await balance(null,false));assert.equal(commands.length,0);
  context.nativeRendererStatus.running=true;assert(await hrtf(null,"hrtf",.04));assert.equal(commands.at(-1).enabled,false,"restart restores the persisted toggle");
  assert.equal(await balance(null,"true"),false);assert(!commands.some(c=>["reset","setProgramGain","setOutputActive","clearSources"].includes(c.type)),"live spatial controls must preserve the programme clock and gain");
  console.log("Desktop spatial balance: persisted off, restart/profile restore, live toggle, rejected commands and personal-profile isolation passed");
})().catch(error=>{console.error(error);process.exitCode=1;});
