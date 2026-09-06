import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import {runInNewContext} from 'node:vm';
import test from 'node:test';
const require=createRequire(import.meta.url),ts=require('typescript');
const exports={};
runInNewContext(ts.transpileModule(readFileSync(new URL('../src/room-path-animation.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS}}).outputText,{exports});
const {pathPosition}=exports;
const point=(path,distance)=>JSON.parse(JSON.stringify(pathPosition(path,distance)));

test('propagation follows segment distance through a reflection instead of interpolating endpoints',()=>{
  const reflected=[[0,0,0],[3,0,0],[3,4,0]];
  assert.deepEqual(point(reflected,0),[0,0,0]);
  assert.deepEqual(point(reflected,3),[3,0,0]);
  assert.deepEqual(point(reflected,5),[3,2,0]);
  assert.deepEqual(point(reflected,7),[3,4,0]);
  assert.equal(point(reflected,7.001),null);
  assert.equal(point(reflected,-.1),null);
});
test('a common propagation distance makes direct sound arrive before a longer reflected path',()=>{
  const direct=[[0,0,0],[3,4,0]],reflected=[[0,0,0],[3,0,0],[3,4,0]];
  assert.equal(point(direct,6),null);
  assert.deepEqual(point(reflected,6),[3,3,0]);
  assert.deepEqual(point([[0,0,0],[0,0,0],[1,0,0]],.5),[.5,0,0]);
});
