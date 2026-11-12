import React, { useEffect, useLayoutEffect, useMemo, useRef } from "react";
import { useFrame, useThree } from "@react-three/fiber/native";
import * as THREE from "three";
import { Listener } from "../../../packages/renderer/src/scene-models";
import { normalizeAiSceneDescription, sceneColorLuminance, type AiSceneDescription, type AiSceneTheme } from "./ai-scene";
import { FoliaCompositor } from "./FoliaCompositor";
import { FOLIA_DEFAULTS, FoliaLatentDriver, type FoliaAudioSignal } from "./folia-runtime";
import { getSpaceRawDraw } from "./cardboard-renderer";
import { SceneFrameClock } from "./scene-motion";
// Only the final texture copy/solid overlay is SDA code. The image itself is
// rendered by Folia's actual MeshGradient + Dithering shaders in FoliaCompositor.
const copyVertex = `attribute vec3 position;varying vec2 vUv;void main(){vUv=position.xy*.5+.5;gl_Position=vec4(position,1.);}`;
const copyFragment = `precision mediump float;varying vec2 vUv;uniform sampler2D u_folia;uniform vec3 u_overlay;uniform float u_opacity;void main(){vec3 c=texture2D(u_folia,vUv).rgb;gl_FragColor=vec4(mix(c,u_overlay,u_opacity),1.);}`;
const fallbackDescription = normalizeAiSceneDescription({ version: 2, name: "Folia", themes: { light: {}, dark: {} } }, 1);
export function localFoliaTheme(accent: string, mode: "light" | "dark"): AiSceneTheme {
  return normalizeAiSceneDescription({ version: 2, themes: {
    light: { accentColor: accent, colors: [accent, "#cab6c7", "#adc8c8"] },
    dark: { accentColor: accent, colors: [accent, "#947995", "#6d9e9f"] },
  } }, 1).themes[mode];
}
/** Direct native adapter for Folia Latent. One shared GL context, including VR. */
export function AiSceneEnvironment({ description, theme: selected, reduceMotion, paused, showListener, audioSignal, stereo = false, frameRate = 60 }: {
  description?: AiSceneDescription | null; theme?: AiSceneTheme; reduceMotion: boolean; paused: boolean; showListener: boolean;
  audioSignal?: FoliaAudioSignal; stereo?: boolean; frameRate?: number;
}) {
  const theme = selected || (description ? normalizeAiSceneDescription(description, description.seed).themes.dark : fallbackDescription.themes.dark);
  const [compositor, driver, clock] = useMemo(() => [new FoliaCompositor(), new FoliaLatentDriver(paused), new SceneFrameClock()] as const, []);
  const renderer = useThree(state => state.gl), size = useThree(state => state.size);
  const themeRevision = useRef(0), staticDrawKey = useRef("");
  const oldTheme = useRef<AiSceneTheme | null>(null), mode = useRef(sceneColorLuminance(theme.backgroundColor) > .3);
  const copyUniforms = useMemo(() => ({ u_folia: { value: compositor.target.texture }, u_overlay: { value: new THREE.Vector3() }, u_opacity: { value: FOLIA_DEFAULTS.overlayOpacity } }), [compositor]);
  useLayoutEffect(() => {
    if (oldTheme.current === theme) return;
    compositor.setTheme(theme); themeRevision.current++;
    const light = sceneColorLuminance(theme.backgroundColor) > .3;
    if (light !== mode.current) compositor.interpolate(1);
    mode.current = light; oldTheme.current = theme;
    copyUniforms.u_overlay.value.set(parseInt(theme.backgroundColor.slice(1,3),16)/255,parseInt(theme.backgroundColor.slice(3,5),16)/255,parseInt(theme.backgroundColor.slice(5,7),16)/255);
  }, [theme, compositor, copyUniforms]);
  useEffect(() => () => compositor.dispose(), [compositor]);
  useFrame((_, dt) => {
    const v = driver.step(dt, audioSignal, paused, reduceMotion);
    const mesh = compositor.meshUniforms, dithering = compositor.ditheringUniforms;
    mesh.u_time.value=v.meshTime;mesh.u_distortion.value=v.distortion;mesh.u_swirl.value=v.swirl;
    mesh.u_sdaScale.value=v.meshScale;mesh.u_sdaSaturation.value=v.saturation;mesh.u_sdaBrightness.value=v.brightness;
    dithering.u_time.value=v.ditheringTime;dithering.u_pxSize.value=v.pixelSize;dithering.u_sdaOpacity.value=v.opacity;dithering.u_sdaScale.value=v.ditherScale;
    compositor.interpolate(reduceMotion ? 1 : 1-Math.exp(-Math.min(.05,Math.max(0,dt))*3.5));
    const staticKey = reduceMotion ? `${size.width}:${size.height}:${stereo}:${renderer.getPixelRatio()}:${themeRevision.current}` : "";
    if (reduceMotion && staticDrawKey.current === staticKey) return;
    if (reduceMotion || clock.due(performance.now()/1000,frameRate)) {
      compositor.draw(renderer,getSpaceRawDraw(renderer),size.width/(stereo?2:1),size.height,renderer.getPixelRatio());
      staticDrawKey.current = staticKey;
    }
  });
  return <>
    <color attach="background" args={[theme.backgroundColor]} />
    <ambientLight intensity={1} /><directionalLight position={[2,4,3]} intensity={1.1} />
    <mesh name="folia-local-visualizer" renderOrder={-10000} frustumCulled={false}>
      <planeGeometry args={[2,2]} /><rawShaderMaterial uniforms={copyUniforms} vertexShader={copyVertex} fragmentShader={copyFragment} depthTest={false} depthWrite={false} />
    </mesh>
    {showListener && <Listener />}
  </>;
}
