export type ArtworkAmbientPaint = { imageOpacity: number; scrimColor: string };

export const ARTWORK_AMBIENT_PAINT: ArtworkAmbientPaint = {
  imageOpacity: .45,
  scrimColor: "rgba(0,0,0,.24)",
} as const;

// Portrait tablet layout has no separate album pane, so its backdrop must stay
// visibly tied to the artwork instead of washing out to the flat tone.
export const TABLET_AMBIENT_PAINT: ArtworkAmbientPaint = {
  imageOpacity: .72,
  scrimColor: "rgba(0,0,0,.12)",
} as const;

export const ARTWORK_BACKDROP_FEATHER_FRACTION = .35;

// Apple Music continues the cover into the controls with the same blurred
// artwork. A long ramp avoids the hard cut produced by a flat colour strip.
export const PORTRAIT_MOTION_BACKDROP_FEATHER_FRACTION = .48;

// Use the original landscape gradient's rightmost 30% coverage. Its existing
// alpha ramp starts transparent and progressively covers the sharp artwork,
// reaching the complete screen-aligned blurred backdrop at the right boundary.
// Tablets can request a narrow feather without changing the phone overlap.
export function phoneLandscapeBlurEdge(left: number, top: number, width: number, height: number, edgeWidth?: number) {
  const available = Math.max(0, width);
  const span = edgeWidth === undefined ? available * .30 : Math.min(available, Math.max(0, Number.isFinite(edgeWidth) ? edgeWidth : 0));
  return { left: left + Math.max(0, width) - span, top, width: span, height: Math.max(0, height) };
}

// The bottom edge uses the same screen-aligned backdrop as the lyric pane.
// Its vertical alpha ramp reaches that backdrop exactly at the cover boundary.
export function artworkBackdropBottomEdge(left: number, top: number, width: number, height: number, edgeHeight?: number) {
  const available = Math.max(0, height);
  const span = edgeHeight === undefined ? available * ARTWORK_BACKDROP_FEATHER_FRACTION : Math.min(available, Math.max(0, Number.isFinite(edgeHeight) ? edgeHeight : 0));
  return { left, top: top + available - span, width: Math.max(0, width), height: span };
}

// Portrait motion artwork uses the measured album frame and fades into the
// screen-aligned blurred backdrop above the controls.
export function portraitMotionBackdropEdge(left: number, top: number, width: number, height: number, edgeHeight?: number) {
  const available = Math.max(0, height);
  const span = edgeHeight === undefined ? available * PORTRAIT_MOTION_BACKDROP_FEATHER_FRACTION : Math.min(available, Math.max(0, Number.isFinite(edgeHeight) ? edgeHeight : 0));
  return { left, top: top + available - span, width: Math.max(0, width), height: span };
}

// Preserve most of the sharp cover while giving its boundary a gradual blend.
// Soften only the outer tenth (up to 48 points) and let a blurred
// continuation extend OUTSIDE it, rather than replacing its right-hand third.
export function artworkRightContinuation(left: number, top: number, width: number, height: number) {
  const sourceWidth = Math.max(0, width), sourceHeight = Math.max(0, height);
  const innerWidth = Math.min(48, sourceWidth * .10);
  const outerWidth = Math.min(160, sourceWidth * .25);
  return { left: left + sourceWidth - innerWidth, top, width: innerWidth + outerWidth, height: sourceHeight,
    innerWidth, outerWidth, imageLeft: outerWidth - sourceWidth + innerWidth,
    sourceWidth };
}
