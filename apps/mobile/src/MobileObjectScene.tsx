import { resetNativeDrawable } from "./nativeDrawable";
import React, { useMemo, useRef } from "react";
import { AppState, Platform, PanResponder, View, Text, Pressable, InteractionManager, Dimensions, type GestureResponderEvent, type PanResponderGestureState } from "react-native";
import { Canvas, useFrame, useThree } from "@react-three/fiber/native";
import * as THREE from "three";
import { PALETTE, Room, SphericalRoom, Listener, GenelecSpeaker, GenelecSub } from "../../../packages/renderer/src/scene-models";
import { rotateScene } from "./scene-gesture";
import { LAYOUTS } from "../../../packages/renderer/src/layouts";
import { admToScenePosition, SCENE_FLOOR_Y, SCENE_ROOM_HALF_EXTENT, SCENE_WALL_HEIGHT, SCENE_WALL_MID_Y, smoothScenePosition, speakerScenePosition } from "../../../packages/renderer/src/scene-coordinates";

export interface MobileObjectPoint { id: number; pos: [number, number, number]; gainDb: number }

function ObjectPoint({ object }: { object: MobileObjectPoint }) {
  const group = useRef<THREE.Group>(null);
  const target = useRef(new THREE.Vector3(...admToScenePosition(object.pos)));
  useFrame((_, dt) => {
    if (!group.current) return;
    target.current.set(...admToScenePosition(object.pos));
    group.current.position.set(...smoothScenePosition(group.current.position.toArray() as [number, number, number], target.current.toArray() as [number, number, number], dt));
  });
  const color = useMemo(() => new THREE.Color().setHSL(0.55 - object.pos[2] * 0.25, 0.9, 0.6), [object.pos[2]]);
  return <group ref={group} position={admToScenePosition(object.pos)}>
    <mesh><sphereGeometry args={[0.09, 12, 12]} /><meshBasicMaterial color={color} transparent opacity={0.1} depthTest={false} depthWrite={false} /></mesh>
    <mesh><sphereGeometry args={[0.06, 10, 10]} /><meshBasicMaterial color={color} transparent opacity={1} depthTest={false} depthWrite={false} /></mesh>
  </group>;
}

function Scene({ objects, cameraInput, layout }: { layout: "2.0" | "7.1.4" | "360RA-13"; objects: readonly MobileObjectPoint[]; cameraInput: { rotation: { x: number; y: number }; distance: number } }) {
  const camera = useThree((state) => state.camera);
  useFrame(() => {
    const spherical = new THREE.Spherical(cameraInput.distance, Math.PI / 2.9 + cameraInput.rotation.y, cameraInput.rotation.x);
    camera.position.setFromSpherical(spherical);
    camera.position.y += 0.5;
    camera.lookAt(0, 0.5, 0);
  });
  return <>
    <color attach="background" args={[PALETTE.dark.bg]} />
    <ambientLight intensity={0.75} /><directionalLight position={[2.5, 4, 2]} intensity={1.2} />
    {layout === "360RA-13" ? <SphericalRoom p={PALETTE.dark} /> : <Room p={PALETTE.dark} />}
    {layout !== "360RA-13" && <gridHelper args={[SCENE_ROOM_HALF_EXTENT * 2, 10, PALETTE.dark.gridMain, PALETTE.dark.floorGrid]} position={[0, SCENE_FLOOR_Y, 0]} />}
    {LAYOUTS[layout].map((speaker) => {
      const position = speakerScenePosition(speaker);
      const facing = new THREE.Object3D();
      facing.position.set(...position);
      facing.lookAt(0, speaker.isLfe ? SCENE_FLOOR_Y + 0.13 : 0, 0);
      return <group key={speaker.name} position={position} quaternion={facing.quaternion.toArray() as [number, number, number, number]}>
        {speaker.isLfe ? <GenelecSub /> : <GenelecSpeaker />}
      </group>;
    })}
    <Listener />
    {objects.map((object) => <ObjectPoint key={object.id} object={object} />)}
  </>;
}

class SceneBoundary extends React.Component<React.PropsWithChildren<{ onError: (message: string) => void }>, { error: string | null }> {
  state: { error: string | null } = { error: null };
  static getDerivedStateFromError(error: Error) { return { error: error.message }; }
  componentDidCatch(error: Error) { this.props.onError(error.message); }
  render() { return this.state.error ? <Text accessibilityRole="alert" style={{ color: "#ffb4a8", padding: 12 }}>空间视图加载失败：{this.state.error}</Text> : this.props.children; }
}

