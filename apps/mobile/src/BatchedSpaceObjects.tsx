import React, { useEffect, useMemo } from "react";
import { useFrame } from "@react-three/fiber/native";
import * as THREE from "three";
import type { MobileObjectPoint } from "./mobile-object-activity";
import { SceneObjectMotion } from "./scene-motion";
import { spaceObjectColor, spaceGlowOpacity } from "./space-visual-model";

const fragment = `varying vec3 vColor; varying float vOpacity;
void main(){ if(vOpacity <= 0.0) discard; gl_FragColor=vec4(vColor,vOpacity);
#include <tonemapping_fragment>
#include <colorspace_fragment>
}`;
const glowFragment = `varying vec3 vColor; varying float vOpacity; varying vec2 vUv;
void main(){ float alpha=pow(max(0.0,1.0-length(vUv*2.0-1.0)),2.4)*vOpacity;
if(alpha < 0.001) discard; gl_FragColor=vec4(vColor,alpha);
#include <tonemapping_fragment>
#include <colorspace_fragment>
}`;
const declarations = `attribute vec3 instancePosition; attribute vec3 instanceColor; attribute float instanceOpacity;
varying vec3 vColor; varying float vOpacity;`;
/** One instanced dot pass and one billboard glow pass for any object count. */
export function BatchedSpaceObjects({ objects, visibility, immersive, phase, reduceMotion, sourceKey, streamTimeSec, paused }: {
  objects: readonly MobileObjectPoint[]; visibility: ReadonlyMap<number, number>; immersive: boolean;
  phase: { current: number }; reduceMotion: boolean; sourceKey?: string; streamTimeSec: number; paused: boolean;
}) {
  const motion = useMemo(() => new SceneObjectMotion(), [sourceKey]);
  const batch = useMemo(() => {
    const capacity = Math.max(1, objects.length), position = new THREE.InstancedBufferAttribute(new Float32Array(capacity * 3), 3).setUsage(THREE.DynamicDrawUsage);
    const color = new THREE.InstancedBufferAttribute(new Float32Array(capacity * 3), 3).setUsage(THREE.DynamicDrawUsage);
    const opacity = new THREE.InstancedBufferAttribute(new Float32Array(capacity), 1).setUsage(THREE.DynamicDrawUsage);
    const glowOpacity = new THREE.InstancedBufferAttribute(new Float32Array(capacity), 1).setUsage(THREE.DynamicDrawUsage);
    const geometry = (base: THREE.BufferGeometry, alpha: THREE.InstancedBufferAttribute) => {
      const value = new THREE.InstancedBufferGeometry(); value.index = base.index;
      for (const [name, attribute] of Object.entries(base.attributes)) value.setAttribute(name, attribute);
      value.setAttribute("instancePosition", position); value.setAttribute("instanceColor", color); value.setAttribute("instanceOpacity", alpha);
      value.instanceCount = objects.length; return value;
    };
    const dotGeometry = geometry(new THREE.SphereGeometry(.06, 10, 10), opacity);
    const glowGeometry = geometry(new THREE.PlaneGeometry(immersive ? .65 : .22, immersive ? .65 : .22), glowOpacity);
    const dotMaterial = new THREE.ShaderMaterial({ transparent: true, depthTest: false, depthWrite: false, fragmentShader: fragment,
      vertexShader: declarations + `void main(){ vColor=instanceColor; vOpacity=instanceOpacity;
      gl_Position=projectionMatrix*modelViewMatrix*vec4(position+instancePosition,1.0); }` });
    const glowMaterial = new THREE.ShaderMaterial({ transparent: true, depthTest: false, depthWrite: false, blending: THREE.AdditiveBlending, fragmentShader: glowFragment,
      vertexShader: declarations + `varying vec2 vUv; void main(){ vUv=uv; vColor=instanceColor; vOpacity=instanceOpacity;
      vec4 center=modelViewMatrix*vec4(instancePosition,1.0); center.xy+=position.xy; gl_Position=projectionMatrix*center; }` });
    return { position, color, opacity, glowOpacity, dotGeometry, glowGeometry, dotMaterial, glowMaterial, tint: new THREE.Color(), colorSource: null as readonly MobileObjectPoint[] | null };
  }, [objects.length, immersive]);
  useEffect(() => () => { batch.dotGeometry.dispose(); batch.glowGeometry.dispose(); batch.dotMaterial.dispose(); batch.glowMaterial.dispose(); }, [batch]);
  useFrame(() => {
    const now = performance.now() / 1000;
    motion.update(objects, streamTimeSec, now, paused);
    const colorsChanged = batch.colorSource !== objects;
    for (let index = 0; index < objects.length; index++) {
      const object = objects[index]!, offset = index * 3;
      const valid = motion.write(object.id, now, batch.position.array as Float32Array, offset);
      const alpha = valid ? visibility.get(object.id) ?? 0 : 0;
      batch.opacity.array[index] = alpha;
      batch.glowOpacity.array[index] = immersive ? spaceGlowOpacity(alpha, phase.current + object.id * .43, reduceMotion) : alpha * .18;
      if (colorsChanged) {
        if (immersive) batch.tint.set(spaceObjectColor(object.id)); else batch.tint.setHSL(.55 - object.pos[2] * .25, .9, .6);
        batch.color.setXYZ(index, batch.tint.r, batch.tint.g, batch.tint.b);
      }
    }
    batch.position.needsUpdate = batch.opacity.needsUpdate = batch.glowOpacity.needsUpdate = true;
    if (colorsChanged) { batch.color.needsUpdate = true; batch.colorSource = objects; }
  });
  return <group>
    <mesh geometry={batch.glowGeometry} material={batch.glowMaterial} frustumCulled={false} />
    <mesh geometry={batch.dotGeometry} material={batch.dotMaterial} frustumCulled={false} />
  </group>;
}
