import { masterBalanceGainDb } from "./bs1770.js";

// Same 57 overlapping windows as the existing streaming controller. Only the
// first-output boundary changes; measurement, target and later updates do not.
export const AV3A_STARTUP_MAX_SECONDS = 7;
export function av3aStartupDecision(
  measurement: { blocks: number; integratedLufs: number | null; truePeakDbtp?: number | null } | null,
  decodedSeconds: number,
  ended: boolean,
): { ready: boolean; gainDb: number | null } {
  const measured = (measurement?.blocks ?? 0) >= 57;
  const ready = measured || ended || decodedSeconds >= AV3A_STARTUP_MAX_SECONDS;
  const lufs = measurement?.integratedLufs;
  return { ready, gainDb: ready && (measured || ended) && lufs != null && Number.isFinite(lufs)
    ? masterBalanceGainDb(lufs, measurement?.truePeakDbtp ?? null) : null };
}
