import { useEffect, useRef, useState } from "react";
import { ArrowLeft, ArrowUp, Folder, FileAudio, Star, Clock, X, RefreshCw, Search, FolderOpen } from "lucide-react";

type Entry = {name:string;path:string;directory:boolean};
type Places = {places:{name:string;path:string}[];recent:string[];favorites:string[];initial:string};
export function MediaPicker({mode,onClose,onSelect,kind="media"}:{mode:"files"|"folder";onClose:()=>void;onSelect:(paths:string[])=>void;kind?:"media"|"hrtf"}) {
  const api = (kind==="hrtf"?window.sdaDesktop!.browsePersonalHrtf:window.sdaDesktop!.browseMedia)!;
  const [places,setPlaces] = useState<Places>({places:[],recent:[],favorites:[],initial:""});
  const [directory,setDirectory] = useState("");
  const [parent,setParent] = useState("");
  const [address,setAddress] = useState("");
  const [entries,setEntries] = useState<Entry[]>([]);
  const [selected,setSelected] = useState<string[]>([]);
  const [history,setHistory] = useState<string[]>([]);
  const [query,setQuery] = useState("");
  const [busy,setBusy] = useState(false);
  const [submitting,setSubmitting] = useState(false);
  const [error,setError] = useState("");
  const [menu,setMenu] = useState<{x:number;y:number;path:string;recent?:boolean}|null>(null);
  const generation = useRef(0), panel = useRef<HTMLDivElement>(null);
  useEffect(()=>{
    const escape=(event:KeyboardEvent)=>{
      if(event.key!=="Escape")return;
      event.stopPropagation();
      if(menu)setMenu(null);else if(!submitting)onClose();
    };
    document.addEventListener("keydown",escape);
    return ()=>document.removeEventListener("keydown",escape);
  },[menu,submitting,onClose]);
  const navigate = async (target:string,back=false) => {
    const revision = ++generation.current;
    setBusy(true);setError("");setMenu(null);
    try {
      const result = await api("list",target);
      if(revision!==generation.current)return;
      if(directory&&!back&&directory!==result.path)setHistory(h=>[...h,directory]);
      setDirectory(result.path);setParent(result.parent);setAddress(result.path);
      setEntries(result.entries);setSelected([]);setQuery("");
    } catch(e) { if(revision===generation.current)setError(String(e)); }
    finally { if(revision===generation.current)setBusy(false); }
  };
  useEffect(()=>{
    let alive=true;
    const startup=generation.current;
    const previous=document.activeElement as HTMLElement|null;
    panel.current?.focus();
    void api("places").then(async value=>{
      if(!alive)return;
      setPlaces(value);
      if(generation.current===startup)await navigate(value.initial);
    }).catch(e=>{if(alive)setError(String(e));});
    return ()=>{alive=false;generation.current++;previous?.focus();};
  },[]);
  const favorite = (path:string) => places.favorites.some(p=>p.toLowerCase()===path.toLowerCase());
  const changeSaved = async (action:"favorite"|"unfavorite"|"forget",path:string) => {
    setMenu(null);
    try { const result=await api(action,path);setPlaces(p=>({...p,...result})); }
    catch(e){setError(String(e));}
  };
  const submit = async (paths?:string[]) => {
    setSubmitting(true);setError("");setMenu(null);
    try {
      const result = mode==="folder" ? await api("folder",paths?.[0]||selected[0]||directory) : await api("files",paths||selected);
      onSelect(result);onClose();
    }catch(e){setError(String(e));setSubmitting(false);}
  };
  const context = (event:React.MouseEvent,path:string,recent=false) => {
    event.preventDefault();setMenu({x:Math.max(8,Math.min(event.clientX,window.innerWidth-210)),y:Math.max(8,Math.min(event.clientY,window.innerHeight-150)),path,recent});
  };
  const visible=entries.filter(e=>e.name.toLowerCase().includes(query.toLowerCase()));
  const keyDown=(e:React.KeyboardEvent)=>{
    if(e.key==="Tab"){
      const nodes=[...panel.current!.querySelectorAll<HTMLElement>('button:not(:disabled),input:not(:disabled),[tabindex="0"]')].filter(el=>el.getClientRects().length);
      const first=nodes[0],last=nodes[nodes.length-1];
      if(e.shiftKey&&(document.activeElement===first||document.activeElement===panel.current)){e.preventDefault();last?.focus();}
      else if(!e.shiftKey&&document.activeElement===last){e.preventDefault();first?.focus();}
    }
  };
  return <div className="media-picker-layer" onMouseDown={e=>{if(e.target===e.currentTarget&&!submitting)onClose();}}>
    <div ref={panel} tabIndex={-1} className="media-picker" role="dialog" aria-modal="true" aria-label={kind==="hrtf"?(mode==="folder"?"选择 pHRTF 导出目录":"导入个人档案（pHRTF / SOFA）"):mode==="files"?"打开媒体":"添加媒体目录"} onKeyDown={keyDown} onClick={()=>menu&&setMenu(null)}>
      <div className="media-picker-heading"><h2>{kind==="hrtf"?(mode==="folder"?"选择 pHRTF 导出目录":"导入个人档案（pHRTF / SOFA）"):mode==="files"?"打开媒体":"添加媒体目录"}</h2><button title="关闭选择器" aria-label="关闭选择器" disabled={submitting} onClick={onClose}><X size={18}/></button></div>
      <div className="media-picker-toolbar">
        <button title="后退" aria-label="后退" disabled={!history.length||busy||submitting} onClick={()=>{const target=history[history.length-1];if(target){setHistory(h=>h.slice(0,-1));void navigate(target,true);}}}><ArrowLeft size={17}/></button>
        <button title="上一级" aria-label="上一级" disabled={!parent||parent===directory||busy||submitting} onClick={()=>void navigate(parent)}><ArrowUp size={17}/></button>
        <form onSubmit={e=>{e.preventDefault();if(!submitting)void navigate(address);}}><input type="text" aria-label="目录路径" value={address} disabled={submitting} onChange={e=>setAddress(e.target.value)}/></form>
        <button title="刷新目录" aria-label="刷新目录" disabled={!directory||busy||submitting} onClick={()=>void navigate(directory,true)}><RefreshCw size={17}/></button>
        <button title={favorite(directory)?"取消收藏当前目录":"收藏当前目录"} aria-label={favorite(directory)?"取消收藏当前目录":"收藏当前目录"} disabled={!directory||submitting} onClick={()=>void changeSaved(favorite(directory)?"unfavorite":"favorite",directory)}><Star size={17} fill={favorite(directory)?"currentColor":"none"}/></button>
      </div>
      <div className="media-picker-body">
        <nav className="media-picker-places" aria-label="目录位置">
          <h3><Star size={13}/>收藏</h3>{places.favorites.map(path=><button key={path} disabled={submitting} title={path} onClick={()=>void navigate(path)} onContextMenu={e=>context(e,path)}><span>{path.split(/[\\/]/).filter(Boolean).pop()||path}</span></button>)}
          {!places.favorites.length&&<small>暂无收藏</small>}
          <h3><Clock size={13}/>最近目录</h3>{places.recent.map(path=><button key={path} disabled={submitting} title={path} onClick={()=>void navigate(path)} onContextMenu={e=>context(e,path,true)}><span>{path.split(/[\\/]/).filter(Boolean).pop()||path}</span></button>)}
          {!places.recent.length&&<small>暂无最近目录</small>}
          <h3>位置</h3>{places.places.map(p=><button key={p.path+p.name} disabled={submitting} title={p.path} onClick={()=>void navigate(p.path)} onContextMenu={e=>context(e,p.path)}><Folder size={15}/><span>{p.name}</span></button>)}
        </nav>
        <section className="media-picker-files">
          <label className="media-picker-search"><Search size={16}/><input type="text" aria-label="筛选当前目录" placeholder="筛选当前目录" value={query} onChange={e=>setQuery(e.target.value)}/></label>
          <div className="media-picker-list" aria-label="目录内容" aria-busy={busy}>
            {busy?<p>正在读取目录…</p>:visible.map(entry=><div key={entry.path} className={selected.includes(entry.path)?"media-picker-row selected":"media-picker-row"} onContextMenu={e=>{if(entry.directory)context(e,entry.path);}}>
              <input type={mode==="files"?"checkbox":"radio"} name="media-selection" aria-label={"选择 "+entry.name} disabled={submitting||(mode==="files"?entry.directory:!entry.directory)} checked={selected.includes(entry.path)} onChange={()=>setSelected(s=>mode==="folder"?[entry.path]:s.includes(entry.path)?s.filter(p=>p!==entry.path):[...s,entry.path])}/>
              <button disabled={submitting} title={entry.name} onClick={()=>{if(entry.directory)void navigate(entry.path);else if(mode==="files")setSelected([entry.path]);}} onDoubleClick={()=>{if(!entry.directory&&mode==="files")void submit([entry.path]);}}>
                {entry.directory?<Folder size={18}/>:<FileAudio size={18}/>}<span>{entry.name}</span><small>{entry.directory?"目录":entry.name.split(".").pop()?.toUpperCase()}</small>
              </button>
            </div>)}
            {!busy&&!visible.length&&<p>{query?"没有匹配项":"此目录没有可显示的媒体"}</p>}
          </div>
        </section>
      </div>
      {error&&<p className="media-picker-error" role="alert">{error}</p>}
      <footer className="media-picker-footer"><span>{mode==="files"?`已选择 ${selected.length} 个文件`:selected[0]||directory}</span><button disabled={submitting} onClick={onClose}>取消</button><button disabled={busy||submitting||!directory||(mode==="files"&&!selected.length)} onClick={()=>void submit()}><FolderOpen size={16}/>{submitting?"正在导入…":mode==="files"?"打开":kind==="hrtf"?"导出到此目录":"添加此目录"}</button></footer>
      {menu&&<div role="menu" className="media-picker-menu" style={{left:menu.x,top:menu.y}}>
        <button role="menuitem" onClick={()=>void navigate(menu.path)}><FolderOpen size={15}/>打开目录</button>
        <button role="menuitem" onClick={()=>void changeSaved(favorite(menu.path)?"unfavorite":"favorite",menu.path)}><Star size={15}/>{favorite(menu.path)?"取消收藏":"收藏目录"}</button>
        {menu.recent&&<button role="menuitem" onClick={()=>void changeSaved("forget",menu.path)}><X size={15}/>移出最近目录</button>}
      </div>}
    </div>
  </div>;
}
