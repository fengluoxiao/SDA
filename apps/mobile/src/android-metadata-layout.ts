// The native sheet owns width, insets and anchors. Keep its complete header
// below the status area, including short landscape and multi-window viewports.
export function androidMetadataBodyHeight(height: number, top: number, bottom: number): number {
  const finite = (value: number) => Number.isFinite(value) ? Math.max(0, value) : 0;
  const dragHandle = 28;
  const topClearance = Math.max(16, finite(top)) + 12;
  const bottomClearance = Math.max(16, finite(bottom));
  return Math.max(0, Math.floor(finite(height) - dragHandle - topClearance - bottomClearance));
}
