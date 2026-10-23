export type MotionRect = { x: number; y: number; width: number; height: number };
export type MiniPlayerFrames = { artwork: MotionRect; progress: MotionRect; time: MotionRect; mini: MotionRect };
export const MINI_MOTION_SAMPLES = Array.from({ length: 21 }, (_, index) => index / 20);
export const MINI_MOTION_DURATION = 460;
export function validMotionRect(rect: MotionRect | null): rect is MotionRect {
  return !!rect && Object.values(rect).every(Number.isFinite) && rect.width > 0 && rect.height > 0;
}
export function relativeMotionRect(rect: MotionRect, root: MotionRect): MotionRect {
  return { ...rect, x: rect.x - root.x, y: rect.y - root.y };
}
export function miniArtworkRect(mini: MotionRect): MotionRect { return { x: mini.x + 7, y: mini.y + 7, width: 34, height: 34 }; }
export function miniTimeCenter(mini: MotionRect) { return { x: mini.x + 24, y: mini.y + 48.5 }; }
const mix = (a: number, b: number, t: number) => a + (b - a) * t;
const smooth = (value: number) => { const t = Math.max(0, Math.min(1, value)); return t * t * (3 - 2 * t); };
// Each element travels directly between its own measured endpoints. The return
// separates artwork, progress and time immediately; there is no shared orbit.
export function miniMotionPose(frames: MiniPlayerFrames, phase: number) {
  const t = Math.max(0, Math.min(1, phase));
  const shrink = smooth(t / .55), coverTravel = smooth((t - .04) / .96);
  const progressTravel = smooth(t), timeTravel = smooth(t);
  const source = { x: frames.artwork.x + frames.artwork.width / 2, y: frames.artwork.y + frames.artwork.height / 2 };
  const destination = { x: frames.mini.x + 24, y: frames.mini.y + 24 };
  const center = { x: mix(source.x, destination.x, coverTravel), y: mix(source.y, destination.y, coverTravel) };
  const width = mix(frames.artwork.width, 34, shrink), height = mix(frames.artwork.height, 34, shrink);
  const ringCenter = { x: mix(frames.progress.x + frames.progress.width / 2, destination.x, progressTravel),
    y: mix(frames.progress.y + frames.progress.height / 2, destination.y, progressTravel) };
  const timeDestination = miniTimeCenter(frames.mini);
  const timeCenter = { x: mix(frames.time.x + frames.time.width / 2, timeDestination.x, timeTravel),
    y: mix(frames.time.y + frames.time.height / 2, timeDestination.y, timeTravel) };
  // Collapse the straight bar before curling it. In reverse, straighten the
  // compact ring first, then extend the line: never bow a full-width track.
  const progressShrink = smooth(t / .65), bend = smooth((t - .65) / .35);
  return { center, width, height, shrink, progressShrink, bend, ringCenter, timeCenter,
    round: smooth((t - .2) / .35), symbolOpacity: smooth((t - .75) / .25), timeScale: mix(1, 9 / 11, timeTravel) };
}
export function progressMotionSegments(frames: MiniPlayerFrames, fraction = 1, count = 24) {
  const ratio = Number.isFinite(fraction) ? Math.max(0, Math.min(1, fraction)) : 0;
  if (ratio === 0) return [];
  const point = (u: number, t: number) => {
    const pose = miniMotionPose(frames, t), angle = (135 + 270 * ratio * u) * Math.PI / 180;
    const length = mix(frames.progress.width, 46, pose.progressShrink);
    return { x: pose.ringCenter.x + mix((ratio * u - .5) * length, 23 * Math.cos(angle), pose.bend),
      y: pose.ringCenter.y + 23 * Math.sin(angle) * pose.bend };
  };
  return Array.from({ length: count }, (_, index) => {
    let previousAngle = 0;
    const samples = MINI_MOTION_SAMPLES.map(t => {
      const a = point(index / count, t), b = point((index + 1) / count, t);
      let angle = Math.atan2(b.y - a.y, b.x - a.x) * 180 / Math.PI;
      while (angle - previousAngle > 180) angle -= 360;
      while (angle - previousAngle < -180) angle += 360;
      previousAngle = angle;
      return { ax: a.x, ay: a.y, bx: b.x, by: b.y, x: (a.x + b.x) / 2, y: (a.y + b.y) / 2, angle: angle + "deg",
        width: Math.hypot(b.x - a.x, b.y - a.y) + .65, height: mix(frames.progress.height, 2, miniMotionPose(frames, t).progressShrink) };
    });
    return { ax: samples.map(s => s.ax), ay: samples.map(s => s.ay), bx: samples.map(s => s.bx), by: samples.map(s => s.by),
      x: samples.map(s => s.x), y: samples.map(s => s.y), angle: samples.map(s => s.angle), width: samples.map(s => s.width), height: samples.map(s => s.height) };
  });
}
