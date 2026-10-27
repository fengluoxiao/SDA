import {ObjectActivityFilter} from "../../../packages/player/src/object-activity";

export interface MobileObjectPoint {
  id: number;
  pos: [number, number, number];
  gainDb: number;
  hasPos?: boolean;
  sceneOpacity?: number;
}

/** Same signal/fade policy as desktop. Keep the full metadata list outside it. */
export class MobileObjectActivity {
  private activity = new ObjectActivityFilter<MobileObjectPoint>();
  private lastStreamTime = -Infinity;
  private all = new Map<number, {source: MobileObjectPoint; point: MobileObjectPoint}>();

  project(objects: readonly MobileObjectPoint[], soundingIds: readonly number[] | undefined,
      clockSec: number, streamTimeSec: number, paused: boolean, showAll: boolean): readonly MobileObjectPoint[] {
    if (!objects.length || streamTimeSec < this.lastStreamTime || streamTimeSec - this.lastStreamTime > 0.5) this.activity.reset();
    this.lastStreamTime = streamTimeSec;
    const sounding = new Set(paused ? [] : soundingIds);
    const visible = this.activity.project(objects, sounding, clockSec);
    // Older native modules/system-owned output may not report source activity.
    // Preserve their positions rather than guessing activity from movement.
    if (!showAll && (soundingIds !== undefined || paused)) return visible;
    const present = new Set<number>();
    const result = objects.filter(o => o.hasPos !== false && o.pos.every(Number.isFinite)).map(object => {
      present.add(object.id);
      const opacity = soundingIds === undefined && !paused || sounding.has(object.id) ? 1 : 0.12;
      let cached = this.all.get(object.id);
      if (cached?.source !== object || cached.point.sceneOpacity !== opacity) {
        cached = {source: object, point: {...object, sceneOpacity: opacity}};
        this.all.set(object.id, cached);
      }
      return cached.point;
    });
    for (const id of this.all.keys()) if (!present.has(id)) this.all.delete(id);
    return result;
  }
}
