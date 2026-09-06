import assert from 'node:assert/strict';
import test from 'node:test';
import {validateConfig} from '../room-lab.cjs';
import profiles from '../cinema-profiles.cjs';
import {readFileSync} from 'node:fs';
const config={layout:'7.1.4',length:6,width:4,height:2.8,earHeight:1.2,placement:.85,material:'treated',order:6};
test('simulation has canonical non-LFE directions for every layout',()=>{
  for(const layout of ['2.0','2.1','5.1','5.1.2','5.1.4','7.1.2','7.1.4','9.1.2','9.1.4','9.1.6']){
    const result=validateConfig({...config,layout});
    const [floor,,top=0]=layout.split('.').map(Number);
    assert.equal(result.speakers.length,floor+top);
    assert(!result.speakers.some(s=>s.name==='LFE'));
  }
  assert.throws(()=>validateConfig({...config,length:NaN}));
  assert.throws(()=>validateConfig({...config,order:1.5}));
  assert.throws(()=>validateConfig({...config,material:'arbitrary'}));
});
test('generated simulation retains provenance and response data through import',()=>{
  const source=JSON.parse(readFileSync('tmp/room-smoke-profile.json','utf8'));
  const profile=profiles.validateRoom(source);
  assert.equal(profile.measurement,'simulated');assert.equal(profile.simulation.engine,'pyroomacoustics 0.10.1');
  assert.deepEqual(profile.speakers[0].roomLeft,source.speakers[0].roomLeft);
  assert(profiles.roomSummary(profile,'fixture').simulation.comparison.gainDb.raw<=0);
});
