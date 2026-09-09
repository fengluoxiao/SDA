const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm');
const ts=require('../../../node_modules/typescript');
const {validate,defaults}=require('../monitor-settings.cjs');
const exportsObject={};
vm.runInNewContext(ts.transpileModule(fs.readFileSync(path.join(__dirname,'../../web/src/hardware-presets.ts'),'utf8'),{
  compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2020},
}).outputText,{exports:exportsObject});
test('AHB2 modes validate and preserve independent listening controls',()=>{
  const current={...defaults(),levelDb:-17,dim:true,muted:true,hardware:{enabled:false,dacBits:20}};
  for(const p of exportsObject.HARDWARE_PRESETS){
    const result=validate(exportsObject.createHardwarePreset(p.id,current)),h=result.hardware;
    assert.equal(result.levelDb,-17);assert.equal(result.dim,true);assert.equal(result.muted,true);
    assert.equal(h.enabled,false);assert.equal(h.dacBits,20);assert.equal(h.inputDb,0);
    // Recover rated terminal power after the model's output-impedance divider.
    const terminalPeak=h.railV*h.loadOhms/(h.loadOhms+h.outputOhms);
    assert.ok(Math.abs(terminalPeak**2/(2*h.loadOhms)-100)<1e-8);
    assert.ok(Math.abs(h.lineRms*10**(h.gainDb/20)/Math.sqrt(100*8)-1)<.01);
    assert.equal(exportsObject.matchingHardwarePreset(h),p.id);
    assert.equal(exportsObject.matchingHardwarePreset({...h,inputDb:-6}),p.id,
      'user input gain is independent of the amplifier mode');
    assert.equal(exportsObject.matchingHardwarePreset({...h,gainDb:h.gainDb+1}),'');
  }
  assert.equal(current.hardware.dacBits,20);
  assert.throws(()=>exportsObject.createHardwarePreset('missing',current));
});
