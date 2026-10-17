export type RailPoint = { x: number; y: number };
export function sideRailIndex(point: RailPoint): number | null {
  if (point.x < -12 || point.x > 60 || point.y < -12 || point.y > 152) return null;
  return Math.max(0, Math.min(2, Math.floor((point.y - 2) / 46)));
}

// Tap selects on release. A held touch previews other tabs while sliding;
// releasing commits once, and leaving the rail or cancellation never picks a tab.
export function createSideRailGesture() {
  let start: RailPoint | null = null, latest: RailPoint | null = null;
  let started = 0, phase: "idle" | "pending" | "dragging" | "cancelled" = "idle";
  let preview: number | null = null;
  const cancel = () => { start = latest = null; phase = "idle"; preview = null; };
  const begin = (point: RailPoint, now: number) => { start = latest = point; started = now; phase = "pending"; preview = sideRailIndex(point); return preview; };
  const move = (point: RailPoint, now: number) => {
    if (!start || phase === "cancelled" || phase === "idle") return null;
    if (phase === "pending" && now - started >= 300) phase = "dragging";
    if (phase === "pending" && Math.hypot(point.x - start.x, point.y - start.y) > 10) { phase = "cancelled"; preview = null; }
    latest = point;
    if (phase === "dragging") preview = sideRailIndex(point);
    return preview;
  };
  const hold = (now: number) => latest ? move(latest, now) : null;
  const end = (point: RailPoint, now: number) => { const value = move(point, now); cancel(); return value; };
  return { begin, move, hold, end, cancel };
}
