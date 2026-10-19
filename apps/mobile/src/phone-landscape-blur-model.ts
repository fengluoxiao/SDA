// Use the original landscape gradient's rightmost 30% coverage. Its existing
// alpha ramp starts transparent and progressively covers the sharp artwork,
// reaching the complete screen-aligned blurred backdrop at the right boundary.
export function phoneLandscapeBlurEdge(left: number, top: number, width: number, height: number) {
  const span = Math.max(0, width) * .30;
  return { left: left + Math.max(0, width) - span, top, width: span, height: Math.max(0, height) };
}
