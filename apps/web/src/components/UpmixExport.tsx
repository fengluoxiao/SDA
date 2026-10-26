import {useEffect,useRef,useState} from "react";
import SheetHeading from "./SheetHeading";
import type {UpmixExportStatus} from "../vite-env";
type Source={kind:"path";path:string}|{kind:"file";file:File}|null;
type Format="wav"|"sdar"|"m4a";
export default function UpmixExport({source,codec,stereo,volumeBalanceEnabled,spatialBalanceEnabled,captureRenderState,onClose}:{source:Source;codec:string;stereo:boolean;volumeBalanceEnabled:boolean;spatialBalanceEnabled:boolean;captureRenderState:()=>Promise<Record<string,unknown>>;onClose:()=>void}){
 const [status,setStatus]=useState<UpmixExportStatus>({state:"idle"}),[starting,setStarting]=useState(false),[error,setError]=useState(""),[capable,setCapable]=useState(false),[formats,setFormats]=useState<string[]>([]);
 const [format,setFormat]=useState<Format>("wav"),[sampleRate,setSampleRate]=useState(48000),[bitrate,setBitrate]=useState(768),[m4aDialog,setM4aDialog]=useState(false),[master,setMaster]=useState(false);
 const dialogRef=useRef<HTMLDialogElement>(null);
 useEffect(()=>{if(m4aDialog&&!dialogRef.current?.open)dialogRef.current?.showModal();},[m4aDialog]);
 const api=window.sdaDesktop,ready=api?.upmixExportBalancesVersion===1&&capable;
 const localPath=source?.kind==="path"?source.path:source?.kind==="file"?api?.localFilePath?.(source.file):undefined;
 const supported=ready&&!!localPath&&(master||(stereo&&["alac","mp3","pcm"].includes(codec)));
 const busy=starting||status.state==="running"||status.state==="cancelling";
 useEffect(()=>{let active=true;void api?.getUpmixExportStatus?.().then(value=>{if(active)setStatus(value);});const off=api?.onUpmixExportStatus?.(setStatus);return()=>{active=false;off?.();};},[]);
 useEffect(()=>{let active=true;void api?.getExportCapabilities?.().then(value=>{if(active){setCapable(value.renderMetadata===1&&value.authoredAssets);setFormats(value.formats);}}).catch(()=>{if(active)setCapable(false);});return()=>{active=false;};},[]);
 useEffect(()=>{let active=true;setMaster(false);if(localPath)void api?.probeAuthoredFile?.(localPath).then(value=>{if(active)setMaster(value?.lossless===true);}).catch(()=>{});return()=>{active=false;};},[localPath]);
 const start=async()=>{
  if(!localPath||!supported||!api?.exportUpmixWav)return;setM4aDialog(false);setStarting(true);setError("");
  try{await api.exportUpmixWav(localPath,{format,sampleRate,bitrateKbps:bitrate,volumeBalanceEnabled,spatialBalanceEnabled,...(!master?{renderSnapshot:await captureRenderState()}:{})});}catch(e){setError(String(e));}finally{setStarting(false);}
 };
 const labels={wav:"WAV 母版",sdar:"SDAR Bed Channel",m4a:"SDA M4A"};
 const stages:Record<string,string>={metadata:"读取封面、歌词与歌曲信息",write:"写入并校验母版",balance:"渲染音量与空间平衡",peak:"检查处理后峰值",encode:"编码并封装最终文件",decode:"解码、上混与响度分析"};
 return <section className="panel float-panel desktop-sheet" aria-label="导出音频">
  <SheetHeading title="导出音频" detail={master?"使用原始床层与文件混音参数":"立体声 → 7.1.4"} onClose={onClose}/>
  <fieldset className="settings-group" disabled={busy}><legend>文件格式</legend>
   <select aria-label="导出格式" value={format} onChange={e=>setFormat(e.target.value as Format)}>{(["wav","sdar","m4a"] as const).map(value=><option key={value} value={value} disabled={!formats.includes(value)}>{labels[value]}</option>)}</select>
   <p>{format==="m4a"?`${sampleRate/1000} kHz · ${bitrate} kbps · 5.1 + 完整床层扩展` : "48 kHz · 24-bit PCM · 12 声道"}</p>
   <p className="dim">{format==="wav"?"桌面母版，保留无损床声道与完整渲染元数据。":format==="sdar"?"无损床层，可由电脑和移动端 SDA 播放。":"SDA 专用 AAC 分发文件；保留后方及高度床层，移动端默认以 7.1.4 渲染，可切换 9.1.6 输出布局。"}</p>
   {format==="m4a"&&<button onClick={()=>setM4aDialog(true)}>采样率与比特率…</button>}
   <p className="dim">封面、歌词、歌手、专辑、年份、逐声道方位与渲染资源一起保存在一个最终文件中。</p>
  </fieldset>
  <fieldset className="settings-group"><legend>{master?"沿用母版参数":"随当前设置写入"}</legend>
   {master?<p className="dim">保留母版的床声道、混音参数和资源，转换时不再次上混或应用播放器平衡设置。</p>:<><p>音量平衡：{volumeBalanceEnabled?"开启":"关闭"} · 空间平衡：{spatialBalanceEnabled?"开启":"关闭"}</p><p className="dim">已应用的处理写入床声道；HRTF、EQ 和房间仿真携带参数与资源，SDA 播放时自动执行，避免重复处理。系统音量仍由听众控制。</p><p className="dim">母版使用全曲固定峰值余量，保留声道比例。AAC 编码后的峰值可能与无损母版有细微差异，播放仍经过最终输出保护。</p></>}
  </fieldset>
  {!ready&&<p className="cinema-warning">请重启桌面测试版以加载完整导出支持。</p>}
  {ready&&!supported&&<p className="dim">请选择 ALAC、MP3、普通立体声 WAV，或已有的 SDA WAV / SDAR 无损母版。</p>}
  {format==="m4a"&&!formats.includes("m4a")&&<p className="cinema-warning">当前构建未安装 AAC 编码器。</p>}
  <button disabled={!supported||busy||!formats.includes(format)} onClick={()=>format==="m4a"?setM4aDialog(true):void start()}>{starting?"选择保存位置…":`导出 ${labels[format]}`}</button>
  {m4aDialog&&<dialog ref={dialogRef} aria-label="M4A 编码设置" className="settings-group panel" style={{color:"var(--text)",background:"var(--panel)",border:"1px solid var(--border)",borderRadius:16,padding:24,width:"min(440px,90vw)"}} onCancel={()=>setM4aDialog(false)}><h3>M4A 编码设置</h3>
   <label>采样率 <select aria-label="M4A 采样率" value={sampleRate} onChange={e=>setSampleRate(Number(e.target.value))}>{[44100,48000,96000].map(rate=><option key={rate} value={rate}>{rate/1000} kHz</option>)}</select></label>
   <label>总目标比特率 <select aria-label="M4A 比特率" value={bitrate} onChange={e=>setBitrate(Number(e.target.value))}>{[256,384,512,768,1024,1536].map(rate=><option key={rate} value={rate}>{rate} kbps</option>)}</select></label>
   <p className="dim">5.1 基础床层及后方、高度扩展共用此比特率，输出一个 SDA 专用 M4A。</p><button disabled={!supported||busy||!formats.includes("m4a")} onClick={()=>void start()}>选择保存位置并导出</button><button onClick={()=>setM4aDialog(false)}>取消</button>
  </dialog>}
  {busy&&!starting&&<><p role="status">{status.inputName} · {status.state==="cancelling"?"正在取消…":stages[status.stage??"decode"]}</p><progress aria-label="音频导出进度" max={1} value={status.progress??0} style={{width:"100%"}}/><p>{Math.round((status.progress??0)*100)}%</p><button disabled={status.state==="cancelling"} onClick={()=>void api?.cancelUpmixExport?.()}>取消导出</button></>}
  {status.state==="done"&&<div role="status"><p>导出完成：{status.outputPath?.split(/[\\/]/).pop()}</p><p className="dim">{((status.frames??0)/48000).toFixed(2)} 秒 · 12 声道 · 母版峰值 {status.truePeakDbtp?.toFixed(2)??"静音"} dBTP</p><button onClick={()=>void api?.revealUpmixExport?.()}>在文件夹中显示</button></div>}
  {(error||status.error)&&<p className="cinema-warning" role="alert">{error||status.error}</p>}
 </section>;
}
