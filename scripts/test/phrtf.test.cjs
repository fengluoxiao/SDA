const assert=require("node:assert/strict"),fs=require("node:fs"),path=require("node:path"),os=require("node:os");
const {pathToFileURL}=require("node:url");
const {buildSync}=require("esbuild");
const {decodeSofa,delayed,importSofa,importInWorker,PERSONAL_SET}=require("../../apps/desktop/personal-hrtf.cjs");
async function main(){
  const temp=fs.mkdtempSync(path.join(os.tmpdir(),"sda-phrtf-test-"));
  try{
    const target=path.join(temp,"phrtf.cjs");buildSync({entryPoints:["apps/web/src/phrtf.ts"],outfile:target,bundle:true,platform:"node",format:"cjs"});
    const p=require(target);
    for(const trial of p.makeTrials(["h3"],"validation")) {
      const start=p.testVisualPosition(trial,0),end=p.testVisualPosition(trial,1);
      assert.deepEqual(p.testVisualPosition(trial,.1),start);
      assert.deepEqual(p.testVisualPosition(trial,.9),end);
      for(let i=0;i<=100;i++) assert(Math.abs(Math.hypot(...p.testVisualPosition(trial,i/100))-1)<1e-10);
      const points=p.motionWaypoints(trial);
      points.forEach((point,i)=>{const actual=p.testVisualPosition(trial,.1+.8*i/(points.length-1));
        const az=point.az*Math.PI/180,el=point.el*Math.PI/180;
        const expected=[-Math.sin(az)*Math.cos(el),Math.sin(el),-Math.cos(az)*Math.cos(el)];
        actual.forEach((v,j)=>assert(Math.abs(v-expected[j])<1e-10));});
    }
    assert(p.testVisualPosition({kind:"location",direction:0},0)[0]<0);
    const layout=[{name:'L',azimuth:31.25,elevation:0,distance:1},{name:'R',azimuth:-31.25,elevation:0,distance:1},{name:'LFE',azimuth:45,elevation:0,distance:1,isLfe:true}];
    const positions=p.layoutPositions(layout);assert.equal(positions.length,2);
    const locationTrials=p.speakerTrials(positions);assert.equal(locationTrials[0].position.az,31.25);
    const accepted=locationTrials.map(t=>({...t,response:true,error:0})),field=p.confirmedField(accepted);
    const paths=p.layoutMotionTrials(positions,field);assert.equal(paths.length,2);assert.equal(p.motionWaypoints(paths[0])[0].az,31.25);
    const modelProfile={version:6,method:'per-speaker-audibility',subject:'generated',parameters:field,confirmations:accepted,answers:accepted,createdAt:new Date().toISOString(),previousHead:'ku100',gainDb:-30,output:'system-default'};
    global.localStorage={getItem:()=>JSON.stringify(modelProfile)};assert.equal(p.readProfile().version,6);
    modelProfile.parameters.anchors[0].parameters=p.generateCandidate().parameters;assert.equal(p.readProfile(),null);
    const candidates=Array.from({length:100},()=>p.generateCandidate());assert.equal(new Set(candidates.map(c=>JSON.stringify(c.parameters))).size,100);
    const generated=p.adaptiveTrials(candidates[0]);assert.equal(generated.length,20);
    const retried=p.adaptiveTrials(candidates[1],generated[5]);assert.equal(p.challengeKey(retried[0]),p.challengeKey(generated[5]));
    assert.equal(new Set(retried.map(p.challengeKey)).size,20);
    let record={version:5,method:'adaptive-audibility',subject:'generated',parameters:candidates[1].parameters,dataset:'parametric',createdAt:new Date().toISOString(),previousHead:'ku100',baseline:'ku100',output:'system-default',gainDb:-30,
      answers:[{...generated[5],response:false,error:1},...retried.map(t=>({...t,response:true,error:0}))]};
    global.localStorage={getItem:()=>JSON.stringify(record)};assert.equal(p.readProfile().version,5);
    record.answers[1].response=false;record.answers[1].error=1;assert.equal(p.readProfile(),null);
    assert.equal(p.angularError(0,0),0);assert(p.angularError(2,3)>89.9);
    assert.equal(p.makeTrials(p.SUBJECTS,"screen").length,38);
    const screen=p.makeTrials(["h3"],"screen"),validation=p.makeTrials(["h3"],"validation");
    assert(screen.every(t=>t.kind==='location'));assert(validation.every(t=>t.kind==='motion'));
    assert.equal(validation.length,8);
    assert(validation.every(t=>t.direction!==t.motionEnd));
    assert.equal(new Set(validation.map(t=>`${t.direction}:${t.motionEnd}`)).size,8);
    assert(validation.some(t=>p.DIRECTIONS[t.direction].el<p.DIRECTIONS[t.motionEnd].el));
    assert(validation.some(t=>p.DIRECTIONS[t.direction].el>p.DIRECTIONS[t.motionEnd].el));
    for(const t of validation){
      const points=p.motionWaypoints(t),start=p.DIRECTIONS[t.direction],end=p.DIRECTIONS[t.motionEnd];
      assert.deepEqual(points[0],{az:start.az,el:start.el});assert.deepEqual(points.at(-1),{az:end.az,el:end.el});
      for(const subject of p.SUBJECTS){
        const manifest=JSON.parse(fs.readFileSync(path.resolve('apps/web/public',subject==='ku100'?'hrtf':`hrtf-${subject}`,'hrtf-set.json')));
        assert(points.every(point=>manifest.positions.some(m=>m.azimuth===point.az&&m.elevation===point.el)));
      }
      const weights=points.map((_,i)=>p.motionWeights(i,points.length));
      for(let k=0;k<129;k++)assert(Math.abs(weights.reduce((sum,c)=>sum+c[k],0)-1)<1e-6);
    }
    for(let run=0;run<100;run++){
      const generated=p.makeTrials(p.SUBJECTS,'screen');
      assert.equal(new Set(generated.map(t=>t.direction)).size,p.DIRECTIONS.length);
      assert.equal(new Set(generated.filter(p.expectedAnswer).map(t=>t.probe)).size,p.DIRECTIONS.length);
      for(const subject of p.SUBJECTS){const pair=generated.filter(t=>t.subject===subject);assert.equal(pair.length,2);assert.notEqual(pair[0].probe,pair[1].probe);assert(pair.every(t=>t.probe===t.direction));}
      assert(generated.slice(1).every((t,i)=>t.probe!==generated[i].probe),'adjacent repeated prompt');
    }
    assert.equal(new Set(p.makeTrials(p.SUBJECTS,"screen").map(t=>`${t.subject}:${t.direction}`)).size,38);
    const legacyTrials=[4,5,6,7,8,9].map(direction=>({subject:'h3',direction,probe:direction,phase:'validation'}));
    const answers=legacyTrials.map(t=>({...t,response:t.direction,error:0}));
    assert.equal(p.rankAnswers([...answers,...legacyTrials.map(t=>({...t,subject:"h4",response:0,error:p.angularError(t.direction,0)}))],"validation")[0].subject,"h3");
    let saved=JSON.stringify({version:1,method:"perceptual-database-match",subject:"h3",answers,createdAt:new Date().toISOString(),baseline:"ku100",previousHead:"ku100",output:"system-default",gainDb:-30});
    global.localStorage={getItem:()=>saved};assert.equal(p.readProfile().subject,"h3");saved=saved.replace('"error":0','"error":null');assert.equal(p.readProfile(),null);
    for(const phase of ['screen','validation']){
      const trials=p.makeTrials(p.SUBJECTS,phase);
      for(const subject of p.SUBJECTS){
        const subset=trials.filter(t=>t.subject===subject);
        assert(subset.every(t=>t.assessment==='self-report'&&t.probe===t.direction));
        for(const response of [true,false])assert.equal(subset.reduce((sum,t)=>sum+p.answerError(t,response),0)/subset.length,response?0:1);
      }
    }
    const binary=legacyTrials.map(t=>({...t,response:t.direction===t.probe,error:0}));
    saved=JSON.stringify({version:2,method:'yes-no-location-match',subject:'h3',answers:binary,createdAt:new Date().toISOString(),baseline:'ku100',previousHead:'ku100',output:'system-default',gainDb:-30});
    assert.equal(p.readProfile().version,2);
    const motion=validation.map(t=>({...t,response:p.expectedAnswer(t),error:0}));
    const legacyMotion=validation.map(({assessment,...t})=>({...t,response:p.expectedAnswer(t),error:0}));
    saved=JSON.stringify({...JSON.parse(saved),version:3,method:'location-motion-match',answers:legacyMotion});assert.equal(p.readProfile().version,3);
    saved=JSON.stringify({...JSON.parse(saved),version:4,method:'subjective-position-path-match',answers:motion});assert.equal(p.readProfile().version,4);
    const corrupted=JSON.parse(saved);corrupted.answers[0].probe=(corrupted.answers[0].direction+1)%10;saved=JSON.stringify(corrupted);assert.equal(p.readProfile(),null);
    assert.equal(p.rankAnswers([...motion,...validation.map(t=>({...t,subject:'h4',response:false,error:p.answerError(t,false)}))],'validation')[0].subject,'h3');
    const curves=Array.from({length:5},(_,i)=>p.motionWeights(i));
    for(let k=0;k<129;k++)assert(Math.abs(curves.reduce((sum,c)=>sum+c[k],0)-1)<1e-6);
    assert.equal(curves[0][0],1);assert.equal(curves[4][128],1);
    assert.deepEqual(p.motionWaypoints({direction:4,motionEnd:5}).map(p=>p.az),[60,30,0,-30,-60]);
    assert(p.motionWaypoints({direction:4,motionEnd:4}).every(p=>p.az===60));
    const impulse=Float32Array.from([1,0,0,0]);assert.equal(delayed(impulse,3,40)[19],1);
    const fractional=delayed(impulse,.5,40);assert(Math.abs(fractional.reduce((a,b)=>a+b,0)-1)<1e-6);
    assert(Math.abs(fractional.reduce((a,b,i)=>a+b*i,0)-16.5)<.01);

    const h5=await import(pathToFileURL(path.resolve("apps/desktop/node_modules/h5wasm/dist/node/hdf5_hl.js")));await h5.ready;
    const source=path.join(temp,"fixture.sofa"),f=new h5.File(source,"w");
    f.create_attribute("SOFAConventions","SimpleFreeFieldHRIR");f.create_attribute("ListenerShortName","Synthetic fixture");
    const ds=(name,data,shape,attrs={})=>{f.create_dataset({name,data,shape,dtype:"<d"});const d=f.get(name);for(const [k,v]of Object.entries(attrs))d.create_attribute(k,v);return d;};
    ds("ListenerPosition",[0,0,0],[1,3],{Type:"cartesian",Units:"metre"});ds("ListenerView",[1,0,0],[1,3],{Type:"cartesian",Units:"metre"});ds("ListenerUp",[0,0,1],[1,3]);
    ds("ReceiverPosition",[0,.09,0,0,-.09,0],[2,3,1],{Type:"cartesian",Units:"metre"});
    ds("Data.SamplingRate",[48000],[1],{Units:"hertz"});
    ds("SourcePosition",Array.from({length:8},(_,i)=>[i*45,0,1.2]).flat(),[8,3],{Type:"spherical",Units:"degree, degree, metre"});
    const raw=new Float64Array(8*2*32);for(let i=0;i<8;i++){raw[i*64]=1;raw[i*64+32]=.5;}
    ds("Data.IR",raw,[8,2,32]);ds("Data.Delay",[2,5],[1,2]);
    assert.equal(decodeSofa(f).directions[1].azimuth,45);
    f.close();
    const imported=await importInWorker(source,path.join(temp,"store"));assert(PERSONAL_SET.test(`hrtf-${imported.id}`));
    const root=path.join(temp,"store",`hrtf-${imported.id}`),m=JSON.parse(fs.readFileSync(path.join(root,"hrtf-set.json")));
    const bytes=fs.readFileSync(path.join(root,m.positions[0].dry));const packed=new Float32Array(bytes.buffer,bytes.byteOffset,bytes.byteLength/4),n=packed.length/2;
    assert.equal(packed[18],1);assert.equal(packed[n+21],.5);assert.equal(m.positions[0].dry,m.positions[0].wet);
    assert.equal(m.processing.normalization,false);assert.equal((await importSofa(source,path.join(temp,"store"))).id,imported.id);
    const wrong={attrs:{SOFAConventions:{value:"GeneralFIR"}}};assert.throws(()=>decodeSofa(wrong),/SimpleFreeFieldHRIR/);
    const real=path.resolve("tmp/h3-src/H3/H3_HRIR_SOFA/H3_48K_24bit_256tap_FIR_SOFA.sofa");
    if(fs.existsSync(real)){const r=new h5.File(real,"r");assert.equal(decodeSofa(r).directions.length,2818);r.close();}
    console.log("pHRTF: disjoint trials, angular ranking, invalid storage, integer/fractional delay, SOFA worker round-trip and idempotent import passed (no audio output).");
  }finally{fs.rmSync(temp,{recursive:true,force:true});}
}
main().catch(e=>{console.error(e);process.exitCode=1;});
