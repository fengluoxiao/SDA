const assert=require('node:assert/strict');
const {test}=require('node:test');
const {buildSync}=require('esbuild');
const THREE=require('three');

function load(file){
 const compiled=buildSync({entryPoints:[file],bundle:true,platform:'node',format:'cjs',write:false,external:['three']});
 const module={exports:{}};new Function('require','module','exports',compiled.outputFiles[0].text)(require,module,module.exports);
 return module.exports;
}
const coords=load('packages/renderer/src/coords.ts');
const scene=load('packages/renderer/src/scene-coordinates.ts');
const {SceneObjectMotion}=load('apps/mobile/src/scene-motion.ts');
const {CardboardPose}=load('apps/mobile/src/cardboard-pose.ts');
const unit=p=>new THREE.Vector3(...p).normalize();

test('display distance cap preserves elevation across coordinate scales',()=>{
 const origin=[.3,.7,.6],reference=coords.admToSpherical(origin);
 for(const scale of [.001,.5,1,4,8,1000,1e20]){
  const p=origin.map(v=>v*scale),actual=coords.admToSpherical(p);
  assert(Math.abs(actual.azimuth-reference.azimuth)<1e-9);
  assert(Math.abs(actual.elevation-reference.elevation)<1e-9,`distance cap altered elevation at scale=${scale}: ${actual.elevation}`);
  assert.equal(actual.distance,Math.min(4,Math.hypot(...p)));
 }
 assert(Math.abs(coords.admToSpherical([0,8,8]).elevation-45)<1e-10);
});

test('spherical and direct ADM scene mappings agree on direction outside the reference cube',()=>{
 for(const p of [[0,8,8],[4,8,-6],[-8,-4,6],[0,0,-8],[8,0,0],[-8,0,0],[0,-8,0]]){
  const direct=unit(scene.admToScenePosition(p));
  const spherical=unit(scene.sphericalToScenePosition(coords.admToSpherical(p)));
  assert(direct.distanceTo(spherical)<1e-10,`scene/audio direction diverged for ${p}`);
 }
});

function nativeSample(camera,angle,platform){
 const q=new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(1,0,0),Math.PI/2).multiply(camera)
  .multiply(new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0,0,1),platform==='ios'?-angle:angle));
 const e=new THREE.Euler().setFromQuaternion(q,'ZXY');return {alpha:e.z,beta:e.x,gamma:e.y};
}

test('small yaw changes preserve front/back signs and the shared audio/Three basis in VR',()=>{
 for(const platform of ['ios','android'])for(const angle of [-Math.PI/2,Math.PI/2]){
  for(const degrees of [-.5,-.1,-.01,.01,.1,.5]){
   const pose=new CardboardPose();pose.update(nativeSample(new THREE.Quaternion(),angle,platform),angle,0,platform);
   const yaw=degrees*Math.PI/180,target=new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0,1,0),yaw);
   for(let i=1;i<=20;i++)pose.update(nativeSample(target,angle,platform),angle,i*16,platform);
   assert(pose.value.angleTo(target)<1e-6);
   for(const p of [[0,1,0],[0,-1,0],[-1,0,0],[1,0,0],[.2,.7,.6],[0,0,-1]]){
    const visual=unit(scene.admToScenePosition(p)).applyQuaternion(pose.value.clone().invert());
    const local=[Math.cos(yaw)*p[0]+Math.sin(yaw)*p[1],-Math.sin(yaw)*p[0]+Math.cos(yaw)*p[1],p[2]];
    assert(visual.distanceTo(unit(scene.admToScenePosition(local)))<1e-6);
   }
  }
 }
});

test('moving scene objects use the shared axes and seek snaps to the current metadata point',()=>{
 const motion=new SceneObjectMotion(),output=new Float32Array(3);
 const point=(pos)=>[{id:7,pos,gainDb:0,hasPos:true}];
 motion.update(point([0,1,0]),1,0,false);
 motion.update(point([.2,.7,-.6]),1.125,.125,false);
 assert(motion.write(7,.25,output,0));
 const expected=scene.admToScenePosition([.2,.7,-.6]);
 for(let i=0;i<3;i++)assert(Math.abs(output[i]-expected[i])<1e-6);
 motion.update(point([-.5,-.7,.4]),.5,.26,false);
 motion.write(7,.26,output,0);
 const seek=scene.admToScenePosition([-.5,-.7,.4]);
 for(let i=0;i<3;i++)assert(Math.abs(output[i]-seek[i])<1e-6);
});
