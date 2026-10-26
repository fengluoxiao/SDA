import {useEffect,useState} from "react";
import SheetHeading from "./SheetHeading";
import type {UpmixExportStatus} from "../vite-env";
type Source={kind:"path";path:string}|{kind:"file";file:File}|null;
export default function UpmixExport({source,codec,stereo,volumeBalanceEnabled,spatialBalanceEnabled,onClose}:{source:Source;codec:string;stereo:boolean;volumeBalanceEnabled:boolean;spatialBalanceEnabled:boolean;onClose:()=>void}){
 const [status,setStatus]=useState<UpmixExportStatus>({state:"idle"}),[starting,setStarting]=useState(false),[error,setError]=useState("");
 const writeVolume=volumeBalanceEnabled,writeSpatial=spatialBalanceEnabled;
 const api=window.sdaDesktop,ready=api?.upmixExportBalancesVersion===1,supported=ready&&!!source&&stereo&&["alac","mp3","pcm"].includes(codec);
 const busy=starting||status.state==="running"||status.state==="cancelling";
 useEffect(()=>{let active=true;void api?.getUpmixExportStatus?.().then(value=>{if(active)setStatus(value);});const off=api?.onUpmixExportStatus?.(setStatus);return()=>{active=false;off?.();};},[]);
 const start=async()=>{
  if(!source||!supported||!api?.exportUpmixWav)return;setStarting(true);setError("");
  try{const path=source.kind==="path"?source.path:api.localFilePath?.(source.file);if(!path)throw Error("请通过桌面文件选择器打开本地音频后导出");await api.exportUpmixWav(path,{volumeBalanceEnabled:writeVolume,spatialBalanceEnabled:writeSpatial});}catch(e){setError(String(e));}finally{setStarting(false);}
 };
 return <section className="panel float-panel desktop-sheet" aria-label="导出上混 WAV">
  <SheetHeading title="导出 WAV" detail="立体声 → 7.1.4" onClose={onClose}/>
  <fieldset className="settings-group"><legend>12 声道 WAV</legend>
   <p>48 kHz · 24-bit PCM · 完整曲目</p>
   <p className="dim">自动保留原曲封面、标题、歌手、专辑、年份等信息，并写入内嵌或同名 LRC / TTML 歌词。所有内容保存在同一个 WAV 中。</p>
   <p className="dim">沿用播放的上混矩阵与 −2 dB 校准。音量平衡和空间平衡可写入声道音频；耳机 HRTF/EQ、房间效果及音量滑块不写入文件。</p>
   <p className="dim">全曲峰值分析后使用一个固定余量，保持各声道比例，输出不高于 −1 dBTP。</p>
   <p className="dim" style={{overflowWrap:"anywhere"}}>声道顺序：FL、FR、FC、LFE、BL、BR、SL、SR、TFL、TFR、TBL、TBR。</p>
  </fieldset>
  <fieldset className="settings-group"><legend>随当前设置写入</legend>
   <p>音量平衡：{writeVolume?"开启":"关闭"} · 空间平衡：{writeSpatial?"开启":"关闭"}</p>
   <p className="dim">将当前启用的两项处理写进同一个 WAV。音量平衡使用全曲固定增益和 +2.5 dB 校准；空间平衡使用同一套原生方向细节处理。</p>
  </fieldset>
  {!ready&&<p className="cinema-warning">请重启桌面测试版，以加载音量平衡和空间平衡导出。</p>}
  {ready&&!supported&&<p className="dim">请选择 ALAC、MP3 或 WAV 的普通立体声音频。已有多声道和对象音轨不参与上混导出。</p>}
  <button disabled={!supported||busy} onClick={()=>void start()}>{starting?"选择保存位置…":"导出 7.1.4 WAV"}</button>
  {busy&&!starting&&<><p role="status">{status.inputName} · {status.state==="cancelling"?"正在取消…":status.stage==="metadata"?"读取封面、歌词与歌曲信息":status.stage==="write"?"写入并校验 WAV":status.stage==="balance"?"渲染音量与空间平衡":status.stage==="peak"?"检查处理后峰值":"解码、上混与响度分析"}</p><progress aria-label="WAV 导出进度" max={1} value={status.progress??0} style={{width:"100%"}}/><p>{Math.round((status.progress??0)*100)}%</p><button disabled={status.state==="cancelling"} onClick={()=>void api?.cancelUpmixExport?.()}>取消导出</button></>}
  {status.state==="done"&&<div role="status"><p>导出完成：{status.outputPath?.split(/[\\/]/).pop()}</p><p className="dim">音量平衡：{status.volumeBalanceEnabled?"已写入":"关闭"} · 空间平衡：{status.spatialBalanceEnabled?"已写入":"关闭"}</p>{status.volumeBalanceEnabled&&<p className="dim">源参考 {status.sourceLufs?.toFixed(2)??"静音"} LUFS · 固定节目增益 {status.programmeGainDb?.toFixed(2)} dB · 校准 +2.5 dB</p>}<p className="dim">{((status.frames??0)/48000).toFixed(2)} 秒 · 12 声道 · 峰值 {status.truePeakDbtp?.toFixed(2)??"静音"} dBTP</p>{status.metadata&&<p className="dim">歌曲信息已写入 · {status.metadata.coverCount} 张封面 · {status.metadata.lyrics.length} 份歌词</p>}<button onClick={()=>void api?.revealUpmixExport?.()}>显示导出文件</button></div>}
  {status.state==="cancelled"&&<p role="status">导出已取消。</p>}
  {(error||status.state==="error")&&<p className="cinema-warning" role="alert">{error||status.error}</p>}
  <p className="dim">导出在后台完成，可以继续播放。每次只生成一个 12 声道 WAV 文件。</p>
 </section>;
}
