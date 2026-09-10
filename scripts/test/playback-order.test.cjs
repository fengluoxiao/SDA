const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { buildSync } = require("esbuild");
const output = path.resolve("tmp/playback-order.cjs");
fs.mkdirSync(path.dirname(output), { recursive: true });
buildSync({ entryPoints: ["apps/web/src/playbackOrder.ts"], bundle: true, platform: "node", format: "cjs", outfile: output });
const order = require(output);
const items = [{id:"a"},{id:"b"},{id:"c"}];
for (const mode of order.PLAYBACK_MODES) {
  assert.equal(order.nextPlaylistItemId([], "a", mode), null);
  assert.equal(order.nextPlaylistItemId(items, "removed", mode), null);
  assert.equal(order.nextPlaylistItemId(items, null, mode), null);
  assert.equal(order.nextPlaylistItemId(items, "a", mode), mode === "repeat-one" ? "a" : "b");
  assert.equal(order.nextPlaylistItemId(items, "c", mode), mode === "repeat-one" ? "c" : mode === "repeat-all" ? "a" : null);
  assert.equal(order.nextPlaylistItemId([items[0]], "a", mode), mode === "sequence" ? null : "a");
}
for (const saved of [null, "garbage", ...order.PLAYBACK_MODES]) {
  global.localStorage = {getItem: () => saved};
  assert.equal(order.readPlaybackMode(), order.PLAYBACK_MODES.includes(saved) ? saved : "sequence");
}
global.localStorage = {getItem: () => {throw Error("denied");}};
assert.equal(order.readPlaybackMode(), "sequence");
delete global.localStorage;
// Execute the actual App callback with controlled queue/player refs, without audio.
const app = fs.readFileSync("apps/web/src/App.tsx", "utf8");
const callback = app.match(/onEnded: (\(\) => \{[\s\S]*?\n        \}),/)[1];
function session(mode="sequence", id="c") {
  const calls=[]; let current=true;
  const env={isCurrent:()=>current,playbackPlaylistRevision:7,playlistRevisionRef:{current:7},
    playlistRef:{current:items},playlistCurrentIdRef:{current:id},playbackModeRef:{current:mode},
    playPlaylistItemRef:{current:id=>calls.push(id)},playingRef:{current:true},setPlaying:value=>calls.push(value),nextPlaylistItemId:order.nextPlaylistItemId};
  const end=new Function("env", `const {${Object.keys(env).join(",")}}=env; let endedHandled=false; return ${callback};`)(env);
  return {env,calls,end,stale:()=>current=false};
}
for (const mode of order.PLAYBACK_MODES) {
  const s=session(mode);s.end();s.end();assert.deepEqual(s.calls,[mode==="repeat-one"?"c":mode==="repeat-all"?"a":false]);
}
let s=session();s.env.playbackModeRef.current="repeat-all";s.end();assert.deepEqual(s.calls,["a"]);
s=session("repeat-one");s.stale();s.end();assert.deepEqual(s.calls,[]);
s=session("repeat-all");s.env.playlistRevisionRef.current++;s.end();assert.deepEqual(s.calls,[]);
s=session("repeat-one");s.env.playlistRef.current=[];s.end();assert.deepEqual(s.calls,[false]);
s=session();s.end();s.env.playbackModeRef.current="repeat-one";s.end();assert.deepEqual(s.calls,[false]);
console.log("Playback order: boundaries, single-item loops, storage, live mode changes, stale/duplicate completion and cleared queues passed; no audio.");
