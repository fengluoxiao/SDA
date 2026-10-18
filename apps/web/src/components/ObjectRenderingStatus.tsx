import {useEffect,useState} from "react";
import {readNearField} from "./NearFieldPanel";
import {readDirectionalHrtf} from "./DirectionalHrtfPanel";

export default function ObjectRenderingStatus({direct}:{direct:boolean}) {
  const [detail,setDetail]=useState("正在检查渲染状态…");
  useEffect(()=>{
    let alive=true,revision=0;
    const update=async()=>{
      const epoch=++revision;
      try {
        const api=window.sdaDesktop;
        const [output,cinema]=await Promise.all([api?.getNativeRendererStatus?.(),api?.getCinemaSettings?.()]);
        const directional=output?.directionalHrtf===true||readDirectionalHrtf(),near=readNearField().enabled;
        const features=[directional?"连续方向":null,near?"近场距离":null].filter(Boolean);
        const mode=output?.binauralEnabled===false?"直接下混":direct?`对象渲染${features.length?` · ${features.join(" + ")}`:""}`:"床声道兼容播放";
        const status=!output?.running?`已保存：${mode}，等待播放`:output.binauralEnabled===false?"当前：直接下混":!output.hrtfReady?`已保存：${mode}，等待 HRTF 就绪`:`当前：${mode}${cinema?.settings.monitor?.hardware?.enabled && direct ? " · 独立对象硬件链" : ""}`;
        if(alive&&epoch===revision)setDetail(status);
      }catch{if(alive&&epoch===revision)setDetail("暂时无法确认渲染状态");}
    };
    void update();window.addEventListener("sda-object-rendering-change",update);
    const timer=setInterval(()=>void update(),1500);
    return()=>{alive=false;clearInterval(timer);window.removeEventListener("sda-object-rendering-change",update);};
  },[direct]);
  return <div className="settings-description" role="status"><p>{detail}</p><p>对象渲染开启时同时使用逐对象和实际方向处理；关闭后使用兼容床声道。切换保留播放进度，已缓冲音频播完后生效。</p></div>;
}
