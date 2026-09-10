import { Suspense, useEffect, useRef } from "react";
import { useFrame, useThree } from "@react-three/fiber";
import { Box3, Euler, Group, Mesh, PerspectiveCamera, Vector3 } from "three";
import {ImmersiveAvatar} from "./ImmersiveAvatar";
const UP=new Vector3(0,1,0);

export function immersiveInputTarget(target: EventTarget | null) {
  return target instanceof HTMLElement && !!target.closest('input,textarea,select,[contenteditable="true"],[role="textbox"],[role="combobox"],[role="slider"]');
}

/** Visual navigation only: walking never changes the audio listener or head tracking. */
export type ImmersiveView = "first" | "second" | "third";

export function ImmersiveCamera({view,onFlightChange}:{view:ImmersiveView;onFlightChange:(flying:boolean)=>void}) {
  const { camera, gl, invalidate, scene } = useThree();
  const avatar=useRef<Group>(null);
  const motion=useRef({speed:0,flying:false});
  const state=useRef({position:new Vector3(),velocity:new Vector3(),rotation:new Euler(0,0,0,"YXZ"),keys:new Set<string>(),flying:false,grounded:true,verticalSpeed:0,jumpGravity:6,lookX:0,lookY:0});
  const currentView=useRef(view);
  useEffect(()=>{currentView.current=view;invalidate();},[view,invalidate]);
  useEffect(()=>{
    const saved={position:camera.position.clone(),quaternion:camera.quaternion.clone(),zoom:camera.zoom,fov:(camera as PerspectiveCamera).fov};
    const s=state.current; s.position.set(0,0,0);s.velocity.set(0,0,0);s.rotation.set(0,0,0);s.keys.clear();s.flying=false;s.grounded=true;s.verticalSpeed=0;s.jumpGravity=6;onFlightChange(false);
    camera.zoom=1;(camera as PerspectiveCamera).fov=85;camera.updateProjectionMatrix();invalidate();
    const canvas=gl.domElement,previousTouchAction=canvas.style.touchAction;
    canvas.style.touchAction="none";
    let pointer:number|null=null,x=0,y=0,lastSpace=-Infinity,skipLockedMove=false;
    const altKeys=new Set<string>();
    let resumeAfterAlt=false;
    const canLock=()=>!!document.fullscreenElement?.contains(canvas);
    const clear=()=>{s.keys.clear();s.velocity.set(0,0,0);s.lookX=0;s.lookY=0;lastSpace=-Infinity;if(pointer!==null&&canvas.hasPointerCapture(pointer))canvas.releasePointerCapture(pointer);pointer=null;};
    const requestLock=()=>{
      if(!canLock()||altKeys.size||document.pointerLockElement===canvas)return;
      try{const request=canvas.requestPointerLock();request?.catch(()=>{});}catch{/* A canvas click can retry if lock is denied. */}
    };
    const keydown=(e:KeyboardEvent)=>{
      if((e.code==="AltLeft"||e.code==="AltRight")&&canLock()){
        e.preventDefault();altKeys.add(e.code);resumeAfterAlt=true;clear();
        if(document.pointerLockElement===canvas)document.exitPointerLock();
        return;
      }
      if(e.code==="Escape"){altKeys.clear();resumeAfterAlt=false;if(document.pointerLockElement===canvas)document.exitPointerLock();clear();return;}
      if(e.code==="F6"){
        e.preventDefault();
        if(!immersiveInputTarget(e.target)){clear();s.position.set(0,0,0);s.verticalSpeed=0;s.jumpGravity=6;s.grounded=true;invalidate();}
        return;
      }
      if(immersiveInputTarget(e.target)||altKeys.size||e.altKey||e.metaKey)return;
      if(!["KeyW","KeyA","KeyS","KeyD","Space","ControlLeft","ControlRight","ShiftLeft","ShiftRight"].includes(e.code))return;
      e.preventDefault();s.keys.add(e.code);
      if(e.code==="Space"&&!e.repeat){
        const now=performance.now();
        if(now-lastSpace<300){s.flying=!s.flying;s.verticalSpeed=0;s.jumpGravity=6;s.grounded=false;lastSpace=-Infinity;onFlightChange(s.flying);}else {lastSpace=now;if(!s.flying&&s.grounded){// Cabinet jumps need enough airtime to cross between channels without a tall arc.
          const onSpeaker=s.position.y>.01;
          s.verticalSpeed=onSpeaker?1.7:2.2;s.jumpGravity=onSpeaker?2.5:6;s.grounded=false;}}
      }
      invalidate();
    };
    const keyup=(e:KeyboardEvent)=>{
      s.keys.delete(e.code);
      if(e.code!=="AltLeft"&&e.code!=="AltRight")return;
      if(!altKeys.delete(e.code))return;
      e.preventDefault();
      if(!altKeys.size&&resumeAfterAlt){
        clear();
        // Lock may still be exiting if Alt was tapped quickly. The change
        // handler retries once that asynchronous unlock has completed.
        if(document.pointerLockElement!==canvas){resumeAfterAlt=false;requestLock();}
      }
    };
    // Consume relative input once per rendered frame. Remote cursor warps and
    // batched high-rate input must not produce a multi-turn jump or a backlog.
    const rotate=(dx:number,dy:number)=>{if(!Number.isFinite(dx)||!Number.isFinite(dy))return;s.lookX+=dx;s.lookY+=dy;invalidate();};
    const down=(e:PointerEvent)=>{
      if(e.button!==0||altKeys.size)return;
      if(e.pointerType==="mouse"&&canLock()&&document.pointerLockElement!==canvas){
        requestLock();
      }
      pointer=e.pointerId;x=e.clientX;y=e.clientY;
      if(document.pointerLockElement!==canvas)canvas.setPointerCapture(pointer);
    };
    const move=(e:PointerEvent)=>{
      if(altKeys.size)return;
      if(document.pointerLockElement===canvas){if(skipLockedMove){skipLockedMove=false;return;}rotate(e.movementX,e.movementY);return;}
      if(pointer===e.pointerId){
        if(e.pointerType==="mouse"&&!(e.buttons&1)){up(e);return;}
        rotate(e.clientX-x,e.clientY-y);x=e.clientX;y=e.clientY;
      }
    };
    const up=(e:PointerEvent)=>{if(pointer!==e.pointerId)return;if(canvas.hasPointerCapture(e.pointerId))canvas.releasePointerCapture(e.pointerId);pointer=null;};
    const release=()=>{altKeys.clear();resumeAfterAlt=false;clear();if(document.pointerLockElement===canvas)document.exitPointerLock();};
    const lockChange=()=>{
      clear();skipLockedMove=document.pointerLockElement===canvas;
      if(skipLockedMove&&(!canLock()||altKeys.size)){document.exitPointerLock();return;}
      if(!skipLockedMove&&!altKeys.size&&resumeAfterAlt){resumeAfterAlt=false;requestLock();}
    };
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
  const speakerBounds=useRef(new Box3());
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
    s.velocity.lerp(direction.current,1-Math.exp(-dt*14));
    if(!s.flying)s.velocity.y=0;
    const previousY=s.position.y,previousX=s.position.x,previousZ=s.position.z;
    s.position.addScaledVector(s.velocity,dt);
    // Include every visible cabinet, including elevated and wall-mounted
    // channels. Leave room for the avatar's feet and a small jump above it.
    scene.updateMatrixWorld(true);
    const cabinets:Box3[]=[];
    const bounds=speakerBounds.current.makeEmpty();
    scene.traverseVisible(node=>{
      if(!(node instanceof Mesh)||!node.userData.immersiveCabinet)return;
      let parent=node.parent;
      while(parent&&typeof parent.userData.speakerName!=="string")parent=parent.parent;
      if(!parent)return;
      const cabinet=new Box3().setFromObject(node);cabinets.push(cabinet);bounds.union(cabinet);
    });
    const minX=Math.min(-1.95,bounds.min.x-.1),maxX=Math.max(1.95,bounds.max.x+.1);
    const minZ=Math.min(-1.95,bounds.min.z-.1),maxZ=Math.max(1.95,bounds.max.z+.1);
    const maxHeight=Math.max(1.7,bounds.max.y+.6+.45);
    s.position.x=Math.max(minX,Math.min(maxX,s.position.x));s.position.z=Math.max(minZ,Math.min(maxZ,s.position.z));
    const radius=.065;
    // A small foot footprint catches cabinet edges; the same width blocks
    // the torso at the sides instead of letting the character enter a box.
    for(const box of cabinets){
      const feet=previousY-.6;
      if(feet>=box.max.y-.002||previousY+.045<=box.min.y)continue;
      const minBX=box.min.x-radius,maxBX=box.max.x+radius,minBZ=box.min.z-radius,maxBZ=box.max.z+radius;
      if(s.position.z>minBZ&&s.position.z<maxBZ){
        if(previousX<=minBX&&s.position.x>minBX){s.position.x=minBX;s.velocity.x=0;}
        else if(previousX>=maxBX&&s.position.x<maxBX){s.position.x=maxBX;s.velocity.x=0;}
      }
      if(s.position.x>minBX&&s.position.x<maxBX){
        if(previousZ<=minBZ&&s.position.z>minBZ){s.position.z=minBZ;s.velocity.z=0;}
        else if(previousZ>=maxBZ&&s.position.z<maxBZ){s.position.z=maxBZ;s.velocity.z=0;}
      }
    }
    if(s.flying){
      s.position.y=Math.max(0,Math.min(maxHeight,s.position.y));
      s.grounded=false;
    }else{
      s.verticalSpeed-=s.jumpGravity*dt;
      s.position.y+=s.verticalSpeed*dt;
      let support=0;
      // Sweep the entire downward step against cabinet tops, never the
      // decorative driver or feet. A box approximates the rounded cabinet.
      if(s.verticalSpeed<=0){
        for(const box of cabinets){
          const top=box.max.y+.6;
          if(s.position.x>box.min.x-radius&&s.position.x<box.max.x+radius&&
             s.position.z>box.min.z-radius&&s.position.z<box.max.z+radius&&
             previousY>=top-.002&&s.position.y<=top){support=Math.max(support,top);}
        }
      }
      s.grounded=s.verticalSpeed<=0&&s.position.y<=support;
      if(s.grounded){s.position.y=support;s.verticalSpeed=0;s.jumpGravity=6;}
      if(s.position.y>maxHeight){s.position.y=maxHeight;s.verticalSpeed=Math.min(0,s.verticalSpeed);}
    }
    camera.quaternion.setFromEuler(s.rotation);
    camera.position.copy(s.position);
    if(currentView.current!=="first"){
      // Shorten the orbit arm along its own ray. Clamping XYZ independently
      // and calling lookAt bent the viewing direction at walls and ceilings.
      camera.position.y+=.02;
      offset.current.set(0,0,currentView.current==="second"?-1.05:1.05).applyEuler(s.rotation);
      let arm=1;
      for(const axis of ['x','y','z'] as const){
        const component=offset.current[axis];
        if(Math.abs(component)<1e-8)continue;
        const boundary=component>0?(axis==='y'?maxHeight+.22:axis==='x'?maxX:maxZ):(axis==='y'?-.35:axis==='x'?minX:minZ);
        arm=Math.min(arm,(boundary-camera.position[axis])/component);
      }
      camera.position.addScaledVector(offset.current,Math.max(0,arm));
      if(currentView.current==="second")camera.quaternion.setFromEuler(new Euler(-s.rotation.x,s.rotation.y+Math.PI,0,"YXZ"));
    }
    if(avatar.current){avatar.current.position.copy(s.position);avatar.current.rotation.y=s.rotation.y;}
    motion.current.speed=Math.hypot(s.velocity.x,s.velocity.z);motion.current.flying=s.flying||!s.grounded;
    if(keys.size||s.velocity.lengthSq()>1e-6||(!s.flying&&!s.grounded))invalidate();
  });
  return <group ref={avatar} visible={view!=="first"}>
    <Suspense fallback={null}>{view!=="first"&&<ImmersiveAvatar motion={motion}/>}</Suspense>
  </group>;
}
