const {test}=require("node:test"),assert=require("node:assert/strict"),fs=require("node:fs"),path=require("node:path"),vm=require("node:vm");
const ts=require("../../../node_modules/typescript");
const {defaults,validate}=require("../monitor-settings.cjs");
const catalog=require("../builtin-rooms/catalog.json");
const functions={};
vm.runInNewContext(ts.transpileModule(fs.readFileSync(path.join(__dirname,"../../web/src/monitor-alignment.ts"),"utf8"),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2020}}).outputText,{exports:functions});
const cinema=speakers=>({enabled:true,speakers});
test("all built-in responses align once and preserve monitor transport state and LFE",()=>{
  for(const entry of catalog.profiles){
    const room={...entry.summary,builtin:true};
    const names=room.rows.map(r=>r.name);if(room.layout!=="2.0")names.push("LFE");
    const current={...defaults(),enabled:true,levelDb:-22,dim:true,muted:true,outputs:{LFE:{trimDb:-3,delayMs:1,invert:true,muted:true}}};
    const result=validate(functions.alignMonitorToRoom(room,cinema({}),current,names,room.layout));
    assert.equal(result.levelDb,current.levelDb);assert(result.dim&&result.muted&&result.enabled);
    assert.deepEqual(result.outputs.LFE,current.outputs.LFE);
    const arrivals=room.rows.map(r=>r.arrivalMs+result.outputs[r.name].delayMs);
    const energies=room.rows.map(r=>r.directEnergyDb+result.outputs[r.name].trimDb);
    assert(Math.max(...arrivals)-Math.min(...arrivals)<0.002);
    assert(Math.max(...energies)-Math.min(...energies)<0.002);
    const repeated=validate(functions.alignMonitorToRoom(room,cinema({}),result,names,room.layout));
    assert.deepEqual(repeated,result);
    const residual=validate(functions.alignMonitorToRoom(room,cinema(room.suggested),result,names,room.layout));
    for(const r of room.rows){assert(Math.abs(residual.outputs[r.name].trimDb)<0.002);assert(Math.abs(residual.outputs[r.name].delayMs)<0.002);}
  }
});
test("alignment rejects mismatched, incomplete and equalized responses",()=>{
  const room={...catalog.profiles[0].summary,builtin:true},names=room.rows.map(r=>r.name);
  assert.throws(()=>functions.alignMonitorToRoom(room,{enabled:false,speakers:{}},defaults(),names,room.layout));
  assert.throws(()=>functions.alignMonitorToRoom(room,cinema({}),defaults(),names,"2.0"));
  assert.throws(()=>functions.alignMonitorToRoom({...room,rows:[]},cinema({}),defaults(),names,room.layout));
  assert.throws(()=>functions.alignMonitorToRoom(room,cinema({[names[0]]:{gainDb:0,delayMs:0,lowDb:1,highDb:0}}),defaults(),names,room.layout));
  assert.throws(()=>functions.alignMonitorToRoom({...room,rows:room.rows.map((r,i)=>({...r,arrivalMs:i*30}))},cinema({}),defaults(),names,room.layout));
});
