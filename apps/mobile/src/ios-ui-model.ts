import type { PlayerProps, QueueTrack } from "./RemotePlayer";

// UI-only selectors: never mutate playback or translate scene coordinates here.
export const IOS_TABS = ["播放", "资料库", "空间"] as const;
export function finitePlaybackMs(value: unknown, fallback = 0): number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : fallback;
}
export function formatPlaybackTime(ms: number): string {
  const seconds = Math.floor(finitePlaybackMs(ms) / 1000);
  return Math.floor(seconds / 60) + ":" + String(seconds % 60).padStart(2, "0");
}
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
  // Measure intrinsic blocks only: no positional offsets or variable margins
  // inside measured blocks. Share spare height among five real flow gaps.
  const padding = 44; // content22 + slider half-touch22 for transport symmetry
  const available = Math.max(0, viewportHeight - blockHeight - padding);
  const baseGap = Math.min(16, available / 10);
  const coverSize = Math.floor(Math.max(48, Math.min(340, Math.max(48, width - 64), available - baseGap * 5)));
  const gap = Math.max(0, Math.min(48, (available - coverSize) / 5));
  return { coverSize, gap };
}

// Keep the approved tablet proportions between metadata, transport and volume.
// Scale only the empty space to the measured iOS landscape viewport; the touch
// targets and text retain their intrinsic size, even with a wrapped song title.
export function iosLandscapeControlLayout(viewportHeight: number, blockHeight: number) {
  const spare = Math.max(0, viewportHeight - blockHeight);
  const topPadding = Math.min(24, spare);
  const scale = Math.min(1, Math.max(0, (spare - topPadding) / 118));
  // Tighten both sides of the transport row without reallocating that space
  // into another gap or reducing the buttons' touch height.
  return { metadataGap: 44 * scale, transportGap: Math.max(0, 42 * scale - 4), volumeGap: Math.max(0, 32 * scale - 4), topPadding };
}

// Controls keep equal halves of the measured viewport. Immersive artwork is
// independent of those safe-area columns: a full-height square at the screen edge.
export function landscapeArtworkLayout(surfaceWidth: number, surfaceHeight: number, viewportWidth: number) {
  const available = Math.max(0, Math.min(surfaceWidth, viewportWidth));
  const columnWidth = available / 2;
  const size = Math.max(0, surfaceHeight);
  return { columnWidth, size, left: 0 };
}

// Keep the accepted row x-position (parent + 38), while moving the scroll
// effect outside the left action lane, including devices with no side inset.
export function iosLeftLibraryGutter(safeLeft: number, cutoutSide: IOSCutoutSide = null) {
  const paddingLeft = Math.min(8, Math.max(0, safeLeft));
  return { marginLeft: 38 - paddingLeft + iosLeftContentOffset(safeLeft, cutoutSide), paddingLeft };
}

// Expo reports UIInterfaceOrientation, not the inverse UIDevice orientation.
export type IOSCutoutSide = "left" | "right" | null;
export function iosCutoutSideForOrientation(orientation: number): IOSCutoutSide {
  if (orientation === 3) return "right";
  if (orientation === 4) return "left";
  return null;
}

// Unknown orientation stays conservative until the initial native query resolves.
// Opposite the cutout, retain the exact previously approved centered position.
export function leftSideDockInset(leftSafeInset: number, cutoutSide: IOSCutoutSide = null) {
  const safe = Math.max(0, leftSafeInset);
  return cutoutSide === "right" ? Math.max(20, (safe + 8) / 2) : Math.max(14, safe) + 6;
}
export function iosLeftContentOffset(leftSafeInset: number, cutoutSide: IOSCutoutSide = null) {
  return leftSideDockInset(leftSafeInset, cutoutSide) - leftSideDockInset(leftSafeInset);
}
export function rightSideDockInset(rightSafeInset: number, cutoutSide: IOSCutoutSide = null) {
  const safe = Math.max(0, rightSafeInset);
  return cutoutSide === "left" ? (safe + 8) / 2 : safe + 4;
}
export function iosRightSideContentGap(rightSafeInset: number, cutoutSide: IOSCutoutSide = null) {
  return rightSafeInset > 0 && cutoutSide !== "left" ? 20 : 8;
}

export function iosRightLibraryGutter(safeRight: number, cutoutSide: IOSCutoutSide = null) {
  const indicatorRight = cutoutSide === "left"
    ? Math.min(12, (Math.max(0, safeRight) + 8) / 4)
    : 2;
  const paddingRight = indicatorRight * 2;
  return { marginRight: 48 + iosRightSideContentGap(safeRight, cutoutSide) - 18 - paddingRight, paddingRight, indicatorRight };
}
