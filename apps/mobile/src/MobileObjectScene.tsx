import { CARDBOARD_DEFAULTS, type CardboardConfig } from "./cardboard-config";
import { CardboardRenderer, createSpaceRenderer, getSpaceRawDraw } from "./cardboard-renderer";
import { CardboardControls } from "./CardboardControls";
import { useCardboardSession } from "./use-cardboard-session";
import type { CardboardPose } from "./cardboard-pose";
import { ChannelSpeakers } from "./ChannelSpeakers";
import { MobileSceneCamera } from "./MobileSceneCamera";
import { LISTENER_VIEW_FOV, listenerViewFov, rotateListenerView, type SceneCameraInput } from "./scene-camera-input";
import { BatchedSpaceObjects } from "./BatchedSpaceObjects";
import { SceneFrameClock } from "./scene-motion";
import { useSceneDisplayPolicy } from "./scene-display";
import { ImmersiveSpaceEnvironment, useSpaceGlowTexture } from "./ImmersiveSpaceScene";
import { SpaceSceneTap } from "./space-visual-model";
import { normalizeNativeShaderLogs } from "./nativeShaderLogs";
import { resetNativeSceneDrawable } from "./nativeDrawable";
import React, { useMemo, useRef } from "react";
import { AccessibilityInfo, AppState, Platform, PanResponder, View, Text, Pressable, Dimensions, type GestureResponderEvent } from "react-native";
import { Canvas, useFrame } from "@react-three/fiber/native";
import * as THREE from "three";
import { PALETTE, Room, SphericalRoom, Listener } from "../../../packages/renderer/src/scene-models";
import { rotateScene } from "./scene-gesture";
import { SCENE_FLOOR_Y, SCENE_ROOM_HALF_EXTENT } from "../../../packages/renderer/src/scene-coordinates";

import {MobileObjectActivity, type MobileObjectPoint} from "./mobile-object-activity";
export type {MobileObjectPoint} from "./mobile-object-activity";

interface ActivityProps {
  soundingIds?: readonly number[];
  paused?: boolean;
  showAllObjects?: boolean;
  streamTimeSec?: number;
  sourceKey?: string;
  onVisibleObjectCountChange?: (count: number) => void;
}

function Scene({ objects, cameraInput, layout, soundingIds, paused = false, showAllObjects = false,
  streamTimeSec = 0, sourceKey, onVisibleObjectCountChange, immersive, accent, reduceMotion, frameRate, firstPerson, showSpeakers, cardboard, vrPose }: ActivityProps & { cardboard?: CardboardConfig; vrPose?: CardboardPose; immersive: boolean; accent: string; reduceMotion: boolean; frameRate: number; firstPerson: boolean; showSpeakers: boolean; layout: "2.0" | "7.1.4" | "360RA-13"; objects: readonly MobileObjectPoint[]; cameraInput: SceneCameraInput }) {
  const glow = useSpaceGlowTexture(immersive);
  const phase = useMemo(() => ({ current: 0 }), [sourceKey]);
  const activity = useMemo(() => new MobileObjectActivity(), [sourceKey]);
  const visibility = useMemo(() => new Map<number, number>(), []);
  const previousCount = useRef(-1);
  useFrame((_, dt) => {
    if (immersive && !paused && !reduceMotion) phase.current += Math.min(.05, Math.max(0, dt));
    const visible = activity.project(objects, soundingIds, performance.now() / 1000, streamTimeSec, paused, showAllObjects);
    visibility.clear();
    for (const object of visible) visibility.set(object.id, object.sceneOpacity ?? 1);
    if (previousCount.current !== visible.length) {
      previousCount.current = visible.length;
      onVisibleObjectCountChange?.(visible.length);
    }
  }, -1);
  const pacing = useMemo(() => new SceneFrameClock(), []);
  useFrame(state => { if (pacing.due(performance.now() / 1000, frameRate)) state.gl.render(state.scene, state.camera); }, 1);
  return <>
    <MobileSceneCamera cardboard={cardboard} vrPose={vrPose} input={cameraInput} firstPerson={firstPerson} immersive={immersive} layout={layout} />
    {immersive ? <ImmersiveSpaceEnvironment layout={layout} accent={accent} glow={glow} showListener={!firstPerson} /> : <>
    <color attach="background" args={[PALETTE.dark.bg]} />
    <ambientLight intensity={0.75} /><directionalLight position={[2.5, 4, 2]} intensity={1.2} />
    {layout === "360RA-13" ? <SphericalRoom p={PALETTE.dark} /> : <Room p={PALETTE.dark} />}
    {layout !== "360RA-13" && <gridHelper args={[SCENE_ROOM_HALF_EXTENT * 2, 10, PALETTE.dark.gridMain, PALETTE.dark.floorGrid]} position={[0, SCENE_FLOOR_Y, 0]} />}
    {!firstPerson && <Listener />}
    </>}
    <ChannelSpeakers layout={layout} visible={showSpeakers} />
    <BatchedSpaceObjects objects={objects} visibility={visibility} immersive={immersive} phase={phase} reduceMotion={reduceMotion} sourceKey={sourceKey} streamTimeSec={streamTimeSec} paused={paused} />
  </>;
}

