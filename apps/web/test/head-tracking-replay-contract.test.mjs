import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const source = readFileSync(join(import.meta.dirname, "..", "src", "App.tsx"), "utf8");
const playStart = source.indexOf("const play = useCallback(");
const playEnd = source.indexOf("playRef.current = play;", playStart);
assert.ok(playStart >= 0 && playEnd > playStart, "play callback must be present");
const play = source.slice(playStart, playEnd);

assert.match(source, /const retiringPlayerRef = useRef<SdaPlayer \| null>\(null\)/);
const replacementReady = play.indexOf("const player = await createPlayer(");
const outgoingDispose = play.indexOf("await previous.dispose()");
const invalidateSink = play.indexOf("nativeSessionEpochRef.current++");
assert.ok(invalidateSink >= 0 && outgoingDispose > invalidateSink, "invalidate the previous native sink before disposal");
assert.ok(replacementReady > outgoingDispose, "stop the previous decoder before resetting the native render clock");
assert.match(play, /if \(!isCurrent\(\) \|\| playerRef\.current !== player \|\| feedRequest !== feedRequestRef\.current\) return/);

assert.match(source, /const headTrackingSessionRef = useRef\(new HeadTrackingSession\(/);
assert.match(source, /headTrackingSessionRef\.current\.update\(rendererHeadPose\(pose\)\)/);
assert.match(source, /const latestHeadPose = headTrackingSessionRef\.current\.sample\(\)/);
assert.match(source, /if \(latestHeadPose\) player\.setHeadPose\(latestHeadPose\)/);
assert.match(source, /player\?\.clearHeadPose\?\.\(\);\s*player\?\.setHeadPose\?\.\(headPose\)/);
assert.doesNotMatch(source, /player\?\.recenterHeadPose\?\.\(\)/);

console.log("head-tracking replay handoff contract tests passed");

assert.match(source, /if \(!playbackHeadCentered\) \{[\s\S]*?headTrackingSessionRef\.current\.beginPlayback\(\)/);

// Exercise the actual start callback behind a delayed native queue, not only
// its source shape. A failed start must leave tracking gated for the retry.
const {runInNewContext}=await import("node:vm");
const start=source.indexOf("            startAt: async (origin) => {");
const end=source.indexOf("            pause: async",start);
const callback=source.slice(start+"            startAt: ".length,end).trim().replace(/,$/,"");
for(const accepted of [true,false]){
 const calls=[];let release;
 const gate=new Promise(resolve=>release=resolve);
 const context={playbackHeadCentered:false,ownsNativeSession:()=>true,console,
  headTrackingSessionRef:{current:{beginPlayback(){calls.push("center");}}},
  desktop:{nativeRendererClearPose:async()=>{calls.push("clear");return true;},nativeRendererStartAt:async()=>{calls.push("start");return accepted;}},
  enqueueNative:async(label,operation)=>{await gate;const value=await operation();if(value===false)throw Error("rejected");return value;}};
 const pending=runInNewContext("("+callback+")",context)(0);
 assert.deepEqual(calls,[],"centering must wait for earlier queued commands");
 release();
 assert.equal(await pending,accepted);
 assert.deepEqual(calls,["center","clear","start"]);
 assert.equal(context.playbackHeadCentered,accepted);
}
assert.match(source,/ownsNativeSession\(\) && playbackHeadCentered/);
