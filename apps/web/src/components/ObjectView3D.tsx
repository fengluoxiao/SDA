import { PALETTE, Room, SphericalRoom, Listener, GenelecSpeaker, GenelecSub, type Palette } from "../../../../packages/renderer/src/scene-models";
import Performance3D from "./Performance3D";
import {spatialRenderer, spatialShadows} from '../spatial-renderer';
import AvatarSkinControl from "./AvatarSkinControl";
import { type HrtfTestVisual, testVisualPosition } from "../phrtf";
/**
 * Live 3D object visualization — the spiritual successor to Omniphony
 * Studio's OSC view: every audio object is a glowing dot moving through a
 * top/front wireframe room, coloured by height. Speaker ring shows the
 * 7.1.4 virtual layout used by the renderer.
 */

import { createContext, memo, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Canvas, useFrame, useThree } from "@react-three/fiber";
import { Html, OrbitControls } from "@react-three/drei";
import * as THREE from "three";
import { ImmersiveCamera, immersiveInputTarget, type ImmersiveView } from "./ImmersiveCamera";
import { Maximize, Minimize, PersonStanding, Plane, Eye } from "lucide-react";
import { admToScenePosition, SCENE_CEILING_Y, SCENE_FLOOR_Y, SCENE_ROOM_HALF_EXTENT, SCENE_WALL_HEIGHT, SCENE_WALL_MID_Y, smoothScenePosition, speakerScenePosition } from "../../../../packages/renderer/src/scene-coordinates";
import { sphericalToWebAudio } from "../../../../packages/renderer/src/coords";
import type { VirtualSpeaker } from "@sda/renderer";
import type { VisualObject } from "@sda/player";
import { speakerLabel } from "../speaker-labels";
export type { VisualObject };

const ROOM = SCENE_ROOM_HALF_EXTENT; // half-extent of the room footprint in scene units
const FLOOR_Y = SCENE_FLOOR_Y;
const CEIL_Y = SCENE_CEILING_Y;
const WALL_H = SCENE_WALL_HEIGHT;
const WALL_MID_Y = SCENE_WALL_MID_Y;

export type Theme = "dark" | "light";

/** 房间配色：深色 / 浅色两套 */
type RequestFrame = () => void;
const RequestFrameContext = createContext<RequestFrame>(() => {});

function ViewportFraming({mobile=false}:{mobile?:boolean}) {
  const { camera, size, invalidate } = useThree();
  useEffect(() => {
    camera.zoom = Math.min(1, size.width / Math.max(1, size.height) / (mobile ? .85 : 1.5));
    camera.updateProjectionMatrix();
    invalidate();
  }, [camera, size.width, size.height, invalidate, mobile]);
  return null;
}

function ObjectListRefresh({ objects }: { objects: readonly VisualObject[] }) {
  const requestFrame = useContext(RequestFrameContext);
  // Removing the last dot also removes its useFrame callback. Repaint the
  // cleared scene explicitly, including while playback/camera are paused.
  useEffect(() => requestFrame(), [objects, requestFrame]);
  return null;
}

function FrameScheduler({ children, maxFps }: { children: ReactNode; maxFps: number | null }) {
  const invalidate = useThree((state) => state.invalidate);
  const handle = useRef(0);
  const lastPaintAt = useRef(0);
  const requestFrame = useCallback(() => {
    if (maxFps === null) {
      invalidate();
      return;
    }
    if (handle.current !== 0) return;
    // requestAnimationFrame fires on vsync boundaries; the previous
    // setTimeout cap painted between vsyncs (33/50 ms alternation), which
    // read as judder on slow-moving objects. Overshoot waits for the next
    // vsync instead of a timer remainder, so paints stay display-aligned.
    const tick = (now: number) => {
      handle.current = 0;
      if (now - lastPaintAt.current >= 1000 / maxFps) {
        lastPaintAt.current = now;
        invalidate();
        return;
      }
      handle.current = requestAnimationFrame(tick);
    };
    handle.current = requestAnimationFrame(tick);
  }, [invalidate, maxFps]);
  useEffect(() => () => cancelAnimationFrame(handle.current), []);
  return <RequestFrameContext.Provider value={requestFrame}>{children}</RequestFrameContext.Provider>;
}

/** 仿真力 The Ones 同轴音箱：圆角箱体 + 大椭圆波导 + 中央同轴单元 + Iso-Pod 支架。
 *  局部 +z 为正面（朝向听者）。 */
