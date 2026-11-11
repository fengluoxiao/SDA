import React, { useEffect, useMemo } from "react";
import * as THREE from "three";
import { LAYOUTS } from "../../../packages/renderer/src/layouts";
import { SCENE_ROOM_HALF_EXTENT, speakerScenePosition } from "../../../packages/renderer/src/scene-coordinates";
import { Listener } from "../../../packages/renderer/src/scene-models";
import { SPACE_STAGE, spaceStageFloor } from "./space-visual-model";

/** One tiny shared radial texture per scene, without a bloom/postprocessing pass. */
export function useSpaceGlowTexture(enabled: boolean) {
  const texture = useMemo(() => {
    if (!enabled) return null;
    const size = 64, bytes = new Uint8Array(size * size * 4);
    for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) {
      const i = (y * size + x) * 4;
      const radius = Math.hypot((x + .5) / size * 2 - 1, (y + .5) / size * 2 - 1);
      bytes[i] = bytes[i + 1] = bytes[i + 2] = 255;
      bytes[i + 3] = Math.round(Math.pow(Math.max(0, 1 - radius), 2.4) * 255);
    }
    const value = new THREE.DataTexture(bytes, size, size, THREE.RGBAFormat);
    value.minFilter = value.magFilter = THREE.LinearFilter; value.generateMipmaps = false; value.needsUpdate = true;
    return value;
  }, [enabled]);
  useEffect(() => () => texture?.dispose(), [texture]);
  return texture;
}

function Orbit({ radius, color, opacity = .15, floorY }: { radius: number; color: string; opacity?: number; floorY: number }) {
  return <mesh rotation={[-Math.PI / 2, 0, 0]} position={[0, floorY + .012, 0]}>
    <ringGeometry args={[radius - .008, radius + .008, 80]} />
    <meshBasicMaterial color={color} transparent opacity={opacity} depthWrite={false} side={THREE.DoubleSide} />
  </mesh>;
}

export function ImmersiveSpaceEnvironment({ layout, accent, glow, showListener = true }: { showListener?: boolean; layout: "2.0" | "7.1.4" | "360RA-13"; accent: string; glow: THREE.Texture | null }) {
  const floorY = spaceStageFloor(layout);
  return <>
    <color attach="background" args={[SPACE_STAGE.background]} />
    <fog attach="fog" args={[SPACE_STAGE.background, 11, 25]} />
    <ambientLight intensity={.9} /><directionalLight position={[2, 4, 3]} intensity={1.1} />
    <mesh rotation={[-Math.PI / 2, 0, 0]} position={[0, floorY, 0]}>
      <circleGeometry args={[SCENE_ROOM_HALF_EXTENT * 1.5, 96]} />
      <meshBasicMaterial color={SPACE_STAGE.floor} />
    </mesh>
    {glow && <mesh rotation={[-Math.PI / 2, 0, 0]} position={[0, floorY + .006, 0]}>
      <planeGeometry args={[SCENE_ROOM_HALF_EXTENT * 3, SCENE_ROOM_HALF_EXTENT * 3]} />
      <meshBasicMaterial map={glow} color={accent} transparent opacity={.19} depthWrite={false} blending={THREE.AdditiveBlending} />
    </mesh>}
    {[.34, .7, 1, 1.3].map((scale, index) => <Orbit key={scale} floorY={floorY} radius={SCENE_ROOM_HALF_EXTENT * scale} color={index === 2 ? accent : SPACE_STAGE.orbit} opacity={index === 2 ? .34 : .15} />)}
    {LAYOUTS[layout].map(speaker => <mesh key={speaker.name} position={speakerScenePosition(speaker)}>
      <sphereGeometry args={[speaker.isLfe ? .055 : .035, 10, 8]} />
      <meshBasicMaterial color="#cad7f1" transparent opacity={.6} />
    </mesh>)}
    {layout === "360RA-13" && <mesh>
      <sphereGeometry args={[SCENE_ROOM_HALF_EXTENT, 24, 12]} />
      <meshBasicMaterial color={SPACE_STAGE.orbit} wireframe transparent opacity={.075} depthWrite={false} />
    </mesh>}
    {showListener && <Listener />}
  </>;
}
