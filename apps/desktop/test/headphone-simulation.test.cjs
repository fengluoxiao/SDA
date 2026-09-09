const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
const {simulate}=require('../headphone-simulation.cjs');
const root=path.resolve(__dirname,'../../web/public/headphone-compensation');
const load=id=>fs.readFileSync(path.join(root,id,'average.f32'));
const dt=load('beyerdynamic-dt-1990-balanced-average-autoeq'),hd=load('sennheiser-hd-820-average-autoeq');
function response(bytes,f){let re=0,im=0;for(let n=0;n<bytes.length/4;n++){const v=bytes.readFloatLE(n*4),w=2*Math.PI*f*n/48000;re+=v*Math.cos(w);im-=v*Math.sin(w);}return 20*Math.log10(Math.hypot(re,im));}
test('same headphone is exact identity without attenuation',()=>{
 const s=simulate(dt,dt,dt,dt);assert.equal(s.preamp,1);assert.equal(s.left.readFloatLE(0),1);
 for(let n=1;n<s.left.length/4;n++)assert.equal(s.left.readFloatLE(n*4),0);
});
test('target colour reverses correction, not the existing EQ',()=>{
 const s=simulate(null,null,dt,dt);
 assert(response(dt,8000)<-9);assert(response(s.left,8000)>8);
 assert.deepEqual(s.left,s.right);assert(s.preamp<1);
});
test('source correction minus target correction, with independent ears',()=>{
 const s=simulate(hd,dt,dt,hd);
 const expected=response(hd,250)-response(dt,250);
 assert(Math.abs(response(s.left,250)-expected)<.5);
 assert(Math.abs(response(s.right,250)+expected)<.5);
});
test('all bundled targets match pink reference energy with finite bounded gain',()=>{
 for(const id of fs.readdirSync(root)) {
  const s=simulate(null,null,load(id),load(id));
  for(let n=0;n<s.left.length/4;n++)assert(Number.isFinite(s.left.readFloatLE(n*4)));
  let energy=0,count=0;
  for(let f=20;f<=20000;f*=1.015){energy+=10**(response(s.left,f)/10)*s.preamp*s.preamp;count++;}
  assert(Math.abs(10*Math.log10(energy/count))<.15,`${id} unmatched reference energy`);
  assert(s.preamp<=10**(6/20)&&s.preamp>=10**(-12/20));
  assert(Math.abs(response(s.left,23000))<.1);
 }
});
test('invalid input rejected before synthesis',()=>{const invalid=Buffer.alloc(8);invalid.writeFloatLE(NaN,0);assert.throws(()=>simulate(null,null,invalid,dt));});
