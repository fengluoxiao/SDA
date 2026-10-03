const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const test = require("node:test");
const esbuild = require("esbuild");
const root = path.resolve(__dirname, "../..");
const bundle = esbuild.buildSync({entryPoints:[path.join(root,"apps/web/src/room-listening.ts")],bundle:true,write:false,platform:"node",format:"cjs"}).outputFiles[0].text;
const exportsObject = {exports:{}};
vm.runInNewContext(bundle, {module:exportsObject, exports:exportsObject.exports});
const {ROOM_LISTENING_LEVELS,roomListeningSettings} = exportsObject.exports;

test("explicit listening action uses shared I levels and preserves calibration", () => {
  const levels = JSON.parse(fs.readFileSync(path.join(root,"packages/renderer/src/room-listening-levels.json"),"utf8"));
  assert.deepEqual(levels,{directDb:0,earlyDb:-6,lateDb:0,earlyMs:50});
  assert.equal(JSON.stringify(ROOM_LISTENING_LEVELS),JSON.stringify(levels));
  assert(Object.isFrozen(ROOM_LISTENING_LEVELS));
  const saved = {enabled:false,reflectionMode:"direct",directDb:2,earlyDb:-2,lateDb:-4,earlyMs:30,
    monitor:{enabled:true},bassEnabled:true,bassDb:-3,speakers:{FrontLeft:{gainDb:-1,delayMs:2}}};
  const before = JSON.stringify(saved);
  const applied = roomListeningSettings(saved);
  assert.equal(applied.enabled,true);assert.equal(applied.reflectionMode,"full");
  for(const [key,value] of Object.entries(levels)) assert.equal(applied[key],value);
  assert.equal(applied.monitor,saved.monitor);assert.equal(applied.speakers,saved.speakers);
  assert.equal(applied.bassEnabled,true);assert.equal(applied.bassDb,-3);
  assert.equal(JSON.stringify(saved),before);
});

test("preset is separate from raw comparison, settings restore and mobile bypass", () => {
  const app=fs.readFileSync(path.join(root,"apps/web/src/App.tsx"),"utf8");
  const preset=app.slice(app.indexOf("const applyRoomPreset="),app.indexOf("const applyRoomComparison="));
  assert(preset.includes("roomListeningSettings(current.settings)"));
  const comparison=app.slice(app.indexOf("const applyRoomComparison="),app.indexOf("const changeVolumeBalance"));
  assert(comparison.includes("directDb:0,earlyDb:0,lateDb:0,earlyMs:50"));
  assert(!comparison.includes("roomListeningSettings("));
  const panel=fs.readFileSync(path.join(root,"apps/web/src/components/CinemaPanel.tsx"),"utf8");
  assert(panel.includes("setSettings(state.settings)"));
  const mobile=fs.readFileSync(path.join(root,"crates/sda-native/src/lib.rs"),"utf8").replace(/\r\n/g,"\n");
  const selection=mobile.slice(mobile.indexOf("pub fn set_room("),mobile.indexOf("pub fn stop("));
  assert(selection.includes("if profile.is_some()"));
  assert(selection.includes("Settings::room_listening()"));
  assert(selection.includes("} else {\n            sda_native_renderer::cinema::Settings::default()"));
});
