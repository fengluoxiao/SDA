import { DEFAULT_LATENT_BACKGROUND_TUNING as T, PAUSE_EASE_MS, resolveLatentBroadbandEnergy, resolveLatentOnsetPulse, resolveLatentAudioSpeedTarget, resolveLatentShaderSpeed, resolveLatentShaderColors } from "../../../vendor/folia/latent.generated";
import type { AiSceneTheme } from "./ai-scene";
export type FoliaAudioSignal = readonly number[]; // power,bass,lowMid,mid,vocal,treble; measured, normalized 0..255
export { T as FOLIA_DEFAULTS };
export function foliaColors(theme: AiSceneTheme) {
  return resolveLatentShaderColors(theme.colors, theme, T.colorSource);
}
/** Native scheduling of Folia's original pause, onset and shader-speed logic. */
export class FoliaLatentDriver {
  meshTime = 0; ditheringTime = 0; playing = 0; power = 0; bass = 0; mid = 0;
  constructor(initialPaused = false) { this.playing = initialPaused ? 0 : 1; }
  private previousEnergy = 0; private onset = 0; private energy = 0;
  step(dt: number, signal: FoliaAudioSignal | undefined, paused: boolean, reduced: boolean) {
    const delta = Math.min(.05, Math.max(0, Number.isFinite(dt) ? dt : 0));
    if (reduced) {
      this.power = this.bass = this.mid = this.energy = this.previousEnergy = this.onset = 0;
      return { meshTime: this.meshTime, ditheringTime: this.ditheringTime, pixelSize: T.ditheringSize,
        opacity: T.ditheringOpacity, distortion: T.meshDistortion, swirl: T.meshSwirl,
        meshScale: 1.025, ditherScale: 1.015, saturation: 1.04, brightness: .94, power: 0 };
    }
    const value = (index: number) => !paused && Number.isFinite(signal?.[index]) ? Math.max(0, Math.min(255, signal![index]!)) : 0;
    this.playing += ((paused ? 0 : 1) - this.playing) * (1 - Math.exp(-delta * 1000 / PAUSE_EASE_MS));
    this.power += (value(0) / 255 - this.power) * .12;
    this.bass += (value(1) / 255 - this.bass) * .16;
    this.mid += (Math.max(value(3), value(4)) / 255 - this.mid) * .13;
    const energy = paused ? 0 : resolveLatentBroadbandEnergy(value(1), value(2), value(3), value(4), value(5));
    this.onset = resolveLatentOnsetPulse(energy, this.previousEnergy, this.onset); this.previousEnergy = energy;
    const target = resolveLatentAudioSpeedTarget(energy, this.onset, T.enhancedBeatResponse);
    this.energy += (target - this.energy) * (target > this.energy ? .42 : .14);
    const speed = (base: number, audio: number) => {
      const quiet = resolveLatentShaderSpeed(base, audio, this.energy, true), live = resolveLatentShaderSpeed(base, audio, this.energy, false);
      return reduced ? 0 : quiet + (live - quiet) * this.playing;
    };
    this.meshTime += delta * speed(T.meshSpeed, T.meshAudioSpeed);
    this.ditheringTime += delta * speed(T.ditheringSpeed, T.ditheringAudioSpeed);
    return { meshTime: this.meshTime, ditheringTime: this.ditheringTime,
      pixelSize: Math.max(.5, T.ditheringSize - this.bass * T.ditheringSize * .34),
      opacity: Math.min(1, T.ditheringOpacity + this.bass * .25),
      distortion: T.meshDistortion + this.power * .62, swirl: T.meshSwirl + this.mid * .38,
      meshScale: 1.025 + this.power * .018, ditherScale: 1.015 + this.bass * .025,
      saturation: 1.04 + this.mid * .34, brightness: .94 + this.power * .16, power: this.power };
  }
}
