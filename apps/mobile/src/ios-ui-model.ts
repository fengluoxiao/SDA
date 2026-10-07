import type { PlayerProps, QueueTrack } from "./RemotePlayer";

// UI-only selectors: never mutate playback or translate scene coordinates here.
export const IOS_TABS = ["播放", "资料库", "空间"] as const;
export function trackTitle(track: Pick<QueueTrack, "name" | "metadata">): string {
  return track.metadata.title || track.name || "未命名音频";
}
export function playbackStatus(p: Pick<PlayerProps, "busy" | "preparingAudio" | "playing" | "paused" | "ended" | "selectedUri">): string {
  if (p.busy || p.preparingAudio) return "正在准备音频…";
  if (p.playing) return p.paused ? "已暂停" : "播放中";
  if (p.ended) return "播放结束";
  return p.selectedUri ? "准备就绪" : "选择文件，开始聆听";
}
export function compatibleRooms(p: Pick<PlayerProps, "rooms" | "layout" | "systemSpatial360RAActive" | "systemSpatial360RA" | "sourceCodec">) {
  // A pending switch back to KU100 must offer the original 13-channel room,
  // while the current track can still be finishing on the system 7.1.4 route.
  const layout = p.systemSpatial360RAActive && p.sourceCodec !== "alac" && !p.systemSpatial360RA ? "360RA-13" : p.layout;
  return p.rooms.filter(room => room.layout === layout);
}
export function isPresetSelected(p: Pick<PlayerProps, "hrtfSet" | "directObjects" | "directionalObjects" | "nearField" | "roomId" | "hrtfWetWeight">,
  preset: { hrtfSet: string; direct: boolean; directional: boolean; nearField: boolean; roomId: string; hrtfWetWeight: number }) {
  return p.hrtfSet === preset.hrtfSet && p.directObjects === preset.direct && p.directionalObjects === preset.directional
    && p.nearField === preset.nearField && p.roomId === preset.roomId && Math.abs(p.hrtfWetWeight - preset.hrtfWetWeight) < 1e-6;
}

// Measure the tab's actual viewport and text/control blocks, not the full phone
// screen. Safe areas, native tabs, wrapped metadata and notices are already excluded.
export function iosPlayerLayout(width: number, viewportHeight: number, blockHeight: number) {
  const gap = Math.min(12, Math.max(4, viewportHeight / 70));
  const padding = 22; // playerContent top + bottom
  const coverSize = Math.max(48, Math.min(260, Math.max(48, width - 64), viewportHeight - blockHeight - padding - gap * 4));
  return { coverSize: Math.floor(coverSize), gap };
}
