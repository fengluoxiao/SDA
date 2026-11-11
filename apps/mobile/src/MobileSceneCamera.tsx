import type { CardboardConfig } from "./cardboard-config";
import type { CardboardPose } from "./cardboard-pose";
import { useMemo, useRef } from "react";
import { useFrame, useThree } from "@react-three/fiber/native";
import { Euler, PerspectiveCamera, Spherical } from "three";
import { spaceCameraDistance } from "./space-visual-model";
import type { SceneCameraInput } from "./scene-camera-input";

/** Visual camera only; never writes to the audio listener or head tracker. */
export function MobileSceneCamera({ input, firstPerson, immersive, layout, cardboard, vrPose }: {
  cardboard?: CardboardConfig; vrPose?: CardboardPose; input: SceneCameraInput; firstPerson: boolean; immersive: boolean; layout: string;
}) {
  const camera = useThree(state => state.camera) as PerspectiveCamera;
  const size = useThree(state => state.size);
  const spherical = useMemo(() => new Spherical(), []);
  const rotation = useMemo(() => new Euler(0, 0, 0, "YXZ"), []);
  const overview = useRef<{ fov: number; near: number; zoom: number; focus: number } | null>(null);
  useFrame(() => {
    if (firstPerson) {
      if (!overview.current) overview.current = { fov: camera.fov, near: camera.near, zoom: camera.zoom, focus: camera.focus };
      camera.position.set(0, 0, 0);
      if (cardboard?.enabled) camera.quaternion.copy(vrPose?.value ?? camera.quaternion);
      else camera.quaternion.setFromEuler(rotation.set(input.look.pitch, input.look.yaw, 0, "YXZ"));
      const fov = cardboard?.enabled ? cardboard.fov : input.fov;
      camera.focus = cardboard?.enabled ? 1000000 : overview.current.focus;
      if (camera.fov !== fov || camera.near !== .02 || camera.zoom !== 1) {
        camera.fov = fov; camera.near = .02; camera.zoom = 1; camera.updateProjectionMatrix();
      }
      return;
    }
    if (overview.current) {
      Object.assign(camera, overview.current); overview.current = null; camera.updateProjectionMatrix();
    }
    spherical.set(immersive ? spaceCameraDistance(input.distance, size.width, size.height) : input.distance,
      Math.PI / 2.9 + input.rotation.y, input.rotation.x);
    camera.position.setFromSpherical(spherical);
    const centerY = immersive || layout === "360RA-13" ? 0 : .5;
    camera.position.y += centerY; camera.lookAt(0, centerY, 0);
  });
  return null;
}
