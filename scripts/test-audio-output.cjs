// Windows integration test. Opens real devices with outputActive=false throughout.
// Exclusive probing can invalidate another application's loopback session,
// even with silent PCM. It is opt-in and must not run over remote desktop.
const {spawn}=require('node:child_process');
const assert=require('node:assert/strict');
const path=require('node:path');
const executable=process.argv.slice(2).find(arg=>!arg.startsWith('--'))||path.resolve(__dirname,'../apps/native-renderer/target/release/sda-native-renderer.exe');
const child=spawn(executable,[],{windowsHide:true,env:{...process.env,SDA_OUTPUT_SETTINGS:JSON.stringify({deviceId:null,exclusive:false})}});
const events=[];let buffer='';
child.stdout.on('data',chunk=>{
  buffer+=chunk;
  for(;;){const end=buffer.indexOf('\n');if(end<0)break;events.push(JSON.parse(buffer.slice(0,end)));buffer=buffer.slice(end+1);}
});
child.stderr.on('data',chunk=>process.stderr.write(chunk));
const wait=async(test)=>{
  for(let i=0;i<150;i++){const value=test();if(value)return value;await new Promise(r=>setTimeout(r,100));}
  throw Error('Native output timeout: '+JSON.stringify(events.slice(-3)));
};
function send(value){const body=Buffer.from(JSON.stringify(value)),header=Buffer.alloc(5);header[0]=74;header.writeUInt32LE(body.length,1);child.stdin.write(Buffer.concat([header,body]));}
async function command(value){const begin=events.length;send(value);return wait(()=>events.slice(begin).find(e=>e.type==='ack'&&e.command===value.type));}
async function health(){const begin=events.length;send({type:'health'});return wait(()=>events.slice(begin).find(e=>e.type==='health'));}
const latest=()=>events.filter(e=>e.type==='outputDevices').at(-1);
(async()=>{
  await wait(()=>events.find(e=>e.type==='ready'));
  assert.equal((await command({type:'listOutputDevices'})).accepted,true);
  const initial=latest();assert.equal(initial.status.state,'ready','test requires an available default endpoint');
  const id=initial.status.actualId;
  assert(initial.devices.some(d=>d.id===id&&d.available));
  assert.equal((await command({type:'pause',paused:true})).accepted,true);
  const before=await health();
  for(let i=0;i<3;i++){
    assert.equal((await command({type:'setOutputDevice',deviceId:id,exclusive:false})).accepted,true);
    assert.equal(latest().status.actualId,id);
  }
  assert.equal((await command({type:'setOutputDevice',deviceId:'missing-output-test',exclusive:false})).accepted,false);
  assert.equal(latest().status.actualId,id,'failed switch restores previous');
  const exclusive=process.argv.includes('--exclusive')
    ? await command({type:'setOutputDevice',deviceId:id,exclusive:true})
    : {accepted:false,skipped:true,detail:'Exclusive test requires explicit --exclusive; do not run during remote capture'};
  assert.equal(latest().status.mode,exclusive.accepted?'exclusive':'shared');
  const exclusiveStatus=latest().status;
  // Exercise actual sample-rate changes without loading a source or emitting sound.
  const alternate=initial.devices.find(d=>d.available&&d.sampleRate&&d.sampleRate!==initial.status.sampleRate);
  if(alternate){assert.equal((await command({type:'setOutputDevice',deviceId:alternate.id,exclusive:false})).accepted,true);assert.equal(latest().status.sampleRate,alternate.sampleRate);}
  assert.equal((await command({type:'setOutputDevice',deviceId:null,exclusive:false})).accepted,true);
  // Deliberately contradictory preferences must never enter exclusive mode
  // or pin a missing endpoint when remote compatibility is requested.
  assert.equal((await command({type:'setOutputDevice',deviceId:'stale-before-rdp',exclusive:true,remoteCompatible:true})).accepted,true);
  assert.equal(latest().status.mode,'shared');
  assert.equal(latest().status.requested.remoteCompatible,true);
  assert.equal(latest().status.requested.deviceId,null);
  assert.equal(latest().status.requested.exclusive,false);
  const after=await health();
  assert.equal(after.outputActive,false);assert.equal(after.paused,true);
  assert.equal(after.samplePos,before.samplePos,'switch must preserve paused transport position');
  console.log(JSON.stringify({pass:true,endpoint:initial.status.actualName,exclusive,exclusiveStatus,alternateRate:alternate?.sampleRate,pausedPosition:after.samplePos}));
})().catch(error=>{console.error(error);process.exitCode=1;}).finally(()=>{child.stdin.end();setTimeout(()=>child.kill(),500).unref();});
