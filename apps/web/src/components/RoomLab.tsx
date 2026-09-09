import Select from "./Select";
import {useEffect,useState} from "react";
import {speakerLabel} from "../speaker-labels";
import type {CinemaRoomSummary,RoomSimulation,RoomSimulationConfig} from "../vite-env";
import {comparisonGain,type ComparisonMode} from "../room-comparison";

export type {ComparisonMode} from "../room-comparison";
export interface RoomAudition {stage:"direct"|"early"|"full";matched:boolean;profileId?:string;}
export interface LayoutMemory {layout:string;muted:string[];solo:string[];focus:string[];}
export interface RoomVisual {simulation:RoomSimulation;speaker:string;profileId?:string;animation?:{playing:boolean;slowdown:number};}
const memoryKey="sda-layout-comparisons-v1";
const wallLabels:Record<string,string>={east:"前墙",west:"后墙",north:"左墙",south:"右墙",ceiling:"天花",floor:"地面"};
const materialLabels:Record<string,string>={panel_fabric_covered_6pcf:"织物覆面吸声板",rockwool_50mm_80kgm3:"50 mm 岩棉 · 80 kg/m³",mineral_wool_50mm_70kgm3:"50 mm 矿棉 · 离墙 300 mm",carpet_1_35_kg_m2:"地毯"};
const defaultConfig:RoomSimulationConfig={layout:"7.1.4",length:6,width:5,height:3.2,earHeight:1.2,placement:.7,listeningDistance:1.2,material:"studio",order:10};
export default function RoomLab({layout,snapshot,onRecall,onCompare,onRestore,onVisual,comparison,visualSpeaker,audition,onAudition,onCalibration,onApply}: {
  onApply:(id:string)=>Promise<void>;
  onCalibration?:()=>void;
  layout:string;snapshot:LayoutMemory;onRecall:(memory:LayoutMemory)=>void;
  onCompare:(mode:ComparisonMode,gainDb:number,roomId:string,stage:RoomAudition["stage"])=>Promise<void>;onRestore:()=>Promise<void>;
  audition:RoomAudition;onAudition:(value:RoomAudition)=>void;
  onVisual:(visual:RoomVisual|null)=>void;comparison:ComparisonMode|null;
  visualSpeaker?:string;
}){
  const [view,setView]=useState("room"),[config,setConfig]=useState(defaultConfig);
  const [custom,setCustom]=useState(false);
  const [applied,setApplied]=useState<string|null>(null);
  const [rooms,setRooms]=useState<CinemaRoomSummary[]>([]),[selected,setSelected]=useState("");
  const [speaker,setSpeaker]=useState(visualSpeaker??"FrontLeft"),[paths,setPaths]=useState(!!visualSpeaker);
  const {stage,matched}=audition;
  const [animate,setAnimate]=useState(true),[slowdown,setSlowdown]=useState(100);
  const [busy,setBusy]=useState(false),[available,setAvailable]=useState(false),[progress,setProgress]=useState("");
  const [error,setError]=useState("");
  const [memories,setMemories]=useState<(LayoutMemory|null)[]>(()=>{try{const v=JSON.parse(localStorage.getItem(memoryKey)??"[null,null]");return [0,1].map(i=>{const m=v?.[i];return m&&typeof m.layout==="string"&&[m.muted,m.solo,m.focus].every(a=>Array.isArray(a)&&a.every(n=>typeof n==="string"))?m:null;});}catch{return [null,null];}});
  const room=rooms.find(r=>r.id===selected),simulation=room?.simulation;
  const compatible=room?.layout===layout;
  const parametersChanged=!!simulation&&Object.keys(defaultConfig).some(key=>key!=="layout"&&config[key as keyof RoomSimulationConfig]!==simulation.config[key as keyof RoomSimulationConfig]);
  const api=window.sdaDesktop;
  useEffect(()=>{let active=true;void (async()=>{
    const status=await api?.roomLabStatus?.();const list=await api?.listCinemaRooms?.();const current=await api?.getCinemaSettings?.();
    if(!active)return;
    setAvailable(!!status?.available);if(status?.available&&status.error)setError(status.error);
    setApplied(current?.settings.enabled?current.profileId:null);
    const candidates=(list??[]).filter(r=>r.measurement==="simulated"&&r.simulation?.comparison);
    candidates.sort((a,b)=>(b.simulation?.revision??1)-(a.simulation?.revision??1));
    setRooms(candidates);setSelected(candidates.find(r=>r.id===audition.profileId)?.id??candidates.find(r=>r.id===current?.profileId)?.id??candidates.find(r=>r.layout===layout)?.id??candidates[0]?.id??"");
  })().catch(e=>active&&setError(String(e)));return()=>{active=false;};},[]);
  useEffect(()=>{if(!busy)return;const timer=setInterval(()=>{void api?.roomLabStatus?.().then(s=>{if(s.running)setProgress(`${s.current} / ${s.total}`);});},1000);return()=>clearInterval(timer);},[busy]);
  useEffect(()=>{if(paths&&!simulation)return;onVisual(paths&&simulation?{simulation,speaker,profileId:selected,animation:{playing:animate,slowdown}}:null);},[paths,simulation,speaker,onVisual,selected,animate,slowdown]);
  useEffect(()=>{if(visualSpeaker&&visualSpeaker!==speaker)setSpeaker(visualSpeaker);},[visualSpeaker]);
  useEffect(()=>{if(simulation)setConfig(simulation.config);},[simulation]);
  useEffect(()=>{if(comparison===null&&rooms.length&&room?.layout!==layout){const match=rooms.find(r=>r.builtin&&r.layout===layout);if(match){setSelected(match.id);setCustom(false);}}},[layout,rooms]);
  const run=async(action:()=>Promise<void>)=>{if(busy)return;setBusy(true);setError("");try{await action();}catch(e){setError(String(e));}finally{setBusy(false);setProgress("");}};
  const gainFor=(mode:ComparisonMode,equal=matched,nextStage=stage)=>equal&&simulation?comparisonGain(layout,simulation,mode,nextStage):0;
  const apply=(mode:ComparisonMode,equal=matched,nextStage=stage)=>void run(async()=>{
    if(!room||!simulation||!compatible)throw new Error("选择与当前布局一致的仿真档案");
    await onCompare(mode,gainFor(mode,equal,nextStage),room.id,nextStage);
    onAudition({stage:nextStage,matched:equal,profileId:room.id});
  });
  const numeric=(key:"length"|"width"|"height"|"earHeight"|"placement"|"order",label:string,min:number,max:number,step:number)=>
    <label>{label}<input aria-label={label} type="number" min={min} max={max} step={step} value={config[key]} disabled={busy||(room?.builtin&&!custom)} onChange={e=>{const value=e.currentTarget.valueAsNumber;if(Number.isFinite(value))setConfig(v=>({...v,[key]:Math.min(max,Math.max(min,value))}));}}/></label>;
  return <section className="panel float-panel room-lab" aria-label="房间实验室">
    <div className="obj-head"><h2>房间实验室 <span className="obj-count">{layout}</span></h2><button disabled={busy||comparison!==null} onClick={onCalibration}>档案与校准</button></div>
    <div className="room-lab-tabs" role="group" aria-label="实验室页面">{[["room","房间"],["compare","对照"],["paths","声路"],["layouts","布局"]].map(([id,label])=><button key={id} aria-pressed={view===id} onClick={()=>setView(id!)}>{label}</button>)}</div>
    {view!=="layouts"&&<label className="cinema-profile">房间档案<Select aria-label="仿真档案" value={selected} disabled={busy||comparison!==null} onChange={e=>{setSelected(e.target.value);setCustom(false);onAudition({...audition,stage:"full",profileId:e.target.value});}}><option value="">选择档案</option>{rooms.map(r=><option value={r.id} key={r.id}>{r.builtin?"内置 · ":"自定义 · "}{r.name}</option>)}</Select></label>}
    {room&&!compatible&&<p className="cinema-warning">档案布局 {room.layout}，当前 {layout}</p>}
    {simulation&&(simulation.revision??1)<2&&<p className="cinema-warning">旧版音箱朝向档案，请重新生成房间。</p>}
    {view==="room"&&<>
      <p className="cinema-status">{simulation?.sourceModel==="ideal-omnidirectional"?"SADIE II KU100 · 理想全向声源 · 仿真房间":"Genelec 8020 实测方向性 + KU100 实测 HRIR"}</p>
      <div className="room-lab-fields">{numeric("length","前后长度 m",3,10,.1)}{numeric("width","左右宽度 m",3,8,.1)}{numeric("height","房间高度 m",2.2,4,.1)}{numeric("earHeight","耳部高度 m",.8,1.6,.1)}{config.material!=="studio"&&numeric("placement","音箱径向比例",.5,1,.05)}{numeric("order","反射阶数",1,12,1)}</div>
      <label className="cinema-profile">房间声学方案<Select value={config.material} disabled={busy||(room?.builtin&&!custom)} onChange={e=>setConfig(v=>({...v,material:e.target.value as RoomSimulationConfig["material"]}))}><option value="studio">录音棚控制室 · 分表面处理</option><option value="rockwool_50mm_80kgm3">岩棉 · 50 mm / 80 kg/m³</option><option value="plasterboard">双层石膏板 · 带矿棉空腔</option><option value="hard_surface">硬质表面 · 文献平均值</option>{["treated","living","reflective"].includes(config.material)&&<option value={config.material}>旧档案 · 未溯源系数</option>}</Select></label>
      <p className="cinema-source">材料系数来自声学文献；控制室采用等距近场监听和分表面处理，尺寸与覆盖比例为设计参数。</p>
      {config.material==="studio"&&<label className="cinema-number"><span>等距监听距离</span><input type="number" aria-label="等距监听距离" min={.8} max={2.5} step={.1} value={config.listeningDistance??1.2} disabled={busy||(room?.builtin&&!custom)} onChange={e=>{const d=e.currentTarget.valueAsNumber;if(Number.isFinite(d))setConfig(v=>({...v,listeningDistance:Math.max(.8,Math.min(2.5,d))}));}}/><small>m</small></label>}
      {simulation?.studioDesign&&<p className="cinema-source">近场 {simulation.studioDesign.nearFieldDistanceMetres} m · 偏干控制室 · 中频混响估算 {Math.min(...simulation.studioDesign.eyringSeconds.slice(1,6)).toFixed(2)}–{Math.max(...simulation.studioDesign.eyringSeconds.slice(1,6)).toFixed(2)} s（非实测）</p>}
      {simulation?.material&&<details className="cinema-source"><summary>材料与电平参考</summary><p>{simulation.surfaces?"按表面覆盖比例计算等效吸声；未覆盖部分为双层石膏板。":simulation.material.description}</p>{simulation.surfaces?Object.entries(simulation.surfaces).map(([wall,v])=><p key={wall}>{wallLabels[wall]??wall}：{materialLabels[v.materialId.replaceAll(".","_")]??v.materialId} · {Math.round(v.coverage*100)}% 覆盖<br/>{v.coeffs.map(a=>a.toFixed(3)).join(" / ")}</p>):<p>{simulation.material.coeffs.join(" / ")}</p>}<p>吸声系数频段：{simulation.material.centerFreqs.join(" / ")} Hz{simulation.surfaces?"；地毯 8 kHz 沿用 4 kHz 文献值。":""}</p><p>1 米相对数字参考；距离按 1/r 衰减，无自动音量补偿，非绝对声压标定。</p><a href={simulation.material.source} target="_blank" rel="noreferrer">材料数据来源</a>{simulation.studioDesign&&<> · <a href={simulation.studioDesign.source} target="_blank" rel="noreferrer">控制室设计参考</a></>}</details>}
      {room&&<div className="cinema-actions"><button disabled={busy||!compatible||parametersChanged||(applied===room.id&&comparison===null)} title={parametersChanged?"参数已修改，请先重新生成房间":undefined} onClick={()=>void run(async()=>{await onApply(room.id);setApplied(room.id);onAudition({...audition,stage:"full",profileId:room.id});})}>{parametersChanged?"需重新生成":applied===room.id&&comparison===null?"已应用":"应用房间"}</button>{room.builtin&&!custom&&<button disabled={busy||comparison!==null} onClick={()=>setCustom(true)}>自定义参数</button>}</div>}
      {(!room?.builtin||custom)&&<div className="cinema-actions"><button disabled={busy||!available||comparison!==null} onClick={()=>void run(async()=>{const result=await api?.roomLabGenerate?.({...config,layout});if(!result)throw new Error("仿真接口不可用");setRooms(v=>[result,...v.filter(r=>r.id!==result.id)]);setSelected(result.id);setConfig(result.simulation?.config??{...config,layout});setCustom(false);onAudition({...audition,stage:"full",profileId:result.id});setView("room");})}>生成房间</button>
        {busy&&<button onClick={()=>void api?.roomLabCancel?.()}>取消生成</button>}<span className="cinema-status">{progress}</span></div>
      }
      {!available&&(!room?.builtin||custom)&&<p className="cinema-warning">自定义生成环境未配置</p>}
      <p className="cinema-status">有限阶镜像声源仿真 · 非房间实测 · LFE 保留现有低频路径</p>
    </>}
    {view==="compare"&&<>
      <div className="room-lab-compare">{([["raw","原始 KU100"],["calibrated","校准 KU100"],["room","虚拟房间"]] as const).map(([mode,label])=><button key={mode} aria-pressed={comparison===mode} disabled={busy||!compatible||!simulation} onClick={()=>apply(mode)}>{label}{simulation&&<small>{gainFor(mode).toFixed(1)} dB</small>}</button>)}</div>
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
      <label className="cinema-profile">音箱<Select aria-label="声路音箱" value={speaker} onChange={e=>setSpeaker(e.target.value)}>{Object.keys(simulation?.positions??{}).map(name=><option key={name} value={name}>{speakerLabel(name)}</option>)}</Select></label>
      <div className="cinema-report-table"><table><thead><tr><th>路径</th><th>长度 m</th><th>传播 ms</th></tr></thead><tbody>{simulation?.paths[speaker]?.map(p=><tr key={p.wall}><td>{{direct:"直达",front:"前墙",back:"后墙",left:"左墙",right:"右墙",floor:"地面",ceiling:"天花"}[p.wall]??p.wall}</td><td>{p.distance.toFixed(2)}</td><td>{p.arrivalMs.toFixed(2)}</td></tr>)}</tbody></table></div>
      <p className="cinema-status">显示直达与一次反射；表内不含测量文件自身延时。</p>
    </>}
    {view==="layouts"&&<div className="room-layout-memories">{[0,1].map(index=><div key={index}><strong>{index===0?"A":"B"} · {memories[index]?.layout??"未保存"}</strong><span>{memories[index]?`静音 ${memories[index]!.muted.length} / Solo ${memories[index]!.solo.length} / 聚焦 ${memories[index]!.focus.length}`:""}</span><button disabled={busy||comparison!==null} onClick={()=>{const next=[...memories];next[index]=snapshot;setMemories(next);localStorage.setItem(memoryKey,JSON.stringify(next));}}>保存当前</button><button disabled={busy||!memories[index]||comparison!==null} onClick={()=>{try{onRecall(memories[index]!);}catch(e){setError(String(e));}}}>载入</button></div>)}</div>}
    {error&&<p className="cinema-warning" role="alert">{error}</p>}
  </section>;
}
