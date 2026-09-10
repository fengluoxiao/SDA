// Cross-runtime dry response parity: the audition plan versus native bus/object PCM.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const {buildSync}=require('esbuild'),{spawnSync}=require('node:child_process');
(async()=>{
 const root=fs.mkdtempSync(path.resolve('tmp/phrtf-routing-'));
 try{
  buildSync({stdin:{contents:'export * from "./apps/web/src/phrtf";export {LAYOUTS} from "./packages/renderer/src/layouts";',resolveDir:process.cwd()},bundle:true,platform:'node',format:'cjs',outfile:path.join(root,'test.cjs')});
  const p=require(path.join(root,'test.cjs')),{synthesizeHrir}=await import('../../apps/desktop/parametric-hrtf.mjs');
  const {generate}=require('../../apps/desktop/generated-hrtf.cjs');const cases=[];
  for(const layout of ['2.0','5.1','7.1.4','9.1.4','9.1.6']){
   const positions=p.layoutPositions(p.LAYOUTS[layout]);
   const field={version:2,power:2,anchors:positions.map((a,i)=>({...a,parameters:{version:1,itd:i%2?'woodworth':'low-frequency',radius:.075+i*.0015,notchHz:6700+i*180,notchDb:4+i*.3}}))};
   const saved=await generate(field,{test:true},root),manifest=path.join(root,'hrtf-'+saved.id,'hrtf-set.json');
   for(const [az,el] of [[-140,0],[-130,0],[-120,0],[-100,0],[0,0],[40,0],[-135,45],[-115,30]]){
    const position={name:'probe',label:'probe',az,el};
    const trial={parameters:field,kind:'motion',position,endPosition:position};
    const plan=p.personalMotionPlan(trial),gains=plan.curves.map(c=>c[0]);
    const expected=new Array(1024).fill(0);
    plan.speakers.forEach((a,j)=>{const ir=synthesizeHrir(a.az,a.el,field);ir.forEach((v,k)=>expected[k]+=v*gains[j]);});
    if(layout==='7.1.4'&&az===-140&&el===0){assert(Math.abs(gains[positions.findIndex(s=>s.name==='RearRight')]-1)<1e-6);}
    cases.push({layout,manifest,az,el,gains,expected});
   }
   for(const trial of p.layoutMotionTrials(positions,field)){
    const plan=p.personalMotionPlan(trial);
    for(let i=0;i<451;i++)assert(Math.abs(plan.curves.reduce((s,c)=>s+c[i]**2,0)-1)<1e-5);
   }
  }
  const fixture=path.join(root,'fixtures.json');fs.writeFileSync(fixture,JSON.stringify(cases));
  const result=spawnSync('cargo',['test','--manifest-path','apps/native-renderer/Cargo.toml','--locked','--offline','personal_audition_matches_native_pcm','--','--ignored','--nocapture'],{env:{...process.env,SDA_PHRTF_ROUTING_FIXTURE:fixture},encoding:'utf8',maxBuffer:4*1024*1024});
  process.stdout.write(result.stdout||'');if(result.status!==0)process.stderr.write(result.stderr||'');assert.equal(result.status,0);
  console.log('40 directions across 5 layouts: audition/native dry bus and object response parity passed. No audio device used.');
 }finally{fs.rmSync(root,{recursive:true,force:true});}
})().catch(e=>{console.error(e);process.exitCode=1});
