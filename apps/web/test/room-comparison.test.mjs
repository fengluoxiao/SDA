import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import {runInNewContext} from 'node:vm';
import test from 'node:test';

const source=new URL('../src/room-comparison.ts',import.meta.url);
const require=createRequire(source);
const ts=require('typescript');
const output=ts.transpileModule(readFileSync(source,'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,esModuleInterop:true}}).outputText;
const exports={};runInNewContext(output,{exports,require});
const {comparisonGain}=exports;
const levels=require('./ku100-comparison-levels.json');

test('all nine comparison conditions share one downward-only reference for every layout',()=>{
  const simulation={comparison:{energyDb:{room:-5,direct:-8,early:-6}}};
  for(const [layout,ku100] of Object.entries(levels.layouts)) {
    const actual=[];
    for(const mode of ['raw','calibrated','room'])for(const stage of ['direct','early','full']) {
      const gain=comparisonGain(layout,simulation,mode,stage);
      assert(Number.isFinite(gain)&&gain<=0&&gain>=-40);
      const energy=mode==='room'?simulation.comparison.energyDb[stage==='full'?'room':stage]:ku100[mode][stage];
      actual.push(energy+gain);
    }
    for(const energy of actual)assert(Math.abs(energy-actual[0])<1e-10);
    assert.notEqual(comparisonGain(layout,simulation,'raw','direct'),comparisonGain(layout,simulation,'raw','full'));
  }
});
test('missing stage reference cannot silently use another stage',()=>{
  assert.throws(()=>comparisonGain('7.1.4',{comparison:{energyDb:{room:-5}}},'room','early'));
});
