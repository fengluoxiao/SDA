/** Pure object-control helpers shared by the UI and player tests. */

import type { ObjectEvent } from "@sda/core";
import type { VisualObject } from "./player.js";

export function placeholderVisualObject(id: number): VisualObject {
  return { id, pos: [0, 0, 0], hasPos: false, size: [0, 0, 0], gainDb: 0, anchor: "room", distanceM: null, distanceInfinite: false };
}

export function sameObjectTarget(left: ObjectEvent | undefined, right: ObjectEvent): boolean {
  return !!left
    && left.hasPos === right.hasPos
    && left.pos[0] === right.pos[0]
    && left.pos[1] === right.pos[1]
    && left.pos[2] === right.pos[2]
    && left.gainDb === right.gainDb
    && (left.diffuse ?? 0) === (right.diffuse ?? 0)
    && !!left.horizontalOnly === !!right.horizontalOnly
    && left.size[0] === right.size[0]
    && left.size[1] === right.size[1]
    && left.size[2] === right.size[2]
    && left.anchor === right.anchor
    && left.distanceM === right.distanceM
    && left.distanceInfinite === right.distanceInfinite
    && left.screenFactor === right.screenFactor
    && left.depthFactor === right.depthFactor;
}

export function canCoalesceObjectEvent(left: ObjectEvent | undefined, right: ObjectEvent): boolean {
  if (!left || !sameObjectTarget(left, right)) return false;
  const leftRampEnd = left.samplePos + Math.max(1, left.rampDuration || 128);
  return leftRampEnd <= right.samplePos;
}

export function withoutPendingObjectEvents(
  events: readonly ObjectEvent[],
  cursor: number,
  id: number,
): ObjectEvent[] {
  return events.slice(cursor).filter((event) => event.id !== id);
}

export function visualObjectFromEvent(event: ObjectEvent): VisualObject {
  return {
    id: event.id,
    pos: event.pos,
    hasPos: event.hasPos,
    size: event.size,
    gainDb: event.gainDb,
    anchor: event.anchor,
    distanceM: event.distanceM,
    distanceInfinite: event.distanceInfinite,
  };
}

export function nextSoloMuteSet(
  objectIds: readonly number[],
  currentMuted: ReadonlySet<number>,
  id: number,
): Set<number> {
  const isCurrentSolo =
    objectIds.length > 1 &&
    objectIds.filter((objectId) => !currentMuted.has(objectId)).length === 1 &&
    !currentMuted.has(id);
  if (isCurrentSolo) return new Set();
  return new Set(objectIds.filter((objectId) => objectId !== id));
}
