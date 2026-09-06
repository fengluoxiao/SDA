import {useEffect,useState} from "react";
import {speakerLabel} from "../speaker-labels";
import type {CinemaRoomSummary,RoomSimulation,RoomSimulationConfig} from "../vite-env";
import {comparisonGain,type ComparisonMode} from "../room-comparison";

export type {ComparisonMode} from "../room-comparison";
export interface RoomAudition {stage:"direct"|"early"|"full";matched:boolean;profileId?:string;}
export interface LayoutMemory {layout:string;muted:string[];solo:string[];focus:string[];}
export interface RoomVisual {simulation:RoomSimulation;speaker:string;profileId?:string;animation?:{playing:boolean;slowdown:number};}
const memoryKey="sda-layout-comparisons-v1";
const defaultConfig:RoomSimulationConfig={layout:"7.1.4",length:6,width:4,height:2.8,earHeight:1.2,placement:.85,material:"treated",order:6};
export default function RoomLab({layout,snapshot,onRecall,onCompare,onRestore,onVisual,comparison,visualSpeaker,audition,onAudition}: {
  layout:string;snapshot:LayoutMemory;onRecall:(memory:LayoutMemory)=>void;
  onCompare:(mode:ComparisonMode,gainDb:number,roomId:string,stage:RoomAudition["stage"])=>Promise<void>;onRestore:()=>Promise<void>;
  audition:RoomAudition;onAudition:(value:RoomAudition)=>void;
  onVisual:(visual:RoomVisual|null)=>void;comparison:ComparisonMode|null;
  visualSpeaker?:string;
}){
  const [view,setView]=useState("room"),[config,setConfig]=useState(defaultConfig);
  const [rooms,setRooms]=useState<CinemaRoomSummary[]>([]),[selected,setSelected]=useState("");
  const [speaker,setSpeaker]=useState(visualSpeaker??"FrontLeft"),[paths,setPaths]=useState(!!visualSpeaker);
  const {stage,matched}=audition;
  const [animate,setAnimate]=useState(true),[slowdown,setSlowdown]=useState(100);
  const [busy,setBusy]=useState(false),[available,setAvailable]=useState(false),[progress,setProgress]=useState("");
  const [error,setError]=useState("");
  const [memories,setMemories]=useState<(LayoutMemory|null)[]>(()=>{try{const v=JSON.parse(localStorage.getItem(memoryKey)??"[null,null]");return [0,1].map(i=>{const m=v?.[i];return m&&typeof m.layout==="string"&&[m.muted,m.solo,m.focus].every(a=>Array.isArray(a)&&a.every(n=>typeof n==="string"))?m:null;});}catch{return [null,null];}});
  const room=rooms.find(r=>r.id===selected),simulation=room?.simulation;
  const compatible=room?.layout===layout;
  const api=window.sdaDesktop;
  useEffect(()=>{let active=true;void (async()=>{
    const status=await api?.roomLabStatus?.();const list=await api?.listCinemaRooms?.();
    if(!active)return;
    setAvailable(!!status?.available);if(status?.error)setError(status.error);
    const candidates=(list??[]).filter(r=>r.measurement==="simulated"&&r.simulation?.comparison);
    candidates.sort((a,b)=>(b.simulation?.revision??1)-(a.simulation?.revision??1));
    setRooms(candidates);setSelected(candidates.find(r=>r.id===audition.profileId)?.id??candidates.find(r=>r.layout===layout)?.id??candidates[0]?.id??"");
  })().catch(e=>active&&setError(String(e)));return()=>{active=false;};},[]);
  useEffect(()=>{if(!busy)return;const timer=setInterval(()=>{void api?.roomLabStatus?.().then(s=>{if(s.running)setProgress(`${s.current} / ${s.total}`);});},1000);return()=>clearInterval(timer);},[busy]);
  useEffect(()=>{if(paths&&!simulation)return;onVisual(paths&&simulation?{simulation,speaker,profileId:selected,animation:{playing:animate,slowdown}}:null);},[paths,simulation,speaker,onVisual,selected,animate,slowdown]);
  useEffect(()=>{if(visualSpeaker&&visualSpeaker!==speaker)setSpeaker(visualSpeaker);},[visualSpeaker]);
  useEffect(()=>{if(simulation)setConfig(simulation.config);},[simulation]);
  const run=async(action:()=>Promise<void>)=>{if(busy)return;setBusy(true);setError("");try{await action();}catch(e){setError(String(e));}finally{setBusy(false);setProgress("");}};
  const gainFor=(mode:ComparisonMode,equal=matched,nextStage=stage)=>equal&&simulation?comparisonGain(layout,simulation,mode,nextStage):0;
  const apply=(mode:ComparisonMode,equal=matched,nextStage=stage)=>void run(async()=>{
    if(!room||!simulation||!compatible)throw new Error("选择与当前布局一致的仿真档案");
    await onCompare(mode,gainFor(mode,equal,nextStage),room.id,nextStage);
    onAudition({stage:nextStage,matched:equal,profileId:room.id});
  });
  const numeric=(key:"length"|"width"|"height"|"earHeight"|"placement"|"order",label:string,min:number,max:number,step:number)=>
    <label>{label}<input aria-label={label} type="number" min={min} max={max} step={step} value={config[key]} disabled={busy} onChange={e=>{const value=e.currentTarget.valueAsNumber;if(Number.isFinite(value))setConfig(v=>({...v,[key]:Math.min(max,Math.max(min,value))}));}}/></label>;
  return <section className="panel float-panel room-lab" aria-label="房间实验室">
    <div className="obj-head"><h2>房间实验室 <span className="obj-count">{layout}</span></h2></div>
    <div className="room-lab-tabs" role="group" aria-label="实验室页面">{[["room","房间"],["compare","对照"],["paths","声路"],["layouts","布局"]].map(([id,label])=><button key={id} aria-pressed={view===id} onClick={()=>setView(id!)}>{label}</button>)}</div>
    {view!=="layouts"&&<label className="cinema-profile">仿真档案<select aria-label="仿真档案" value={selected} disabled={busy||comparison!==null} onChange={e=>{setSelected(e.target.value);onAudition({...audition,stage:"full",profileId:e.target.value});}}><option value="">尚未生成</option>{rooms.map(r=><option value={r.id} key={r.id}>{r.name}{(r.simulation?.revision??1)<2?` / ${{treated:"吸声较强",living:"普通室内",reflective:"反射较强"}[r.simulation!.config.material]} / 旧版`:""}</option>)}</select></label>}
    {room&&!compatible&&<p className="cinema-warning">档案布局 {room.layout}，当前 {layout}</p>}
    {simulation&&(simulation.revision??1)<2&&<p className="cinema-warning">旧版音箱朝向档案，请重新生成房间。</p>}
    {view==="room"&&<>
      <p className="cinema-status">Genelec 8020 实测方向性 + KU100 实测 HRIR</p>
      <div className="room-lab-fields">{numeric("length","前后长度 m",3,10,.1)}{numeric("width","左右宽度 m",3,8,.1)}{numeric("height","房间高度 m",2.2,4,.1)}{numeric("earHeight","耳部高度 m",.8,1.6,.1)}{numeric("placement","音箱径向比例",.5,1,.05)}{numeric("order","反射阶数",1,12,1)}</div>
      <label className="cinema-profile">墙面吸声假设<select value={config.material} disabled={busy} onChange={e=>setConfig(v=>({...v,material:e.target.value as RoomSimulationConfig["material"]}))}><option value="treated">吸声较强</option><option value="living">普通室内</option><option value="reflective">反射较强</option></select></label>
      <div className="cinema-actions"><button disabled={busy||!available||comparison!==null} onClick={()=>void run(async()=>{const result=await api?.roomLabGenerate?.({...config,layout});if(!result)throw new Error("仿真接口不可用");setRooms(v=>[result,...v.filter(r=>r.id!==result.id)]);setSelected(result.id);setView("compare");})}>生成房间</button>
        {busy&&<button onClick={()=>void api?.roomLabCancel?.()}>取消生成</button>}<span className="cinema-status">{progress}</span></div>
      {!available&&<p className="cinema-warning">仿真运行环境不可用</p>}
      <p className="cinema-status">有限阶镜像声源仿真 · 非房间实测 · LFE 保留现有低频路径</p>
    </>}
    {view==="compare"&&<>
      <div className="room-lab-compare">{([["raw","原始 KU100"],["calibrated","校准 KU100"],["room","真力虚拟房间"]] as const).map(([mode,label])=><button key={mode} aria-pressed={comparison===mode} disabled={busy||!compatible||!simulation} onClick={()=>apply(mode)}>{label}{simulation&&<small>{gainFor(mode).toFixed(1)} dB</small>}</button>)}</div>
      <div className="room-lab-tabs room-lab-stages" role="group" aria-label="房间反射试听">{([["direct","仅直达"],["early","直达 + 早反射"],["full","完整房间"]] as const).map(([value,label])=><button key={value} aria-pressed={comparison!==null&&stage===value} disabled={busy||comparison===null||!compatible||!simulation||(simulation.revision??1)<2} onClick={()=>{if(comparison)apply(comparison,matched,value);}}>{label}</button>)}</div>
      <label className="settings-switch"><span>参考电平匹配</span><input type="checkbox" role="switch" aria-label="参考电平匹配" checked={matched} disabled={busy} onChange={e=>{const value=e.target.checked;if(comparison)apply(comparison,value);else onAudition({...audition,matched:value});}}/></label>
      <p className="cinema-status">20 Hz–20 kHz 粉红噪声响应能量；不保证主观等响。对照期间使用普通 KU100、影院中性设置与房间立体声模式。</p>
      {simulation?.comparison.limited&&<p className="cinema-warning">补偿达到 −40 dB 下限，参考电平未完全匹配。</p>}
      <button disabled={busy||comparison===null} onClick={()=>void run(onRestore)}>退出对照并恢复</button>
    </>}
    {view==="paths"&&<>
      <label className="settings-switch"><span>显示声路</span><input role="switch" type="checkbox" aria-label="显示声路" disabled={!simulation} checked={paths} onChange={e=>setPaths(e.target.checked)}/></label>
      <label className="settings-switch"><span>传播动画</span><input role="switch" type="checkbox" aria-label="传播动画" disabled={!paths} checked={animate} onChange={e=>setAnimate(e.target.checked)}/></label>
      <label className="room-animation-speed"><span>慢放 {slowdown} 倍</span><input type="range" aria-label="传播慢放倍率" min="25" max="200" step="25" value={slowdown} disabled={!paths} onChange={e=>setSlowdown(Number(e.target.value))}/></label>
      <label className="cinema-profile">音箱<select aria-label="声路音箱" value={speaker} onChange={e=>setSpeaker(e.target.value)}>{Object.keys(simulation?.positions??{}).map(name=><option key={name} value={name}>{speakerLabel(name)}</option>)}</select></label>
      <div className="cinema-report-table"><table><thead><tr><th>路径</th><th>长度 m</th><th>传播 ms</th></tr></thead><tbody>{simulation?.paths[speaker]?.map(p=><tr key={p.wall}><td>{{direct:"直达",front:"前墙",back:"后墙",left:"左墙",right:"右墙",floor:"地面",ceiling:"天花"}[p.wall]??p.wall}</td><td>{p.distance.toFixed(2)}</td><td>{p.arrivalMs.toFixed(2)}</td></tr>)}</tbody></table></div>
      <p className="cinema-status">显示直达与一次反射；表内不含测量文件自身延时。</p>
    </>}
    {view==="layouts"&&<div className="room-layout-memories">{[0,1].map(index=><div key={index}><strong>{index===0?"A":"B"} · {memories[index]?.layout??"未保存"}</strong><span>{memories[index]?`静音 ${memories[index]!.muted.length} / Solo ${memories[index]!.solo.length} / 聚焦 ${memories[index]!.focus.length}`:""}</span><button disabled={busy||comparison!==null} onClick={()=>{const next=[...memories];next[index]=snapshot;setMemories(next);localStorage.setItem(memoryKey,JSON.stringify(next));}}>保存当前</button><button disabled={busy||!memories[index]||comparison!==null} onClick={()=>{try{onRecall(memories[index]!);}catch(e){setError(String(e));}}}>载入</button></div>)}</div>}
    {error&&<p className="cinema-warning" role="alert">{error}</p>}
  </section>;
}
