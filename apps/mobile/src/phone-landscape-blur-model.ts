// Feather only the final 8% (at most 32pt) of the existing sharp cover.
// A transparent-to-opaque alpha mask reveals the screen-aligned blurred
// backdrop, matching it exactly at the original cover's right boundary.
export function phoneLandscapeBlurEdge(left: number, top: number, width: number, height: number) {
  const span = Math.min(32, Math.max(0, width) * .08);
  return { left: left + Math.max(0, width) - span, top, width: span, height: Math.max(0, height) };
}
