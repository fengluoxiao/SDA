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
