import {useEffect,useMemo,useRef,useState,type RefObject} from "react";
import {useFrame,useThree} from "@react-three/fiber";
import {BoxGeometry,Group,Texture,TextureLoader,NearestFilter,SRGBColorSpace,MathUtils} from "three";
import {useAvatarSkin} from "../avatar-skin";

/** Minecraft skin box net: top/bottom, right/front/left/back; front faces -Z. */
export function skinBox(w:number,h:number,d:number,u:number,v:number,inflation=0){
 const geometry=new BoxGeometry(w+inflation,h+inflation,d+inflation);
 const faces=[[u+d+w,v+d,d,h],[u,v+d,d,h],[u+d,v,w,d],[u+d+w,v,w,d],[u+2*d+w,v+d,w,h],[u+d,v+d,w,h]];
 const uv=geometry.getAttribute("uv");
 faces.forEach(([x,y,a,b],face)=>{
  const coords=[[x!,y!],[x!+a!,y!],[x!,y!+b!],[x!+a!,y!+b!]];
  coords.forEach(([px,py],i)=>uv.setXY(face*4+i,px!/64,1-py!/64));
 });
 return geometry;
}
function Part({size,uv,texture,overlay=false}:{size:[number,number,number];uv:[number,number];texture:Texture;overlay?:boolean}){
 const geometry=useMemo(()=>skinBox(...size,...uv,overlay?.5:0),[...size,...uv,overlay]);
 useEffect(()=>()=>geometry.dispose(),[geometry]);
 return <mesh geometry={geometry}><meshStandardMaterial map={texture} roughness={1} alphaTest={overlay?.1:0} /></mesh>;
}
export function ImmersiveAvatar({motion}:{motion:RefObject<{speed:number;flying:boolean}>}){
 const skin=useAvatarSkin(),[texture,setTexture]=useState<Texture|null>(null),invalidate=useThree(s=>s.invalidate);
 const limbs=useRef<(Group|null)[]>([]),phase=useRef(0),amount=useRef(0);
 useEffect(()=>{let live=true;const loaded=new TextureLoader().load(skin.url,value=>{
  value.magFilter=value.minFilter=NearestFilter;value.generateMipmaps=false;value.colorSpace=SRGBColorSpace;
  if(live){setTexture(value);invalidate();}
 });return()=>{live=false;loaded.dispose()};},[skin.url,invalidate]);
 useFrame((_,delta)=>{
  const {speed,flying}=motion.current!,dt=Math.min(delta,.05);
  amount.current=MathUtils.damp(amount.current,flying?0:Math.min(1,speed/.8),12,dt);phase.current+=dt*speed*9;
  const stride=Math.sin(phase.current)*.65*amount.current;
  for(let i=0;i<4;i++){const limb=limbs.current[i];if(limb){limb.rotation.x=(i===0||i===3?1:-1)*stride;limb.rotation.z=i<2?(i===0?1:-1)*(flying?.22:0):0;}}
  if(speed>.001||amount.current>.001)invalidate();
 });
 if(!texture)return null;
 const arm=skin.slim?3:4;
 return <group scale={.02} position={[0,-.6,0]}>
  <group position={[0,28,0]}><Part size={[8,8,8]} uv={[0,0]} texture={texture}/><Part size={[8,8,8]} uv={[32,0]} texture={texture} overlay/></group>
  <group position={[0,18,0]}><Part size={[8,12,4]} uv={[16,16]} texture={texture}/><Part size={[8,12,4]} uv={[16,32]} texture={texture} overlay/></group>
  {[-1,1].map((side,i)=><group key={side} ref={g=>{limbs.current[i]=g}} position={[side*(4+arm/2),24,0]}>
   <group position={[0,-6,0]}><Part size={[arm,12,4]} uv={i===0?[40,16]:[32,48]} texture={texture}/><Part size={[arm,12,4]} uv={i===0?[40,32]:[48,48]} texture={texture} overlay/></group>
  </group>)}
  {[-1,1].map((side,i)=><group key={side} ref={g=>{limbs.current[i+2]=g}} position={[side*2,12,0]}>
   <group position={[0,-6,0]}><Part size={[4,12,4]} uv={i===0?[0,16]:[16,48]} texture={texture}/><Part size={[4,12,4]} uv={i===0?[0,32]:[0,48]} texture={texture} overlay/></group>
  </group>)}
 </group>;
}
