import type { VisualObject } from "./player.js";

const MIN_TRAVEL = 0.002; // normalized ADM units; accumulate sub-threshold steps
const ATTACK_SEC = 0.08;
const HOLD_SEC = 0.22;
const FADE_SEC = 0.42;
interface MotionState {
  pos: [number, number, number];
  anchor: VisualObject["anchor"];
  start: number;
  end: number;
  projected?: VisualObject;
  source?: VisualObject;
}

/** A display projection only. Never remove PCM lanes, declarations or events.
 * Uses the presentation clock, so pausing also pauses fades. Initial placement,
 * gain/extent changes and duplicate metadata do not count as spatial motion. */
export class ObjectMotionFilter {
  private states = new Map<number, MotionState>();
  private lastTime = -Infinity;
  private snapshot: VisualObject[] = [];

  reset(): void {
    this.states.clear();
    this.lastTime = -Infinity;
    this.snapshot = [];
  }

  project(objects: readonly VisualObject[], timeSec: number): VisualObject[] {
    if (!Number.isFinite(timeSec)) return this.snapshot;
    // Seek/recovery is initial placement, not every object flying across the room.
    if (timeSec < this.lastTime || timeSec - this.lastTime > 0.5) this.reset();
    this.lastTime = timeSec;
    const present = new Set<number>();
    const visible: VisualObject[] = [];
    for (const object of objects) {
      present.add(object.id);
      if (!object.hasPos || !object.pos.every(Number.isFinite)) {
        this.states.delete(object.id);
        continue;
      }
      let state = this.states.get(object.id);
      if (!state || state.anchor !== object.anchor) {
        state = { pos: [...object.pos], anchor: object.anchor, start: -Infinity, end: -Infinity };
        this.states.set(object.id, state);
      } else if (Math.hypot(...object.pos.map((value, i) => value - state!.pos[i]!)) >= MIN_TRAVEL) {
        const timing = object.visualTiming;
        const eventTime = timing && Number.isFinite(timing.timeSec) ? timing.timeSec : timeSec;
        const start = Math.min(timeSec, eventTime);
        const ramp = timing && Number.isFinite(timing.rampSec) ? Math.max(0, timing.rampSec) : 0;
        const end = start + ramp;
        // Keep the attack continuous across successive movement blocks.
        if (start > state.end + HOLD_SEC) state.start = start;
        state.end = Math.max(state.end, end);
        state.pos = [...object.pos];
      }
      const attack = Math.min(1, Math.max(0, (timeSec - state.start) / ATTACK_SEC));
      const fade = Math.min(1, Math.max(0, 1 - (timeSec - state.end - HOLD_SEC) / FADE_SEC));
      const opacity = attack * fade;
      if (opacity <= 0) continue;
      if (state.source !== object || state.projected?.sceneOpacity !== opacity) {
        state.source = object;
        state.projected = { ...object, sceneOpacity: opacity };
      }
      visible.push(state.projected!);
    }
    for (const id of this.states.keys()) if (!present.has(id)) this.states.delete(id);
    if (visible.length !== this.snapshot.length || visible.some((object, i) => object !== this.snapshot[i])) {
      this.snapshot = visible;
    }
    return this.snapshot;
  }
}
