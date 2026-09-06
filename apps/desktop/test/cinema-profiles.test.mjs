import assert from 'node:assert/strict';
import test from 'node:test';
import profiles from '../cinema-profiles.cjs';

const fixture = () => ({version:1,name:'Synthetic test fixture',source:'Unit test; not a measured room',license:'Test only',measurement:'dummy-head',sampleRate:48000,layout:'2.0',speakers:[['FrontLeft',30,20,1],['FrontRight',-30,68,0.5]].map(([name,azimuth,onset,level])=>{
  const left=Array(512).fill(0),right=Array(512).fill(0);left[onset]=level;right[onset+4]=level;
  return {name,azimuth,elevation:0,onsetSample:onset,directLeft:left,directRight:right,roomLeft:[...left],roomRight:[...right]};
})});
test('room validation, suggested relative alignment, and integrity identifiers',()=>{
 const room=profiles.validateRoom(fixture());
 const {suggested,rows}=profiles.analyzeRoom(room);
 assert.equal(suggested.FrontLeft.delayMs,1);
 assert.equal(suggested.FrontRight.delayMs,0);
 assert(Math.abs(suggested.FrontLeft.gainDb+6.0206)<1e-4);
 assert.equal(suggested.FrontRight.gainDb,0);
 assert.equal(rows[0].itdMs,4/48);
 assert.notEqual(profiles.roomId(Buffer.from('a')),profiles.roomId(Buffer.from('b')));
});
test('incomplete, mismatched, nonfinite and mislabeled measurements are rejected',()=>{
 for(const change of [r=>r.speakers.pop(),r=>r.speakers[0].roomLeft[0]=Infinity,r=>r.speakers[0].azimuth=0,r=>r.source='',r=>r.sampleRate=44100,r=>r.speakers[0].roomRight.pop()]){
  const r=fixture();change(r);assert.throws(()=>profiles.validateRoom(r));
 }
});
test('cinema control bounds include channel restrictions',()=>{
 const settings={enabled:true,directDb:0,earlyDb:0,lateDb:0,earlyMs:50,bassEnabled:false,crossoverHz:80,bassDb:0,speakers:{}};
 assert.deepEqual(profiles.validateSettings(settings),settings);
 for(const reflectionMode of ['direct','early','full'])assert.equal(profiles.validateSettings({...settings,reflectionMode}).reflectionMode,reflectionMode);
 assert.throws(()=>profiles.validateSettings({...settings,reflectionMode:'invalid'}));
 assert.throws(()=>profiles.validateSettings({...settings,crossoverHz:500}));
 assert.throws(()=>profiles.validateSettings({...settings,speakers:{LFE:{gainDb:0,delayMs:0,lowDb:1,highDb:0}}}));
});
