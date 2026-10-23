export const NATIVE_SIDE_TAB_LENGTH = 140;
// Regular-width UIKit needs room for all three items instead of its overflow picker.
export const NATIVE_TABLET_SIDE_TAB_LENGTH = 320;
// UIKit needs a wide controller to fit all items, but its floating capsule is
// intrinsically shorter. Dock the visible capsule, not the controller's blank space.
export const NATIVE_TABLET_SIDE_TAB_VISIBLE_LENGTH = 160;

export type SideTabInsets = { left: number; right: number; bottom: number };
const nonnegative = (value: number) => Number.isFinite(value) ? Math.max(0, value) : 0;

// Rotate only the visible side bar. Give its UIKit controller extra room
// for its own insets, then offset that room before rotation. Item icons
// belong to UIKit and follow the same native bar layout.
export function nativeSideTabFrame(insets: SideTabInsets, length = NATIVE_SIDE_TAB_LENGTH) {
  const bottom = nonnegative(insets.bottom);
  // The controller's bottom-safe strip is already retained on its short axis.
  // Reserve only the remaining side clearance on the rotated long axis;
  // adding both full side insets made the visible bar longer than its dock.
  const left = Math.max(0, nonnegative(insets.left) - bottom);
  const right = Math.max(0, nonnegative(insets.right) - bottom);
  return { width: length + left + right, height: 48 + bottom, left: -left, top: 0 };
}
