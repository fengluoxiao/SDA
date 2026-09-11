const test=require('node:test'),assert=require('node:assert/strict');
test('track switch silences cached media, rejects stale epochs and waits for ready audio',async()=>{
 const {beginTrackSwitch,acceptHlsMedia,receiverView,cancelTrackSwitch}=await import('../remote-web/playback-sync.mjs');
 const audio={src:'old',paused:false,currentTime:52,readyState:4,pause(){this.paused=true;},removeAttribute(){this.src='';},load(){this.currentTime=0;}};
 const owner={audio,mediaEpoch:4,loadedMediaEpoch:4,ready:true};
 const info=(epoch,ready)=>({mediaEpoch:epoch,mediaReady:ready,stream:`/hls/test/e${epoch}/index.m3u8`,positionBase:0});
 beginTrackSwitch(owner);assert.equal(audio.paused,true);assert.equal(audio.src,'');
 assert.equal(receiverView(owner,{playing:true,position:52}).position,0);
 assert.equal(acceptHlsMedia(owner,info(4,true)),false);
 assert.equal(acceptHlsMedia(owner,info(5,false)),false);assert.equal(audio.src,'');
 // A second click must not accept the first switch's late ready message.
 beginTrackSwitch(owner);assert.equal(acceptHlsMedia(owner,info(5,true)),false);
 assert.equal(acceptHlsMedia(owner,info(6,true)),true);
 assert.equal(acceptHlsMedia(owner,info(5,true)),false);assert.match(audio.src,/e6/);
 owner.pendingStart=false;owner.mediaWaiting=false;audio.paused=false;audio.currentTime=1.25;
 assert.equal(receiverView(owner,{playing:true,position:7}).position,1.25);
 beginTrackSwitch(owner);cancelTrackSwitch(owner);assert.match(audio.src,/e6/);assert.equal(owner.mediaPending,false);
});
test('reset before track state can accept the current pending epoch',async()=>{
 const {beginTrackSwitch,acceptHlsMedia}=await import('../remote-web/playback-sync.mjs');
 const owner={mediaEpoch:1,loadedMediaEpoch:1,audio:{pause(){},removeAttribute(){},load(){}}};
 acceptHlsMedia(owner,{mediaEpoch:2,mediaReady:false});beginTrackSwitch(owner,false);
 assert.equal(acceptHlsMedia(owner,{mediaEpoch:2,mediaReady:true,stream:'new'}),true);
 assert.equal(owner.mediaClockBase,0);assert.equal(owner.mediaPending,false);
});
test('host pause/resume synchronizes native media without replaying each state update',async()=>{
 const {syncHostPlayback}=await import('../remote-web/playback-sync.mjs');let plays=0;const owner={audio:{paused:false,pause(){this.paused=true;}}};const resume=async o=>{plays++;o.audio.paused=false;};
 syncHostPlayback(owner,{playing:true,paused:false},resume,assert.fail);assert.equal(plays,1);
 syncHostPlayback(owner,{playing:true,paused:true},resume,assert.fail);assert.equal(owner.audio.paused,true);
 syncHostPlayback(owner,{playing:true,paused:false},resume,assert.fail);assert.equal(owner.audio.paused,false);assert.equal(plays,2);
 owner.audio.pause();syncHostPlayback(owner,{playing:true,paused:false},resume,assert.fail);assert.equal(plays,2);
 syncHostPlayback(owner,{playing:false,paused:false},resume,assert.fail);assert.equal(owner.audio.paused,true);
 owner.testing=true;syncHostPlayback(owner,{playing:true,paused:false},resume,assert.fail);assert.equal(plays,2);assert.equal(owner.hostRunning,true);
});
test('host startup keeps idle media paused until confirmed output',async()=>{
 const {syncHostPlayback}=await import('../remote-web/playback-sync.mjs');let plays=0;
 const owner={audio:{paused:false,pause(){this.paused=true;}}};
 const resume=async o=>{plays++;o.audio.paused=false;};
 syncHostPlayback(owner,{playing:true,loading:true},resume,assert.fail);
 assert.equal(owner.audio.paused,true);assert.equal(plays,0);
 syncHostPlayback(owner,{playing:true,loading:false},resume,assert.fail);
 syncHostPlayback(owner,{playing:true,loading:false},resume,assert.fail);
 assert.equal(plays,1);
});
