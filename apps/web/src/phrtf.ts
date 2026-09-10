import type {VirtualSpeaker} from "@sda/renderer";
import {VbapSolver} from "../../../packages/renderer/src/vbap";
import {speakerLabel} from "./speaker-labels";
import {synthesizeHrir,generateParameters,parameterKey,validParameters,type PhrtfParameters,type DirectionalHrtfParameters} from "../../desktop/parametric-hrtf.mjs";
export type {PhrtfParameters,DirectionalHrtfParameters};
export interface TestPosition {name:string;label:string;az:number;el:number}
export const trialStart=(t:Trial)=>t.position??DIRECTIONS[t.direction]!;
export const trialEnd=(t:Trial)=>t.endPosition??DIRECTIONS[t.motionEnd??t.direction]!;
/** Perceptual database matching, not a measurement of the listener's own HRTF. */
export const PHRTF_KEY = "sda-phrtf-profile-v1";
export const SUBJECTS = ["ku100", ...Array.from({length:18}, (_,i)=>`h${i+3}`)];
export const DIRECTIONS = [
  {az:30,el:0,label:"左前方"},
  {az:-140,el:0,label:"右后方"},
  {az:90,el:45,label:"左上方"},
  {az:-90,el:45,label:"右上方"},
  {az:60,el:0,label:"左前侧"},
  {az:-60,el:0,label:"右前侧"},
  {az:110,el:0,label:"左后侧"},
  {az:-110,el:0,label:"右后侧"},
  {az:45,el:45,label:"左前上方"},
  {az:-45,el:45,label:"右前上方"},
  {az:135,el:45,label:"左后上方"},
  {az:-135,el:45,label:"右后上方"},
];
// Paths only use directions present in every bundled subject's measurement grid.
export const MOTION_PATHS = [
  {direction:4,motionEnd:5,points:[[60,0],[30,0],[0,0],[-30,0],[-60,0]]},
  {direction:5,motionEnd:4,points:[[-60,0],[-30,0],[0,0],[30,0],[60,0]]},
  {direction:0,motionEnd:6,points:[[30,0],[60,0],[100,0],[110,0]]},
  {direction:7,motionEnd:5,points:[[-110,0],[-100,0],[-60,0]]},
  {direction:4,motionEnd:8,points:[[60,0],[45,45]]},
  {direction:9,motionEnd:5,points:[[-45,45],[-60,0]]},
  {direction:8,motionEnd:10,points:[[45,45],[90,45],[135,45]]},
  {direction:11,motionEnd:9,points:[[-135,45],[-90,45],[-45,45]]},
] as const;
// SDA/ITU uses positive azimuth to the listener's LEFT.
export interface Trial {position?:TestPosition;endPosition?:TestPosition;parameters?:PhrtfParameters;subject:string; direction:number; probe:number; phase:"screen"|"validation"; kind?:"location"|"motion"; motionEnd?:number; motionPathVersion?:2; assessment?:"self-report"}
export interface Answer extends Trial {response:boolean; error:number; interpolationPower?:number}
interface LegacyAnswer {subject:string;direction:number;phase:"screen"|"validation";response:number;error:number}
export interface PersonalProfile {
  version:1|2|3|4|5|6; confirmations?:Answer[]; parameters?:PhrtfParameters; method:"per-speaker-audibility"|"adaptive-audibility"|"perceptual-database-match"|"yes-no-location-match"|"location-motion-match"|"subjective-position-path-match"; subject:string; createdAt:string;
  dataset:"SADIE II"|"parametric"; answers:Answer[]|LegacyAnswer[]; output:"system-default";
  baseline?:string; previousHead:string; gainDb:number; generationCount?:number; responsesTotal?:number; historyTruncated?:boolean; renderMethod?:"speaker-vbap-v1";
}
export function layoutPositions(layout:readonly VirtualSpeaker[]):TestPosition[]{
 return layout.filter(s=>!s.isLfe&&!s.binauralOnly).map(s=>({name:s.name,label:speakerLabel(s.name),az:((s.azimuth+180)%360+360)%360-180,el:s.elevation}));
}
export function speakerTrials(positions:TestPosition[]):Trial[]{return positions.map((position,direction)=>({subject:"generated",parameters:generateParameters(),position,direction,probe:direction,phase:"screen",kind:"location",assessment:"self-report"}));}
export function confirmedField(confirmed:Answer[],power=2):DirectionalHrtfParameters {
 return {version:2,power,anchors:confirmed.filter(a=>a.kind==="location"&&a.response&&a.position&&a.parameters?.version===1).map(a=>({name:a.position!.name,az:a.position!.az,el:a.position!.el,parameters:a.parameters as import("../../desktop/parametric-hrtf.mjs").SingleHrtfParameters}))};
}
export function layoutMotionTrials(positions:TestPosition[],parameters:DirectionalHrtfParameters):Trial[]{
 if(positions.length<2)return [];
 return positions.map((position,i)=>({subject:"generated",parameters,position,endPosition:positions[(i+1)%positions.length]!,direction:i,probe:i,motionEnd:(i+1)%positions.length,phase:"validation",kind:"motion",assessment:"self-report"}));
}
export function challengeKey(t:Trial){return t.kind==="motion"?`m:${t.direction}:${t.motionEnd}`:`l:${t.direction}`;}
export function generateCandidate(){return {subject:"generated",parameters:generateParameters()};}
export function adaptiveTrials(candidate:{subject:string;parameters:PhrtfParameters},first?:Trial):Trial[]{
 const positions=shuffled(DIRECTIONS.map((_,direction)=>({...candidate,direction,probe:direction,kind:"location" as const,phase:"screen" as const,assessment:"self-report" as const})));
 const motion=makeTrials([candidate.subject],"validation").map(t=>({...t,parameters:candidate.parameters}));
 const all=[...positions,...motion];
 return first?[{...first,...candidate},...all.filter(t=>challengeKey(t)!==challengeKey(first))]:all;
}
export function angularError(a:number,b:number):number {
  const x=DIRECTIONS[a], y=DIRECTIONS[b];
  if(!x||!y)throw new Error("无效测试方向");
  const r=Math.PI/180;
  const dot=Math.sin(x.el*r)*Math.sin(y.el*r)+Math.cos(x.el*r)*Math.cos(y.el*r)*Math.cos((x.az-y.az)*r);
  return Math.acos(Math.max(-1,Math.min(1,dot)))/r;
}
export function shuffled<T>(items:T[],random= Math.random):T[] {
  const out=[...items];
  for(let i=out.length-1;i>0;i--){const j=Math.floor(random()*(i+1));[out[i],out[j]]=[out[j]!,out[i]!];}
  return out;
}
export function makeTrials(subjects:string[],phase:Trial["phase"]):Trial[] {
  if(phase==="validation") {
    return shuffled(subjects.flatMap(subject=>MOTION_PATHS.map(({direction,motionEnd})=>({subject,direction,motionEnd,probe:direction,
      motionPathVersion:2 as const,kind:"motion" as const,assessment:"self-report" as const,phase}))));
  }
  // Spread the short screen across the whole measured direction pool. Each
  // question reports the actual rendered position, never a distractor.
  const pool=shuffled(DIRECTIONS.map((_,i)=>i));
  const pending=shuffled(subjects.flatMap((subject,i)=>{
    const matched=pool[i%pool.length]!,other=pool[(i+1)%pool.length]!;
    return [{subject,direction:matched,probe:matched,kind:"location" as const,assessment:"self-report" as const,phase},
      {subject,direction:other,probe:other,kind:"location" as const,assessment:"self-report" as const,phase}];
  }));
  const trials:Trial[]=[];
  while(pending.length){
    const previous=trials.at(-1);
    const counts=new Map<number,number>();for(const t of pending)counts.set(t.probe,(counts.get(t.probe)??0)+1);
    // Consume frequent prompts first so they cannot accumulate at the end.
    let index=-1,best=-1;
    pending.forEach((t,i)=>{if(t.probe===previous?.probe)return;
      const score=counts.get(t.probe)!*2+Number(t.direction!==previous?.direction);
      if(score>best){best=score;index=i;}
    });
    trials.push(pending.splice(Math.max(0,index),1)[0]!);
  }
  return trials;
}
export function expectedAnswer(trial:Trial):boolean {return trial.assessment==="self-report"?true:trial.kind==="motion"?trial.direction!==trial.motionEnd:trial.direction===trial.probe;}
export function answerError(trial:Trial,response:boolean):number {return response===expectedAnswer(trial)?0:1;}
/** Versioned routes retain the exact measured waypoints used by each assessment. */
export function motionWaypoints(trial:Trial) {
  if(trial.position&&trial.endPosition){
    const a=trial.position,b=trial.endPosition,delta=((b.az-a.az+540)%360)-180;
    return Array.from({length:9},(_,i)=>({az:a.az+delta*i/8,el:a.el+(b.el-a.el)*i/8}));
  }
  if(trial.motionPathVersion===2){
    const path=MOTION_PATHS.find(p=>p.direction===trial.direction&&p.motionEnd===trial.motionEnd);
    if(!path)throw new Error("未知移动路径");
    return path.points.map(([az,el])=>({az,el}));
  }
  const start=DIRECTIONS[trial.direction],end=DIRECTIONS[trial.motionEnd??trial.direction];
  if(!start||!end)throw new Error("运动端点无效");
  return Array.from({length:5},(_,i)=>({az:start.az+(end.az-start.az)*i/4,el:start.el+(end.el-start.el)*i/4}));
}
export function motionWeights(index:number,points=5,samples=129):Float32Array {
  return Float32Array.from({length:samples},(_,i)=>{
    const progress=Math.max(0,Math.min(1,(i/(samples-1)-.1)/.8))*(points-1);
    return Math.max(0,1-Math.abs(progress-index));
  });
}

