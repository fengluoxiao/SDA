import type { MobileObjectPoint } from "./mobile-object-activity";
import { SCENE_ROOM_HALF_EXTENT as scale } from "../../../packages/renderer/src/scene-coordinates";
type Track = { from: number[]; to: number[]; at: number; duration: number };
/** Two visual positions per live object, never audio data or a song cache. */
export class SceneObjectMotion {
  private tracks = new Map<number, Track>();
  private source: readonly MobileObjectPoint[] | null = null;
  private stream = -Infinity;
  private wall = -Infinity;
  private paused = false;
  update(objects: readonly MobileObjectPoint[], stream: number, now: number, paused: boolean) {
    if (objects === this.source && paused === this.paused && stream === this.stream) return;
    const jump = stream < this.stream - .001 || stream - this.stream > .5;
    const elapsed = Number.isFinite(this.wall) ? Math.max(1 / 120, Math.min(.15, now - this.wall)) : 0;
    if (jump) this.tracks.clear();
    const present = new Set<number>();
    for (const object of objects) {
      if (object.hasPos === false || !object.pos.every(Number.isFinite)) continue;
      present.add(object.id);
      const to = [object.pos[0] * scale, object.pos[2] * scale, -object.pos[1] * scale];
      const old = this.tracks.get(object.id);
      const fraction = old ? Math.min(1, Math.max(0, (now - old.at) / Math.max(1e-6, old.duration))) : 1;
      const from = old ? old.to.map((v, axis) => old.from[axis]! + (v - old.from[axis]!) * fraction) : to;
      this.tracks.set(object.id, { from: paused ? to : from, to, at: now, duration: paused ? 0 : elapsed });
    }
    for (const id of this.tracks.keys()) if (!present.has(id)) this.tracks.delete(id);
    this.source = objects; this.stream = stream; this.wall = now; this.paused = paused;
  }
  write(id: number, now: number, output: Float32Array, offset: number): boolean {
    const track = this.tracks.get(id);
    if (!track) return false;
    const t = track.duration > 0 ? Math.min(1, Math.max(0, (now - track.at) / track.duration)) : 1;
    for (let axis = 0; axis < 3; axis++) output[offset + axis] = track.from[axis]! + (track.to[axis]! - track.from[axis]!) * t;
    return true;
  }
  get size() { return this.tracks.size; }
}

/** Deadline pacing tolerates display timestamp rounding without halving 60 Hz. */
export class SceneFrameClock {
  private next = -Infinity;
  private fps = 0;
  due(now: number, fps: number): boolean {
    const rate = fps === 120 ? 120 : 60, interval = 1 / rate;
    if (rate !== this.fps || now < this.next - 1 || now > this.next + .25) this.next = now;
    this.fps = rate;
    if (now + .0005 < this.next) return false;
    this.next += interval;
    return true;
  }
}
