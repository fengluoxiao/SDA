const fs=require("node:fs"),path=require("node:path"),http=require("node:http"),assert=require("node:assert/strict");
const {buildSync}=require("esbuild");
const {chromium}=require(process.env.SDA_PLAYWRIGHT||"C:/Users/fengluoxiao/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright");
async function main(){
  const root=path.resolve("tmp/phrtf-ui-test");fs.mkdirSync(root,{recursive:true});
  const entry=`import React,{useState} from 'react';import {createRoot} from 'react-dom/client';import Panel from './apps/web/src/components/PersonalHrtfPanel';import {PersonalHrtfAudition,expectedAnswer,DIRECTIONS,generateCandidate,trialStart,trialEnd} from './apps/web/src/phrtf';import {LAYOUTS} from './packages/renderer/src/layouts';import './apps/web/src/styles.css';import './apps/web/src/workbench.css';
  const originalPlay=PersonalHrtfAudition.prototype.play;PersonalHrtfAudition.prototype.play=function(trial,gain,onVisual){window.currentTrial=trial;window.expectedAnswer=expectedAnswer(trial);window.expectedPrompt=trial.kind==='motion'?(trial.motionEnd===trial.direction?trialStart(trial).label+' · 保持静止':trialStart(trial).label+' → '+trialEnd(trial).label):trialStart(trial).label;return originalPlay.call(this,trial,gain,onVisual)};
  window.offlineMotionQA=async()=>{
    const previous=window.AudioContext;let rendered;
    window.AudioContext=class {constructor(){const context=new OfflineAudioContext(2,144000,48000);context.resume=async()=>{};
      const create=context.createBufferSource.bind(context);context.createBufferSource=()=>{const source=create(),start=source.start.bind(source);source.start=(at)=>{start(at);rendered=context.startRendering()};return source};return context;}};
    try{const values=[];const parameters=generateCandidate().parameters;const field={version:2,power:2,anchors:[{name:'L',az:60,el:0,parameters},{name:'R',az:-60,el:0,parameters}]};for(const end of [4,5]){const engine=new PersonalHrtfAudition();await engine.play({subject:'generated',parameters:field,direction:4,probe:4,motionEnd:end,kind:'motion',phase:'validation'},-30);const buffer=await rendered;
      const ratio=(a,b)=>{let l=0,r=0;for(let i=a;i<b;i++){l+=buffer.getChannelData(0)[i]**2;r+=buffer.getChannelData(1)[i]**2}return 10*Math.log10(l/r)};
      values.push({early:ratio(4000,9000),late:ratio(107000,112000),finite:[0,1].every(c=>buffer.getChannelData(c).every(Number.isFinite))});}return values;
    }finally{window.AudioContext=previous;}
  };
  function App(){const [head,setHead]=useState('ku100');return <div className="app" style={{width:420,maxWidth:'100%',height:'auto',display:'block',margin:'20px auto',padding:16,overflow:'visible'}}><Panel layout={LAYOUTS["7.1.4"]} currentHead={head} playing={false} locked={false} onVisual={value=>{window.visualActive=!!value;window.visualStarts=(window.visualStarts||0)+Number(!!value)}} onApply={async (id,parameters,assessment)=>{window.applied=id;window.savedParameters=parameters;window.savedAssessment=assessment;setHead(id)}}/></div>};createRoot(document.getElementById('root')).render(<App/>);`;
  buildSync({stdin:{contents:entry,resolveDir:process.cwd(),loader:"tsx"},nodePaths:[path.resolve("apps/web/node_modules")],bundle:true,outfile:path.join(root,"app.js"),jsx:"automatic",define:{"process.env.NODE_ENV":'"production"'}});
  const server=http.createServer((req,res)=>{
    const url=new URL(req.url,"http://localhost"),name=decodeURIComponent(url.pathname).slice(1);
    if(!name){res.setHeader("Content-Type","text/html");res.end('<html data-theme="dark"><meta charset="utf-8"><link rel="stylesheet" href="/app.css"><div id="root"></div><script src="/app.js"></script></html>');return;}
    const file=path.resolve(name.startsWith("hrtf")?"apps/web/public":root,name);
    if(!file.startsWith(path.resolve("apps/web/public")+path.sep)&&!file.startsWith(root+path.sep)){res.writeHead(403).end();return;}
    try{res.setHeader("Content-Type",name.endsWith(".js")?"application/javascript":name.endsWith(".css")?"text/css":"application/octet-stream");res.end(fs.readFileSync(file));}catch{res.writeHead(404).end();}
  });
  await new Promise(r=>server.listen(0,"127.0.0.1",r));
  server.unref();
  const browser=await chromium.launch({headless:true,channel:"msedge",args:["--mute-audio"]});
  try{
    let assetRequests=0;
    const page=await browser.newPage({viewport:{width:900,height:1000}}),errors=[];page.on("pageerror",e=>errors.push(String(e)));
    page.on("request",r=>{if(/\/hrtf[^/]*\//.test(r.url()))assetRequests++});
    await page.addInitScript(()=>{
      // No device initialization or audible output. Real assets are still fetched and decoded.
      class FakeAudio {
        state='running';destination={};currentTime=0;
        resume(){return Promise.resolve()}close(){this.state='closed';return Promise.resolve()}
        createBuffer(channels,length,sampleRate){const data=Array.from({length:channels},()=>new Float32Array(length));return {length,sampleRate,copyToChannel:(v,c)=>data[c].set(v),getChannelData:c=>data[c]};}
        createConvolver(){return {connect(node){return node},disconnect(){}}}
        createGain(){return {gain:{value:0,setValueCurveAtTime(curve,at,duration){window.motionCurves=(window.motionCurves||0)+1;if(![129,451].includes(curve.length)||duration!==2.4)throw Error('wrong movement envelope')}},connect(node){return node},disconnect(){}}}
        createBufferSource(){return {connect(node){return node},disconnect(){},start(){setTimeout(()=>this.onended?.(),0)},stop(){this.onended?.()}}}
      }
      window.AudioContext=FakeAudio;
    });
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    const start=page.getByRole("button",{name:"开始感知测试"});assert(await start.isDisabled());
    await page.getByRole("checkbox").check();await start.click();
    assert.equal(await page.locator('.phrtf-directions button').count(),2);
    await page.screenshot({path:path.join(root,"dark.png")});
    await page.waitForFunction(()=>!document.querySelector('.phrtf-directions button').disabled);
    const before=await page.evaluate(()=>({prompt:window.expectedPrompt,parameters:window.currentTrial.parameters}));
    await page.getByRole('button',{name:'否',exact:true}).click();
    await page.waitForFunction(()=>!document.querySelector('.phrtf-directions button').disabled);
    const after=await page.evaluate(()=>({prompt:window.expectedPrompt,parameters:window.currentTrial.parameters}));
    assert.equal(after.prompt,before.prompt);assert.notDeepEqual(after.parameters,before.parameters);
    let count=1,secondRetried=false,motionRetried=false,secondParameters;
    while(await page.locator('.phrtf-directions').count()){
      await page.waitForFunction(()=>!document.querySelector('.phrtf-directions button').disabled);
      const current=await page.evaluate(()=>window.currentTrial);
      if(current.kind==='location'&&current.direction===1&&!secondRetried){
        secondRetried=true;await page.getByRole('button',{name:'否',exact:true}).click();count++;
        await page.waitForFunction(()=>!document.querySelector('.phrtf-directions button').disabled);
        const retried=await page.evaluate(()=>window.currentTrial);
        assert.equal(retried.position.name,current.position.name);assert.notDeepEqual(retried.parameters,current.parameters);secondParameters=retried.parameters;
      }
      if(current.kind==='motion'&&!motionRetried){
        motionRetried=true;await page.getByRole('button',{name:'否',exact:true}).click();count++;
        await page.waitForFunction(()=>!document.querySelector('.phrtf-directions button').disabled);
        const retried=await page.evaluate(()=>window.currentTrial);
        assert.deepEqual(retried.parameters.anchors,current.parameters.anchors);assert.equal(retried.parameters.power,current.parameters.power);
      }
      assert(await page.locator('.phrtf-question strong').isVisible());
      assert.equal(await page.locator('.phrtf-question strong').innerText(),await page.evaluate(()=>window.expectedPrompt));
      await page.getByRole('button',{name:await page.evaluate(()=>window.expectedAnswer)?'是':'否',exact:true}).click();
      if(++count>25)throw new Error("trial loop did not finish");
    }
    assert.equal(count,25);assert.equal(await page.evaluate(()=>window.visualStarts),25);assert.equal(await page.evaluate(()=>window.visualActive),false);assert.equal(assetRequests,0);
    await page.waitForFunction(()=>!!localStorage.getItem("sda-phrtf-profile-v1"));
    assert(await page.evaluate(()=>!!JSON.parse(localStorage.getItem('sda-phrtf-profile-v1')).answers.length));
    const saved=await page.evaluate(()=>JSON.parse(localStorage.getItem('sda-phrtf-profile-v1')));
    assert.equal(saved.version,6);assert.equal(saved.renderMethod,'speaker-vbap-v1');assert.equal(saved.parameters.anchors.length,11);
    assert.deepEqual(saved.parameters.anchors[0].parameters,after.parameters);assert.deepEqual(saved.parameters.anchors[1].parameters,secondParameters);
    assert.equal(saved.confirmations.length,11);
    await page.reload();await page.getByRole("button",{name:"应用档案",exact:true}).click();
    assert(await page.evaluate(()=>window.applied==='generated'&&window.savedParameters.version===2));
    await page.getByRole("button",{name:"恢复匹配前档案"}).click();
    await page.evaluate(()=>document.documentElement.dataset.theme='light');
    await page.setViewportSize({width:440,height:1000});await page.waitForTimeout(300);await page.screenshot({path:path.join(root,"light.png")});
    assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));assert.deepEqual(errors,[]);
    await page.getByRole("checkbox").check();await page.getByRole("button",{name:"开始感知测试"}).click();
    let rejected=0;const uniqueModels=new Set();
    while(rejected<64){
      await page.waitForFunction(()=>!document.querySelector('.phrtf-directions button').disabled);
      uniqueModels.add(await page.evaluate(()=>JSON.stringify(window.currentTrial.parameters)));
      await page.getByRole('button',{name:'否',exact:true}).click();rejected++;
    }
    await page.waitForFunction(()=>!document.querySelector('.phrtf-directions button').disabled);
    assert.equal(uniqueModels.size,64);assert(await page.locator('.phrtf-directions').isVisible());
    assert.equal(await page.getByText('暂未找到明显可感知的方案',{exact:true}).count(),0);
    assert.equal(await page.getByRole('button',{name:'重试保存 pHRTF'}).count(),0);
    const savedBefore=await page.evaluate(()=>localStorage.getItem('sda-phrtf-profile-v1'));
    await page.getByRole('button',{name:'结束测试',exact:true}).click();
    assert(await page.getByRole('button',{name:'开始感知测试',exact:true}).isVisible());
    assert.equal(await page.evaluate(()=>localStorage.getItem('sda-phrtf-profile-v1')),savedBefore);
    // The reported failure also happens with no retries: cover that complete flow.
    await page.getByRole('button',{name:'开始感知测试',exact:true}).click();
    let allYes=0;const exactAnchors=[];
    while(await page.locator('.phrtf-directions').count()){
      await page.waitForFunction(()=>!document.querySelector('.phrtf-directions button').disabled);
      const t=await page.evaluate(()=>window.currentTrial);
      if(t.kind==='location')exactAnchors.push({name:t.position.name,az:t.position.az,el:t.position.el,parameters:t.parameters});
      await page.getByRole('button',{name:'是',exact:true}).click();
      if(++allYes>22)throw Error('all-yes flow did not finish');
    }
    await page.waitForFunction(()=>JSON.parse(localStorage.getItem('sda-phrtf-profile-v1')).responsesTotal===22);
    const allYesSaved=await page.evaluate(()=>JSON.parse(localStorage.getItem('sda-phrtf-profile-v1')));
    assert.equal(allYes,22);assert(allYesSaved.answers.every(a=>a.response===true));
    assert.deepEqual(allYesSaved.parameters.anchors,exactAnchors);
    assert.equal(allYesSaved.renderMethod,'speaker-vbap-v1');
    const audio=await page.evaluate(()=>window.offlineMotionQA());
    assert(audio.every(a=>a.finite));assert(Math.abs(audio[0].early-audio[0].late)<1);
    assert(audio[1].early*audio[1].late<0,'motion must move between opposite-ear energy dominance');
    assert(Math.abs(audio[1].early-audio[1].late)>3,'motion must change the actual binaural signal');
    console.log(JSON.stringify({offlineMotion:audio}));
    console.log(JSON.stringify({trials:count,bundledHrtfReads:assetRequests,rejected,audibleOutput:false,persistence:true,apply:true,restore:true,lightDark:true}));
  }finally{await browser.close();server.close();}
}
main().catch(e=>{console.error(e);process.exitCode=1;});
