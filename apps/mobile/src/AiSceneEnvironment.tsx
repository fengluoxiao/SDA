import React, { useMemo, useRef } from "react";
import { useFrame } from "@react-three/fiber/native";
import * as THREE from "three";
import { Listener } from "../../../packages/renderer/src/scene-models";
import type { AiSceneDescription, SceneElement } from "./ai-scene";
import { aiSceneParticlePositions } from "./ai-scene-layout";
// Fixed bounded shaders: model output is data, never executable GLSL/JS.
export const AI_FIELD_VERTEX = `varying vec3 vDirection; void main(){vDirection=normalize(position);gl_Position=projectionMatrix*modelViewMatrix*vec4(position,1.0);}`;
export const AI_FIELD_FRAGMENT = `precision mediump float;
varying vec3 vDirection; uniform float uTime,uScale,uFlow,uTwist,uBands,uIntensity; uniform vec3 uBackground,uA,uB,uC;
void main(){ vec3 p=normalize(vDirection)*uScale; float t=uTime*uFlow;
float bend=sin(p.y*uBands+t)*uTwist;
float wave=sin(p.x*uBands+bend+t)*cos(p.z*1.7-t*.4);
float mist=.5+.5*sin(wave*2.2+p.y*2.0+sin(p.z*2.1+t*.3));
vec3 tint=mix(uA,uB,smoothstep(.1,.85,mist)); tint=mix(tint,uC,pow(max(0.,wave),3.)*.65);
float strength=uIntensity*(.18+.82*pow(mist,2.)); gl_FragColor=vec4(mix(uBackground,tint,strength),1.); }`;
export const AI_PARTICLE_VERTEX = `uniform float uTime,uSpeed,uSize; varying float vAlpha;
void main(){vec3 p=position; p.y+=sin(uTime*uSpeed+position.x)*.22;
vec4 mv=modelViewMatrix*vec4(p,1.);gl_Position=projectionMatrix*mv;gl_PointSize=clamp(uSize*260./max(.8,-mv.z),1.,12.);
vAlpha=.55+.25*sin(position.x*3.+uTime*uSpeed);}`;
export const AI_PARTICLE_FRAGMENT = `precision mediump float; uniform vec3 uColor; varying float vAlpha;
void main(){float r=length(gl_PointCoord-vec2(.5))*2.;float a=pow(max(0.,1.-r),2.)*vAlpha;gl_FragColor=vec4(uColor,a);}`;
function Geometry({ shape }: { shape: SceneElement["shape"] }) {
  switch (shape) {
    case "sphere": return <sphereGeometry args={[1, 16, 10]} />;
    case "box": return <boxGeometry args={[1, 1, 1]} />;
    case "ring": return <torusGeometry args={[1, .025, 6, 48]} />;
    case "cone": return <coneGeometry args={[1, 2, 12]} />;
    case "cylinder": return <cylinderGeometry args={[.6, .6, 2, 12]} />;
    case "plane": return <planeGeometry args={[2, 2]} />;
  }
}
/** Same scene/camera/VR renderer as real source objects; one cheap sky and particle draw. */
export function AiSceneEnvironment({ description: d, reduceMotion, paused, showListener }: { description: AiSceneDescription; reduceMotion: boolean; paused: boolean; showListener: boolean }) {
  const group = useRef<THREE.Group>(null), time = useRef(0);
  const positions = useMemo(() => aiSceneParticlePositions(d), [d]);
  const uniforms = useMemo(() => ({ uTime: { value: 0 }, uScale: { value: d.field.scale }, uFlow: { value: d.field.flow }, uTwist: { value: d.field.twist }, uBands: { value: d.field.bands }, uIntensity: { value: d.field.intensity }, uBackground: { value: new THREE.Color(d.background) }, uA: { value: new THREE.Color(d.palette[0]) }, uB: { value: new THREE.Color(d.palette[1]) }, uC: { value: new THREE.Color(d.palette[2]) } }), [d]);
  const particleUniforms = useMemo(() => ({ uTime: { value: 0 }, uSpeed: { value: d.particles.speed }, uSize: { value: d.particles.size }, uColor: { value: new THREE.Color(d.particles.color) } }), [d]);
  useFrame((_, delta) => {
    if (!paused && !reduceMotion) time.current += Math.min(.05, Math.max(0, delta));
    const t = reduceMotion ? 0 : time.current;
    uniforms.uTime.value = particleUniforms.uTime.value = t;
    group.current?.children.forEach((mesh, i) => {
      const e = d.elements[i]; if (!e) return;
      mesh.rotation.set(e.rotation[0] + e.spin[0] * t, e.rotation[1] + e.spin[1] * t, e.rotation[2] + e.spin[2] * t);
      mesh.position.y = e.position[1] + Math.sin(t * .35 + i) * e.float;
    });
  });
  return <>
    <color attach="background" args={[d.background]} />
    <ambientLight intensity={.9} /><directionalLight position={[2, 4, 3]} intensity={1.1} />
    <mesh renderOrder={-100}>
      <sphereGeometry args={[64, 24, 16]} />
      <shaderMaterial side={THREE.BackSide} depthWrite={false} uniforms={uniforms} vertexShader={AI_FIELD_VERTEX} fragmentShader={AI_FIELD_FRAGMENT} />
    </mesh>
    <group ref={group}>{d.elements.map((e, i) => <mesh key={i} position={e.position} rotation={e.rotation} scale={e.scale} renderOrder={-20}>
      <Geometry shape={e.shape} /><meshBasicMaterial color={e.color} transparent opacity={e.opacity} wireframe={e.wireframe} side={THREE.DoubleSide} depthWrite={false} />
    </mesh>)}</group>
    {d.particles.count > 0 && <points frustumCulled={false} renderOrder={-10}>
      <bufferGeometry><bufferAttribute attach="attributes-position" args={[positions, 3]} /></bufferGeometry>
      <shaderMaterial transparent depthWrite={false} blending={THREE.AdditiveBlending} uniforms={particleUniforms} vertexShader={AI_PARTICLE_VERTEX} fragmentShader={AI_PARTICLE_FRAGMENT} />
    </points>}
    {showListener && <Listener />}
  </>;
}
