import {useSyncExternalStore} from "react";
export type AvatarSkin={url:string;slim:boolean};
const key="sda-avatar-skin-v1";
const fallback:AvatarSkin={url:`${import.meta.env.BASE_URL}avatars/pixel/studio.png`,slim:false};
let current=fallback;
try{const saved=JSON.parse(localStorage.getItem(key)??"null");if(saved&&typeof saved.url==="string"&&(saved.url.startsWith("data:image/png;base64,")||saved.url===fallback.url)&&saved.url.length<2000000)current={url:saved.url,slim:saved.slim===true};}catch{}
const listeners=new Set<()=>void>();
export function useAvatarSkin(){return useSyncExternalStore(fn=>{listeners.add(fn);return()=>listeners.delete(fn)},()=>current);}
export function saveAvatarSkin(skin:AvatarSkin){localStorage.setItem(key,JSON.stringify(skin));current=skin;listeners.forEach(fn=>fn());}
export function resetAvatarSkin(){localStorage.removeItem(key);current=fallback;listeners.forEach(fn=>fn());}
/** Decode and normalize classic 64x32 skins into the modern 64x64 layout. */
export async function importAvatarSkin(file:File):Promise<string>{
 if(file.size>2*1024*1024)throw new Error("请选择小于 2 MB 的 PNG 皮肤");
 const bytes=new Uint8Array(await file.arrayBuffer());
 if([137,80,78,71,13,10,26,10].some((v,i)=>bytes[i]!==v))throw new Error("请选择 PNG 皮肤");
 const image=await createImageBitmap(file);
 try{
  if(image.width!==64||![32,64].includes(image.height))throw new Error("皮肤尺寸须为 64×64 或 64×32");
  const canvas=document.createElement("canvas");canvas.width=canvas.height=64;const ctx=canvas.getContext("2d")!;ctx.drawImage(image,0,0);
  if(image.height===32){
   for(const [sx,sy,w,h,dx,dy] of [[4,16,4,4,20,48],[8,16,4,4,24,48],[0,20,4,12,24,52],[4,20,4,12,20,52],[8,20,4,12,16,52],[12,20,4,12,28,52],[44,16,4,4,36,48],[48,16,4,4,40,48],[40,20,4,12,40,52],[44,20,4,12,36,52],[48,20,4,12,32,52],[52,20,4,12,44,52]]){
    ctx.save();ctx.translate(dx!+w!,dy!);ctx.scale(-1,1);ctx.drawImage(image,sx!,sy!,w!,h!,0,0,w!,h!);ctx.restore();
   }
  }
  // Vanilla-compatible base opacity and the old opaque-background hat rule.
  const pixels=ctx.getImageData(0,0,64,64);
  if(image.height===32){
   let translucent=false;
   for(let y=0;y<32;y++)for(let x=32;x<64;x++)if(pixels.data[(y*64+x)*4+3]!<128)translucent=true;
   if(!translucent)for(let y=0;y<32;y++)for(let x=32;x<64;x++)pixels.data[(y*64+x)*4+3]=0;
  }
  for(const [x0,y0,x1,y1] of [[0,0,32,16],[0,16,64,32],[16,48,48,64]])
   for(let y=y0!;y<y1!;y++)for(let x=x0!;x<x1!;x++)pixels.data[(y*64+x)*4+3]=255;
  ctx.putImageData(pixels,0,0);
  return canvas.toDataURL("image/png");
 }finally{image.close();}
}
