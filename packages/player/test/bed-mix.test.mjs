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

// File-owned routing is protected from the first init, not just after PCM arrives.
const protectedPlayer = Object.create(SdaPlayer.prototype);
const calls = [];
Object.assign(protectedPlayer, {
  authoredSourceExpected: true, authoredInput: null,
  cb: {onError: message => {throw Error(message);}},
  knownBedLabels: ['L','R','LFE'], objectChannels: new Map([[1,{}]]),
  nativeRendererSink: {
    setMuted: (...args) => calls.push(['mute',...args]),
    setLfeMuted: (...args) => calls.push(['lfe',...args]),
    setSpeakerMutes: (...args) => calls.push(['speakers',...args]),
  },
  renderer: {
    setSourceMuted: (...args) => calls.push(['web-mute',...args]),
    setLfeMuted: (...args) => calls.push(['web-lfe',...args]),
  },
});
function ordinaryRoutingSync(){
  protectedPlayer.syncSpeakerMutes(new Set(),new Set());
  protectedPlayer.syncSpeakerMutes(new Set(['FrontLeft']),new Set(['FrontRight']));
  protectedPlayer.syncBedMix(new Set(['L']),new Set(['R']));
  protectedPlayer.applyBedMute('bed:0','L',48000);
  protectedPlayer.setLfeMuted(true);
  protectedPlayer.setObjectMuted(1,true);
  protectedPlayer.syncObjectMutes(new Set([1]));
}
ordinaryRoutingSync();
assert.deepEqual(calls,[], 'preparing SDAR cannot queue ordinary routing controls');
protectedPlayer.authoredSourceExpected = false;
protectedPlayer.authoredInput = {};
ordinaryRoutingSync();
assert.deepEqual(calls,[], 'active SDAR cannot overwrite file routing on layout/state sync');
protectedPlayer.authoredInput = null;
protectedPlayer.syncSpeakerMutes(new Set(['FrontLeft']));
assert.deepEqual(calls,[['speakers',['FrontLeft'],[]]],'ordinary playback still forwards monitoring controls');
console.log('SDAR routing protection: pending/active files skip ordinary speaker, bed, object and LFE sync; ordinary playback remains controllable');

const editable=Object.create(SdaPlayer.prototype),order=[];let resumeAllowed=true;
Object.assign(editable,{authoredSourceExpected:true,authoredMasterEditable:true,disposed:false,pausedState:false,authoredControlChain:Promise.resolve(),authoredInput:{setCinema:async settings=>{order.push(settings.id);return true},shouldResume:()=>resumeAllowed},pause:async()=>{order.push('pause');editable.pausedState=true},resume:async()=>{order.push('resume');editable.pausedState=false}});
await Promise.all([editable.editAuthoredCinema({id:'room-a'},null),editable.editAuthoredCinema({id:'room-b'},null)]);
assert.deepEqual(order,['pause','room-a','resume','pause','room-b','resume'],'master edits serialize transport changes');
order.length=0;resumeAllowed=false;await editable.editAuthoredCinema({id:'room-c'},null);assert.deepEqual(order,['pause','room-c'],'a user pause during reconfiguration is preserved');
editable.authoredMasterEditable=false;await assert.rejects(()=>editable.editAuthoredCinema({},null),/只有 WAV 母版/);
const opening=Object.create(SdaPlayer.prototype);Object.assign(opening,{authoredSourceExpected:true,authoredInput:null,initArgs:{},nativeRendererSink:{setLayout:()=>{throw Error('ordinary layout must not reset an opening file')}}});opening.setLayout([],true);
console.log('WAV-only authoring serializes room edits, respects pause intent and protects pending file initialization');
