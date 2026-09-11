import {test} from 'node:test';
import assert from 'node:assert/strict';
import {settingsWaitView} from '../remote-web/tools.mjs';

test('effect wait follows played audio and distinguishes pause from elapsed wall time',()=>{
  const wait={track:'a',until:50};
  assert.match(settingsWaitView(wait,{track:'a',position:42,running:true}),/8 秒/);
  assert.match(settingsWaitView(wait,{track:'a',position:46,running:true}),/4 秒/);
  assert.match(settingsWaitView(wait,{track:'a',position:46,running:false}),/继续播放/);
  assert.equal(settingsWaitView(wait,{track:'a',position:50,running:true}),null);
  assert.equal(settingsWaitView(wait,{track:'b',position:0,running:true}),null);
});