function FocusSpeaker({ speaker, dimmed, focused, onFocus, interactive = true }: {
  interactive?: boolean;
  speaker: { name: string; isLfe?: boolean; position: THREE.Vector3; quaternion: THREE.Quaternion };
  dimmed: boolean;
  focused: boolean;
  onFocus?: (name: string) => void;
}) {
  const group = useRef<THREE.Group>(null);
  const [hovered, setHovered] = useState(false);
  const { gl, invalidate } = useThree();
  useEffect(() => {
    group.current?.traverse(object => {
      if (!(object instanceof THREE.Mesh) || object.userData.focusHitbox) return;
      const materials = Array.isArray(object.material) ? object.material : [object.material];
      for (const material of materials) {
        material.transparent = dimmed;
        material.opacity = dimmed ? 0.22 : 1;
        material.depthWrite = !dimmed;
        material.needsUpdate = true;
      }
    });
    invalidate();
  }, [dimmed, invalidate]);
  useEffect(() => {
    if (!interactive || !hovered) return;
    gl.domElement.style.cursor = onFocus ? "pointer" : "not-allowed";
    return () => { gl.domElement.style.cursor = ""; };
  }, [hovered, gl, onFocus, interactive]);
  return <group ref={group} name={`speaker:${speaker.name}`} position={speaker.position} quaternion={speaker.quaternion}
    userData={{ speakerName: speaker.name, focused, dimmed }}
    onClick={interactive ? event => {
      if (event.delta > 4) return;
      event.stopPropagation();
      onFocus?.(speaker.name);
    } : undefined}
    onPointerOver={interactive ? event => { event.stopPropagation(); setHovered(true); } : undefined}
    onPointerOut={interactive ? () => setHovered(false) : undefined}>
    {speaker.isLfe ? <GenelecSub /> : <GenelecSpeaker />}
    {interactive && <mesh userData={{ focusHitbox: true }}>
      <boxGeometry args={speaker.isLfe ? [0.3, 0.3, 0.27] : [0.22, 0.28, 0.2]} />
      <meshBasicMaterial transparent opacity={0} depthWrite={false} />
    </mesh>}
    {interactive && hovered && <Html position={[0, 0.23, 0]} center style={{ pointerEvents: "none" }}>
      <span className="speaker-tooltip">{speakerLabel(speaker.name)} · {!onFocus ? "请先取消声道静音和独奏" : focused ? "取消聚焦" : "聚焦"}</span>
    </Html>}
  </group>;
}

const SpeakerRing = memo(function SpeakerRing({ layout, focusedSpeakers, onSpeakerFocus, hiddenSpeakerNames, interactive = true }: {
  interactive?: boolean;
  layout: readonly VirtualSpeaker[];
  focusedSpeakers?: ReadonlySet<string>;
  onSpeakerFocus?: (name: string) => void;
  hiddenSpeakerNames?: ReadonlySet<string>;
  testVisual?: HrtfTestVisual|null;
}) {
  const speakers = useMemo(
    () =>
      layout.map((s) => {
        const position = new THREE.Vector3(...speakerScenePosition(s));
        const dummy = new THREE.Object3D();
        dummy.position.copy(position);
        if (s.isLfe) dummy.lookAt(0, FLOOR_Y + 0.13, 0);
        else dummy.lookAt(0, 0, 0);
        return { name: s.name, isLfe: s.isLfe, position, quaternion: dummy.quaternion.clone() };
      }),
    [layout],
  );
  return (
    <group>
      {speakers.filter(s => !hiddenSpeakerNames?.has(s.name)).map((s) => (
        <FocusSpeaker key={s.name} interactive={interactive} speaker={s} dimmed={!!focusedSpeakers?.size && !focusedSpeakers.has(s.name)}
          focused={focusedSpeakers?.has(s.name) ?? false} onFocus={onSpeakerFocus} />
      ))}
    </group>
  );
});

/** 听者：仿纽曼 KU 100 人头麦 —— 光滑无五官的蛋形头、两侧硅胶耳廓、
 *  平直颈部切口 + 话筒立杆。耳廓中心对齐 y=0（ADM 耳位）。 */
