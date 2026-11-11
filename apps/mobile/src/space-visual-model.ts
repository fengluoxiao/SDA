import { SCENE_FLOOR_Y, SCENE_ROOM_HALF_EXTENT } from "../../../packages/renderer/src/scene-coordinates";
/** Display-only values: this module never changes source activity or audio. */
export const SPACE_STAGE = {
  background: "#0a101f", floor: "#121c30", orbit: "#52617e", foreground: "#eef2ff",
} as const;
const objectColors = ["#a894ff", "#69dad0", "#ffb8b4", "#8bbaff", "#e1b2ff", "#b7dfab"];
export function spaceObjectColor(id: number): string {
  const index = Number.isFinite(id) ? ((Math.trunc(id) % objectColors.length) + objectColors.length) % objectColors.length : 0;
  return objectColors[index]!;
}
export function spaceGlowOpacity(visibility: number, phase: number, reduceMotion: boolean): number {
  const alpha = Number.isFinite(visibility) ? Math.min(1, Math.max(0, visibility)) : 0;
  const pulse = reduceMotion || !Number.isFinite(phase) ? 1 : .9 + .1 * Math.cos(phase * 1.6);
  return alpha * .68 * pulse;
}

/** Fit the room horizontally on tall viewports while retaining the user's zoom ratio. */
export function spaceCameraDistance(distance: number, width: number, height: number): number {
  const base = Number.isFinite(distance) && distance > 0 ? distance : 7;
  if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) return base;
  const halfAngle = Math.atan(Math.tan(25 * Math.PI / 180) * Math.min(1, Math.max(.2, width / height)));
  return base * Math.max(1, (2.8 / Math.sin(halfAngle)) / 7);
}

export function spaceVisualColumns(width: number, height: number): boolean {
  return Number.isFinite(width) && Number.isFinite(height) && width >= 680 && height >= 210 && width / height >= 1.55;
}

/** Spherical layouts include lower-hemisphere speakers; keep the display base below them. */
export function spaceStageFloor(layout: string): number {
  return layout === "360RA-13" ? -SCENE_ROOM_HALF_EXTENT : SCENE_FLOOR_Y;
}

/** A cancelled, held, dragged or multi-touch interaction is never a fullscreen tap. */
export class SpaceSceneTap {
  private start: { x: number; y: number; time: number } | null = null;
  begin(touches: readonly { pageX: number; pageY: number }[], time: number) {
    this.start = touches.length === 1 ? { x: touches[0]!.pageX, y: touches[0]!.pageY, time } : null;
  }
  update(touches: readonly { pageX: number; pageY: number }[]) {
    const point = touches[0];
    if (this.start && (touches.length !== 1 || !point || Math.hypot(point.pageX - this.start.x, point.pageY - this.start.y) > 8)) this.cancel();
  }
  end(time: number): boolean {
    const start = this.start;
    this.cancel();
    return !!start && time >= start.time && time - start.time <= 300;
  }
  cancel() { this.start = null; }
}