/** The same speaker routing used by program playback, with fixed accepted HRIRs. */
export function personalMotionPlan(trial:Trial,samples=451) {
  const field=trial.parameters;
  if(field?.version!==2||trial.kind!=="motion")throw new Error("移动测试缺少已确认的音箱布局");
  const layout=field.anchors.map(a=>({name:a.name,azimuth:a.az,elevation:a.el,distance:1}));
  const solver=new VbapSolver(layout),curves=layout.map(()=>new Float32Array(samples));
  for(let i=0;i<samples;i++){
    const [x,y,z]=testVisualPosition(trial,i/(samples-1));
    const gains=solver.pan({azimuth:-Math.atan2(x,-z)*180/Math.PI,elevation:Math.asin(Math.max(-1,Math.min(1,y)))*180/Math.PI,distance:1},0);
    gains.forEach((g,bus)=>{curves[bus]![i]=g;});
  }
  return {speakers:field.anchors,curves};
}
export function rankAnswers(answers:Answer[],phase:Trial["phase"]) {
  const groups=new Map<string,number[]>();
  for(const a of answers.filter(a=>a.phase===phase))groups.set(a.subject,[...(groups.get(a.subject)??[]),a.error]);
  return [...groups].map(([subject,errors])=>({subject,count:errors.length,mean:errors.reduce((a,b)=>a+b,0)/errors.length}))
    .sort((a,b)=>a.mean-b.mean||a.subject.localeCompare(b.subject));
}
export function readProfile():PersonalProfile|null {
  try {
    const p=JSON.parse(localStorage.getItem(PHRTF_KEY)??"null");
    if(p?.version===6){
      if(p.method!=="per-speaker-audibility"||p.subject!=="generated"||!validParameters(p.parameters)||p.parameters.version!==2
        ||!Array.isArray(p.confirmations)||p.confirmations.length>128||!Array.isArray(p.answers)||p.answers.length>1000
        ||typeof p.createdAt!=="string"||typeof p.previousHead!=="string"||!Number.isFinite(p.gainDb)||p.output!=="system-default")return null;
      const locations=p.confirmations.filter((a:Answer)=>a.kind==="location");
      if(locations.length!==p.parameters.anchors.length||p.confirmations.some((a:Answer)=>a.response!==true||a.error!==0))return null;
      for(const a of p.parameters.anchors){const matches=locations.filter((t:Answer)=>t.position?.name===a.name);
        if(matches.length!==1||matches[0].position.az!==a.az||matches[0].position.el!==a.el||parameterKey(matches[0].parameters)!==parameterKey(a.parameters))return null;}
      return p;
    }
    if(p?.version===5){
      if(p.method!=="adaptive-audibility"||p.subject!=="generated"||!validParameters(p.parameters)||!Array.isArray(p.answers)||p.answers.length>1000
        ||typeof p.createdAt!=="string"||typeof p.previousHead!=="string"||p.output!=="system-default"||!Number.isFinite(p.gainDb))return null;
      if(p.answers.some((a:Answer)=>a.subject!=="generated"||!validParameters(a.parameters)||typeof a.response!=="boolean"||a.error!==(a.response?0:1)
        ||a.assessment!=="self-report"||a.probe!==a.direction||!Number.isInteger(a.direction)||!DIRECTIONS[a.direction]
        ||(a.kind!=="location"&&a.kind!=="motion")||(a.kind==="location"&&a.phase!=="screen")
        ||(a.kind==="motion"&&(a.phase!=="validation"||a.motionPathVersion!==2||!MOTION_PATHS.some(m=>m.direction===a.direction&&m.motionEnd===a.motionEnd)))))return null;
      const final=p.answers.slice(-20) as Answer[];
      if(final.length!==20||final.some(a=>!a.response||parameterKey(a.parameters!)!==parameterKey(p.parameters))||new Set(final.map(challengeKey)).size!==20)return null;
      return p;
    }
    if(!((p?.version===1&&p.method==="perceptual-database-match")||(p?.version===2&&p.method==="yes-no-location-match")||(p?.version===3&&p.method==="location-motion-match")||(p?.version===4&&p.method==="subjective-position-path-match"))||!SUBJECTS.includes(p.subject)||!Array.isArray(p.answers)
      ||p.answers.length>1000||typeof p.createdAt!=="string"||!SUBJECTS.includes(p.baseline)||typeof p.previousHead!=="string"
      ||p.output!=="system-default"||!Number.isFinite(p.gainDb))return null;
    if(p.answers.some((a:Answer|LegacyAnswer)=>!SUBJECTS.includes(a.subject)||!["screen","validation"].includes(a.phase)
      ||!Number.isInteger(a.direction)||!DIRECTIONS[a.direction]||!Number.isFinite(a.error)))return null;
    if(p.version===1){
      if(p.answers.some((a:LegacyAnswer)=>!Number.isInteger(a.response)||Math.abs(angularError(a.direction,a.response)-a.error)>1e-6))return null;
    }else if(p.answers.some((a:Answer)=>typeof a.response!=="boolean"||!Number.isInteger(a.probe)||!DIRECTIONS[a.probe]
      ||(p.version===2&&a.kind==="motion")||(p.version>=3&&(a.kind!=="location"&&a.kind!=="motion"))
      ||(p.version<4&&a.assessment!==undefined)||(p.version>=4&&(a.assessment!=="self-report"||a.probe!==a.direction))
      ||(a.motionPathVersion!==undefined&&(a.motionPathVersion!==2||a.kind!=="motion"||!MOTION_PATHS.some(path=>path.direction===a.direction&&path.motionEnd===a.motionEnd)))
      ||(a.kind==="motion"&&(!Number.isInteger(a.motionEnd)||!DIRECTIONS[a.motionEnd!]))||a.error!==answerError(a,a.response)))return null;
    if(p.answers.filter((a:Answer|LegacyAnswer)=>a.subject===p.subject&&a.phase==="validation").length!==(p.version>=3?8:6))return null;
    return p;
  }catch{return null;}
}

