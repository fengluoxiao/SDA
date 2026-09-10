import {useEffect,useState} from "react";
export function readDirectionalHrtf():boolean { return localStorage.getItem("sda-directional-hrtf-v1")==="true"; }
export default function DirectionalHrtfPanel(){
  const [enabled,setEnabled]=useState(readDirectionalHrtf),[busy,setBusy]=useState(false),[error,setError]=useState("");
  const [status,setStatus]=useState("");
  useEffect(()=>{
    let alive=true;
    const check=async()=>{try{
      const api=window.sdaDesktop;
      const [output,cinema]=await Promise.all([api?.getNativeRendererStatus?.(),api?.getCinemaSettings?.()]);
      if(alive)setStatus(!output?.running?"已保存 · 等待播放":output.hrtfReady===false?"已保存 · 等待 HRTF 就绪":cinema?.settings.monitor?.hardware?.enabled?"硬件仿真开启，暂用扬声器路径":"已启用 · 对象方向直达声 + 独立房间反射");
    }catch{if(alive)setStatus("等待输出状态");}};
    void check();const timer=setInterval(()=>void check(),1500);return()=>{alive=false;clearInterval(timer);};
  },[]);
  const apply=async(next:boolean)=>{
    setBusy(true);setError("");
    try{
      const api=window.sdaDesktop;if(!api?.nativeRendererDirectionalHrtf)throw Error("请重启更新后的 Electron");
      const output=await api.getNativeRendererStatus?.();
      if(output?.running&&!await api.nativeRendererDirectionalHrtf(next))throw Error("连续方向设置未被接受");
      localStorage.setItem("sda-directional-hrtf-v1",String(next));setEnabled(next);
    }catch(e){setError(String(e));}finally{setBusy(false);}
  };
  return <fieldset className="settings-group settings-section" disabled={busy}>
    <legend>连续方向 HRTF · 实验性</legend>
    <label className="settings-switch"><span>按对象实际方向渲染</span><input role="switch" type="checkbox" checked={enabled} onChange={e=>void apply(e.target.checked)}/></label>
    <details className="settings-details"><summary>算法与性能说明</summary>
    <p className="settings-description">对象直达声在 HRTF 数据方向之间连续插值，保留双耳到达时间差。房间反射继续使用原有扬声器路径，可与近场、声源面积与扩散同时开启。</p>
    <small>自动使用逐对象卷积，计算开销会增加。精度取决于当前 HRTF 数据；稀疏个人档案不会因此变成实测密集档案。带区域排除的对象保留原路由。</small>
    </details>
    <p role="status" className="settings-description">{enabled?status:"已关闭 · 使用原扬声器方向渲染"}</p>
    {error&&<p role="alert" className="cinema-warning">{error}</p>}
  </fieldset>;
}
