import { useEffect, useState } from "react";
import Select from "./Select";
import "./OutputPanel.css";

export interface OutputSettings { deviceId: string | null; exclusive: boolean; remoteCompatible?:boolean }
export interface OutputDevices {
  status: { requested: OutputSettings; actualId?: string | null; actualName?: string | null;
    mode?: string | null; sampleRate?: number | null; channels?: number | null;
    bufferMs?: number | null; state: string; detail: string };
  devices: {id:string;name:string;available:boolean;isDefault:boolean;sampleRate?:number|null;channels?:number|null}[];
}
export default function OutputPanel() {
  const [data,setData] = useState<OutputDevices | null>(null);
  const [draft,setDraft] = useState<OutputSettings>({deviceId:null,exclusive:false});
  const [busy,setBusy] = useState(false);
  const [error,setError] = useState("");
  const bridge=window.sdaDesktop;
  useEffect(() => {
    let alive=true;
    const off=bridge?.onOutputDevices?.(value=>{if(alive)setData(value);});
    bridge?.getOutputDevices?.().then(value=>{if(alive){setData(value);setDraft(value.status.requested);}})
      .catch(e=>{if(alive)setError(String(e));});
    return ()=>{alive=false;off?.();};
  },[bridge]);
  if(!bridge?.getOutputDevices)return null;
  const apply=async()=>{
    setBusy(true);setError("");
    try {
      const result=await bridge.setOutputDevice!(draft);
      if(result.status)setData(result);
      if(!result.accepted)setError(result.status?.detail || "切换失败，原输出恢复失败时请重试");
    }catch(e){setError(String(e));}finally{setBusy(false);}
  };
  const refresh=async()=>{
    setBusy(true);setError("");
    try{setData(await bridge.getOutputDevices!());}catch(e){setError(String(e));}finally{setBusy(false);}
  };
  return <fieldset className="settings-group output-manager" disabled={busy}>
    <legend>音频输出设备</legend>
    <label>输出到
      <Select aria-label="输出设备" disabled={draft.remoteCompatible} value={draft.deviceId??""} onChange={e=>setDraft({...draft,deviceId:e.target.value||null})}>
        <option value="">跟随系统默认</option>
        {draft.deviceId&&!data?.devices.some(d=>d.id===draft.deviceId)&&<option value={draft.deviceId}>已保存的设备（未连接）</option>}
        {data?.devices.map(d=><option key={d.id} value={d.id} disabled={!d.available}>{d.name}{d.isDefault?" · 默认":""}{!d.available?" · 不可用":""}</option>)}
      </Select>
    </label>
    <label>访问方式
      <Select aria-label="输出访问方式" value={draft.remoteCompatible?"remote":draft.exclusive?"exclusive":"shared"} onChange={e=>setDraft({...draft,deviceId:e.target.value==="remote"?null:draft.deviceId,remoteCompatible:e.target.value==="remote",exclusive:e.target.value==="exclusive"})}>
        <option value="remote">远程兼容 · UU / RDP</option><option value="shared">WASAPI 共享</option><option value="exclusive">WASAPI 独占 · 本机监听</option>
      </Select>
    </label>
    {draft.remoteCompatible&&<small>跟随系统默认，使用共享混音供远程采集；远程软件切换默认设备时自动跟随。</small>}
    {draft.exclusive&&<small role="note">独占绕过系统混音，UU 等远程软件可能听不到。远程听音请选择「远程兼容」。</small>}
    <div className="output-manager-actions"><button data-button="primary" onClick={()=>void apply()} disabled={!data}>{busy?"处理中…":data?.status.state==="unavailable"?"应用并重试":"应用"}</button><button onClick={()=>void refresh()}>刷新设备</button></div>
    <div className="output-manager-status" aria-live="polite">
      <strong>{data?.status.state==="ready"?data.status.actualName:"输出不可用"}</strong>
      {data?.status.state==="ready"&&<span>{data.status.mode==="exclusive"?"实际独占":"实际共享"} · {(data.status.sampleRate??0)/1000} kHz · {data.status.channels} 声道 · 缓冲 {data.status.bufferMs?.toFixed(1)} ms</span>}
      <small>内部渲染 48 kHz / 双耳双声道，按设备采样率转换。指定设备断开后不会自动切到外放。</small>
      {(error||data?.status.detail)&&<p role="status">{error||data?.status.detail}</p>}
    </div>
  </fieldset>;
}
