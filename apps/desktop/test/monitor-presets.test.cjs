const {test}=require("node:test"),assert=require("node:assert/strict"),fs=require("node:fs"),path=require("node:path"),vm=require("node:vm");
const ts=require("../../../node_modules/typescript");
const {validate,defaults}=require("../monitor-settings.cjs");
const source=fs.readFileSync(path.join(__dirname,"../../web/src/monitor-presets.ts"),"utf8");
const moduleExports={};
vm.runInNewContext(ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2020}}).outputText,{exports:moduleExports});
test("built-in monitor presets validate all factory layouts and preserve live gain/mutes",()=>{
  const base=["FrontLeft","FrontRight"];
  const surround=[...base,"Center","LFE","SurroundLeft","SurroundRight"];
  const seven=[...surround,"RearLeft","RearRight","TopFrontLeft","TopFrontRight","TopRearLeft","TopRearRight"];
  for(const names of [base,surround,seven,[...seven,"WideLeft","WideRight"],[...seven,"WideLeft","WideRight","TopMiddleLeft","TopMiddleRight"]]){
    const current={...defaults(),enabled:true,levelDb:-27,dim:true,muted:true,outputs:{FrontLeft:{trimDb:3,delayMs:4,invert:true,muted:true}}};
    for(const id of ["transparent","bass-80"]){
      if(id==="bass-80"&&!names.includes("LFE")){assert.throws(()=>moduleExports.createMonitorPreset(id,current,names));continue;}
      const result=validate(moduleExports.createMonitorPreset(id,current,names));
      assert.equal(result.levelDb,-27);assert(result.muted);assert(result.dim);assert(result.enabled);
      assert(result.outputs.FrontLeft.muted);assert.equal(result.outputs.FrontLeft.trimDb,0);
      assert.equal(result.outputs.FrontLeft.delayMs,0);assert.equal(Object.keys(result.outputs).length,names.length);
      assert.equal(result.bassEnabled,id==="bass-80");assert.equal(result.crossoverHz,80);
      assert.equal(current.outputs.FrontLeft.trimDb,3);
    }
  }
});
