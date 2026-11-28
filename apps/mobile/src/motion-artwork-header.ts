/** Keep the compact header confined to an actual motion-cover playback page. */
export function motionArtworkHeaderHidden(options: {
  motionEnabled: boolean;
  hideHeader: boolean;
  motionUri: string;
  page: number;
  lyricsSelected: boolean;
  tabletLandscape: boolean;
}): boolean {
  return options.motionEnabled && options.hideHeader && !!options.motionUri && options.page === 0
    && (!options.lyricsSelected || options.tabletLandscape);
}
