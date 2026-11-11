export type CardboardConfig = {
  enabled: boolean; eyeDistanceMm: number; lensDistanceMm: number; screenWidthMm: number;
  fov: number; k1: number; k2: number;
};
/** Manual starting point, not a calibrated profile for a particular viewer. */
export const CARDBOARD_DEFAULTS: CardboardConfig = {
  enabled: false, eyeDistanceMm: 64, lensDistanceMm: 64, screenWidthMm: 140, fov: 85, k1: .22, k2: .24,
};
export const CARDBOARD_RANGES = {
  eyeDistanceMm: [50, 76, 1], lensDistanceMm: [50, 80, 1], screenWidthMm: [90, 190, 1],
  fov: [50, 110, 1], k1: [0, .8, .02], k2: [0, .5, .02],
} as const;
export function normalizeCardboardConfig(value: unknown): CardboardConfig {
  const result = { ...CARDBOARD_DEFAULTS };
  if (!value || typeof value !== "object") return result;
  const saved = value as Record<string, unknown>;
  if (typeof saved.enabled === "boolean") result.enabled = saved.enabled;
  for (const key of Object.keys(CARDBOARD_RANGES) as (keyof typeof CARDBOARD_RANGES)[]) {
    const candidate = saved[key];
    if (typeof candidate === "number" && Number.isFinite(candidate)) {
      const [min, max] = CARDBOARD_RANGES[key]; result[key] = Math.max(min, Math.min(max, candidate));
    }
  }
  return result;
}
/** Output display UV to undistorted source UV; same mapping as the GPU shader. */
export function cardboardSampleUv(x: number, y: number, eye: 0 | 1, aspect: number, config: CardboardConfig) {
  const centerX = eye === 0 ? 1 - config.lensDistanceMm / config.screenWidthMm : config.lensDistanceMm / config.screenWidthMm;
  const tangent = Math.tan(config.fov * Math.PI / 360);
  const dx = (x - centerX) * 2 * aspect * tangent, dy = (y - .5) * 2 * tangent;
  const r2 = dx * dx + dy * dy, scale = 1 + config.k1 * r2 + config.k2 * r2 * r2;
  return { x: .5 + dx * scale / (2 * aspect * tangent), y: .5 + dy * scale / (2 * tangent) };
}
