import {useEffect,useState} from "react";
import {createPortal} from "react-dom";
import {MediaPicker} from "./MediaPicker";
type Entry={id:string;name:string;directions:number;method:string};
export function PersonalHrtfLibrary({currentHead,disabled,revision,onApply}:{currentHead:string;disabled:boolean;revision?:string;onApply:(id:string)=>Promise<void>}){
 const [entries,setEntries]=useState<Entry[]>([]),[busy,setBusy]=useState(false),[message,setMessage]=useState(""),[error,setError]=useState("");
 const [picker,setPicker]=useState<{id?:string}|null>(null),[editing,setEditing]=useState<{id:string;copy:boolean;name:string}|null>(null);
 const api=window.sdaDesktop;
 const refresh=async()=>{const values=await api?.listPersonalHrtf?.();if(values)setEntries(values);};
 useEffect(()=>{let alive=true;void api?.listPersonalHrtf?.().then(v=>{if(alive)setEntries(v)}).catch(e=>{if(alive)setError(String(e))});return()=>{alive=false};},[revision,currentHead,disabled]);
 const run=async(work:()=>Promise<void>)=>{if(busy||disabled)return;setBusy(true);setError("");setMessage("");try{await work();await refresh()}catch(e){setError(String(e))}finally{setBusy(false)}};
 const imported=(paths:string[])=>void run(async()=>{if(paths.length!==1)throw Error("一次请选择一个个人档案");const result=await api?.importPersonalHrtf?.(paths[0]!);if(!result)throw Error("个人档案导入接口不可用");setMessage(`已导入“${result.name}”，点击切换即可使用。`)});
 const exported=(id:string,paths:string[])=>void run(async()=>{const result=await api?.personalHrtfArchive?.("export",id,paths[0]!);if(!result?.path)throw Error("导出接口不可用");setMessage(`已导出：${result.path}`)});
 const saveName=()=>{const value=editing;if(!value)return;void run(async()=>{if(value.copy){if(!api?.personalHrtfArchive)throw Error("另存接口不可用");await api.personalHrtfArchive("copy",value.id,value.name)}else{if(!api?.renamePersonalHrtf)throw Error("命名接口不可用");await api.renamePersonalHrtf(value.id,value.name)}setEditing(null);setMessage(value.copy?"已另存为独立档案。":"名称已保存。")})};
 const blocked=busy||disabled;
 return <section className="phrtf-library" aria-label="个人档案管理">
  <div className="phrtf-library-head"><h4>个人档案</h4><button disabled={blocked||!api?.importPersonalHrtf} onClick={()=>setPicker({})}>导入档案</button></div>
  <small>测试完成后自动保存。可保留多份并切换；导出 .phrtf 包含响应、参数和测试记录，导入后直接使用。也支持 SOFA。</small>
  {!entries.length&&<p className="phrtf-library-empty">尚无个人档案，完成测试后会显示在这里。</p>}
  {entries.map(p=><article className={`phrtf-library-item${p.id===currentHead?" active":""}`} key={p.id}>
   <div><strong>{p.name}</strong>{p.id===currentHead&&<span className="phrtf-badge">使用中</span>}</div>
   <small>{p.directions} 个方向 · {p.method==="parametric-feedback"?"个人生成":"SOFA 测量"}</small>
   <div className="phrtf-actions">
    <button disabled={blocked||p.id===currentHead} onClick={()=>void run(async()=>{await onApply(p.id);setMessage(`已切换到“${p.name}”。`)})}>{p.id===currentHead?"已启用":"切换"}</button>
    <button disabled={blocked||!api?.renamePersonalHrtf} onClick={()=>setEditing({id:p.id,name:p.name,copy:false})}>命名</button>
    <button disabled={blocked||!api?.personalHrtfArchive} onClick={()=>setEditing({id:p.id,name:`${p.name} 副本`,copy:true})}>另存为</button>
    <button disabled={blocked||!api?.personalHrtfArchive} onClick={()=>setPicker({id:p.id})}>导出</button>
   </div>
  </article>)}
  {editing&&<form className="phrtf-library-name" onSubmit={e=>{e.preventDefault();saveName()}}><label>{editing.copy?"新档案名称":"档案名称"}<input autoFocus maxLength={80} aria-label="档案名称" disabled={blocked} value={editing.name} onChange={e=>setEditing({...editing,name:e.target.value})}/></label><div className="phrtf-actions"><button disabled={blocked||!editing.name.trim()} type="submit">保存名称</button><button type="button" disabled={blocked} onClick={()=>setEditing(null)}>取消</button></div></form>}
  {message&&<p role="status" className="phrtf-library-message">{message}</p>}{error&&<p role="alert" className="phrtf-error">{error}</p>}
  {picker&&createPortal(<MediaPicker kind="hrtf" mode={picker.id?"folder":"files"} onClose={()=>setPicker(null)} onSelect={paths=>picker.id?exported(picker.id,paths):imported(paths)}/>,document.body)}
 </section>;
}
