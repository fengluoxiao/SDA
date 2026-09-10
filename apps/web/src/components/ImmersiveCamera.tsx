import { useEffect, useRef } from "react";
import { useFrame, useThree } from "@react-three/fiber";
import { Euler, Group, PerspectiveCamera, Vector3 } from "three";
import {ImmersiveAvatar} from "./ImmersiveAvatar";
const UP=new Vector3(0,1,0);

export function immersiveInputTarget(target: EventTarget | null) {
  return target instanceof HTMLElement && !!target.closest('input,textarea,select,[contenteditable="true"],[role="textbox"],[role="combobox"],[role="slider"]');
}

/** Visual navigation only: walking never changes the audio listener or head tracking. */
export function ImmersiveCamera({thirdPerson,onFlightChange}:{thirdPerson:boolean;onFlightChange:(flying:boolean)=>void}) {
  const { camera, gl, invalidate } = useThree();
  const avatar=useRef<Group>(null);
  const motion=useRef({speed:0,flying:false});
  const state=useRef({position:new Vector3(),velocity:new Vector3(),rotation:new Euler(0,0,0,"YXZ"),keys:new Set<string>(),flying:false,lookX:0,lookY:0});
  const third=useRef(thirdPerson);
  useEffect(()=>{third.current=thirdPerson;invalidate();},[thirdPerson,invalidate]);
  useEffect(()=>{
    const saved={position:camera.position.clone(),quaternion:camera.quaternion.clone(),zoom:camera.zoom,fov:(camera as PerspectiveCamera).fov};
    const s=state.current; s.position.set(0,0,0);s.velocity.set(0,0,0);s.rotation.set(0,0,0);s.keys.clear();s.flying=false;onFlightChange(false);
    camera.zoom=1;(camera as PerspectiveCamera).fov=85;camera.updateProjectionMatrix();invalidate();
    const canvas=gl.domElement,previousTouchAction=canvas.style.touchAction;
    canvas.style.touchAction="none";
    let pointer:number|null=null,x=0,y=0,lastSpace=-Infinity,skipLockedMove=false;
    const canLock=()=>!!document.fullscreenElement?.contains(canvas);
    const clear=()=>{s.keys.clear();s.velocity.set(0,0,0);s.lookX=0;s.lookY=0;lastSpace=-Infinity;if(pointer!==null&&canvas.hasPointerCapture(pointer))canvas.releasePointerCapture(pointer);pointer=null;};
    const keydown=(e:KeyboardEvent)=>{
      if(e.code==="Escape"){if(document.pointerLockElement===canvas)document.exitPointerLock();clear();return;}
      if(e.code==="F6"){
        e.preventDefault();
        if(!immersiveInputTarget(e.target)){clear();s.position.set(0,0,0);invalidate();}
        return;
      }
      if(immersiveInputTarget(e.target)||e.altKey||e.metaKey)return;
      if(!["KeyW","KeyA","KeyS","KeyD","Space","ControlLeft","ControlRight","ShiftLeft","ShiftRight"].includes(e.code))return;
      e.preventDefault();s.keys.add(e.code);
      if(e.code==="Space"&&!e.repeat){
        const now=performance.now();
        if(now-lastSpace<300){s.flying=!s.flying;lastSpace=-Infinity;onFlightChange(s.flying);}else lastSpace=now;
      }
      invalidate();
    };
    const keyup=(e:KeyboardEvent)=>{s.keys.delete(e.code);};
    // Consume relative input once per rendered frame. Remote cursor warps and
    // batched high-rate input must not produce a multi-turn jump or a backlog.
    const rotate=(dx:number,dy:number)=>{if(!Number.isFinite(dx)||!Number.isFinite(dy))return;s.lookX+=dx;s.lookY+=dy;invalidate();};
    const down=(e:PointerEvent)=>{
      if(e.button!==0)return;
      if(e.pointerType==="mouse"&&canLock()&&document.pointerLockElement!==canvas){
        try{const request=canvas.requestPointerLock();request?.catch(()=>{});}catch{/* Drag remains available if the browser denies lock. */}
      }
      pointer=e.pointerId;x=e.clientX;y=e.clientY;
      if(document.pointerLockElement!==canvas)canvas.setPointerCapture(pointer);
    };
    const move=(e:PointerEvent)=>{
      if(document.pointerLockElement===canvas){if(skipLockedMove){skipLockedMove=false;return;}rotate(e.movementX,e.movementY);return;}
      if(pointer===e.pointerId){
        if(e.pointerType==="mouse"&&!(e.buttons&1)){up(e);return;}
        rotate(e.clientX-x,e.clientY-y);x=e.clientX;y=e.clientY;
      }
    };
    const up=(e:PointerEvent)=>{if(pointer!==e.pointerId)return;if(canvas.hasPointerCapture(e.pointerId))canvas.releasePointerCapture(e.pointerId);pointer=null;};
    const release=()=>{clear();if(document.pointerLockElement===canvas)document.exitPointerLock();};
    const lockChange=()=>{clear();skipLockedMove=document.pointerLockElement===canvas;if(skipLockedMove&&!canLock())document.exitPointerLock();};
    const visibility=()=>{if(document.hidden)release();};
    const focus=(e:FocusEvent)=>{if(immersiveInputTarget(e.target))clear();};
    window.addEventListener("keydown",keydown);window.addEventListener("keyup",keyup);window.addEventListener("blur",release);
    document.addEventListener("visibilitychange",visibility);document.addEventListener("focusin",focus);document.addEventListener("pointerlockchange",lockChange);document.addEventListener("fullscreenchange",release);
    canvas.addEventListener("pointerdown",down);canvas.addEventListener("pointermove",move);canvas.addEventListener("pointerup",up);canvas.addEventListener("pointercancel",up);
    canvas.addEventListener("lostpointercapture",up);
    return ()=>{
      window.removeEventListener("keydown",keydown);window.removeEventListener("keyup",keyup);window.removeEventListener("blur",release);
      document.removeEventListener("visibilitychange",visibility);document.removeEventListener("focusin",focus);document.removeEventListener("pointerlockchange",lockChange);document.removeEventListener("fullscreenchange",release);
      canvas.removeEventListener("pointerdown",down);canvas.removeEventListener("pointermove",move);canvas.removeEventListener("pointerup",up);canvas.removeEventListener("pointercancel",up);
      canvas.removeEventListener("lostpointercapture",up);
      if(pointer!==null&&canvas.hasPointerCapture(pointer))canvas.releasePointerCapture(pointer);
      if(document.pointerLockElement===canvas)document.exitPointerLock();
      clear();canvas.style.touchAction=previousTouchAction;
      camera.position.copy(saved.position);camera.quaternion.copy(saved.quaternion);camera.zoom=saved.zoom;(camera as PerspectiveCamera).fov=saved.fov;camera.updateProjectionMatrix();invalidate();
    };
  },[camera,gl,invalidate,onFlightChange]);
  const direction=useRef(new Vector3()),offset=useRef(new Vector3());
  useFrame((_,delta)=>{
    const s=state.current,dt=Math.min(delta,.05),keys=s.keys;
    const maxTurn=8*Math.min(dt,1/30);
    const yaw=-s.lookX*.003,pitch=-s.lookY*.003;
    const turnScale=Math.min(1,maxTurn/Math.max(1e-9,Math.hypot(yaw,pitch)));
    s.rotation.y=(s.rotation.y+yaw*turnScale)%(Math.PI*2);
    s.rotation.x=Math.max(-1.48,Math.min(1.48,s.rotation.x+pitch*turnScale));
    s.lookX=0;s.lookY=0;
    const x=Number(keys.has("KeyD"))-Number(keys.has("KeyA")),z=Number(keys.has("KeyS"))-Number(keys.has("KeyW"));
    const y=s.flying?Number(keys.has("Space"))-Number(keys.has("ControlLeft")||keys.has("ControlRight")):0;
    const speed=(keys.has("ShiftLeft")||keys.has("ShiftRight"))?1.8:.9;
    direction.current.set(x,y,z);if(direction.current.lengthSq()>1)direction.current.normalize();
    direction.current.applyAxisAngle(UP,s.rotation.y).multiplyScalar(speed);
    s.velocity.lerp(direction.current,1-Math.exp(-dt*14));s.position.addScaledVector(s.velocity,dt);
    s.position.x=Math.max(-1.8,Math.min(1.8,s.position.x));s.position.z=Math.max(-1.8,Math.min(1.8,s.position.z));
    s.position.y=s.flying?Math.max(0,Math.min(1.7,s.position.y)):Math.max(0,s.position.y-dt*1.8);
    camera.quaternion.setFromEuler(s.rotation);
    camera.position.copy(s.position);
    if(third.current){
      // Shorten the orbit arm along its own ray. Clamping XYZ independently
      // and calling lookAt bent the viewing direction at walls and ceilings.
      camera.position.y+=.02;
      offset.current.set(0,0,1.05).applyEuler(s.rotation);
      let arm=1;
      for(const axis of ['x','y','z'] as const){
        const component=offset.current[axis];
        if(Math.abs(component)<1e-8)continue;
        const boundary=component>0?1.92:axis==='y'?-.35:-1.92;
        arm=Math.min(arm,(boundary-camera.position[axis])/component);
      }
      camera.position.addScaledVector(offset.current,Math.max(0,arm));
    }
    if(avatar.current){avatar.current.position.copy(s.position);avatar.current.rotation.y=s.rotation.y;}
    motion.current.speed=Math.hypot(s.velocity.x,s.velocity.z);motion.current.flying=s.flying;
    if(keys.size||s.velocity.lengthSq()>1e-6||(!s.flying&&s.position.y>0))invalidate();
  });
  return <group ref={avatar} visible={thirdPerson}>
    <ImmersiveAvatar motion={motion}/>
  </group>;
}
