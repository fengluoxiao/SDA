import type {VirtualSpeaker} from "@sda/renderer";
import {useEffect,useMemo,useRef,useState} from "react";
import {trialStart,trialEnd,layoutPositions,speakerTrials,confirmedField,layoutMotionTrials,PHRTF_KEY,PersonalHrtfAudition,answerError,generateCandidate,readProfile,
  type DirectionalHrtfParameters,type TestPosition,type PhrtfParameters,type HrtfTestVisual,type Answer,type PersonalProfile,type Trial} from "../phrtf";
import "./PersonalHrtfPanel.css";
import {PersonalHrtfLibrary} from "./PersonalHrtfLibrary";

export default function PersonalHrtfPanel({currentHead,playing,locked,onApply,onVisual,layout}:{
  layout:readonly VirtualSpeaker[];onVisual?:(value:HrtfTestVisual|null)=>void;currentHead:string;playing:boolean;locked:boolean;onApply:(subject:string,parameters?:PhrtfParameters,assessment?:unknown)=>Promise<void>;
}) {
  const [profile,setProfile]=useState(readProfile);
  const [trials,setTrials]=useState<Trial[]>([]),[answers,setAnswers]=useState<Answer[]>([]);
  const [index,setIndex]=useState(0),[phase,setPhase]=useState<"idle"|"screen"|"validation"|"result">("idle");
  const [busy,setBusy]=useState(false),[heard,setHeard]=useState(false),[error,setError]=useState("");
  const [gain,setGain]=useState(-30),[confirmed,setConfirmed]=useState(false);
  const audition=useRef<PersonalHrtfAudition|null>(null),alive=useRef(true),epoch=useRef(0),guard=useRef(false);
  const previous=useRef(currentHead),responsesTotal=useRef(0);
  useEffect(()=>{alive.current=true;return()=>{alive.current=false;epoch.current++;audition.current?.dispose();};},[]);
  useEffect(()=>{if(playing||locked){epoch.current++;audition.current?.stop();setHeard(false);setBusy(false);guard.current=false;}},[playing,locked]);
  const trial=trials[index];
  const positions=useMemo(()=>layoutPositions(layout),[layout]);
  const snapshot=useRef<TestPosition[]>([]),confirmations=useRef<Answer[]>([]),field=useRef<DirectionalHrtfParameters|null>(null);
  const layoutChanged=(phase==="screen"||phase==="validation")&&JSON.stringify(positions)!==JSON.stringify(snapshot.current);
  useEffect(()=>{if(layoutChanged){epoch.current++;audition.current?.stop();setHeard(false);setBusy(false);guard.current=false;}},[layoutChanged]);
  const [candidateIndex,setCandidateIndex]=useState(0);
  const cancel=()=>{epoch.current++;audition.current?.dispose();audition.current=null;setPhase("idle");setTrials([]);setAnswers([]);setHeard(false);setBusy(false);guard.current=false;setError("");};
  const start=()=>{
    previous.current=currentHead;responsesTotal.current=0;
    snapshot.current=positions;confirmations.current=[];field.current=null;setCandidateIndex(0);
    setTrials(speakerTrials(positions));setAnswers([]);setIndex(0);setPhase("screen");setHeard(false);setError("");
  };
  const listen=async()=>{
    if(!trial||guard.current||playing||locked||layoutChanged)return;
    guard.current=true;setBusy(true);setHeard(false);setError("");const token=++epoch.current;
    try{
      const output=await window.sdaDesktop?.getOutputDevices?.();
      if(output?.status.mode?.toLowerCase().includes("exclusive")||output?.status.requested.exclusive)throw new Error("请先在输出设置中切换为共享模式，再进行定位测试。");
      await (audition.current??=new PersonalHrtfAudition()).play(trial,gain,onVisual);
      if(alive.current&&token===epoch.current)setHeard(true);
    }catch(e){if(alive.current&&token===epoch.current)setError(String(e));}
    finally{if(alive.current&&token===epoch.current){guard.current=false;setBusy(false);}}
  };
  // A submitted answer advances the sound automatically; replay remains explicit.
  useEffect(()=>{if((phase==="screen"||phase==="validation")&&!playing&&!locked)void listen();},[phase,index,candidateIndex]);
  const answer=(response:boolean)=>{
    if(!trial||!heard||busy||playing||locked||layoutChanged)return;
    responsesTotal.current++;
    const recorded:Answer={...trial,response,error:answerError(trial,response),...(trial.kind==="motion"?{parameters:undefined,interpolationPower:(trial.parameters as DirectionalHrtfParameters).power}:{})};
    const next=[...answers.slice(-999),recorded];
    setAnswers(next);setHeard(false);
    if(!response){
      if(phase==="screen"){
        const parameters=generateCandidate().parameters;
        setTrials(items=>items.map((t,i)=>i===index?{...t,parameters}:t));
      }else{
        // Repeat the actual playback route; keep all accepted speaker responses intact.
        // Playback uses fixed speaker HRIRs: a retry must not tune an unused parameter.
        setTrials(items=>items.map((t,i)=>i>=index?{...t,parameters:field.current!}:t));
      }
      setCandidateIndex(candidateIndex+1);return;
    }
    if(phase==="screen")confirmations.current=[...confirmations.current,{...trial,response:true,error:0}];
    if(index+1<trials.length){setIndex(index+1);return;}
    if(phase==="screen"){
      field.current=confirmedField(confirmations.current);
      const movement=layoutMotionTrials(snapshot.current,field.current);
      if(movement.length){setTrials(movement);setIndex(0);setPhase("validation");return;}
    }
    setPhase("result");
    void apply(resultProfile(next),true);
  };
  const resultProfile=(record:Answer[]):PersonalProfile=>({version:6,method:"per-speaker-audibility",subject:"generated",parameters:field.current!,confirmations:confirmations.current,
    renderMethod:"speaker-vbap-v1",createdAt:new Date().toISOString(),dataset:"parametric",answers:record,output:"system-default",generationCount:candidateIndex+1,responsesTotal:responsesTotal.current,
    historyTruncated:responsesTotal.current>1000,previousHead:previous.current,gainDb:gain});

  const apply=async(p:PersonalProfile,save:boolean)=>{
    if(guard.current||(save&&playing)||locked)return;guard.current=true;setBusy(true);setError("");
    try{
      await onApply(p.subject,p.parameters,p.version>=5?p:undefined);
      if(save){localStorage.setItem(PHRTF_KEY,JSON.stringify(p));setProfile(p);}
    }catch(e){if(alive.current)setError(String(e));}
    finally{guard.current=false;if(alive.current)setBusy(false);}
  };
  const restore=async()=>{
    if(!profile||guard.current)return;guard.current=true;setBusy(true);setError("");
    try{await onApply(profile.previousHead);}catch(e){setError(String(e));}finally{guard.current=false;setBusy(false);}
  };
  return <section className="phrtf" aria-label="个性化 HRTF">
    <header><div><h3>个性化 HRTF</h3><p>自适应感知测试 · 参数化 pHRTF 生成</p></div><span className="phrtf-badge">实验性</span></header>
    {phase==="idle"&&<>
      <p>按当前房间的音箱逐个试听。回答否，只调整当前音箱并重听；回答是，保留它的响应并进入下一只。已经通过的音箱不会重置。</p>
      {profile&&profile.version>=5&&<div className="phrtf-saved"><strong>{profile.version>=5?"个人生成 pHRTF":`${profile.subject.toUpperCase()} · 旧版匹配档案`} · 已保存</strong>
        <small>{new Date(profile.createdAt).toLocaleDateString()} · 当前耳机与佩戴条件下的定位匹配</small>
        <div className="phrtf-actions"><button disabled={busy||locked} onClick={()=>void apply(profile,false)}>应用档案</button>
          <button disabled={busy||locked} onClick={()=>void restore()}>恢复匹配前档案</button></div></div>}
      <small>每只音箱的响应独立生成和确认；最后合成同一份 pHRTF。低音炮不参与方向定位测试。</small>
      <p>当前布局：{positions.length} 只定位音箱，之后试听 {positions.length>1?positions.length:0} 条音箱之间的移动路径。保持头朝前、佩戴不变。</p>
      <label className="phrtf-check"><input type="checkbox" checked={confirmed} onChange={e=>setConfirmed(e.target.checked)}/>
        <span>已戴耳机，系统默认输出指向这副耳机，并关闭系统空间音效。测试音走系统默认共享输出，不跟随 SDA 的指定设备。</span></label>
      <button className="phrtf-primary" disabled={!confirmed||playing||locked||busy||positions.length===0} onClick={start}>开始感知测试</button>
    </>}
    {(phase==="screen"||phase==="validation")&&<>
      <div className="phrtf-progress"><strong>{trial?.kind==="motion"?"移动感知":"位置感知"}</strong><span>{phase==="screen"?"音箱":"路径"} {index+1} / {trials.length}</span></div>
      <progress max={trials.length} value={index}/>
      <small>{phase==="screen"?`已完成 ${index} 只音箱；否只重试当前音箱，是进入下一只。`:"按正式播放的音箱路由试听；否会重听当前路径，不改动已确认响应。"}</small>
      <p>{busy?"正在播放，请对照中间视图的测试标记…":"听完回答是或否，下一段声音会自动播放。"}</p>
      <label className="phrtf-level">测试电平 <input aria-label="测试电平" type="range" min="-48" max="-18" step="1" value={gain} disabled={busy||answers.length>0} onChange={e=>setGain(Number(e.target.value))}/><span>{gain} dBFS</span></label>
      <small>此数值是测试信号增益，不是声压级。第一题可调至舒适音量，提交后锁定。</small>
      <div className="phrtf-actions"><button className="phrtf-primary" disabled={busy||playing||locked||layoutChanged} onClick={()=>void listen()}>{busy?"正在播放…":heard?"重听":"播放测试音"}</button><button onClick={cancel}>结束测试</button></div>
      <div className="phrtf-question" aria-live="polite">{heard&&trial?trial.kind==="motion"?<>
        实际播放的路径<strong>{trialStart(trial).label} → {trialEnd(trial).label}</strong>
        你能明显感知到声音沿这条路径移动吗？
      </>:<>实际播放的位置<strong>{trialStart(trial).label}</strong>你能明显感知到声音来自这个方向吗？</>:"听完后显示真实位置或路径"}</div>
      <div className="phrtf-directions" role="group" aria-label="位置确认">
        <button disabled={!heard||busy||playing||locked||layoutChanged} onClick={()=>answer(true)}>是</button>
        <button disabled={!heard||busy||playing||locked||layoutChanged} onClick={()=>answer(false)}>否</button>
      </div>
      <small>视觉会影响位置判断，本测试用于视听主观匹配，不是盲测。听不清可重听或结束，不要为了完成而猜测。暂停测试后可留在本面板，关闭面板不会保存未完成测试。</small>
    </>}
    {phase==="result"&&<>
      <h4>本次独立生成的 pHRTF 已通过感知确认</h4>
      <p>当前布局的各只音箱已分别通过感知确认。保留每只音箱的响应，自动保存为一份 pHRTF，并用于播放器。</p>
      <p role="status">{busy?"正在生成并保存 pHRTF…":profile?.answers===answers?"pHRTF 已保存并应用。":"保存未完成，可重试。"}</p>
      <small>位置与移动测试使用已确认的音箱响应，移动按正式播放的 VBAP 路由合成。测试为干声点对象；歌曲的扩散、头部姿态、房间及其他处理仍可能改变定位。这是主观确认的近似响应，不是耳朵测量。</small>
      <div className="phrtf-actions"><button disabled={busy||playing||locked} className="phrtf-primary" onClick={()=>void (profile?.answers===answers?apply(profile,false):apply(resultProfile(answers),true))}>{profile?.answers===answers?"重新应用 pHRTF":"重试保存 pHRTF"}</button><button disabled={busy} onClick={cancel}>返回</button></div>
    </>}
    {layoutChanged&&<p role="status">布局已改变，测试已暂停。恢复原布局可继续，或结束后按新布局重新开始。</p>}
    {playing&&<p role="status">定位测试需暂停歌曲；已保存的个人档案可在播放中切换。</p>}
    {locked&&<p role="status">请先结束房间对照或等待当前 HRTF 切换完成。</p>}
    {error&&<p className="phrtf-error" role="alert">{error}</p>}
    {(phase==="idle"||phase==="result")&&<PersonalHrtfLibrary currentHead={currentHead} disabled={busy||locked} revision={profile?.createdAt} onApply={async id=>{await onApply(id)}}/>}
  </section>;
}
