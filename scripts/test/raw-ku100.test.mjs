import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { collectIrs } from '../lib/hrtf-source.mjs';

const require = createRequire(import.meta.url);
const ts = require('../../node_modules/.pnpm/typescript@5.9.3/node_modules/typescript');
const code = ts.transpileModule(readFileSync('packages/renderer/src/hrtf.ts','utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {mixIrForWet,buildBusIrs} = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
const context = {sampleRate:48000,createBuffer(channels,length,rate){
  const data = Array.from({length:channels},()=>new Float32Array(length));
  return {length,sampleRate:rate,copyToChannel(values,i){data[i].set(values);},getChannelData(i){return data[i];}};
}};

test('raw bypass does not normalize, balance ears, shift arrival or mirror right wide',()=>{
  const raw={azimuth:-60,elevation:0,dry:new Float32Array([0,2,0,0,0,0,1,0]),dryLen:4,wet:new Float32Array([0,0,0,3,0,0,0,0.5]),wetLen:4};
  const set={sampleRate:48000,calibrated:false,preserveMeasurements:true,subjectId:'ku100',completeSubject:true,positions:[raw,{...raw,azimuth:60,dry:new Float32Array(8)}]};
  const dry=mixIrForWet(context,set,{...raw,azimuth:0},0);
  assert.deepEqual(dry.getChannelData(0),raw.dry.slice(0,4));
  assert.deepEqual(dry.getChannelData(1),raw.dry.slice(4));
  const wet=mixIrForWet(context,set,raw,1);
  assert.deepEqual(wet.getChannelData(0),raw.wet.slice(0,4));
  const bus=buildBusIrs(context,set,[{name:'WideRight',azimuth:-60,elevation:0}],'near').get(0);
  assert(Math.abs(bus.getChannelData(0)[1]-1.92)<1e-6);
});

test('all bundled raw KU100 assets equal original WAV samples',async()=>{
  for(const [kind,path] of [['dry','D1_HRIR_WAV/48K_24bit'],['wet','D1_BRIR_WAV/48K_24bit']]){
    const collection=await collectIrs('tmp/sadie-source/D1.zip',path);
    const source=new Map(collection.impulses.map(ir=>[ir.sourcePath,ir]));
    for(const directory of ['hrtf-raw','hrtf-dense-raw']){
      const base=`apps/web/public/${directory}`;
      const manifest=JSON.parse(readFileSync(`${base}/hrtf-set.json`,'utf8'));
      assert.equal(manifest.processing.preserveMeasurements,true);
      for(const entry of manifest.positions){
        const original=source.get(entry.measurement[kind].sourcePath);
        const expected=new Float32Array(original.left.length*2);
        expected.set(original.left);expected.set(original.right,original.left.length);
        assert.deepEqual(readFileSync(`${base}/${entry[kind]}`),Buffer.from(expected.buffer));
      }
    }
  }
});
