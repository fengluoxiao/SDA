import type { Spherical } from "./coords";
import { sphericalToAdm } from "./coords";
import type { VirtualSpeaker } from "./layouts";

export type ScenePosition = [number, number, number];

export const SCENE_ROOM_HALF_EXTENT = 2;
export const SCENE_FLOOR_Y = -0.6;
export const SCENE_CEILING_Y = SCENE_ROOM_HALF_EXTENT;
export const SCENE_WALL_HEIGHT = SCENE_CEILING_Y - SCENE_FLOOR_Y;
export const SCENE_WALL_MID_Y = (SCENE_CEILING_Y + SCENE_FLOOR_Y) / 2;

/** ADM uses +X right, +Y front, +Z up; Three uses +X right, +Y up, -Z front. */
export function admToScenePosition(pos: readonly [number, number, number]): ScenePosition {
  return [pos[0] * SCENE_ROOM_HALF_EXTENT, pos[2] * SCENE_ROOM_HALF_EXTENT, -pos[1] * SCENE_ROOM_HALF_EXTENT];
}

/** Map renderer layout directions into the exact scene axes used for object positions. */
export function sphericalToScenePosition(position: Spherical): ScenePosition {
  return admToScenePosition(sphericalToAdm(position));
}

/** Shared 7.1.4 speaker placements; LFE is decorative and does not pan in the renderer. */
export function speakerScenePosition(speaker: VirtualSpeaker): ScenePosition {
  if (speaker.isLfe) return [-0.7, SCENE_FLOOR_Y + 0.13, -SCENE_ROOM_HALF_EXTENT + 0.13];
  return sphericalToScenePosition(speaker);
}

export function smoothScenePosition(current: ScenePosition, target: ScenePosition, deltaSeconds: number, speed = 20): ScenePosition {
  const factor = 1 - Math.exp(-deltaSeconds * speed);
  return [
    current[0] + (target[0] - current[0]) * factor,
    current[1] + (target[1] - current[1]) * factor,
    current[2] + (target[2] - current[2]) * factor,
  ];
}
