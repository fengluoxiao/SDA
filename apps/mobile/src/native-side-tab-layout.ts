export const NATIVE_SIDE_TAB_LENGTH = 140;

export type SideTabInsets = { left: number; right: number; bottom: number };
const nonnegative = (value: number) => Number.isFinite(value) ? Math.max(0, value) : 0;

// Rotate only the visible side bar. Give its UIKit controller extra room
// for its own insets, then offset that room before rotation. Item icons
// belong to UIKit and follow the same native bar layout.
export function nativeSideTabFrame(insets: SideTabInsets) {
  const bottom = nonnegative(insets.bottom);
  // The controller's bottom-safe strip is already retained on its short axis.
  // Reserve only the remaining side clearance on the rotated long axis;
  // adding both full side insets made the visible bar longer than its dock.
  const left = Math.max(0, nonnegative(insets.left) - bottom);
  const right = Math.max(0, nonnegative(insets.right) - bottom);
  return { width: NATIVE_SIDE_TAB_LENGTH + left + right, height: 48 + bottom, left: -left, top: 0 };
}
