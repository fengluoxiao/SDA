import assert from "node:assert/strict";
import { SdaPlayer } from "../src/player.ts";

const player = Object.create(SdaPlayer.prototype);
const web = new Map();
const native = new Map();
Object.assign(player, {
  knownBedLabels: ["L", "R", "LFE", "Obj_14"],
  renderer: { setSourceMuted: (id, muted) => web.set(id, muted) },
  nativeRendererSink: { setMuted: (id, muted) => native.set(id, muted) },
});
player.syncBedMix(new Set(["R"]), new Set(["L", "R"]));
assert.deepEqual([...web], [["bed:0", false], ["bed:1", true], ["bed:2", true]]);
assert.deepEqual(native, web);
player.syncBedMix(new Set(), new Set(["LFE"]));
assert.deepEqual([...web], [["bed:0", true], ["bed:1", true], ["bed:2", false]]);
player.syncBedMix(new Set(["L", "R"]), new Set());
assert.deepEqual([...web], [["bed:0", true], ["bed:1", true], ["bed:2", false]]);
// A late declaration inherits the durable selection, including after a seek.
player.syncBedMix(new Set(), new Set(["L"]));
player.applyBedMute("bed:4", "TopFrontLeft", 48000);
assert.equal(native.get("bed:4"), true);
player.syncBedMix(new Set(), new Set());
assert.equal(native.get("bed:2"), false);
assert.equal(native.has("bed:3"), false);
console.log("bed mix: multi-solo, multi-mute, mute precedence, LFE and late sources passed");
let speakerNames;
let focus;
player.nativeRendererSink.setSpeakerMutes = (names, selected) => { speakerNames = names; focus = selected; };
const selection = new Set(["FrontLeft", "TopRearRight", "LFE"]);
player.syncSpeakerMutes(selection);
selection.clear();
assert.deepEqual(speakerNames, ["FrontLeft", "TopRearRight", "LFE"]);
assert.deepEqual([...player.mutedSpeakers], speakerNames);
player.syncSpeakerMutes(new Set());
assert.deepEqual(speakerNames, []);
const focused = new Set(["FrontLeft", "FrontRight"]);
player.syncSpeakerMutes(new Set(["LFE"]), focused);
focused.clear();
assert.deepEqual(focus, ["FrontLeft", "FrontRight"]);
assert.deepEqual([...player.focusedSpeakers], focus);
assert.deepEqual(speakerNames, []);
assert.equal(player.mutedSpeakers.size, 0);
player.syncSpeakerMutes(new Set(["LFE"]), new Set());
assert.deepEqual(focus, []);
assert.equal(player.focusedSpeakers.size, 0);
assert.deepEqual(speakerNames, ["LFE"]);
console.log("speaker monitor: durable native selection and clear passed");
