export interface MobileObjectPoint {
  id: number;
  pos: [number, number, number];
  gainDb: number;
  hasPos?: boolean;
  sceneOpacity?: number;
}

/** Display only: confirmed sounding objects or an explicit full inventory.
 * Silent/unknown sources have no dim marker or lingering glow in sounding-only
 * mode. Movement and metadata gain never stand in for measured signal activity.
 */
export class MobileObjectActivity {
  private points = new Map<number, { source: MobileObjectPoint; point: MobileObjectPoint }>();
  project(objects: readonly MobileObjectPoint[], soundingIds: readonly number[] | undefined,
      _clockSec: number, _streamTimeSec: number, paused: boolean, showAll: boolean): readonly MobileObjectPoint[] {
    const sounding = new Set(paused ? [] : soundingIds);
    const present = new Set<number>();
    const result: MobileObjectPoint[] = [];
    for (const object of objects) {
      present.add(object.id);
      if (object.hasPos === false || !object.pos.every(Number.isFinite)) continue;
      if (!showAll && !sounding.has(object.id)) continue;
      let cached = this.points.get(object.id);
      if (cached?.source !== object) {
        cached = { source: object, point: { ...object, sceneOpacity: 1 } };
        this.points.set(object.id, cached);
      }
      result.push(cached!.point);
    }
    for (const id of this.points.keys()) if (!present.has(id)) this.points.delete(id);
    return result;
  }
}
