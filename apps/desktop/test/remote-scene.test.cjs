const test=require('node:test'),assert=require('node:assert/strict');const {RemoteSession,validateControl}=require('../remote-session.cjs');
test('scene metadata is bounded, finite and never forwards arbitrary fields',()=>{
 const host=new RemoteSession({});host.publishScene({trackId:'track',objects:Array.from({length:300},(_,id)=>({id,pos:[NaN,Infinity,1],size:[1,0,0],hasPos:true,gainDb:0,path:'secret'})),layout:[{name:'Center',azimuth:0,elevation:0,distance:1,path:'secret'}],muted:[1],sounding:[2],hiddenSpeakers:[]});
 assert.equal(host.scene.objects.length,256);assert.deepEqual(host.scene.objects[0].pos,[0,0,1]);assert.ok(!JSON.stringify(host.scene).includes('secret'));assert.equal(host.scene.layout[0].distance,1);assert.deepEqual(validateControl({action:'scene',value:'secret'}),{action:'scene'});
 host.publishScene({objects:[],layout:[],trackId:'new'});assert.equal(host.scene.objects.length,0);assert.equal(host.scene.trackId,'new');
});
