import type { VisualObject } from "./player.js";

const HOLD_SEC = 0.10;
const FADE_SEC = 0.25;
interface ActivityState {
  lastSound: number;
  source?: VisualObject;
  projected?: VisualObject;
}

/** Display only: renderer-confirmed post-gain/post-mute signal activity.
 * Position/movement never gates an audible object. The animation clock keeps
 * fades running when playback is paused; audio and metadata stay untouched. */
export class ObjectActivityFilter {
  private states = new Map<number, ActivityState>();
  private lastTime = -Infinity;
  private snapshot: VisualObject[] = [];

  reset(): void {
    this.states.clear();
    this.lastTime = -Infinity;
    this.snapshot = [];
  }

  project(objects: readonly VisualObject[], soundingIds: ReadonlySet<number>, clockSec: number): VisualObject[] {
    if (!Number.isFinite(clockSec)) return this.snapshot;
    if (clockSec < this.lastTime) this.reset();
    this.lastTime = clockSec;
    const present = new Set<number>();
    const visible: VisualObject[] = [];
    for (const object of objects) {
      present.add(object.id);
      if (!object.hasPos || !object.pos.every(Number.isFinite)) {
        this.states.delete(object.id);
        continue;
      }
      let state = this.states.get(object.id);
      if (soundingIds.has(object.id)) {
        if (!state) {
          state = {lastSound: clockSec};
          this.states.set(object.id, state);
        }
        state.lastSound = clockSec;
      }
      if (!state) continue;
      const opacity = Math.min(1, Math.max(0, 1 - (clockSec - state.lastSound - HOLD_SEC) / FADE_SEC));
      if (opacity <= 0) {
        this.states.delete(object.id);
        continue;
      }
      if (state.source !== object || state.projected?.sceneOpacity !== opacity) {
        state.source = object;
        state.projected = {...object, sceneOpacity: opacity};
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
