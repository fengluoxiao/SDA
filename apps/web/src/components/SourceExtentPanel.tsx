import {useState} from "react";
import "./SourceExtentPanel.css";
export interface SourceExtentSettings {enabled:boolean;width:number;diffusion:number}
export function readSourceExtent():SourceExtentSettings {
  try {const v=JSON.parse(localStorage.getItem("sda-source-extent-v1")??"null");
    if(typeof v?.enabled==="boolean"&&[v.width,v.diffusion].every(n=>Number.isFinite(n)&&n>=0&&n<=1))return v;
  }catch{}
  return {enabled:false,width:0,diffusion:0};
}
export default function SourceExtentPanel(){
  const [value,setValue]=useState(readSourceExtent),[busy,setBusy]=useState(false),[error,setError]=useState("");
  const apply=async(next:SourceExtentSettings)=>{
    if(busy)return;setBusy(true);setError("");
    try{const api=window.sdaDesktop;if(!api?.nativeRendererSourceExtent)throw Error("请重启更新后的 Electron");
      if(!(await api.getNativeRendererStatus?.())?.running)throw Error("请先启动音频输出");
      if(!await api.nativeRendererSourceExtent(next))throw Error("声源范围设置未被接受");
      localStorage.setItem("sda-source-extent-v1",JSON.stringify(next));setValue(next);
    }catch(e){setError(String(e));}finally{setBusy(false);}
  };
  return <fieldset className="settings-group settings-section" disabled={busy}>
    <legend>声源面积与扩散 · 实验性</legend>
    <label className="settings-switch"><span>分布声源渲染</span><input type="checkbox" role="switch" checked={value.enabled} onChange={e=>void apply({...value,enabled:e.target.checked})}/></label>
    <p className="settings-description">按对象的宽、高和扩散信息展开声音，保留中心定位。仅作用于原生对象轨，不分离合唱，也不修改立体声或固定声床。</p>
    <label className="source-extent-control"><span>最小宽度 <output>{Math.round(value.width*120)}°</output></span><input aria-label="最小对象宽度" type="range" min="0" max="1" step="0.01" value={value.width} onChange={e=>setValue({...value,width:Number(e.target.value)})}/></label>
    <label className="source-extent-control"><span>最小扩散 <output>{Math.round(value.diffusion*100)}%</output></span><input aria-label="最小对象扩散" type="range" min="0" max="1" step="0.01" value={value.diffusion} onChange={e=>setValue({...value,diffusion:Number(e.target.value)})}/></label>
    <p className="settings-description">两个最小值会影响所有对象。设为 0 跟随母版；缺少范围信息的对象仍保持点声源。扩散为去相关能量比例，不是混响量。</p>
    <div className="cinema-actions"><button onClick={()=>void apply({...value,enabled:true})}>应用范围</button><button onClick={()=>void apply({enabled:true,width:0,diffusion:0})}>按母版</button><button onClick={()=>void apply({enabled:true,width:.25,diffusion:.12})}>宽声源试听</button></div>
    <small>试听参数是可调整的偏好，不代表录音中的真实合唱面积。关闭即可对照原渲染。</small>
    {error&&<p role="alert" className="cinema-warning">{error}</p>}
  </fieldset>;
}