function ObjectName({id,theme}:{id:number;theme:Theme}) {
  const texture=useMemo(()=>{
    const canvas=document.createElement("canvas");canvas.width=256;canvas.height=64;
    const ctx=canvas.getContext("2d")!;
    ctx.fillStyle=theme==="light"?"rgba(255,255,255,.92)":"rgba(24,28,30,.9)";
    ctx.beginPath();ctx.roundRect(2,2,252,60,16);ctx.fill();
    ctx.fillStyle=theme==="light"?"#18332a":"#f0f5f2";
    ctx.font="500 28px sans-serif";ctx.textAlign="center";ctx.textBaseline="middle";
    ctx.fillText(`对象 ${id}`,128,33);
    const value=new THREE.CanvasTexture(canvas);value.colorSpace=THREE.SRGBColorSpace;return value;
  },[id,theme]);
  useEffect(()=>()=>texture.dispose(),[texture]);
  return <sprite position={[0,.18,0]} scale={[.64,.16,1]} renderOrder={13}><spriteMaterial map={texture} transparent depthTest={false} depthWrite={false} toneMapped={false}/></sprite>;
}

const ObjectDot = memo(function ObjectDot({
  obj,
  showName=false,
  muted,
  sounding,
  theme,
}: {
  showName?:boolean;
  obj: VisualObject;
  muted: boolean;
  sounding: boolean;
  theme: Theme;
}) {
  const ref = useRef<THREE.Group>(null);
  const initialPosition = useMemo(() => admToScenePosition(obj.pos), []);
  const target = useMemo(() => new THREE.Vector3(), []);
  const requestFrame = useContext(RequestFrameContext);
  useEffect(() => requestFrame(), [requestFrame, obj.pos[0], obj.pos[1], obj.pos[2]]);
  useFrame((_, dt) => {
    if (!ref.current) return;
    // Smooth toward the latest event position (renderer ramps audio; we ease
    // the view). The exponential factor is frame-rate independent — the old
    // clamped linear factor reached 1 after frame gaps and snapped the dot to
    // the target instead of easing.
    target.set(...admToScenePosition(obj.pos));
    const next = smoothScenePosition(ref.current.position.toArray() as [number, number, number], target.toArray() as [number, number, number], dt);
    ref.current.position.set(...next);
    if (ref.current.position.distanceToSquared(target) > 1e-8) requestFrame();
  });
  const height = obj.pos[2]; // ADM z = up
  const color = useMemo(
    () => theme === "light"
      ? new THREE.Color().setHSL(0.55 - height * 0.25, 0.78, 0.32, THREE.SRGBColorSpace)
      : new THREE.Color().setHSL(0.55 - height * 0.25, 0.9, 0.6),
    [height, theme],
  );
  // ADM size[0]（宽度 0..1）→ 半透明扩散光晕半径
  const spread = Math.min(1, Math.max(0, obj.size?.[0] ?? 0));
  // 静音对象：调暗（保留轮廓可辨识位置，区别于有声对象）
  const dotOpacity = muted ? (theme === "light" ? 0.35 : 0.18) : 1;
  return (
    <group ref={ref} position={initialPosition} renderOrder={10}>
      {showName&&<ObjectName id={obj.id} theme={theme}/>}
      {/* 尺寸光晕是叠加层：始终画在房间墙和网格之上。 */}
      <mesh renderOrder={10}>
        <sphereGeometry args={[(0.09 + spread * 0.3) * (sounding ? 1.12 : 1), 12, 12]} />
        <meshBasicMaterial color={color} transparent opacity={muted ? 0.03 : sounding ? 0.18 : 0.1} depthTest={false} depthWrite={false} />
      </mesh>
      {theme === "light" && <mesh renderOrder={11}>
        <sphereGeometry args={[0.069, 12, 12]} />
        <meshBasicMaterial color="#173d40" transparent opacity={muted ? 0.3 : 0.9} toneMapped={false} depthTest={false} depthWrite={false} />
      </mesh>}
      <mesh renderOrder={12}>
        <sphereGeometry args={[0.06, 10, 10]} />
        <meshBasicMaterial color={color} transparent opacity={dotOpacity} toneMapped={theme !== "light"} depthTest={false} depthWrite={false} />
      </mesh>
    </group>
  );
}, (prev, next) => {
  const a = prev.obj;
  const b = next.obj;
  return prev.showName === next.showName && prev.muted === next.muted
    && prev.theme === next.theme
    && prev.sounding === next.sounding
    && a.id === b.id
    && a.pos[0] === b.pos[0]
    && a.pos[1] === b.pos[1]
    && a.pos[2] === b.pos[2]
    && a.size[0] === b.size[0]
    && a.size[1] === b.size[1]
    && a.size[2] === b.size[2]
    && a.gainDb === b.gainDb;
});

