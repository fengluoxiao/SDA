import assert from 'node:assert/strict';
import { build } from 'esbuild';
import { readFileSync } from 'node:fs';
const compiled=await build({entryPoints:['apps/mobile/src/nativeShaderLogs.ts'],bundle:true,platform:'node',format:'cjs',write:false});
const module={exports:{}};
new Function('module','exports',compiled.outputFiles[0].text)(module,module.exports);
const {normalizeNativeShaderLogs}=module.exports;
for(const absent of [undefined,null,'']) {
 const calls=[];const program={},shader={};
 const gl={
  getProgramInfoLog(value){assert.equal(this,gl);calls.push(value);return absent;},
  getShaderInfoLog(value){assert.equal(this,gl);calls.push(value);return absent;},
 };
 // The same first-use calls in the pinned Three WebGLProgram fail before normalization.
 if(absent==null) assert.throws(()=>gl.getProgramInfoLog(program).trim(),TypeError);
 calls.length=0;
 normalizeNativeShaderLogs(gl);
 assert.equal(gl.getProgramInfoLog(program).trim(),'');
 assert.equal(gl.getShaderInfoLog(shader).trim(),'');
 assert.deepEqual(calls,[program,shader]);
 const wrapped=gl.getProgramInfoLog;normalizeNativeShaderLogs(gl);assert.equal(gl.getProgramInfoLog,wrapped);
}
const diagnostic='  driver compilation error\n';
const failure=new Error('context lost');
const gl={getProgramInfoLog(){return diagnostic;},getShaderInfoLog(){throw failure;}};
normalizeNativeShaderLogs(gl);
assert.equal(gl.getProgramInfoLog({}),diagnostic,'real diagnostics must remain intact');
assert.throws(()=>gl.getShaderInfoLog({}),error=>error===failure,'native failures must not be swallowed');
const scene=readFileSync('apps/mobile/src/MobileObjectScene.tsx','utf8');
assert.match(scene,/const context = state\.gl\.getContext\(\) as any;\s*normalizeNativeShaderLogs\(context\);/);
assert.doesNotMatch(scene,/checkShaderErrors\s*=\s*false/);
console.log('Native shader diagnostics regression passed: null/undefined first-use logs, native binding, idempotence, preserved diagnostics/exceptions and renderer integration.');