export function MobileObjectScene({ objects, layout, active, onInteractionChange }: { active: boolean; layout: "2.0" | "7.1.4" | "360RA-13"; objects: readonly MobileObjectPoint[]; onInteractionChange?: (active: boolean) => void }) {
  // Never keep the iOS GL animation loop running behind the lock screen.
  // Retain the scene/camera so foreground return does not rebuild its assets.
  const [foreground, setForeground] = React.useState(AppState.currentState === "active");
  React.useEffect(() => {
    if (Platform.OS !== "ios") return;
    const subscription = AppState.addEventListener("change", state => setForeground(state === "active"));
    return () => subscription.remove();
  }, []);
  const renderActive = active && (Platform.OS !== "ios" || foreground);
  const view = useRef<View>(null);
  const interaction = useRef(onInteractionChange);
  React.useEffect(() => () => interaction.current?.(false), []);
  interaction.current = onInteractionChange;
  const [viewport, setViewport] = React.useState({ width: 0, height: 0 });
  const [mounted, setMounted] = React.useState(false);
  const [attempt, setAttempt] = React.useState(0);
  const [ready, setReady] = React.useState(false);
  const [failure, setFailure] = React.useState<string | null>(null);
  const firstFrame = useRef(false);
  const alive = useRef(true);
  React.useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  const currentAttempt = useRef(attempt);
  currentAttempt.current = attempt;
  // Avoid creating the first GL surface during an offscreen pager transition.
  // Wait for a visible, measured, settled native view; watchdog failures below.
  React.useEffect(() => {
    if (!renderActive || mounted || viewport.width <= 0 || viewport.height <= 0) return;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const task = InteractionManager.runAfterInteractions(() => {
      timer = setTimeout(() => setMounted(true), 300);
    });
    return () => { task.cancel(); if (timer) clearTimeout(timer); };
  }, [renderActive, mounted, viewport.width, viewport.height]);
  React.useEffect(() => {
    if (!renderActive || !mounted || ready || failure) return;
    const timer = setTimeout(() => {
      if (firstFrame.current) return;
      if (attempt < 2) { firstFrame.current = false; setAttempt(value => value + 1); }
      else setFailure("3D 上下文未完成首帧渲染");
    }, 6000);
    return () => clearTimeout(timer);
  }, [renderActive, mounted, ready, failure, attempt]);
  const report = React.useCallback((value: Record<string, unknown>) => {
    const engine = (globalThis as any).expo?.modules?.SdaEngine;
    if (engine?.sceneSmokeEnabled?.()) view.current?.measureInWindow((x, y, width, height) => engine.reportSceneSmoke(JSON.stringify({ layout, screenWidth: Dimensions.get("window").width, ...value, bounds: { x, y, width, height } })));
  }, [layout]);
  React.useEffect(() => { if (failure) report({ ok: false, error: failure }); }, [failure, report]);
  const retry = () => { firstFrame.current = false; setReady(false); setFailure(null); setAttempt(value => value + 1); };
  const input = useRef({ rotation: { x: 0.72, y: 0.25 }, distance: 7 });
  const previousPinch = useRef(0);
  const previousDrag = useRef<{ x: number; y: number } | null>(null);
  const viewportHeight = useRef(300);
  const pan = useRef(PanResponder.create({
    onStartShouldSetPanResponder: () => true,
    onStartShouldSetPanResponderCapture: () => true,
    onPanResponderTerminationRequest: () => false,
    onShouldBlockNativeResponder: () => true,
    onMoveShouldSetPanResponder: () => true,
    onPanResponderGrant: (event: GestureResponderEvent) => {
      interaction.current?.(true);
      const touches = event.nativeEvent.touches;
      previousDrag.current = touches.length === 1 ? { x: touches[0]!.pageX, y: touches[0]!.pageY } : null;
      previousPinch.current = touches.length >= 2 ? Math.hypot(touches[0]!.pageX - touches[1]!.pageX, touches[0]!.pageY - touches[1]!.pageY) : 0;
    },
    onPanResponderMove: (event: GestureResponderEvent) => {
      const touches = event.nativeEvent.touches;
      if (touches.length >= 2) {
        previousDrag.current = null;
        const pinch = Math.hypot(touches[0]!.pageX - touches[1]!.pageX, touches[0]!.pageY - touches[1]!.pageY);
        if (previousPinch.current > 0) input.current.distance = THREE.MathUtils.clamp(input.current.distance * previousPinch.current / Math.max(1, pinch), 3.5, 12);
        previousPinch.current = pinch;
      } else if (touches.length === 1) {
        const point = { x: touches[0]!.pageX, y: touches[0]!.pageY };
        if (previousDrag.current) {
          input.current.rotation = rotateScene(input.current.rotation, point.x - previousDrag.current.x, point.y - previousDrag.current.y, viewportHeight.current);
        }
        previousDrag.current = point;
        previousPinch.current = 0;
      }
    },
    onPanResponderRelease: () => { previousDrag.current = null; previousPinch.current = 0; interaction.current?.(false); },
    onPanResponderTerminate: () => { previousDrag.current = null; previousPinch.current = 0; interaction.current?.(false); },
  })).current;
  return <View ref={view} collapsable={false} style={{ flex: 1, overflow: "hidden" }} onLayout={event => {
    const { width, height } = event.nativeEvent.layout;
    viewportHeight.current = height;
    setViewport(previous => previous.width === width && previous.height === height ? previous : { width, height });
  }} {...pan.panHandlers}>
    {mounted && viewport.width > 0 && viewport.height > 0 && <SceneBoundary key={attempt} onError={setFailure}><Canvas frameloop={Platform.OS === "ios" && !renderActive ? "never" : "always"} style={{ width: viewport.width, height: viewport.height }} camera={{ position: [5, 4.2, 6], fov: 50 }} gl={{ antialias: false, alpha: false }} onCreated={state => {
      const renderFrame = state.gl.render.bind(state.gl);
      const context = state.gl.getContext() as any;
      const smoke = (globalThis as any).expo?.modules?.SdaEngine?.sceneSmokeEnabled?.() === true;
      let frames = 0;
      let smokeReported = false;
      state.gl.render = (scene, camera) => {
        // Expo owns/presents/resizes the drawable outside Three's state cache.
        // Rebind its current default FBO rather than retaining a deleted/stale
        // drawable after the native tab has laid out or resumed.
        if (Platform.OS === "ios") {
          resetNativeDrawable(state.gl, context, state.size.width, state.size.height);
        }
        renderFrame(scene, camera);
        if (currentAttempt.current !== attempt || !alive.current) return;
        if (state.gl.info.render.calls <= 0 || state.gl.info.render.triangles <= 0) return;
        frames++;
        // Readiness is the first actual draw, not the later CI pixel sample.
        // Otherwise a slow simulator rebuilds healthy contexts before frame 30.
        if (!firstFrame.current) {
          firstFrame.current = true;
          setTimeout(() => { if (alive.current && currentAttempt.current === attempt) setReady(true); }, 0);
        }
        if (smoke && frames >= 30 && !smokeReported) {
          smokeReported = true;
          // Sample immediately after drawing, before Expo presents this buffer.
          // Do not depend on endFrameEXP being called exactly once per render.
          const width = context.drawingBufferWidth, height = context.drawingBufferHeight;
          const buffer = new Uint8Array(width * height * 4);
          context.readPixels(0, 0, width, height, context.RGBA, context.UNSIGNED_BYTE, buffer);
          let brightPixels = 0;
          for (let i = 0; i < buffer.length; i += 4) if (Math.max(buffer[i]!, buffer[i + 1]!, buffer[i + 2]!) > 70) brightPixels++;
          const result = { ok: true, brightPixels, bufferWidth: width, bufferHeight: height,
            glError: context.getError(), framebufferStatus: context.checkFramebufferStatus(context.FRAMEBUFFER),
            camera: state.camera.position.toArray(), viewport: Array.from(context.getParameter(context.VIEWPORT)),
            programs: state.gl.info.programs?.map((program: any) => program.diagnostics),
            calls: state.gl.info.render.calls, triangles: state.gl.info.render.triangles,
            width: state.size.width, height: state.size.height, attempt, frames };
          setTimeout(() => { if (alive.current && currentAttempt.current === attempt) report(result); }, 0);
        }
      };
    }}>
      <Scene layout={layout} objects={objects} cameraInput={input.current} />
    </Canvas></SceneBoundary>}
    {(!ready || failure) && <View pointerEvents={failure ? "auto" : "none"} style={{ position: "absolute", left: 0, right: 0, top: 0, bottom: 0, alignItems: "center", justifyContent: "center" }}>
      <Text style={{ color: failure ? "#ffb4a8" : "#a8adb4", padding: 12 }}>{failure || "正在初始化 3D 空间视图…"}</Text>
      {failure && <Pressable accessibilityRole="button" accessibilityLabel="重试空间视图" onPress={retry}><Text style={{ color: "#ffffff", padding: 12 }}>重试空间视图</Text></Pressable>}
    </View>}
  </View>;
}