function HrtfTestMarker({visual,layout}:{visual:HrtfTestVisual;layout:readonly VirtualSpeaker[]}) {
 const ref=useRef<THREE.Group>(null),requestFrame=useContext(RequestFrameContext);
 useEffect(()=>{requestFrame();},[visual,requestFrame]);
 useFrame(()=>{
  if(!ref.current)return;
  const t=visual.elapsed();ref.current.visible=t>=0&&t<=visual.duration;
  const [x,y,z]=testVisualPosition(visual.trial,t/visual.duration);
  ref.current.position.set(x*ROOM,y*ROOM,z*ROOM);ref.current.lookAt(0,0,0);
  if(t<visual.duration)requestFrame();
 });
 const moving=visual.trial.kind==="motion";
 const position=testVisualPosition(visual.trial,0);
 const existing=!moving&&layout.find(s=>!s.isLfe&&sphericalToWebAudio(s).every((v,i)=>Math.abs(v-position[i]!)<.001));
 return <group ref={ref} visible={false}>
  {moving?<mesh><sphereGeometry args={[.085,24,16]}/><meshBasicMaterial color="#ffb020"/></mesh>:existing?null:<GenelecSpeaker/>}
  <mesh><sphereGeometry args={[moving?.14:.25,24,16]}/><meshBasicMaterial color="#ffb020" transparent opacity={.22} depthWrite={false}/></mesh>
  <Html center position={[0,.32,0]} style={{pointerEvents:"none",whiteSpace:"nowrap",color:"#fff",background:"#684200",border:"1px solid #ffb020",borderRadius:8,padding:"4px 8px",fontSize:12}}>{moving?"测试 OBJ":existing?`${existing.name} · 测试中`:"测试音箱"}</Html>
 </group>;
}

