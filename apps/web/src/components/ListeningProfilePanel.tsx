import { useRef, useState } from "react";
type Profile = { rear: number; height: number; bassDb: number; trebleDb: number };
const defaults: Profile = { rear: 1, height: 1, bassDb: 0, trebleDb: 0 };
const rows: {key:keyof Profile;label:string;min:number;max:number;step:number;description:string}[] = [
  {key:"rear",label:"后方辨识",min:.5,max:1.5,step:.1,description:"只调整原有细节，保留声源位置"},
  {key:"height",label:"上下辨识",min:.5,max:1.5,step:.1,description:"头顶和底部方向连续变化"},
  {key:"bassDb",label:"耳机低频",min:-3,max:3,step:.5,description:"宽频段低架调整，dB"},
  {key:"trebleDb",label:"耳机高频",min:-3,max:3,step:.5,description:"宽频段高架调整，dB"},
];
export function ListeningProfilePanel() {
  const [original] = useState(() => window.sdaDesktop?.getListeningProfile?.() ?? {...defaults});
  const [candidate,setCandidate]=useState(original), draft=useRef(original);
  const [comparing,setComparing]=useState(false),[error,setError]=useState("");
  const serial=useRef(0);
  if(!window.sdaDesktop?.setListeningProfile)return null;
  const apply=async(profile:Profile,compare=false)=>{
    const token=++serial.current;
    try {
      if(!await window.sdaDesktop!.setListeningProfile!(profile))throw Error("试听设置未被接受");
      if(token===serial.current){setComparing(compare);setError("");}
    }catch(e){if(token===serial.current)setError(String(e));}
  };
  return <fieldset className="settings-group">
    <legend>耳机与个人试听校准</legend>
    <p className="dim">戴好常用耳机，播放熟悉且包含后方、上下声源的片段，再逐项试听。这些是听感偏好，不是个人 HRTF 或耳机频响测量；主音量与峰值保护仍然生效。</p>
    {rows.map(row=><label key={row.key} className="settings-row" style={{display:"block",marginBottom:14}}>
      <span>{row.label} · {candidate[row.key].toFixed(1)}{row.key.endsWith("Db")?" dB":""}</span>
      <input style={{width:"100%"}} aria-label={row.label} type="range" min={row.min} max={row.max} step={row.step} value={candidate[row.key]}
        onChange={e=>{const next={...draft.current,[row.key]:Number(e.target.value)};draft.current=next;setCandidate(next);void apply(next);}} />
      <small className="dim">{row.description}</small>
    </label>)}
    <div style={{display:"flex",gap:8,flexWrap:"wrap"}}>
      <button onClick={()=>void apply(comparing?draft.current:original,!comparing)}>{comparing?"听本次选择":"听进入前设置"}</button>
      <button onClick={()=>void apply(draft.current)}>保存本次选择</button>
      <button onClick={()=>{draft.current={...defaults};setCandidate(draft.current);void apply(draft.current);}}>恢复默认</button>
      <button onClick={()=>{draft.current=original;setCandidate(original);void apply(original);}}>取消并恢复</button>
    </div>
    <p role="status">{error || (comparing?"正在听进入前的设置":"正在听本次选择；调整会自动保存")}</p>
  </fieldset>;
}
