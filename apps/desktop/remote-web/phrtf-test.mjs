import {PersonalHrtfAudition,speakerTrials,confirmedField,layoutMotionTrials,generateCandidate,trialStart,trialEnd,testVisualPosition,answerError} from "./phrtf-core.mjs";
const el=(tag,text)=>{const n=document.createElement(tag);if(text)n.textContent=text;return n;};
export function openPhrtfTest(state,request) {
  const positions=state.speakers.filter(s=>s.name!=="LFE").map(s=>({name:s.name,label:s.label,az:((s.az+180)%360+360)%360-180,el:s.el}));
  const initialLayout=JSON.stringify(positions),previousHead=state.head;
  const dialog=el("dialog"),heading=el("h2","个人耳廓 · 感知测试"),status=el("p"),question=el("p"),controls=el("div"),canvas=el("canvas");
  dialog.className="phrtf-web-test";canvas.width=480;canvas.height=270;canvas.setAttribute("aria-label","测试方向示意图");
  let trials=speakerTrials(positions),index=0,phase="screen",confirmations=[],answers=[],field=null,busy=false,heard=false,closed=false,epoch=0,animation=0,total=0,canSave=false;
  const audition=new PersonalHrtfAudition();
  void request("testAudioSuspend").catch(e=>{status.textContent=e.message;});
  function button(text,action){const b=el("button",text);b.onclick=action;return b;}
  const exit=button("结束测试",()=>dialog.close());
  const yes=button("是",()=>answer(true)),no=button("否",()=>answer(false)),replay=button("播放测试音",()=>void listen());
  const gainLabel=el("label","测试电平 "),gain=el("input");gain.type="range";gain.min=-48;gain.max=-18;gain.step=1;gain.value=-30;gain.setAttribute("aria-label","耳廓测试电平");const level=el("span","−30 dBFS");gain.oninput=()=>level.textContent=`${gain.value} dBFS`;gainLabel.append(gain,level);
  const confirmLabel=el("label"),confirm=el("input");confirm.type="checkbox";confirmLabel.append(confirm,el("span","已戴好耳机，关闭手机系统空间音效，并保持头朝前"));confirm.onchange=refresh;
  const save=button("保存到主机并应用",()=>void saveResult());save.hidden=true;
  controls.append(replay,yes,no,save,exit);dialog.append(heading,el("p","测试音由手机直接播放，不经过主机的房间或监听。是保留当前响应；否只重试当前音箱。"),confirmLabel,gainLabel,canvas,status,question,controls);document.body.append(dialog);
  function refresh(){replay.disabled=busy||!confirm.checked||canSave;yes.disabled=no.disabled=busy||!heard||canSave;gain.disabled=busy||total>0;save.disabled=busy;}
  function draw(visual) {
    cancelAnimationFrame(animation);const ctx=canvas.getContext("2d");
    const render=()=>{
      ctx.clearRect(0,0,480,270);ctx.strokeStyle="#80998e";ctx.fillStyle="#80998e";ctx.font="12px sans-serif";
      ctx.beginPath();ctx.ellipse(240,150,190,85,0,0,2*Math.PI);ctx.stroke();ctx.fillText("前方",226,32);ctx.fillText("左",16,154);ctx.fillText("右",451,154);
      ctx.beginPath();ctx.arc(240,150,6,0,2*Math.PI);ctx.fill();
      if(visual){const p=testVisualPosition(visual.trial,Math.min(1,visual.elapsed()/visual.duration));const x=240+p[0]*178,y=150+p[2]*75-p[1]*55;ctx.fillStyle="#39aa76";ctx.beginPath();ctx.arc(x,y,11,0,2*Math.PI);ctx.fill();}
      if(visual&&!closed)animation=requestAnimationFrame(render);
    };render();
  }
  async function listen() {
    if(closed||busy||!confirm.checked||canSave)return;
    busy=true;heard=false;refresh();const token=++epoch,trial=trials[index];
    status.textContent=`${phase==="screen"?"音箱":"移动路径"} ${index+1} / ${trials.length}`;
    question.textContent=trial.kind==="motion"?`${trialStart(trial).label} → ${trialEnd(trial).label}`:trialStart(trial).label;
    try{await audition.play(trial,Number(gain.value),draw);if(!closed&&epoch===token){heard=true;question.textContent+="：你能明显感知到吗？";}}
    catch(e){if(!closed&&epoch===token)status.textContent=e.message;}
    finally{if(!closed&&epoch===token){busy=false;refresh();}}
  }
  function answer(response) {
    if(!heard||busy||closed)return;
    const trial=trials[index],record={...trial,response,error:answerError(trial,response)};total++;
    // Store only bounded evidence; confirmed anchors remain complete.
    answers.push({...record,...(trial.kind==="motion"?{parameters:undefined}: {})});answers=answers.slice(-64);
    if(!response){if(phase==="screen")trials[index]={...trial,parameters:generateCandidate().parameters};void listen();return;}
    if(phase==="screen")confirmations.push(record);
    index++;
    if(index>=trials.length){
      if(phase==="screen"){field=confirmedField(confirmations);trials=layoutMotionTrials(positions,field);phase="validation";index=0;}
      else {canSave=true;heard=false;save.hidden=false;status.textContent="感知确认完成，可以保存到主机";refresh();void saveResult();return;}
    }
    void listen();
  }
  async function saveResult(){
    if(busy||closed||!canSave)return;busy=true;refresh();status.textContent="正在由主机生成并保存个人档案…";
    try{await request("hrtfGenerate",{parameters:field,assessment:{version:6,method:"per-speaker-audibility",subject:"generated",parameters:field,confirmations,answers,
      renderMethod:"speaker-vbap-v1",createdAt:new Date().toISOString(),dataset:"parametric",output:"remote-browser",previousHead,gainDb:Number(gain.value),responsesTotal:total,historyTruncated:total>64}});
      if(!closed){status.textContent="已保存到主机并应用，可在耳廓档案中切换";save.hidden=true;}
    }catch(e){if(!closed)status.textContent=e.message;}finally{busy=false;refresh();}
  }
  dialog.addEventListener("close",()=>{closed=true;epoch++;cancelAnimationFrame(animation);audition.dispose();dialog.remove();void request("testAudioResume").catch(()=>{});});
  dialog.showModal();draw(null);refresh();status.textContent=`当前布局 ${state.layout}，共 ${positions.length} 只方向音箱`;
  return {close:()=>{if(!closed)dialog.close();},update(next,online,playing){
    const layout=next?.speakers?.filter(s=>s.name!=="LFE").map(s=>({name:s.name,label:s.label,az:((s.az+180)%360+360)%360-180,el:s.el}));
    if(!online||playing||JSON.stringify(layout)!==initialLayout){epoch++;audition.stop();busy=false;heard=false;confirm.checked=false;status.textContent="主机播放或布局已变化，请结束测试后重新开始";confirm.disabled=true;refresh();}
  }};
}
