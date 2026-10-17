export type RailPoint = { x: number; y: number };
export function sideRailIndex(point: RailPoint, tracking = false): number | null {
  // Tracking is forgiving after a gesture starts on the actual 48x140 rail.
  // Slight diagonals and endpoint overshoot must not cancel a deliberate scrub.
  const marginX = tracking ? 64 : 12;
  const marginY = tracking ? 140 : 12;
  if (point.x < -marginX || point.x > 48 + marginX || point.y < -marginY || point.y > 140 + marginY) return null;
  return Math.max(0, Math.min(2, Math.floor((point.y - 2) / 46)));
}

// Tap selects on release. A held touch previews other tabs while sliding;
// releasing commits once, and leaving the rail or cancellation never picks a tab.
export function createSideRailGesture() {
  let start: RailPoint | null = null, latest: RailPoint | null = null;
  let started = 0, phase: "idle" | "pending" | "dragging" = "idle";
  let preview: number | null = null;
  const cancel = () => { start = latest = null; phase = "idle"; preview = null; };
  const begin = (point: RailPoint, now: number) => { start = latest = point; started = now; phase = "pending"; preview = sideRailIndex(point); return preview; };
  const move = (point: RailPoint, now: number) => {
    if (!start || phase === "idle") return null;
    // Scrubbing inside a navigation rail is intentional even if movement starts
    // before the long-press timer. Do not cancel the gesture for that movement.
    if (phase === "pending" && (now - started >= 300 || Math.hypot(point.x - start.x, point.y - start.y) >= 6)) phase = "dragging";
    latest = point;
    if (phase === "dragging") preview = sideRailIndex(point, true);
    return preview;
  };
  const hold = (now: number) => latest ? move(latest, now) : null;
  const end = (point: RailPoint, now: number) => { const value = move(point, now); cancel(); return value; };
  return { begin, move, hold, end, cancel };
}

// Indicator top offset relative to the resting first segment. Unlike the
// selected tab index, this is continuous while the finger moves between tabs.
export function sideRailIndicatorOffset(y: number): number {
  return Math.max(0, Math.min(92, y - 24));
}
