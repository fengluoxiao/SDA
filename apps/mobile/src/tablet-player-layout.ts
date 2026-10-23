// Shared by iPad and Android tablets. Dimensions refer to the measured player
// viewport after native page headers and safe areas have taken their space.
export function isTabletPlayer(os: string, isPad: boolean, width: number, height: number, phonePortraitStyle = false) {
  const nativeTablet = os === "ios" ? isPad : os === "android" && Math.min(width, height) >= 600;
  return nativeTablet || ((os === "ios" || os === "android") && height > width && phonePortraitStyle);
}

export function tabletPlayerLayout(width: number, height: number, blockHeight: number, landscape: boolean, sideNavigation: boolean) {
  const contentWidth = Math.min(1260, Math.max(0, width));
  const horizontalPadding = Math.min(contentWidth / 8, sideNavigation ? 80 : contentWidth >= 1000 ? 48 : 28);
  const verticalPadding = height >= 500 ? 24 : 12;
  const innerWidth = Math.max(0, contentWidth - horizontalPadding * 2);
  const columnGap = landscape ? Math.min(64, innerWidth * .06) : 28;
  const columnWidth = landscape ? Math.max(0, (innerWidth - columnGap) / 2) : innerWidth;
  const spacingScale = Math.min(1, Math.max(0, (height - verticalPadding * 2 - blockHeight) / 180));
  const metadataGap = 32 * spacingScale;
  const transportGap = 28 * spacingScale;
  const volumeGap = 24 * spacingScale;
  const controlsHeight = blockHeight + metadataGap + transportGap + volumeGap;
  const coverHeight = landscape ? height - verticalPadding * 2 : height - verticalPadding * 2 - controlsHeight - columnGap;
  const coverSize = Math.max(0, Math.min(520, columnWidth, coverHeight));
  return { contentWidth, horizontalPadding, verticalPadding, columnGap, columnWidth, coverSize,
    controlsWidth: Math.min(landscape ? 440 : 520, columnWidth), metadataGap, transportGap, volumeGap };
}

// One shared lane contains the 48pt rail, 20pt outer margin and at least
// 28pt clearance to the page. It also contains the 44pt header actions.
export function tabletNavigationLane(safeInset: number) {
  return 96 + (Number.isFinite(safeInset) ? Math.max(0, safeInset) : 0);
}

// Keep the page centered on the screen when the rail changes sides. Use the
// larger safe inset on both edges so an asymmetric cutout cannot bias the page.
export function tabletNavigationPageInsets(leftInset: number, rightInset: number) {
  const safeLeft = Number.isFinite(leftInset) ? Math.max(0, leftInset) : 0;
  const safeRight = Number.isFinite(rightInset) ? Math.max(0, rightInset) : 0;
  const margin = tabletNavigationLane(Math.max(safeLeft, safeRight));
  return { marginLeft: margin, marginRight: margin };
}
