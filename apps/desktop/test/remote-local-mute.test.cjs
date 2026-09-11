const test=require('node:test');const assert=require('node:assert/strict');const {RemoteSession,validateControl}=require('../remote-session.cjs');
test('computer mute defaults on and uses authoritative host preference',()=>{
 let muted=true;const host=new RemoteSession({localMuted:()=>muted});host.publishState({title:'Song',localMuted:false});assert.equal(host.state.localMuted,true);assert.equal(host.status().localMuted,true);
 muted=false;host.publishState({title:'Song',localMuted:true});assert.equal(host.state.localMuted,false);assert.equal(host.status().localMuted,false);
 assert.deepEqual(validateControl({action:'localMute',value:false}),{action:'localMute',value:false});assert.throws(()=>validateControl({action:'localMute',value:'false'}));
});
