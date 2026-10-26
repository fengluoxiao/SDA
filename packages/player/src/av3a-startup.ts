import { masterBalanceGainDb } from "./bs1770.js";

// Same native thresholds: ALAC may use seven valid windows; other references
// retain 57. The caller selects one gain and never retargets a playing track.
export const AV3A_STARTUP_MAX_SECONDS = 7;
export function av3aStartupDecision(
  measurement: { blocks: number; integratedLufs: number | null; truePeakDbtp?: number | null } | null,
  decodedSeconds: number,
  ended: boolean,
  requiredBlocks = 57,
): { ready: boolean; gainDb: number | null } {
  const measured = (measurement?.blocks ?? 0) >= requiredBlocks;
  const ready = measured || ended || decodedSeconds >= AV3A_STARTUP_MAX_SECONDS;
  const lufs = measurement?.integratedLufs;
  return { ready, gainDb: ready && (measured || ended) && lufs != null && Number.isFinite(lufs)
    ? masterBalanceGainDb(lufs, measurement?.truePeakDbtp ?? null) : null };
}
