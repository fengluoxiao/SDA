import {useEffect,useState} from "react";
import SheetHeading from "./SheetHeading";
import type {UpmixExportStatus} from "../vite-env";
type Source={kind:"path";path:string}|{kind:"file";file:File}|null;
export default function UpmixExport({source,codec,stereo,onClose}:{source:Source;codec:string;stereo:boolean;onClose:()=>void}){
 const [status,setStatus]=useState<UpmixExportStatus>({state:"idle"}),[starting,setStarting]=useState(false),[error,setError]=useState("");
 const api=window.sdaDesktop,supported=!!source&&stereo&&["alac","mp3","pcm"].includes(codec);
 const busy=starting||status.state==="running"||status.state==="cancelling";
 useEffect(()=>{let active=true;void api?.getUpmixExportStatus?.().then(value=>{if(active)setStatus(value);});const off=api?.onUpmixExportStatus?.(setStatus);return()=>{active=false;off?.();};},[]);
 const start=async()=>{
  if(!source||!supported||!api?.exportUpmixWav)return;setStarting(true);setError("");
  try{const path=source.kind==="path"?source.path:api.localFilePath?.(source.file);if(!path)throw Error("请通过桌面文件选择器打开本地音频后导出");await api.exportUpmixWav(path);}catch(e){setError(String(e));}finally{setStarting(false);}
 };
 return <section className="panel float-panel desktop-sheet" aria-label="导出上混 WAV">
  <SheetHeading title="导出 WAV" detail="立体声 → 7.1.4" onClose={onClose}/>
  <fieldset className="settings-group"><legend>12 声道 WAV</legend>
   <p>48 kHz · 24-bit PCM · 完整曲目</p>
   <p className="dim">沿用播放的上混矩阵与 −2 dB 校准。空间平衡、耳机 HRTF/EQ、房间效果及监听音量不写入文件。</p>
   <p className="dim">全曲峰值分析后使用一个固定余量，保持各声道比例，输出不高于 −1 dBTP。</p>
   <p className="dim" style={{overflowWrap:"anywhere"}}>声道顺序：FL、FR、FC、LFE、BL、BR、SL、SR、TFL、TFR、TBL、TBR。</p>
  </fieldset>
  {!supported&&<p className="dim">请选择 ALAC、MP3 或 WAV 的普通立体声音频。已有多声道和对象音轨不参与上混导出。</p>}
  <button disabled={!supported||busy} onClick={()=>void start()}>{starting?"选择保存位置…":"导出 7.1.4 WAV"}</button>
  {busy&&!starting&&<><p role="status">{status.inputName} · {status.state==="cancelling"?"正在取消…":status.stage==="write"?"写入并校验 WAV":"解码、上混与峰值分析"}</p><progress aria-label="WAV 导出进度" max={1} value={status.progress??0} style={{width:"100%"}}/><p>{Math.round((status.progress??0)*100)}%</p><button disabled={status.state==="cancelling"} onClick={()=>void api?.cancelUpmixExport?.()}>取消导出</button></>}
  {status.state==="done"&&<div role="status"><p>导出完成：{status.outputPath?.split(/[\\/]/).pop()}</p><p className="dim">{((status.frames??0)/48000).toFixed(2)} 秒 · 12 声道 · 峰值 {status.truePeakDbtp?.toFixed(2)??"静音"} dBTP</p><button onClick={()=>void api?.revealUpmixExport?.()}>显示导出文件</button></div>}
  {status.state==="cancelled"&&<p role="status">导出已取消。</p>}
  {(error||status.state==="error")&&<p className="cinema-warning" role="alert">{error||status.error}</p>}
  <p className="dim">导出在后台完成，可以继续播放。文件超过 4 GB 时自动使用 RF64 WAV。</p>
 </section>;
}
