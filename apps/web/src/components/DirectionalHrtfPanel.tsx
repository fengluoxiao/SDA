import {useEffect,useState} from "react";
export function readDirectionalHrtf():boolean { return localStorage.getItem("sda-directional-hrtf-v1")==="true"; }
export default function DirectionalHrtfPanel({codec}:{codec?:string}={}){
  const [enabled,setEnabled]=useState(readDirectionalHrtf),[busy,setBusy]=useState(false),[error,setError]=useState("");
  const [status,setStatus]=useState("");
  const is360Ra = ["mpegh","mha1","mhm1"].includes(codec ?? "");
  useEffect(()=>{
    let alive=true;
    const check=async()=>{try{
      const api=window.sdaDesktop;
      const [output,cinema]=await Promise.all([api?.getNativeRendererStatus?.(),api?.getCinemaSettings?.()]);
      if(alive)setStatus(!output?.running?"已保存 · 等待播放":output.hrtfReady===false?"已保存 · 等待 HRTF 就绪":!enabled&&output?.directionalHrtf?"高密度 ADM · 自动连续方向 + 共享房间":is360Ra?(enabled?"360RA 13 · 对象连续方向 HRTF":"360RA 13 · 虚拟扬声器 HRTF"):cinema?.settings.monitor?.hardware?.enabled?"已启用 · 独立对象硬件链 + 方向 HRTF":"已启用 · 对象方向直达声 + 独立房间反射");
    }catch{if(alive)setStatus("等待输出状态");}};
    void check();const timer=setInterval(()=>void check(),1500);return()=>{alive=false;clearInterval(timer);};
  },[enabled,is360Ra]);
  const apply=async(next:boolean)=>{
    setBusy(true);setError("");
    try{
      const api=window.sdaDesktop;if(!api?.nativeRendererDirectionalHrtf)throw Error("请重启更新后的 Electron");
      const output=await api.getNativeRendererStatus?.();
      if(output?.running&&!await api.nativeRendererDirectionalHrtf(next))throw Error("连续方向设置未被接受");
      localStorage.setItem("sda-directional-hrtf-v1",String(next));setEnabled(next);window.dispatchEvent(new Event("sda-object-rendering-change"));
    }catch(e){setError(String(e));}finally{setBusy(false);}
  };
  return <fieldset className="settings-group settings-section" disabled={busy}>
    <legend>连续方向 HRTF · 实验性</legend>
    <label className="settings-switch"><span>按对象实际方向渲染</span><input role="switch" type="checkbox" checked={enabled} onChange={e=>void apply(e.target.checked)}/></label>
    <details className="settings-details"><summary>算法与性能说明</summary>
    <p className="settings-description">对象直达声在 HRTF 数据方向之间连续插值，保留双耳到达时间差。房间反射继续使用原有扬声器路径，可与近场、声源面积与扩散同时开启。硬件仿真开启时，每个对象先经过独立硬件链再进行 HRTF 渲染；这与共用扬声器功放的混合失真不同。</p>
    <small>自动使用逐对象卷积，计算开销会增加。精度取决于当前 HRTF 数据；稀疏个人档案不会因此变成实测密集档案。带区域排除的对象保留原路由；扩散对象使用独立多方向 HRTF 与卷积，不合并进声床；完全扩散时定位感会自然减弱。</small>
    </details>
    <p role="status" className="settings-description">{enabled?status:"连续方向已关闭 · 基础逐对象与近场保持各自设置"}</p>
    {error&&<p role="alert" className="cinema-warning">{error}</p>}
  </fieldset>;
}
