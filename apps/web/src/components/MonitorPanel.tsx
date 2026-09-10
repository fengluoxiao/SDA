import Select from "./Select";
import {SlidersHorizontal, Layers, Volume2, Waves, CircuitBoard} from "lucide-react";
import {AHB2_SOURCE,HARDWARE_PRESETS,createHardwarePreset,matchingHardwarePreset} from "../hardware-presets";
import {useEffect,useState} from "react";
import type {VirtualSpeaker} from "@sda/renderer";
import type {MonitorSettings,MonitorOutput} from "../vite-env";
import {alignMonitorToRoom,alignmentContext} from "../monitor-alignment";
import {speakerLabel} from "../speaker-labels";
import {MONITOR_PRESETS,createMonitorPreset} from "../monitor-presets";
const defaults=():MonitorSettings=>({enabled:false,levelDb:0,dim:false,dimDb:-20,muted:false,bassEnabled:false,crossoverHz:80,bassDb:0,outputs:{}});
const output=():MonitorOutput=>({trimDb:0,delayMs:0,invert:false,muted:false});
const hardwareDefaults=()=>({enabled:false,inputDb:0,dacBits:24,lineRms:2,gainDb:26,railV:28,currentA:7,loadOhms:8,outputOhms:0.05,bandwidthHz:60000});
export default function MonitorPanel({layout,speakers,comparisonActive=false,onClose}:{onClose?:()=>void;layout:string;speakers:readonly VirtualSpeaker[];comparisonActive?:boolean;onExitComparison?:()=>Promise<void>}) {
  const [settings,setSettings]=useState(defaults),[saved,setSaved]=useState("");
  const [busy,setBusy]=useState(true),[error,setError]=useState("");
  const [view,setView]=useState<"level"|"outputs"|"bass"|"hardware">("level");
  const hardware={...hardwareDefaults(),...settings.hardware};
  const hardwarePreset=matchingHardwarePreset(hardware);
  const [preset,setPreset]=useState("");
  const [alignment,setAlignment]=useState<{context:string;name:string}|null>(null);
  useEffect(()=>{let live=true;void window.sdaDesktop?.getCinemaSettings?.().then(s=>{
    if(live){const m=s.settings.monitor??defaults();setSettings(m);setSaved(JSON.stringify(m));if(s.error)setError(s.error);}
  }).catch(e=>live&&setError(String(e))).finally(()=>live&&setBusy(false));return()=>{live=false;};},[]);
  const apply=async(next:MonitorSettings)=>{
    if(busy)return;setBusy(true);setError("");
    try{
      const api=window.sdaDesktop,state=await api?.getCinemaSettings?.();
      if(alignment&&state&&alignment.context!==alignmentContext(state.settings,state.profileId,layout))throw new Error("房间或布局已变化，请重新生成通道对齐");
      if(!state||!await api?.nativeRendererCinema?.({...state.settings,monitor:next},state.profileId))throw new Error("监听处理器设置未被接受");
      setSettings(next);setSaved(JSON.stringify(next));
    }catch(e){setError(String(e));}finally{setBusy(false);}
  };
  const loadAlignment=async()=>{
    if(busy)return;setBusy(true);setError("");
    try{
      const api=window.sdaDesktop;
      const [state,rooms]=await Promise.all([api?.getCinemaSettings?.(),api?.listCinemaRooms?.()]);
      const room=rooms?.find(r=>r.id===state?.profileId);
      if(!state||!room)throw new Error("请先在房间面板应用内置房间");
      const next=alignMonitorToRoom(room,state.settings,settings,speakers.map(s=>s.name),layout);
      setSettings(next);setAlignment({context:alignmentContext(state.settings,state.profileId,layout),name:room.name});setView("outputs");
    }catch(e){setError(String(e));}finally{setBusy(false);}
  };
  const numeric=(key:"levelDb"|"dimDb"|"crossoverHz"|"bassDb",label:string,min:number,max:number,unit:string)=><label className="cinema-number"><span>{label}</span><input type="number" aria-label={label} value={settings[key]} min={min} max={max} step={key==="crossoverHz"?1:.5} disabled={busy} onChange={e=>{const n=e.currentTarget.valueAsNumber;if(Number.isFinite(n))setSettings(s=>({...s,[key]:Math.max(min,Math.min(max,n))}));}}/><small>{unit}</small></label>;
  const toggle=(key:"enabled"|"dim"|"muted"|"bassEnabled",label:string)=><label className="settings-switch"><span>{label}</span><input type="checkbox" role="switch" aria-label={label} checked={settings[key]} disabled={busy||(key==="bassEnabled"&&layout==="2.0")} onChange={e=>setSettings(s=>({...s,[key]:e.target.checked}))}/></label>;
  const changeOutput=(name:string,value:Partial<MonitorOutput>)=>setSettings(s=>({...s,outputs:{...s.outputs,[name]:{...output(),...s.outputs[name],...value}}}));
  return <section className="panel float-panel cinema-panel monitor-panel" aria-label="监听处理器">
    <div className="monitor-heading"><div><SlidersHorizontal size={20}/><h2>监听处理器</h2></div><span>{layout} · 48 kHz</span></div>
    <fieldset className="monitor-controls">
    <div className="monitor-master">{toggle("enabled","启用监听处理器")}<p>管理监听电平、输出通道与硬件仿真</p></div>
    <details className="monitor-presets"><summary><Layers size={15}/>内置监听配置<span>{preset?MONITOR_PRESETS.find(p=>p.id===preset)?.name:"选择与载入"}</span></summary><div className="monitor-preset-body">
    <label className="cinema-profile"><span>内置监听配置</span>
      <Select aria-label="内置监听配置" value={preset} disabled={busy} onChange={e=>setPreset(e.target.value)}>
        <option value="">选择配置</option>
        {MONITOR_PRESETS.map(p=><option key={p.id} value={p.id} disabled={p.bassEnabled&&!speakers.some(s=>s.name==="LFE")}>{p.name}{p.bassEnabled&&!speakers.some(s=>s.name==="LFE")?"（需要低音输出）":""}</option>)}
      </Select>
    </label>
    <div className="cinema-actions"><button disabled={busy||!preset} title="载入当前布局的中性通道参数；保留主音量、DIM 开关、静音及处理器开关，点击应用后生效" onClick={()=>{
      try{setSettings(createMonitorPreset(preset,settings,speakers.map(s=>s.name)));setAlignment(null);setError("");}catch(e){setError(String(e));}
    }}>载入配置</button><span className="cinema-source" title={MONITOR_PRESETS.find(p=>p.id===preset)?.url}>{MONITOR_PRESETS.find(p=>p.id===preset)?.source}</span></div>
    </div></details>
    <div className="speaker-group-tabs cinema-tabs" role="group" aria-label="监听处理器页面">
      {([["level","监听电平"],["outputs","输出通道"],["bass","低频管理"],["hardware","硬件链路"]] as const).map(([id,label])=><button key={id} aria-pressed={view===id} onClick={()=>setView(id)}>{id==="level"?<Volume2 size={16}/>:id==="outputs"?<Layers size={16}/>:id==="bass"?<Waves size={16}/>:<CircuitBoard size={16}/>}<span>{label}</span></button>)}
    </div>
    {view==="level"&&<div className="cinema-band">
      {numeric("levelDb","监听衰减",-80,0,"dB")}
      {toggle("dim","DIM")}{numeric("dimDb","DIM 衰减",-40,0,"dB")}{toggle("muted","总静音")}
    </div>}
    {view==="outputs"&&<><div className="cinema-actions"><button disabled={busy||comparisonActive} title="根据当前内置房间的直达声响应生成相对电平和到达时间补偿；对照试听期间不可生成" onClick={()=>void loadAlignment()}>对齐当前房间</button></div>
    {alignment&&<p className="cinema-source" title="基于仿真直达声的相对对齐，非实际声压校准；房间改变后需重新生成。LFE 保留原值。">{alignment.name}</p>}
    <div className="monitor-table"><table><thead><tr><th>输出</th><th>电平 dB</th><th>延时 ms</th><th>反相</th><th>静音</th></tr></thead><tbody>
      {speakers.map(s=>{const o=settings.outputs[s.name]??output();return <tr key={s.name}><td>{speakerLabel(s.name)}</td>
        {(["trimDb","delayMs"] as const).map(k=><td key={k}><input type="number" aria-label={`${speakerLabel(s.name)} ${k}`} value={o[k]} min={k==="trimDb"?-24:0} max={k==="trimDb"?6:20} step={.1} disabled={busy} onChange={e=>{const n=e.currentTarget.valueAsNumber;if(Number.isFinite(n))changeOutput(s.name,{[k]:Math.max(k==="trimDb"?-24:0,Math.min(k==="trimDb"?6:20,n))});}}/></td>)}
        {(["invert","muted"] as const).map(k=><td key={k}><input type="checkbox" aria-label={`${speakerLabel(s.name)} ${k==="invert"?"反相":"静音"}`} checked={o[k]} disabled={busy} onChange={e=>changeOutput(s.name,{[k]:e.target.checked})}/></td>)}
      </tr>;})}
    </tbody></table></div></>}
    {view==="bass"&&<div className="cinema-band">{toggle("bassEnabled","低频管理")}{numeric("crossoverHz","LR4 分频点",40,160,"Hz")}{numeric("bassDb","重定向低频电平",-24,6,"dB")}</div>}
    {view==="hardware"&&<div className="cinema-band">
      <label className="settings-switch"><span>启用硬件链路</span><input type="checkbox" role="switch" aria-label="启用硬件链路" disabled={busy} checked={hardware.enabled} onChange={e=>setSettings(s=>({...s,hardware:{...hardware,enabled:e.target.checked}}))}/></label>
      <span className="cinema-source monitor-signal-path" title="通用电路近似，非实测品牌复刻。原始双声道作用于左右输出；空间模式作用于音箱总线，暂代逐对象 HRTF。监听电平是模型之后的听音衰减。">{hardware.enabled?"音箱总线 · 4× 过采样":"旁路"} · 电阻负载</span>
      <label className="cinema-profile"><span>功放参数配置</span>
        <Select aria-label="功放参数配置" value={hardwarePreset} disabled={busy} onChange={e=>{if(e.target.value)setSettings(s=>createHardwarePreset(e.target.value,s));}}>
          <option value="">自定义参数</option>
          {HARDWARE_PRESETS.map(p=><option key={p.id} value={p.id}>{p.name}</option>)}
        </Select>
      </label>
      {hardwarePreset ? <div className="cinema-source">
        <div className="monitor-spec-chips"><span>AHB2</span><span>立体声</span><span>100 W / 8 Ω</span></div>
        <details className="monitor-reference"><summary>参数来源与近似范围</summary>
          <p>线路电压按所选灵敏度匹配，输入增益可独立调整。</p>
          <p>厂家标称：增益 9.2 / 17 / 23 dB，输入灵敏度 9.8 / 4 / 2 Vrms，峰值电流 29 A，频响优于 0.1 Hz–200 kHz（+0/−3 dB）。</p>
          <p>100 W / 8 Ω 换算为负载峰值 40 V；模型电压上限含输出阻抗压降。输出阻抗由 1 kHz 阻尼系数 254 换算，约 0.0315 Ω。</p>
          <p>这是规格约束的近似，不是实测复刻。带宽使用简化滤波，未复现频变阻抗、THX 电路或保护系统。DAC 位深独立设置，非 AHB2 参数。</p>
          <a href={AHB2_SOURCE} target="_blank" rel="noreferrer">厂家规格</a>
        </details>
      </div> : <p className="cinema-source">自定义电路近似。输入增益 0 dB 为原电平；正增益可能导致削波。</p>}
      <label className="cinema-profile"><span>输入增益快捷选择</span>
        <Select aria-label="输入增益快捷选择" value={[0,-3,-6,-12,3,6].includes(hardware.inputDb)?String(hardware.inputDb):"custom"} disabled={busy}
          onChange={e=>{if(e.target.value!=="custom")setSettings(s=>({...s,hardware:{...hardware,inputDb:Number(e.target.value)}}));}}>
          <option value="0">0 dB · 默认原电平</option>
          {[-3,-6,-12,3,6].map(n=><option key={n} value={String(n)}>{n>0?"+":""}{n} dB</option>)}
          <option value="custom" disabled>自定义 · 在下方输入</option>
        </Select>
      </label>
      <span className="cinema-source">输入增益独立于功放档位；正增益可能使满幅信号削波。</span>
      <div className="monitor-parameter-grid">{([
        ["inputDb","输入增益",-60,12,.5,"dB"],["dacBits","DAC 位深",8,24,1,"bit"],
        ["lineRms","满幅线路输出",.1,12,.1,"Vrms"],["gainDb","功放增益",0,40,.5,"dB"],
        ["railV","等效峰值电压上限",1,80,.01,"V"],["currentA","峰值电流上限",.01,30,.1,"A"],
        ["loadOhms","负载阻抗",2,600,1,"Ω"],["outputOhms","输出阻抗",0,20,.01,"Ω"],
        ["bandwidthHz","标称带宽近似",5000,250000,1000,"Hz"],
      ] as const).map(([key,label,min,max,step,unit])=><label className="cinema-number" key={key}><span>{label}</span><input type="number" aria-label={label} value={hardware[key]} min={min} max={max} step={step} disabled={busy} onChange={e=>{let n=e.currentTarget.valueAsNumber;if(Number.isFinite(n)){n=Math.max(min,Math.min(max,n));if(key==="dacBits")n=Math.round(n);setSettings(s=>({...s,hardware:{...hardware,[key]:n}}));}}}/><small>{unit}</small></label>)}</div>
    </div>}
    <div className="cinema-footer"><span>{busy?"处理中":JSON.stringify(settings)===saved?"已应用":"未应用"}</span>
      <button disabled={busy||!saved||JSON.stringify(settings)===saved} onClick={()=>{setSettings(JSON.parse(saved));setAlignment(null);}}>撤销更改</button>
      <button disabled={busy||JSON.stringify(settings)===saved} onClick={()=>void apply(settings)}>应用</button>
    </div>
    </fieldset>
    {error&&<p role="alert" className="cinema-warning">{error}</p>}
  </section>;
}
