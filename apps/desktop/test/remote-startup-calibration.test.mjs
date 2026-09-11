import {test} from 'node:test';
import assert from 'node:assert/strict';
import {calibrateStartup} from '../remote-web/startup-calibration.mjs';
class Audio extends EventTarget {
  currentTime=12;muted=false;paused=true;plays=0;
  play(){assert.equal(this.muted,true);this.paused=false;this.plays++;return Promise.resolve();}
  pause(){this.paused=true;}
  advance(){this.currentTime+=.08;this.dispatchEvent(new Event('timeupdate'));}
}
const owner=()=>({audio:new Audio(),mediaEpoch:3,syncRevision:1,syncPhase:'preparing'});
test('cold warmup and warm latency measurement stay silent and restore original program position',()=>{
 const o=owner();assert.equal(calibrateStartup(o),false);assert.equal(o.audio.muted,true);
 o.audio.advance();assert.equal(o.audio.currentTime,12);assert.equal(o.audio.muted,false);assert.equal(o.audio.paused,true);
 assert.equal(o.startupEpoch,undefined);assert.equal(calibrateStartup(o),false);assert.equal(o.audio.plays,1);
 assert.ok(o.startupWarmReadyAt>performance.now());o.startupWarmReadyAt=0;
 assert.equal(calibrateStartup(o),false);assert.equal(o.audio.plays,2);
 o.audio.advance();assert.equal(o.startupEpoch,3);assert.equal(o.audio.currentTime,12);
 assert.equal(calibrateStartup(o),true);assert.equal(o.audio.plays,2);
 assert.ok(o.startupLeadMs>=0&&o.startupLeadMs<=1800);
});
test('cancelled startup restores mute without marking a replacement track calibrated',()=>{
 const o=owner();o.audio.muted=true;calibrateStartup(o);o.mediaEpoch=4;o.startupProbe.cancel();
 assert.equal(o.audio.muted,true);assert.equal(o.audio.paused,true);assert.equal(o.startupEpoch,undefined);assert.equal(o.startupProbe,null);
 o.audio.advance();assert.equal(o.startupEpoch,undefined);
});
test('blocked native playback restores audio and does not claim ready',async()=>{
 const o=owner();o.audio.play=()=>Promise.reject(Error('blocked'));calibrateStartup(o);
 await Promise.resolve();assert.equal(o.syncPhase,'blocked');assert.equal(o.audio.muted,false);assert.equal(o.startupEpoch,undefined);
});
