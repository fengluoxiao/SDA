import { DEFAULT_LATENT_BACKGROUND_TUNING as T, PAUSE_EASE_MS, resolveLatentBroadbandEnergy, resolveLatentShaderSpeed, resolveLatentShaderColors } from "../../../vendor/folia/latent.generated";
import type { AiSceneTheme } from "./ai-scene";
export type FoliaAudioSignal = readonly number[]; // power,bass,lowMid,mid,vocal,treble; measured, normalized 0..255
export { T as FOLIA_DEFAULTS };
export function foliaColors(theme: AiSceneTheme) {
  return resolveLatentShaderColors(theme.colors, theme, T.colorSource);
}
/** A quiet spatial backdrop: original Folia mesh shader, without patterned
 * dithering, zoom/brightness pulses or audio-driven swirls. */
export const FOLIA_SPATIAL_TUNING = Object.freeze({ speed: .035, audioSpeed: .075, distortion: .16, swirl: 0 });
export class FoliaLatentDriver {
  meshTime = 0; playing = 0; power = 0;
  constructor(initialPaused = false) { this.playing = initialPaused ? 0 : 1; }
  private energy = 0;
  step(dt: number, signal: FoliaAudioSignal | undefined, paused: boolean, reduced: boolean) {
    const delta = Math.min(.05, Math.max(0, Number.isFinite(dt) ? dt : 0));
    const value = (index: number) => !paused && Number.isFinite(signal?.[index]) ? Math.max(0, Math.min(255, signal![index]!)) : 0;
    if (reduced) this.power = this.energy = 0;
    else {
      this.playing += ((paused ? 0 : 1) - this.playing) * (1 - Math.exp(-delta * 1000 / PAUSE_EASE_MS));
      this.power += (value(0) / 255 - this.power) * (1 - Math.exp(-delta * 3));
      const energy = paused ? 0 : resolveLatentBroadbandEnergy(value(1), value(2), value(3), value(4), value(5));
      this.energy += (energy - this.energy) * (1 - Math.exp(-delta * 1.5));
      const t = FOLIA_SPATIAL_TUNING;
      const quiet = resolveLatentShaderSpeed(t.speed, t.audioSpeed, this.energy, true);
      const live = resolveLatentShaderSpeed(t.speed, t.audioSpeed, this.energy, false);
      this.meshTime += delta * (quiet + (live - quiet) * this.playing);
    }
    return { meshTime: this.meshTime, distortion: FOLIA_SPATIAL_TUNING.distortion,
      swirl: FOLIA_SPATIAL_TUNING.swirl, meshScale: 1, saturation: 1, brightness: 1, power: this.power };
  }
}

/** A useFrame callback is outside React error boundaries. Latch an individual
 * visual failure, report it once, and let the native audio player continue. */
export class FoliaFrameGuard {
  failed = false;
  fail(error: unknown, report?: (error: unknown) => void): void {
    if (this.failed) return;
    this.failed = true; report?.(error);
  }
  run(frame: () => void, report?: (error: unknown) => void): boolean {
    if (this.failed) return false;
    try { frame(); return true; }
    catch (error) { this.fail(error, report); return false; }
  }
}
