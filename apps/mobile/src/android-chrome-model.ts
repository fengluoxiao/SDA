export const ANDROID_CHROME_TABS = ["播放", "资料库", "空间"] as const;

export function androidChromeState(player: {
  selectedUri: string; playing: boolean; paused: boolean; busy: boolean;
}, selected: number) {
  const hasTrack = !!player.selectedUri;
  const showMini = hasTrack && selected !== 0;
  return { hasTrack, showMini, playing: player.playing && !player.paused, height: showMini ? 128 : 74 };
}

export function isAndroidTabIndex(index: number): boolean {
  return Number.isInteger(index) && index >= 0 && index < ANDROID_CHROME_TABS.length;
}

// The header paints behind the status bar; only its controls are inset.
export function androidHeaderLayout(safeTop: number) {
  const top = Number.isFinite(safeTop) ? Math.max(0, safeTop) : 0;
  return { top, height: 56 + top, contentTop: 64 + top };
}
