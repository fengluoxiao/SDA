// Compare Android's native C adapter with Windows's real MpeghDecoder.ts + WASM.
// Build: cargo build --manifest-path crates/sda-native/Cargo.toml --no-default-features --example mpegh_capture
// Run: node scripts/test-mpegh-native.mjs <path-to-mpegh_capture-exe> [fixture.mhas]
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { execFileSync } from 'node:child_process';
import assert from 'node:assert/strict';
import ts from 'typescript';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const executable=resolve(process.argv[2]);
const fixture=resolve(process.argv[3]??join(root,'packages/core/mpegh/fixtures/motion.mhas'));
const serial=process.env.SDA_ADB_SERIAL, adb=process.env.SDA_ADB??'adb';
const remote='/data/local/tmp/sda-mpegh-parity';
const device=(args)=>execFileSync(adb,['-s',serial,...args],{stdio:'inherit',windowsHide:true});
if(serial) {
  device(['shell','mkdir','-p',remote]);device(['push',executable,remote+'/capture']);
  device(['push',fixture,remote+'/input.mhas']);device(['shell','chmod','700',remote+'/capture']);
}
const dest=join(root,'tmp/mpegh-parity');mkdirSync(dest,{recursive:true});
const module=(await import(pathToFileURL(join(root,'packages/core/pkg-mpegh/mpegh.js')).href)).default;
const loaded=new Map();
function loadTs(path) {
  path=resolve(path); if(loaded.has(path))return loaded.get(path);
  let source=readFileSync(path,'utf8');
  source=source.replace(/^import createModule .*$/m,'').replace(/^import wasmUrl .*$/m,'');
  const exports={};loaded.set(path,exports);
  const compiled=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
  new Function('require','exports','createModule','wasmUrl',compiled)(
    name=>loadTs(resolve(dirname(path),name.replace(/\.js$/,'')+'.ts')), exports,
    options=>module({...options,locateFile:()=>join(root,'packages/core/pkg-mpegh/mpegh.wasm')}),'unused');
  return exports;
}
const {MpeghDecoder,initMpegh}=loadTs(join(root,'packages/core/src/mpegh.ts'));
await initMpegh();
const input=readFileSync(fixture), decoder=new MpeghDecoder();
const frames=[],pcm=[];
// Byte-sized input exercises the same stateful framing boundary as the native capture.
for(const byte of input) {
  decoder.push(Uint8Array.of(byte));
  for(let frame;frame=decoder.nextFrame();) {
    frames.push({codec:frame.codec,sampleRate:frame.sampleRate,samplePos:frame.samplePos,
      samples:frame.channels[0].length,labels:frame.labels,events:frame.events,
      objectChannels:frame.objectChannels,rawBedLabels:frame.rawBedLabels});
    for(const channel of frame.channels)pcm.push(Buffer.from(channel.buffer,channel.byteOffset,channel.byteLength));
  }
}
decoder.flush();decoder.free();
writeFileSync(join(dest,'windows.json'),JSON.stringify(frames));
writeFileSync(join(dest,'windows.pcm'),Buffer.concat(pcm));
const results=[];
for(const chunk of [1,1024]) {
  const prefix=join(dest,'native-'+chunk);
  if(serial) {
    device(['shell',remote+'/capture',remote+'/input.mhas',remote+'/native-'+chunk,String(chunk)]);
    for(const suffix of ['json','pcm'])device(['pull',remote+'/native-'+chunk+'.'+suffix,prefix+'.'+suffix]);
  } else execFileSync(executable,[fixture,prefix,String(chunk)],{stdio:'inherit',windowsHide:true});
  const actual=JSON.parse(readFileSync(prefix+'.json','utf8'));
  assert.equal(actual.length,frames.length);
  let maxMetadataError=0;
  function compare(a,b,path='') {
    if(typeof a==='number'&&typeof b==='number') { maxMetadataError=Math.max(maxMetadataError,Math.abs(a-b));assert.ok(Math.abs(a-b)<=1e-12,`${path}: ${a} != ${b}`);return; }
    if(a&&typeof a==='object') {
      const keys=Object.keys(a).filter(k=>!(k==='diffuse'&&a[k]===0&&!Object.hasOwn(b,k)));
      const other=Object.keys(b).filter(k=>!(k==='diffuse'&&b[k]===0&&!Object.hasOwn(a,k)));
      assert.deepEqual(keys.sort(),other.sort(),path);
      for(const key of keys)compare(a[key],b[key],`${path}.${key}`);
    } else assert.equal(a,b,path);
  }
  compare(actual,frames);
  const expected=Buffer.concat(pcm), native=readFileSync(prefix+'.pcm');
  assert.equal(native.length,expected.length);
  let maxError=0,squared=0;
  for(let p=0;p<native.length;p+=4) {const e=Math.abs(native.readFloatLE(p)-expected.readFloatLE(p));assert.ok(Number.isFinite(e));maxError=Math.max(maxError,e);squared+=e*e;}
  assert.ok(maxError<=2e-6,`PCM peak error ${maxError} exceeds -114 dBFS tolerance`);
  results.push({chunk,frames:frames.length,floatSamples:native.length/4,maxPcmError:maxError,rmsPcmError:Math.sqrt(squared/(native.length/4)),maxMetadataError});
}
writeFileSync(join(dest,'result.json'),JSON.stringify({fixture,platform:serial??process.platform,results},null,2));
console.log(JSON.stringify(results,null,2));
