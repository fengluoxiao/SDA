import type { VisualObject } from "./player.js";

const HOLD_SEC = 0.10;
const FADE_SEC = 0.25;
export interface ActivityObject {
  id: number;
  pos: [number, number, number];
  hasPos?: boolean;
  sceneOpacity?: number;
}
interface ActivityState<T> {
  lastSound: number;
  source?: T;
  projected?: T;
}

/** Display only: renderer-confirmed post-gain/post-mute signal activity.
 * Position/movement never gates an audible object. The animation clock keeps
 * fades running when playback is paused; audio and metadata stay untouched. */
export class ObjectActivityFilter<T extends ActivityObject = VisualObject> {
  private states = new Map<number, ActivityState<T>>();
  private lastTime = -Infinity;
  private snapshot: T[] = [];

  reset(): void {
    this.states.clear();
    this.lastTime = -Infinity;
    this.snapshot = [];
  }

  project(objects: readonly T[], soundingIds: ReadonlySet<number>, clockSec: number): T[] {
    if (!Number.isFinite(clockSec)) return this.snapshot;
    if (clockSec < this.lastTime) this.reset();
    this.lastTime = clockSec;
    const present = new Set<number>();
    const visible: T[] = [];
    for (const object of objects) {
      present.add(object.id);
      if (object.hasPos === false || !object.pos.every(Number.isFinite)) {
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
