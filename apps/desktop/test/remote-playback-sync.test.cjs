const test=require('node:test'),assert=require('node:assert/strict');
test('host pause/resume synchronizes native media without replaying each state update',async()=>{
 const {syncHostPlayback}=await import('../remote-web/playback-sync.mjs');let plays=0;const owner={audio:{paused:false,pause(){this.paused=true;}}};const resume=async o=>{plays++;o.audio.paused=false;};
 syncHostPlayback(owner,{playing:true,paused:false},resume,assert.fail);assert.equal(plays,1);
 syncHostPlayback(owner,{playing:true,paused:true},resume,assert.fail);assert.equal(owner.audio.paused,true);
 syncHostPlayback(owner,{playing:true,paused:false},resume,assert.fail);assert.equal(owner.audio.paused,false);assert.equal(plays,2);
 owner.audio.pause();syncHostPlayback(owner,{playing:true,paused:false},resume,assert.fail);assert.equal(plays,2);
 syncHostPlayback(owner,{playing:false,paused:false},resume,assert.fail);assert.equal(owner.audio.paused,true);
 owner.testing=true;syncHostPlayback(owner,{playing:true,paused:false},resume,assert.fail);assert.equal(plays,2);assert.equal(owner.hostRunning,true);
});
