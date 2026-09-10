import {useEffect,useRef} from "react";
import {testVisualPosition,type HrtfTestVisual} from "../phrtf";
export function FlatHrtfTest({visual}:{visual:HrtfTestVisual}) {
 const ref=useRef<HTMLDivElement>(null);
 useEffect(()=>{let frame=0;const tick=()=>{
  const t=visual.elapsed(),p=testVisualPosition(visual.trial,t/visual.duration);
  if(ref.current){ref.current.style.visibility=t>=0&&t<=visual.duration?"visible":"hidden";
   ref.current.style.left=`${50+p[0]*38}%`;ref.current.style.top=`${50+p[2]*38}%`;
   ref.current.textContent=`${visual.trial.kind==="motion"?"测试 OBJ":"测试音箱"}${p[1]>.1?" ↑":""}`;}
  if(t<visual.duration)frame=requestAnimationFrame(tick);
 };tick();return()=>cancelAnimationFrame(frame);},[visual]);
 return <div ref={ref} style={{position:"absolute",visibility:"hidden",zIndex:5,transform:"translate(-50%,-50%)",background:"#684200",color:"white",border:"2px solid #ffb020",borderRadius:8,padding:6,pointerEvents:"none"}}/>;
}
