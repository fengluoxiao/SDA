export const LISTENER_VIEW_FOV = 85;
export type ListenerLook = { yaw: number; pitch: number };
export type SceneCameraInput = {
  rotation: { x: number; y: number }; distance: number;
  look: ListenerLook; fov: number;
};
/** Same forward axis and pitch limit as desktop's first-person view. */
export function rotateListenerView(look: ListenerLook, dx: number, dy: number, height: number): ListenerLook {
  if (!Number.isFinite(dx) || !Number.isFinite(dy)) return look;
  const scale = 2 * Math.PI * .9 / Math.max(1, Number.isFinite(height) ? height : 300);
  return { yaw: (look.yaw - dx * scale) % (2 * Math.PI), pitch: Math.max(-1.48, Math.min(1.48, look.pitch - dy * scale)) };
}
export function listenerViewFov(fov: number, ratio: number): number {
  if (!Number.isFinite(fov) || !Number.isFinite(ratio) || ratio <= 0) return LISTENER_VIEW_FOV;
  return Math.max(35, Math.min(105, fov * ratio));
}
