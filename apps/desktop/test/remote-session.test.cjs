const test=require("node:test");
const assert=require("node:assert/strict");
const net=require("node:net");
const {EventEmitter}=require("node:events");
const {PassThrough,Writable}=require("node:stream");
const {RemoteSession,packet,decodePackets,parseInvite,validateControl}=require("../remote-session.cjs");
const delay=ms=>new Promise(r=>setTimeout(r,ms));
async function until(fn,ms=8000){const end=Date.now()+ms;while(!fn()){if(Date.now()>end)throw Error("condition timeout");await delay(10);}}
async function freePort(){const server=net.createServer();await new Promise(r=>server.listen(0,"127.0.0.1",r));const port=server.address().port;await new Promise(r=>server.close(r));return port;}
function receiver(records){
  const process=new EventEmitter();process.stdout=new PassThrough();process.stderr=new PassThrough();
  let consumed=0,queued=0;
  const decode=decodePackets((kind,body)=>{if(kind==="A"){records.push(Buffer.from(body));queued+=480;}else if(kind==="R"){consumed+=queued;queued=0;}else throw Error("unknown packet");});
  process.stdin=new Writable({write(chunk,_encoding,done){decode(chunk);done();}});
  const emit=value=>process.stdout.write(JSON.stringify(value)+"\n");
  setImmediate(()=>emit({type:"outputDevices",status:{state:"ready",actualName:"Test PCM endpoint",mode:"exclusive",sampleRate:48000}}));
  const timer=setInterval(()=>{const count=Math.min(960,queued);queued-=count;consumed+=count;emit({type:"remoteProgress",consumed,queued,buffering:false});},20);
  process.kill=()=>{clearInterval(timer);process.stdin.destroy();process.stdout.end();process.emit("exit",0);};return process;
}
async function fixture(){
  const records=[],sent=[],commands=[];let local=null;
  const host=new RemoteSession({status:()=>{},control:command=>{commands.push(command);host.completeControl(command.id);},disconnected:()=>{},route:async config=>{
    local?.destroy();local=null;if(!config.address)return true;
    const [address,port]=config.address.split(":");local=net.connect(+port,address);local.on("error",()=>{});
    await new Promise((resolve,reject)=>{local.once("connect",resolve);local.once("error",reject);});local.write(config.token);
    local.on("data",credits=>{for(const credit of credits){assert.equal(credit,80);const body=Buffer.alloc(3840);for(let i=0;i<960;i++)body.writeFloatLE(Math.sin((sent.length*960+i)*0.03)*0.7,i*4);sent.push(body);const frame=packet("A",body);local.write(frame.subarray(0,3));local.write(frame.subarray(3));}});return true;
  }});
  const results=[];
  const client=new RemoteSession({status:()=>{},executable:()=>"fake",prepareClient:()=>{},spawnReceiver:()=>receiver(records),result:r=>results.push(r)});
  await host.host({port:await freePort(),bufferMs:100});
  const invite=host.invite("127.0.0.1");
  return {host,client,invite,records,sent,commands,results,close:async()=>{await client.stop();await host.stop();local?.destroy();}};
}
test("framing preserves fragmented/coalesced float PCM exactly and rejects oversized frames",()=>{
  const body=Buffer.alloc(3840);for(let i=0;i<960;i++)body.writeFloatLE(i%2?-i/1024:i/1024,i*4);
  const wire=Buffer.concat([packet("A",body),packet("R"),packet("S",{title:"测试"})]);const records=[];
  const read=decodePackets((kind,data)=>records.push([kind,Buffer.from(data)]));
  for(let i=0;i<wire.length;i+=7)read(wire.subarray(i,i+7));
  assert.deepEqual(records[0],["A",body]);assert.equal(records[1][0],"R");assert.equal(records[2][0],"S");
  const invalid=Buffer.alloc(5);invalid.writeUInt32LE(262145,1);assert.throws(()=>read(invalid),/上限/);
});
test("invite and remote commands reject invalid schemes, credentials and arbitrary operations",()=>{
  const key="a".repeat(64)+"."+"b".repeat(64);assert.equal(parseInvite(`sda://127.0.0.1:49632#${key}`).port,49632);
  for(const value of [`https://host:49632#${key}`,`sda://user@host:49632#${key}`,"sda://host:12#123","junk"])assert.throws(()=>parseInvite(value));
  for(const command of [{action:"openPath",value:"C:/secret"},{action:"volume",value:2},{action:"volume",value:NaN},{action:"playbackMode",value:"bad"}])assert.throws(()=>validateControl(command));
  assert.deepEqual(validateControl({action:"pause",extra:"discard"}),{action:"pause"});
});
test("TLS pinned one-to-one streaming is byte exact, bounded by receiver credits, and controls are acknowledged",async()=>{
  const f=await fixture();try{
    await f.client.join({invite:f.invite});await until(()=>f.records.length>=30);
    assert.equal(f.client.phase,"connected");assert.equal(f.host.phase,"connected");
    for(let i=0;i<f.records.length;i++)assert.deepEqual(f.records[i],f.sent[i]);
    assert.ok(f.host.queued<=4800);
    f.host.publishState({title:"Song",playlist:[{id:"1",title:"Song",path:"PRIVATE"}],position:2,volume:1});
    await until(()=>f.client.state?.title==="Song");assert.equal(f.client.state.playlist[0].path,undefined);
    const id=f.client.command({action:"pause"});await until(()=>f.results.some(r=>r.id===id));assert.equal(f.commands[0].action,"pause");
    const second=new RemoteSession({status:()=>{},executable:()=>"fake",spawnReceiver:()=>receiver([])});
    try{await second.join({invite:f.invite}).catch(()=>{});await until(()=>second.phase==="disconnected"||second.role==="off");assert.equal(f.host.peer?.destroyed,false);assert.equal(f.client.phase,"connected");}finally{await second.stop();}
    await f.client.stop();await until(()=>f.host.phase==="waiting");
    const start=f.records.length;await f.client.join({invite:f.invite});await until(()=>f.records.length>start+3);
    assert.equal(f.client.phase,"connected");
  }finally{await f.close();}
});
test("wrong pairing key never reaches control or audio and host remains available",async()=>{
  const f=await fixture();try{
    const wrong=f.invite.slice(0,f.invite.indexOf("#")+1)+"0".repeat(64)+"."+f.host.certificate.fingerprint;
    await assert.rejects(f.client.join({invite:wrong}));assert.equal(f.host.peer,null);assert.equal(f.sent.length,0);
    await f.client.join({invite:f.invite});await until(()=>f.records.length>2);
  }finally{await f.close();}
});
test("wrong certificate fingerprint is rejected before pairing or audio",async()=>{
  const f=await fixture();try{
    const wrong=f.invite.slice(0,f.invite.lastIndexOf(".")+1)+"0".repeat(64);
    await assert.rejects(f.client.join({invite:wrong}),/指纹/);assert.equal(f.host.peer,null);assert.equal(f.sent.length,0);
  }finally{await f.close();}
});
test("occupied port leaves no partial host session",async()=>{
  const occupied=net.createServer();await new Promise(r=>occupied.listen(0,"0.0.0.0",r));
  const host=new RemoteSession({status:()=>{},route:async()=>true,disconnected:()=>{}});
  try{await assert.rejects(host.host({port:occupied.address().port}),/监听端口/);assert.equal(host.role,"off");assert.equal(host.key,null);}
  finally{await host.stop();await new Promise(r=>occupied.close(r));}
});

