// Poll only while this page is visible. Audio transport is independent.
export function createScene(request){
 const panel=document.getElementById('scene-section'),host=document.getElementById('scene-host'),status=document.getElementById('scene-status');
 let active=false,online=false,view=null,timer=0,generation=0,last=null,loader=null;
 const theme=()=>document.documentElement.dataset.theme==='light'?'light':'dark';
 async function poll(epoch){
   try{
     const scene=await request('scene');
     if(epoch!==generation)return;
     if(scene&&Array.isArray(scene.objects)&&Array.isArray(scene.layout)){
       if(!view){loader??=import('./scene-view.mjs');const module=await loader;if(epoch!==generation)return;view=module.mountScene(host);}
       last=scene;view.update(scene,theme());status.textContent=scene.objects.length?`${scene.objects.length} 个对象 · 主机实时位置`:'当前没有动态对象';
     }else status.textContent='等待主机的空间信息';
   }catch{if(epoch===generation)status.textContent='空间视图暂不可用，音频不受影响';}
   finally{if(epoch===generation)timer=setTimeout(()=>void poll(epoch),100);}
 }
 function refresh(){
   generation++;clearTimeout(timer);
   if(active&&online&&!document.hidden){status.textContent='加载空间视图…';void poll(generation);}
   else{view?.dispose();view=null;last=null;host.replaceChildren();}
 }
 document.addEventListener('sda-page',e=>{const next=e.detail===2;if(next!==active){active=next;refresh();}});
 document.addEventListener('visibilitychange',refresh);
 new MutationObserver(()=>{if(view&&last)view.update(last,theme());}).observe(document.documentElement,{attributes:true,attributeFilter:['data-theme']});
 return {connected(value){if(online!==value){online=value;refresh();}},reset(){last=null;view?.dispose();view=null;host.replaceChildren();}};
}