async function asset(path:string):Promise<ArrayBuffer> {
  if(window.sdaDesktop?.readBundledHrtf){const b=await window.sdaDesktop.readBundledHrtf(path);return new Uint8Array(b).buffer;}
  const response=await fetch(new URL(path,document.baseURI));
  if(!response.ok)throw new Error(`HRTF 资源读取失败 (${response.status})`);
  return response.arrayBuffer();
}
interface Measurement {azimuth:number;elevation:number;dry:string}
export interface HrtfTestVisual { trial:Trial; elapsed:()=>number; duration:number }
/** Same waypoints and initial/final holds as the audio gain curves. Three.js coordinates. */
export function testVisualPosition(trial:Trial,progress:number):[number,number,number] {
 const points=trial.kind==="motion"?motionWaypoints(trial):[trialStart(trial)];
 const t=Math.max(0,Math.min(1,(progress-.1)/.8))*(points.length-1);
 const a=points[Math.floor(t)]!,b=points[Math.min(points.length-1,Math.floor(t)+1)]!,f=t-Math.floor(t);
 const az=(a.az+(b.az-a.az)*f)*Math.PI/180,el=(a.el+(b.el-a.el)*f)*Math.PI/180;
 return [-Math.sin(az)*Math.cos(el),Math.sin(el),-Math.cos(az)*Math.cos(el)];
}
/** Separate, dry test path: never changes the player's room, EQ, volume or output settings. */
export class PersonalHrtfAudition {
  private context:AudioContext|null=null;
  private source:AudioBufferSourceNode|null=null;
  private generation=0;
  private cache=new Map<string,AudioBuffer>();
  private subjects=new Map<string,{positions:Measurement[];sampleRate:number}>();
  private gains=new Map<string,number>();
  private generatedKey="";
  private visual:((value:HrtfTestVisual|null)=>void)|undefined;
  stop(){this.visual?.(null);this.visual=undefined;this.generation++;try{this.source?.stop();}catch{}this.source=null;}
  dispose(){this.stop();const c=this.context;this.context=null;void c?.close();this.cache.clear();}
  async play(trial:Trial,gainDb:number,onVisual?:(value:HrtfTestVisual|null)=>void):Promise<void> {
    this.stop();const generation=this.generation;
    const context=this.context??=new AudioContext({sampleRate:48000});
    await context.resume();
    const d=trialStart(trial);if(!d)throw new Error("方向无效");
    const motion=trial.kind==="motion",positions=motion?motionWaypoints(trial):[d];
    let filters:AudioBuffer[],normalization=1;
    const speakerMotion=motion&&trial.parameters?.version===2?personalMotionPlan(trial):null;
    if(trial.parameters){
      const generatedKey=parameterKey(trial.parameters);
      if(generatedKey!==this.generatedKey){this.cache.clear();this.generatedKey=generatedKey;}
      const make=(az:number,el:number)=>{const key=`generated:${parameterKey(trial.parameters!)}:${az}:${el}`;
        let b=this.cache.get(key);if(!b){const packed=synthesizeHrir(az,el,trial.parameters!);b=context.createBuffer(2,packed.length/2,48000);
          b.copyToChannel(packed.slice(0,packed.length/2),0);b.copyToChannel(packed.slice(packed.length/2),1);this.cache.set(key,b);}return b;};
      filters=(speakerMotion?speakerMotion.speakers:positions).map(p=>make(p.az,p.el));
    }else{
    const root=trial.subject==="ku100"?"hrtf":`hrtf-${trial.subject}`;
    let manifest=this.subjects.get(root);
    if(!manifest){manifest=JSON.parse(new TextDecoder().decode(await asset(`${root}/hrtf-set.json`)));
      if(!manifest||manifest.sampleRate!==48000||!Array.isArray(manifest.positions))throw new Error("HRTF 格式不支持");
      this.subjects.set(root,manifest);}
    const load=async(p:Measurement)=>{
      const assetKey=`${root}/${p.dry}`,key=assetKey;
      if(!/^hrtf(?:-h\d+)?\/azm?\d+_elm?\d+_dry\.f32$/.test(assetKey))throw new Error("HRTF 资源路径无效");
      let buffer=this.cache.get(key);
      if(!buffer){const bytes=await asset(assetKey);if(bytes.byteLength%8||bytes.byteLength<16||bytes.byteLength>65536)throw new Error("HRIR 长度无效");
        const data=new Float32Array(bytes);if(!data.every(Number.isFinite))throw new Error("HRIR 含无效数值");
        buffer=context.createBuffer(2,data.length/2,48000);
        buffer.copyToChannel(data.slice(0,data.length/2),0);buffer.copyToChannel(data.slice(data.length/2),1);
        this.cache.set(key,buffer);}
      return buffer;
    };
    // One bilateral, front-reference gain per subject; never normalize each ear/direction separately.
    const gainKey=root;
    if(!this.gains.has(gainKey)){
      const front=manifest.positions.find(p=>p.azimuth===0&&p.elevation===0);if(!front)throw new Error("缺少正前方参考测量");
      const b=await load(front);let energy=0;for(let c=0;c<2;c++)for(const v of b.getChannelData(c))energy+=v*v;
      if(!(energy>1e-12))throw new Error("参考响应无有效能量");
      this.gains.set(gainKey,1/Math.sqrt(energy/2));
    }
    filters=await Promise.all(positions.map(async direction=>{
      const p=manifest!.positions.find(p=>Math.abs(p.azimuth-direction.az)<1e-6&&Math.abs(p.elevation-direction.el)<1e-6);
      if(!p)throw new Error("该档案缺少测试路径的实测响应");return load(p);
    }));
    normalization=this.gains.get(gainKey)!;
    }
    if(generation!==this.generation||context.state==="closed")return;
    // Fixed seeded broadband bursts and ramps: same stimulus for every candidate.
    const duration=motion?2.4:1;
    const noise=context.createBuffer(1,Math.round(duration*48000),48000), data=noise.getChannelData(0);let seed=0x719abd;
    for(let i=0;i<data.length;i++){seed^=seed<<13;seed^=seed>>>17;seed^=seed<<5;
      const t=i/48000,local=t%.32,envelope=motion?Math.min(1,t/.04,(duration-t)/.04):local<.22?Math.min(1,local/.015,(.22-local)/.015):0;
      data[i]=((seed>>>0)/4294967296*2-1)*envelope;}
    const source=context.createBufferSource(),gain=context.createGain();
    source.buffer=noise;
    gain.gain.value=Math.pow(10,Math.max(-48,Math.min(-18,gainDb))/20)*normalization;
    const at=context.currentTime+.03;
    const nodes=filters.map((ir,index)=>{
      const convolver=context.createConvolver(),weight=context.createGain();convolver.normalize=false;convolver.buffer=ir;
      if(motion)weight.gain.setValueCurveAtTime(speakerMotion?.curves[index]??motionWeights(index,filters.length),at,duration);
      else weight.gain.value=1;
      // Route the mono source into physical buses before their HRIRs, as in playback.
      if(speakerMotion)source.connect(weight).connect(convolver).connect(gain);
      else source.connect(convolver).connect(weight).connect(gain);
      return {convolver,weight};
    });
    gain.connect(context.destination);this.source=source;this.visual=onVisual;
    await new Promise<void>(resolve=>{source.onended=()=>{source.disconnect();for(const n of nodes){n.convolver.disconnect();n.weight.disconnect();}gain.disconnect();if(this.source===source){this.source=null;this.visual?.(null);this.visual=undefined;}resolve();};source.start(at);onVisual?.({trial,duration,elapsed:()=>context.currentTime-at});});
  }
}
