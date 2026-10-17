// Explicit room-listening preset, not an asset correction or calibration migration.
// Raw room comparison and saved user settings must remain unchanged.
import levels from "../../../packages/renderer/src/room-listening-levels.json";
export const ROOM_LISTENING_LEVELS = Object.freeze(levels);

// Invoked only by the explicit Apply Room action, never while loading settings.
export function roomListeningSettings<T extends object>(current: T) {
  return {...current, enabled:true, reflectionMode:"full" as const, ...ROOM_LISTENING_LEVELS};
}
