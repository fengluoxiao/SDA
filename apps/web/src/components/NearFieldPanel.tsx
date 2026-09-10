import {useEffect, useState} from "react";
import "./SourceExtentPanel.css";

export interface NearFieldSettings {enabled:boolean;metresPerUnit:number}
export function readNearField():NearFieldSettings {
  try {
    const v=JSON.parse(localStorage.getItem("sda-near-field-v1")??"null");
    if(typeof v?.enabled==="boolean"&&Number.isFinite(v.metresPerUnit)&&v.metresPerUnit>=.25&&v.metresPerUnit<=4)
      return {enabled:v.enabled,metresPerUnit:v.metresPerUnit};
  }catch{}
  return {enabled:false,metresPerUnit:1};
}
export default function NearFieldPanel(){
  const [value,setValue]=useState(readNearField);
  const [scale,setScale]=useState(value.metresPerUnit);
  const [busy,setBusy]=useState(false),[error,setError]=useState("");
  const [blocked,setBlocked]=useState("正在检查输出状态");
  useEffect(()=>{
    let alive=true;
    const check=async()=>{
      try {
        const api=window.sdaDesktop;
        const [status,cinema]=await Promise.all([api?.getNativeRendererStatus?.(),api?.getCinemaSettings?.()]);
        if(alive)setBlocked(!status?.running?"已保存 · 等待音频输出启动":status.hrtfReady===false?"已保存 · 等待 HRTF 就绪":!cinema?"无法确认房间状态":cinema.settings.monitor?.hardware?.enabled?"硬件仿真开启，近场处理暂不生效":cinema.settings.enabled?"已启用 · 修正直达声，保留房间反射":"");
      }catch{if(alive)setBlocked("无法确认输出状态");}
    };
    void check();const timer=window.setInterval(()=>void check(),1500);
    return ()=>{alive=false;clearInterval(timer);};
  },[]);
  const apply=async(next:NearFieldSettings)=>{
    if(busy)return;setBusy(true);setError("");
    try {
      const api=window.sdaDesktop;
      if(!api?.nativeRendererNearField)throw Error("请重启更新后的 Electron");
      const status=await api.getNativeRendererStatus?.();
      if(status?.running && !await api.nativeRendererNearField(next))throw Error("近场设置未被接受");
      if(!status?.running)setBlocked("已保存 · 等待音频输出启动");
      else if(status.hrtfReady===false)setBlocked("已保存 · 等待 HRTF 就绪");
      localStorage.setItem("sda-near-field-v1",JSON.stringify(next));setValue(next);setScale(next.metresPerUnit);
    }catch(e){setError(String(e));}finally{setBusy(false);}
  };
  return <fieldset className="settings-group settings-section" disabled={busy}>
    <legend>近场声源渲染 · 实验性</legend>
    <label className="settings-switch"><span>近距离双耳差异</span><input type="checkbox" role="switch" checked={value.enabled} onChange={e=>void apply({...value,enabled:e.target.checked})}/></label>
    <details className="settings-details"><summary>高级参数与说明</summary>
    <p className="settings-description">让靠近听者一侧的对象产生更明显的低频双耳差异，保留原有方向与高频线索。自动使用逐对象 HRTF，处理开销随对象数量增加。</p>
    <label className="source-extent-control"><span>坐标单位对应 <output>{scale.toFixed(2)} m</output></span><input aria-label="近场距离映射" type="range" min="0.25" max="4" step="0.05" value={scale} onChange={e=>setScale(Number(e.target.value))}/></label>
    <p className="settings-description">对象坐标不是实际米数。映射后距中心 0.2–1 m 的对象受到处理；更近按 0.2 m 计算，更远保持原样。距离映射不是母版测量值。</p>
    <div className="cinema-actions"><button onClick={()=>void apply({enabled:true,metresPerUnit:scale})}>应用距离</button><button onClick={()=>void apply({enabled:false,metresPerUnit:1})}>恢复默认</button></div>
    <small>几何近似，不是实测近场 HRTF。仅修正对象直达声，可与房间同时使用，保留原有反射。无整体距离增益或额外混响。硬件仿真开启时暂时旁路。</small>
    </details>
    <p role="status" className="settings-description">{value.enabled?(blocked||"已启用 · 等待或处理原生对象音轨"):"已关闭"}</p>
    {error&&<p role="alert" className="cinema-warning">{error}</p>}
  </fieldset>;
}
