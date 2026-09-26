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
        const mode=direct||directional||near?`逐对象处理${features.length?` · ${features.join(" + ")}`:""}`:"虚拟扬声器处理";
        const status=!output?.running?`已保存：${mode}，等待播放`:!output.hrtfReady?`已保存：${mode}，等待 HRTF 就绪`:cinema?.settings.monitor?.hardware?.enabled?(directional?`当前：${mode} · 独立对象硬件链`:"硬件仿真开启，当前使用虚拟扬声器处理"):`当前：${mode}`;
        if(alive&&epoch===revision)setDetail(status);
      }catch{if(alive&&epoch===revision)setDetail("暂时无法确认渲染状态");}
    };
    void update();window.addEventListener("sda-object-rendering-change",update);
    const timer=setInterval(()=>void update(),1500);
    return()=>{alive=false;clearInterval(timer);window.removeEventListener("sda-object-rendering-change",update);};
  },[direct]);
  return <div className="settings-description" role="status"><p>{detail}</p><p>基础逐对象、连续方向与近场可同时开启，共用对象渲染通路。连续方向或近场开启时，会自动使用逐对象处理；三个都关闭时使用虚拟扬声器路径。硬件仿真开启时，需开启连续方向才能使用独立对象硬件链与 HRTF。</p></div>;
}