test("custom pairing keys are stable, random defaults survive reopening, and invalid keys leave hosting off",async()=>{
  const host=new RemoteSession({status:()=>{},route:async()=>true});
  try {
    const port=await freePort();
    await host.host({port,pairingKey:" 我的密钥-123 "});const first=Buffer.from(host.key);
    assert.deepEqual(parseInvite(host.invite("127.0.0.1")).key,first);
    assert.equal(new URL(host.webInvite("127.0.0.1")).hash.slice(1),first.toString("hex"));
    await host.stop();await host.host({port,pairingKey:"我的密钥-123"});assert.deepEqual(host.key,first);
    await host.stop();await host.host({port,pairingKey:"different"});assert.notDeepEqual(host.key,first);
    await host.stop();await host.host({port,pairingKey:" "});const random=Buffer.from(host.key);
    await host.stop();await host.host({port});assert.deepEqual(host.key,random);
    await host.stop();await host.host({port,pairingKey:"ab".repeat(32)});assert.equal(host.key.toString("hex"),"ab".repeat(32));
    await host.stop();for(const pairingKey of [null,123,"a".repeat(257)])await assert.rejects(host.host({port,pairingKey}),/密钥/);
    assert.equal(host.role,"off");assert.equal(host.key,null);
  } finally {await host.stop();}
});

test("remote sound controls validate settings and never forward file paths",()=>{
  const monitor=require('../monitor-settings.cjs').defaults();
  const settings={enabled:false,directDb:0,earlyDb:0,lateDb:0,earlyMs:50,bassEnabled:false,crossoverHz:80,bassDb:0,speakers:{},monitor};
  const change=validateControl({action:'monitorSettings',value:{expected:JSON.stringify(monitor),settings:{...monitor,path:'PRIVATE'}}});
  assert.equal(change.value.settings.path,undefined);
  assert.throws(()=>validateControl({action:'monitorSettings',value:{expected:'{}',settings:{...monitor,levelDb:9}}}));
  const room=validateControl({action:'roomSettings',value:{expected:'{}',settings}});assert.equal(room.value.settings.monitor,undefined);
  assert.throws(()=>validateControl({action:'roomApply',value:'../secret.json'}));
  assert.throws(()=>validateControl({action:'hrtf',value:'C:/secret'}));
  assert.throws(()=>validateControl({action:'roomGenerate',value:{layout:'7.1.4',material:'studio',length:999}}));
  const host=new RemoteSession({});const tools=host.curateTools({layout:'7.1.4',head:'ku100',cinema:{settings,profileId:null},speakers:[],heads:[],rooms:[{id:'a'.repeat(64),name:'Room',layout:'7.1.4',path:'PRIVATE',simulation:{secret:'PRIVATE'}}]});
  assert.equal(JSON.stringify(tools).includes('PRIVATE'),false);assert.equal(tools.cinema.settings.monitor.levelDb,0);
});

test("saved custom key survives reopening and a fresh session, explicit empty resets it",async()=>{
  let saved="";const hooks={route:async()=>true,savedPairingKey:()=>saved,rememberPairingKey:key=>{saved=key;}};let host=new RemoteSession(hooks);
  try {
    const port=await freePort();await host.host({port,pairingKey:"自定义密钥-123"});const key=host.key.toString("hex");assert.equal(saved,"自定义密钥-123");
    await host.stop();await host.host({port});assert.equal(host.key.toString("hex"),key);
    await host.stop();host=new RemoteSession(hooks);await host.host({port});assert.equal(host.key.toString("hex"),key);
    assert.equal(host.status().pairingKey,undefined);await host.stop();await host.host({port,pairingKey:""});assert.equal(saved,"");assert.notEqual(host.key.toString("hex"),key);
  } finally {await host.stop();}
});