class SceneBoundary extends React.Component<React.PropsWithChildren<{ onError: (message: string) => void }>, { error: string | null }> {
  state: { error: string | null } = { error: null };
  static getDerivedStateFromError(error: Error) { return { error: error.message }; }
  componentDidCatch(error: Error) { this.props.onError(error.message); }
  render() { return this.state.error ? <Text accessibilityRole="alert" style={{ color: "#ffb4a8", padding: 12 }}>空间视图加载失败：{this.state.error}</Text> : this.props.children; }
}

export function MobileObjectScene({ objects, layout, active, displayActive, onInteractionChange, onTap, frameRate = 60, fullscreen = false, showSpeakers = false, cardboard = CARDBOARD_DEFAULTS, onCardboardChange, onVrExit, immersive = false, accent = "#7799CC", ...activityProps }: ActivityProps & { cardboard?: CardboardConfig; onCardboardChange?: (config: CardboardConfig) => void; onVrExit?: () => void; frameRate?: number; fullscreen?: boolean; showSpeakers?: boolean; displayActive?: boolean; immersive?: boolean; accent?: string; active: boolean; layout: "2.0" | "7.1.4" | "360RA-13"; objects: readonly MobileObjectPoint[]; onInteractionChange?: (active: boolean) => void; onTap?: () => void }) {
  // Stop hidden/background GL on every platform; keep audio CPU capacity.
  // Retain the scene/camera so foreground return does not rebuild its assets.
  const [foreground, setForeground] = React.useState(AppState.currentState === "active");
  React.useEffect(() => {
    const subscription = AppState.addEventListener("change", state => setForeground(state === "active"));
    // The initial async native state may resolve between render and subscription.
    setForeground(AppState.currentState === "active");
    return () => subscription.remove();
  }, []);
  const renderActive = active && foreground;
  const effectiveFrameRate = useSceneDisplayPolicy((displayActive ?? active) && foreground, frameRate, fullscreen);
  const view = useRef<React.ComponentRef<typeof View>>(null);
  const [reduceMotion, setReduceMotion] = React.useState(false);
  React.useEffect(() => {
    let active = true;
    void AccessibilityInfo.isReduceMotionEnabled().then(value => { if (active) setReduceMotion(value); }).catch(() => {});
    const subscription = AccessibilityInfo.addEventListener("reduceMotionChanged", value => { if (active) setReduceMotion(value); });
    return () => { active = false; subscription.remove(); };
  }, []);
  const tapHandler = useRef(onTap);
  tapHandler.current = onTap;
  const tap = useRef(new SpaceSceneTap());
  const interaction = useRef(onInteractionChange);
  React.useEffect(() => () => interaction.current?.(false), []);
  interaction.current = onInteractionChange;
  const [viewport, setViewport] = React.useState({ width: 0, height: 0 });
  const vrRequested = fullscreen && cardboard.enabled;
  const vrActive = vrRequested && viewport.width > viewport.height;
  // Layout settling during the forced rotation must not cancel/restore the VR session.
  const vrSession = useCardboardSession(vrRequested && (displayActive ?? active), effectiveFrameRate, foreground);
  const [vrTools, setVrTools] = React.useState(true);
  const [vrCalibration, setVrCalibration] = React.useState(false);
  const vrRenderer = useRef<CardboardRenderer | null>(null);
  tapHandler.current = vrRequested ? () => setVrTools(value => !value) : onTap;
  const vrFrame = useRef({ active: vrActive, cardboard, calibration: vrCalibration });
  vrFrame.current = { active: vrActive, cardboard, calibration: vrCalibration };
  React.useEffect(() => { setVrTools(true); setVrCalibration(false); }, [vrRequested]);
  React.useEffect(() => {
    if (!vrActive) { vrRenderer.current?.dispose(); vrRenderer.current = null; }
  }, [vrActive]);
  React.useEffect(() => () => { vrRenderer.current?.dispose(); vrRenderer.current = null; }, []);
  const [mounted, setMounted] = React.useState(false);
  const [attempt, setAttempt] = React.useState(0);
  const [ready, setReady] = React.useState(false);
  React.useEffect(() => {
    const engine = (globalThis as any).expo?.modules?.SdaEngine;
    engine?.setSceneRenderingRequested?.(renderActive && mounted);
    return () => engine?.setSceneRenderingRequested?.(false);
  }, [renderActive, mounted]);

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
    const task = requestIdleCallback(() => {
      timer = setTimeout(() => setMounted(true), 300);
    });
    return () => { cancelIdleCallback(task); if (timer) clearTimeout(timer); };
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
  const input = useRef<SceneCameraInput>({ rotation: { x: 0.72, y: 0.25 }, distance: 7, look: { yaw: 0, pitch: 0 }, fov: LISTENER_VIEW_FOV });
  const vrGesture = useRef(vrRequested); vrGesture.current = vrRequested;
  const firstPerson = useRef(fullscreen); firstPerson.current = fullscreen;
  const previousPinch = useRef(0);
  const previousDrag = useRef<{ x: number; y: number } | null>(null);
  const viewportHeight = useRef(300);
  React.useLayoutEffect(() => {
    if (fullscreen) { input.current.look = { yaw: 0, pitch: 0 }; input.current.fov = LISTENER_VIEW_FOV; }
    previousDrag.current = null; previousPinch.current = 0; tap.current.cancel();
  }, [fullscreen]);
  const pan = useRef(PanResponder.create({
    onStartShouldSetPanResponder: () => true,
    onStartShouldSetPanResponderCapture: () => true,
    onPanResponderTerminationRequest: () => false,
    onShouldBlockNativeResponder: () => true,
    onMoveShouldSetPanResponder: () => true,
    onPanResponderGrant: (event: GestureResponderEvent) => {
      interaction.current?.(true);
      const touches = event.nativeEvent.touches;
      tap.current.begin(touches, Date.now());
      previousDrag.current = touches.length === 1 ? { x: touches[0]!.pageX, y: touches[0]!.pageY } : null;
      previousPinch.current = touches.length >= 2 ? Math.hypot(touches[0]!.pageX - touches[1]!.pageX, touches[0]!.pageY - touches[1]!.pageY) : 0;
    },
    onPanResponderStart: event => {
      const touches = event.nativeEvent.touches;
      tap.current.update(touches);
      if (touches.length >= 2) {
        previousDrag.current = null;
        previousPinch.current = Math.hypot(touches[0]!.pageX - touches[1]!.pageX, touches[0]!.pageY - touches[1]!.pageY);
      }
    },
    onPanResponderMove: (event: GestureResponderEvent) => {
      tap.current.update(event.nativeEvent.touches);
      if (vrGesture.current) return;
      const touches = event.nativeEvent.touches;
      if (touches.length >= 2) {
        previousDrag.current = null;
        const pinch = Math.hypot(touches[0]!.pageX - touches[1]!.pageX, touches[0]!.pageY - touches[1]!.pageY);
        if (previousPinch.current > 0) {
          const ratio = previousPinch.current / Math.max(1, pinch);
          if (firstPerson.current) input.current.fov = listenerViewFov(input.current.fov, ratio);
          else input.current.distance = THREE.MathUtils.clamp(input.current.distance * ratio, 3.5, 12);
        }
        previousPinch.current = pinch;
      } else if (touches.length === 1) {
        const point = { x: touches[0]!.pageX, y: touches[0]!.pageY };
        if (previousDrag.current) {
          const dx = point.x - previousDrag.current.x, dy = point.y - previousDrag.current.y;
          if (firstPerson.current) input.current.look = rotateListenerView(input.current.look, dx, dy, viewportHeight.current);
          else input.current.rotation = rotateScene(input.current.rotation, dx, dy, viewportHeight.current);
        }
        previousDrag.current = point;
        previousPinch.current = 0;
      }
    },
    onPanResponderRelease: event => {
      if (event.nativeEvent.changedTouches.length) tap.current.update(event.nativeEvent.changedTouches);
      const activate = tap.current.end(Date.now());
      previousDrag.current = null; previousPinch.current = 0; interaction.current?.(false);
      if (activate) tapHandler.current?.();
    },
    onPanResponderTerminate: () => { tap.current.cancel(); previousDrag.current = null; previousPinch.current = 0; interaction.current?.(false); },
  })).current;
  return <View style={{ flex: 1 }}><View ref={view} collapsable={false} style={{ flex: 1, overflow: "hidden" }} onLayout={event => {
    const { width, height } = event.nativeEvent.layout;
    viewportHeight.current = height;
    setViewport(previous => previous.width === width && previous.height === height ? previous : { width, height });
  }} {...pan.panHandlers}>
    {/* Let the native GL view resize with its parent immediately on resume;
        cached viewport dimensions only gate the initial mount. */}
    {mounted && viewport.width > 0 && viewport.height > 0 && <SceneBoundary key={attempt} onError={setFailure}><Canvas frameloop={renderActive ? "always" : "never"} style={{ flex: 1 }} camera={{ position: [5, 4.2, 6], fov: 50 }} gl={createSpaceRenderer} onCreated={state => {
      vrRenderer.current?.dispose(); vrRenderer.current = null;
      const renderFrame = getSpaceRawDraw(state.gl);
      const context = state.gl.getContext() as any;
      normalizeNativeShaderLogs(context);
      const smoke = (globalThis as any).expo?.modules?.SdaEngine?.sceneSmokeEnabled?.() === true;
      let frames = 0;
      let smokeReported = false;
      state.gl.render = (scene, camera) => {
        if (currentAttempt.current !== attempt || !alive.current) return;
        // Expo owns/presents/resizes the drawable outside Three's state cache.
        // Rebind its current default FBO rather than retaining a deleted/stale
        // drawable after the native tab has laid out or resumed.
        if (Platform.OS === "ios") {
          resetNativeSceneDrawable(state.get, context);
        }
        if (vrFrame.current.active) {
          vrRenderer.current ??= new CardboardRenderer();
          vrRenderer.current.render(state.gl, renderFrame, scene, camera as THREE.PerspectiveCamera, vrFrame.current.cardboard, vrFrame.current.calibration);
        } else renderFrame(scene, camera);
        // Raw offscreen eye draws never call Fiber's per-render endFrameEXP.
        // Exactly one completed screen image is presented per rendered frame.
        context.endFrameEXP?.();
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
          // Expo's presented default framebuffer is not reliably readable on
          // iOS (the screenshot can be valid while readPixels returns 1282).
          // Probe the same scene/camera through an explicitly readable target.
          const width = 256, height = 256;
          const buffer = new Uint8Array(width * height * 4);
          const target = new THREE.WebGLRenderTarget(width, height, { depthBuffer: true });
          const previousTarget = state.gl.getRenderTarget();
          let probeError = 0;
          try {
            // Separate stale default-drawable errors from probe validation.
            for (let i = 0; i < 8 && context.getError() !== context.NO_ERROR; i++) {}
            state.gl.setRenderTarget(target);
            renderFrame(scene, camera);
            state.gl.readRenderTargetPixels(target, 0, 0, width, height, buffer);
            probeError = context.getError();
          } finally {
            state.gl.setRenderTarget(previousTarget);
            target.dispose();
          }
          let brightPixels = 0;
          for (let i = 0; i < buffer.length; i += 4) if (Math.max(buffer[i]!, buffer[i + 1]!, buffer[i + 2]!) > 70) brightPixels++;
          const result = { ok: true, brightPixels, bufferWidth: width, bufferHeight: height,
            pixelProbe: "explicit-render-target", glError: probeError, framebufferStatus: context.checkFramebufferStatus(context.FRAMEBUFFER),
            camera: camera.position.toArray(), viewport: Array.from(context.getParameter(context.VIEWPORT)),
            programs: state.gl.info.programs?.map((program: any) => program.diagnostics),
            calls: state.gl.info.render.calls, triangles: state.gl.info.render.triangles,
            width: state.get().size.width, height: state.get().size.height, attempt, frames };
          setTimeout(() => { if (alive.current && currentAttempt.current === attempt) report(result); }, 0);
        }
      };
    }}>
      <Scene cardboard={vrActive ? cardboard : undefined} vrPose={vrSession.pose.current} {...activityProps} immersive={immersive} accent={accent} reduceMotion={reduceMotion} frameRate={effectiveFrameRate} firstPerson={fullscreen} showSpeakers={showSpeakers} layout={layout} objects={objects} cameraInput={input.current} />
    </Canvas></SceneBoundary>}
    {(!ready || failure) && <View pointerEvents={failure ? "auto" : "none"} style={{ position: "absolute", left: 0, right: 0, top: 0, bottom: 0, alignItems: "center", justifyContent: "center" }}>
      <Text style={{ color: failure ? "#ffb4a8" : "#a8adb4", padding: 12 }}>{failure || "正在初始化 3D 空间视图…"}</Text>
      {failure && <Pressable accessibilityRole="button" accessibilityLabel="重试空间视图" onPress={retry}><Text style={{ color: "#ffffff", padding: 12 }}>重试空间视图</Text></Pressable>}
    </View>}
  </View>
    {vrRequested && <CardboardControls config={cardboard} onChange={onCardboardChange} landscape={vrActive} status={vrSession.status} visible={vrTools || !vrActive} calibrating={vrCalibration} onCalibrate={setVrCalibration} onHide={() => setVrTools(false)} onRecenter={vrSession.recenter} onRetry={vrSession.retry} onExit={onVrExit} />}
  </View>;
}
