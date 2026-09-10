import {useRef,useState} from "react";
import {Shirt,Upload,RotateCcw} from "lucide-react";
import {useAvatarSkin,saveAvatarSkin,resetAvatarSkin,importAvatarSkin} from "../avatar-skin";
export default function AvatarSkinControl(){
 const skin=useAvatarSkin(),input=useRef<HTMLInputElement>(null);const [open,setOpen]=useState(false),[error,setError]=useState("");
 return <div className="avatar-skin-control"><button aria-expanded={open} onClick={()=>setOpen(v=>!v)} title="Minecraft 兼容皮肤"><Shirt size={15}/>皮肤</button>
 {open&&<div className="avatar-skin-menu"><strong>像素角色皮肤</strong><small>PNG · 64×64 / 64×32</small>
 <button onClick={()=>input.current?.click()}><Upload size={14}/>导入皮肤</button>
 <button aria-pressed={skin.slim} onClick={()=>{try{saveAvatarSkin({...skin,slim:!skin.slim});setError("")}catch{setError("无法保存皮肤设置")}}}>{skin.slim?"纤细手臂（3 像素）":"经典手臂（4 像素）"}</button>
 <button onClick={()=>{try{resetAvatarSkin();setError("")}catch{setError("无法恢复默认皮肤")}}}><RotateCcw size={14}/>恢复默认</button>
 {error&&<span role="alert">{error}</span>}</div>}
 <input ref={input} type="file" accept="image/png" hidden onChange={async e=>{const file=e.currentTarget.files?.[0];e.currentTarget.value="";if(!file)return;try{const url=await importAvatarSkin(file);const header=new DataView(await file.slice(0,24).arrayBuffer());saveAvatarSkin({url,slim:header.getUint32(20)===32?false:skin.slim});setError("")}catch(err){setError(err instanceof Error?err.message:"皮肤导入失败")}}}/></div>;
}
