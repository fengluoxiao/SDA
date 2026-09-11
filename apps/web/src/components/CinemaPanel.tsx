import Select from "./Select";
import {ROOM_LISTENING_LEVELS} from "../room-listening";
import { useEffect, useState } from "react";
import type { VirtualSpeaker } from "@sda/renderer";
import { speakerLabel } from "../speaker-labels";
import type { CinemaSettings, CinemaSpeakerCalibration, CinemaRoomSummary } from "../vite-env";

const defaults = (): CinemaSettings => ({enabled:false,...ROOM_LISTENING_LEVELS,bassEnabled:false,crossoverHz:80,bassDb:0,speakers:{}});
const neutral = (): CinemaSpeakerCalibration => ({gainDb:0,delayMs:0,lowDb:0,highDb:0});
export default function CinemaPanel({ layout, speakers, onBack, onClose }: {onClose?:()=>void;layout:string; speakers: readonly VirtualSpeaker[]; onBack?:()=>void}) {
  const [settings,setSettings] = useState(defaults);
  const [profileId,setProfileId] = useState<string|null>(null);
  const [rooms,setRooms] = useState<CinemaRoomSummary[]>([]);
  const [saved,setSaved] = useState("");
  const [busy,setBusy] = useState(true);
  const [error,setError] = useState("");
  const [view,setView] = useState<"room"|"speakers"|"report">("room");
  const desktop = window.sdaDesktop;
  const room = rooms.find(r=>r.id===profileId);
  const compatible = !room || room.layout === layout;
  const current = JSON.stringify({settings,profileId});
  useEffect(()=>{
    let active=true;
    void (async()=>{
      if(!desktop?.getCinemaSettings || !desktop.listCinemaRooms) throw new Error("请重启 Electron 加载影院接口");
      const [state,list] = await Promise.all([desktop.getCinemaSettings(),desktop.listCinemaRooms()]);
      if(!active)return;
      setSettings(state.settings);setProfileId(state.profileId);setRooms(list);
      setSaved(JSON.stringify({settings:state.settings,profileId:state.profileId}));
      if(state.error)setError(state.error);
    })().catch(e=>{if(active)setError(String(e));}).finally(()=>{if(active)setBusy(false);});
    return ()=>{active=false;};
  },[desktop]);
  const run = async (task:()=>Promise<void>)=>{
    if(busy)return;
    setBusy(true);setError("");
    try{await task();}catch(e){setError(String(e));}finally{setBusy(false);}
  };
  const number = (key: "directDb"|"earlyDb"|"lateDb"|"earlyMs"|"crossoverHz"|"bassDb", label:string,min:number,max:number,unit:string) =>
    <label className="cinema-number"><span>{label}</span><input aria-label={label} type="number" min={min} max={max} step={key.endsWith("Db")?0.5:1}
      value={settings[key]} disabled={busy} onChange={e=>{const value=e.currentTarget.valueAsNumber;if(Number.isFinite(value))setSettings(s=>({...s,[key]:Math.max(min,Math.min(max,value))}));}}/><small>{unit}</small></label>;
  const updateSpeaker = (name:string,key:keyof CinemaSpeakerCalibration,value:number)=>{
    if(!Number.isFinite(value))return;
    setSettings(s=>({...s,speakers:{...s.speakers,[name]:{...neutral(),...s.speakers[name],[key]:value}}}));
  };
  const saveCalibration = (calibration: Record<string,CinemaSpeakerCalibration>) => void run(async()=>{
    const next = {...settings,enabled:true,speakers:{...settings.speakers,...calibration}};
    if(!await desktop?.nativeRendererCinema?.(next,profileId))throw new Error("原生渲染器未接受影院设置");
    setSettings(next);
    setSaved(JSON.stringify({settings:next,profileId}));
  });
  const equidistant = speakers.length > 0 && speakers.every(s=>s.distance===speakers[0]!.distance);
  return <div className="panel float-panel cinema-panel" aria-label="房间档案与校准">
    <div className="obj-head"><h2>房间校准 <span className="obj-count">{layout}</span></h2><button onClick={onBack}>返回房间</button>
      <label className="cinema-enable">启用 <input type="checkbox" role="switch" aria-label="启用房间校准" checked={settings.enabled} disabled={busy} onChange={e=>setSettings(s=>({...s,enabled:e.target.checked}))}/></label>
    </div>
    <div className="speaker-group-tabs cinema-tabs" role="group" aria-label="影院页面">
      {([["room","房间"],["speakers","音箱校准"],["report","测量报告"]] as const).map(([id,label])=><button key={id} aria-pressed={view===id} onClick={()=>setView(id)}>{label}</button>)}
    </div>
    {view==="room" && <>
      <label className="cinema-profile">房间档案<Select aria-label="房间档案" value={profileId??""} disabled={busy} onChange={e=>setProfileId(e.target.value||null)}>
        <option value="">当前完整 HRTF / BRIR</option>{rooms.map(r=><option key={r.id} value={r.id}>{r.name} · {r.layout}</option>)}
      </Select></label>
      <div className="cinema-actions"><button disabled={busy} onClick={()=>void run(async()=>{const r=await desktop?.importCinemaRoom?.();if(r){setRooms(list=>[...list.filter(v=>v.id!==r.id),r]);setProfileId(r.id);}})}>导入档案</button>
        <button data-button="danger" disabled={busy||!room||room.builtin} onClick={()=>void run(async()=>{if(room&&await desktop?.deleteCinemaRoom?.(room.id)){setRooms(list=>list.filter(r=>r.id!==room.id));setProfileId(null);}})}>删除档案</button></div>
      {room&&<p className="cinema-status">{room.measurement==="simulated"?"实测音箱 / 模拟房间":room.measurement==="personal"?"个人测量（档案声明）":"人头麦测量"} · {room.sampleRate/1000} kHz</p>}
      {!compatible&&<p className="cinema-warning">档案为 {room?.layout}；当前 {layout} 使用原 HRTF。</p>}
      <div className="cinema-band">
        {number("directDb","直达声",-24,6,"dB")}{number("earlyDb","早期反射",-40,6,"dB")}
        {number("lateDb","混响尾部",-40,6,"dB")}{number("earlyMs","早晚分界",10,100,"ms")}
      </div>
    </>}
    {view==="speakers"&&<>
      <div className="cinema-actions">
        <button disabled={busy||!equidistant} onClick={()=>saveCalibration(Object.fromEntries(speakers.map(s=>[s.name,neutral()])))}>等距中性基准 · 保存</button>
        {room&&<button disabled={busy||!compatible} onClick={()=>saveCalibration(room.suggested)}>实测对齐建议 · 保存</button>}
      </div>
      <p className="cinema-status">等距中性基准：零补偿；非真力 GLM 校准。</p>
      <div className="cinema-calibration-head"><span>音箱</span><span>电平 dB</span><span>延时 ms</span><span>低频 dB</span><span>高频 dB</span></div>
      {speakers.map(speaker=>{const entry=settings.speakers[speaker.name]??neutral();return <div className="cinema-calibration-row" key={speaker.name}>
        <span title={speaker.name}>{speakerLabel(speaker.name)}</span>
        {(["gainDb","delayMs","lowDb","highDb"] as const).map(key=><input key={key} aria-label={`${speakerLabel(speaker.name)} ${key}`} type="number"
          min={key==="delayMs"?0:key==="gainDb"?-24:-6} max={key==="delayMs"?20:6} step={key==="delayMs"?0.1:0.5} value={entry[key]}
          disabled={busy||(speaker.isLfe&&(key==="lowDb"||key==="highDb"))} onChange={e=>updateSpeaker(speaker.name,key,Math.max(key==="delayMs"?0:key==="gainDb"?-24:-6,Math.min(key==="delayMs"?20:6,e.currentTarget.valueAsNumber)))}/>)}
      </div>})}
      <button disabled={busy} onClick={()=>setSettings(s=>({...s,speakers:{}}))}>清除音箱校准</button>
    </>}
    {view==="report"&&<>
      {!room?<p className="cinema-status">尚未导入实测房间档案</p>:<>
        <p className="cinema-source">{room.source}</p><p className="cinema-source">许可：{room.license}</p>
        <div className="cinema-report-table"><table><thead><tr><th>音箱</th><th>到达 ms</th><th>耳间差 ms</th><th>直达能量 dB</th></tr></thead><tbody>{room.rows.map(row=><tr key={row.name}><td>{speakerLabel(row.name)}</td><td>{row.arrivalMs.toFixed(2)}</td><td>{row.itdMs.toFixed(2)}</td><td>{row.directEnergyDb.toFixed(1)}</td></tr>)}</tbody></table></div>
        <p className="cinema-status">相对响应能量，非声压级。建议以最晚到达、最低电平为基准。</p>
        {room.limited&&<p className="cinema-warning">部分补偿超出可调范围，建议值已限幅。</p>}
        <div className="cinema-actions"><button disabled={busy||!compatible} onClick={()=>setSettings(s=>({...s,speakers:{...s.speakers,...room.suggested}}))}>采用对齐建议</button>
          <button disabled={busy} onClick={()=>void run(async()=>{await desktop?.exportCinemaReport?.(room.id);})}>导出报告</button></div>
      </>}
    </>}
    {error&&<p role="alert" className="cinema-warning">{error}</p>}
    <div className="cinema-footer"><span>{busy?"处理中":current===saved?"已应用":"未应用"}</span>
      <button disabled={busy||!saved} onClick={()=>{const state=JSON.parse(saved);setSettings(state.settings);setProfileId(state.profileId);}}>撤销更改</button>
      <button data-button="primary" disabled={busy||current===saved} onClick={()=>void run(async()=>{if(!await desktop?.nativeRendererCinema?.(settings,profileId))throw new Error("原生渲染器未接受影院设置");setSaved(current);})}>应用</button>
    </div>
  </div>;
}
