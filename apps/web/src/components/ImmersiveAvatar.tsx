import {useRef, type RefObject} from "react";
import {useFrame} from "@react-three/fiber";
import {RoundedBox} from "@react-three/drei";
import {Group} from "three";

/** Lightweight studio explorer, built locally without textures or downloaded assets. */
export function ImmersiveAvatar({motion}:{motion:RefObject<{speed:number;flying:boolean}>}){
  const body=useRef<Group>(null),leftArm=useRef<Group>(null),rightArm=useRef<Group>(null);
  const leftLeg=useRef<Group>(null),rightLeg=useRef<Group>(null),phase=useRef(0);
  useFrame((_,delta)=>{
    const {speed,flying}=motion.current!;
    const dt=Math.min(delta,.05),amount=Math.min(1,speed/.9);
    phase.current+=dt*speed*10;
    const stride=flying?0:Math.sin(phase.current)*.48*amount;
    if(body.current)body.current.position.y=flying?0:Math.abs(Math.sin(phase.current))*.009*amount;
    for(const [limb,angle] of [[leftArm,-stride],[rightArm,stride],[leftLeg,stride],[rightLeg,-stride]] as const){
      if(limb.current)limb.current.rotation.x=angle;
    }
    if(leftArm.current)leftArm.current.rotation.z=flying?-.25:-.08;
    if(rightArm.current)rightArm.current.rotation.z=flying?.25:.08;
  });
  return <group ref={body}>
    {/* Ceramic helmet, dark visor and over-ear headphones. Front is -Z. */}
    <mesh position={[0,-.067,0]} scale={[.9,1,.88]}><sphereGeometry args={[.112,24,16]}/><meshStandardMaterial color="#e6ebe9" roughness={.35} metalness={.12}/></mesh>
    <RoundedBox args={[.15,.066,.045]} radius={.021} smoothness={3} position={[0,-.063,-.084]}><meshStandardMaterial color="#172f36" roughness={.22} metalness={.35}/></RoundedBox>
    <mesh position={[0,-.08,0]}><torusGeometry args={[.118,.012,8,24,Math.PI]}/><meshStandardMaterial color="#35494b" roughness={.45}/></mesh>
    {[-1,1].map(side=><group key={side} position={[side*.108,-.076,0]}>
      <mesh rotation={[0,0,Math.PI/2]}><cylinderGeometry args={[.039,.039,.035,16]}/><meshStandardMaterial color="#30494b" roughness={.45}/></mesh>
      <mesh position={[side*.019,0,0]} rotation={[0,0,Math.PI/2]}><cylinderGeometry args={[.025,.025,.004,16]}/><meshStandardMaterial color="#68c4ad" roughness={.35} metalness={.3}/></mesh>
    </group>)}
    <mesh position={[0,-.177,0]}><cylinderGeometry args={[.039,.045,.042,12]}/><meshStandardMaterial color="#34474a"/></mesh>
    <RoundedBox args={[.21,.205,.135]} radius={.046} smoothness={3} position={[0,-.289,0]}><meshStandardMaterial color="#dde5e1" roughness={.55}/></RoundedBox>
    <RoundedBox args={[.117,.13,.047]} radius={.018} smoothness={3} position={[0,-.285,.075]}><meshStandardMaterial color="#3b5555" roughness={.5}/></RoundedBox>
    <RoundedBox args={[.064,.009,.004]} radius={.003} smoothness={2} position={[0,-.252,.100]}><meshStandardMaterial color="#75dcc2" emissive="#287761" emissiveIntensity={.4}/></RoundedBox>
    <mesh position={[0,-.39,0]} scale={[1,.65,1]}><sphereGeometry args={[.084,16,12]}/><meshStandardMaterial color="#34474a" roughness={.65}/></mesh>
    {([-1,1] as const).map(side=><group key={side} ref={side===-1?leftArm:rightArm} position={[side*.126,-.225,0]}>
      <mesh><sphereGeometry args={[.04,12,10]}/><meshStandardMaterial color="#4f7871" roughness={.55}/></mesh>
      <mesh position={[0,-.071,0]}><capsuleGeometry args={[.032,.08,4,12]}/><meshStandardMaterial color="#d6e1dc" roughness={.55}/></mesh>
      <mesh position={[0,-.145,-.003]}><capsuleGeometry args={[.028,.018,4,12]}/><meshStandardMaterial color="#354c4d" roughness={.65}/></mesh>
    </group>)}
    {([-1,1] as const).map(side=><group key={side} ref={side===-1?leftLeg:rightLeg} position={[side*.052,-.409,0]}>
      <mesh position={[0,-.067,0]}><capsuleGeometry args={[.036,.09,4,12]}/><meshStandardMaterial color="#566b69" roughness={.65}/></mesh>
      <RoundedBox args={[.079,.05,.112]} radius={.018} smoothness={3} position={[0,-.166,-.018]}><meshStandardMaterial color="#e0e7e3" roughness={.6}/></RoundedBox>
      <RoundedBox args={[.08,.012,.114]} radius={.005} smoothness={2} position={[0,-.19,-.018]}><meshStandardMaterial color="#304447" roughness={.8}/></RoundedBox>
    </group>)}
  </group>;
}