export function ObjectView({
  spherical = false,
  immersive = false,
  mobile = false,
  showObjectNames = false,
  objects,
  layout,
  theme = "dark",
  mutedIds,
  soundingIds,
  focusedSpeakers,
  onSpeakerFocus,
  hiddenSpeakerNames,
  testVisual,
}: {
  spherical?: boolean;
  immersive?: boolean;
  mobile?: boolean;
  showObjectNames?:boolean;
  objects: VisualObject[];
  layout: readonly VirtualSpeaker[];
  theme?: Theme;
  /** 被静音对象 id —— 在 3D 视图里调暗。 */
  mutedIds?: ReadonlySet<number>;
  /** Worklet-confirmed post-gain/post-mute object signal IDs. */
  soundingIds?: ReadonlySet<number>;
  focusedSpeakers?: ReadonlySet<string>;
  onSpeakerFocus?: (name: string) => void;
  hiddenSpeakerNames?: ReadonlySet<string>;
  testVisual?: HrtfTestVisual|null;
}) {
  const p = PALETTE[theme];
  const shell=useRef<HTMLDivElement>(null);
  const [view,setView]=useState<ImmersiveView>("first"),[flying,setFlying]=useState(false);
  const [fullscreen,setFullscreen]=useState(false),[navigationError,setNavigationError]=useState("");
  useEffect(()=>{
    if(!immersive)return;
    const host=shell.current;
    const key=(e:KeyboardEvent)=>{if(e.code!=="F5")return;e.preventDefault();e.stopPropagation();if(!e.repeat&&!immersiveInputTarget(e.target))setView(v=>v==="first"?"second":v==="second"?"third":"first");};
    const change=()=>setFullscreen(document.fullscreenElement===shell.current);
    change();
    window.addEventListener("keydown",key,true);document.addEventListener("fullscreenchange",change);
    return()=>{window.removeEventListener("keydown",key,true);document.removeEventListener("fullscreenchange",change);if(host&&document.fullscreenElement===host)void document.exitFullscreen().catch(()=>{});};
  },[immersive]);
  const toggleFullscreen=async()=>{try{setNavigationError("");if(document.fullscreenElement===shell.current)await document.exitFullscreen();else await shell.current?.requestFullscreen();}catch{setNavigationError("无法进入全屏，请重试。");}};
  const rendererMode = window.sdaDesktop?.rendererMode;
  const isSwiftShader = mobile || rendererMode === "swiftshader";
  const createRenderer=useMemo(()=>spatialRenderer(rendererMode==='swiftshader',isSwiftShader),[rendererMode,isSwiftShader]);
  return (
    <div data-field-shape={spherical ? "sphere" : "room"} ref={shell} className={`object-scene${immersive?" is-immersive":""}`} style={{background:p.bg}}>
    <Canvas
      shadows={spatialShadows}
      frameloop="demand"
      camera={{ position: [5, 4.2, 6], fov: 50 }}
      style={{ background: p.bg }}
      // SwiftShader is software rasterization: render one device pixel per CSS
      // pixel and skip MSAA to avoid multiplying the fill cost.
      dpr={isSwiftShader ? 1 : [1, 1.5]}
      gl={createRenderer}
    >
      {/* Object motion paints on the same vsync path as camera drags (drei's
          controls invalidate at full rate regardless). Only SwiftShader —
          software rasterization competing with the audio renderer for CPU —
          keeps a 30 fps display-aligned cap. */}
      <FrameScheduler maxFps={isSwiftShader ? 30 : null}>
        <ObjectListRefresh objects={objects} />
        {testVisual&&<HrtfTestMarker visual={testVisual} layout={layout.filter(s=>!hiddenSpeakerNames?.has(s.name))}/>}
        <Performance3D />
        {immersive ? <ImmersiveCamera view={view} onFlightChange={setFlying}/> : <ViewportFraming mobile={mobile} />}
        {!immersive && (spherical ? <SphericalRoom p={p} /> : <Room p={p} />)}
        <SpeakerRing interactive={!mobile} layout={layout} focusedSpeakers={focusedSpeakers} onSpeakerFocus={immersive?undefined:onSpeakerFocus} hiddenSpeakerNames={hiddenSpeakerNames} />
        {!immersive && <Listener />}
        {objects.map((o) => (
          <ObjectDot showName={showObjectNames} key={o.id} obj={o} theme={theme} muted={mutedIds?.has(o.id) ?? false} sounding={!(mutedIds?.has(o.id) ?? false) && (soundingIds?.has(o.id) ?? false)} />
        ))}
        {!immersive && !spherical && <gridHelper args={[ROOM * 2, 10, p.gridMain, p.floorGrid]} position={[0, FLOOR_Y, 0]} />}
        {/* 听者半身像的光照 */}
        <ambientLight intensity={0.75} />
        <directionalLight position={[2.5, 4, 2]} intensity={1.2} />
        {/* 左键拖动旋转视角 / 右键拖动平移 / 滚轮缩放空间 */}
        {!immersive && <OrbitControls
          makeDefault
          target={[0, spherical ? 0 : 0.5, 0]}
          enableDamping={!isSwiftShader}
          dampingFactor={0.08}
          rotateSpeed={0.9}
          minDistance={0.5}
          maxDistance={12}
        />}
      </FrameScheduler>
    </Canvas>
    {immersive&&<div className="immersive-hud">
      <div className="immersive-toolbar">
        <AvatarSkinControl/>
        <span className={`immersive-motion${flying?" flying":""}`} role="status">{flying?<Plane size={15}/>:<PersonStanding size={15}/>} {flying?"飞行中":"步行"}</span>
        <button onClick={()=>setView(v=>v==="first"?"second":v==="second"?"third":"first")} aria-label="切换第一、第二或第三人称视角"><Eye size={15}/>{view==="first"?"第一人称":view==="second"?"第二人称":"第三人称"}<kbd>F5</kbd></button>
        <button onClick={()=>void toggleFullscreen()} aria-label={fullscreen?"退出全屏":"进入全屏"}>{fullscreen?<Minimize size={16}/>:<Maximize size={16}/>}</button>
      </div>
      <p>{fullscreen?"按住 Alt 显示鼠标 · 松开恢复转向 · Esc 释放鼠标":"按住画面拖动转向 · 松手停止"} · WASD 移动 · Shift 加速</p>
      <p>空格跳跃 · 双击空格切换飞行 · F6 返回原点{flying?" · 空格上升 · Ctrl 下降":""}</p>
      {navigationError&&<p role="alert">{navigationError}</p>}
    </div>}
    </div>
  );
}

