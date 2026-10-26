import type { DecodedFrameData } from "@sda/core";

export const ALAC_STEREO_UPMIX_LABELS = ["L", "R", "C", "LFE", "Ls", "Rs", "Lb", "Rb", "Tfl", "Tfr", "Trl", "Trr"] as const;
export const SDA_UPMIX_LEVEL_TRIM_DB = -2;
const SDA_UPMIX_LEVEL_GAIN = Math.fround(0.7943282);
export function isAlacStereoFrame(frame: Pick<DecodedFrameData, "codec" | "channels" | "labels" | "objectChannels" | "events">): boolean {
  return frame.codec === "alac" && frame.channels.length === 2 && frame.labels.length === 2
    && frame.objectChannels.length === 0 && frame.events.length === 0
    && ["L", "Left", "FrontLeft"].includes(frame.labels[0]!)
    && ["R", "Right", "FrontRight"].includes(frame.labels[1]!);
}
/** Native-mobile matrix. The authored side signal, including bass, stays intact. */
export class AlacStereoUpmixer {
  private lfeState = 0;
  private nextSample: number | null = null;
  private liveMix: number | null = null;
  reset(): void { this.lfeState = 0; this.nextSample = null; this.liveMix = null; }
  upmix(frame: DecodedFrameData): DecodedFrameData {
    if (!isAlacStereoFrame(frame)) return frame;
    if (this.nextSample !== frame.samplePos) this.reset();
    const left = frame.channels[0]!, right = frame.channels[1]!;
    const samples = Math.min(left.length, right.length);
    this.nextSample = frame.samplePos + samples;
    const channels = Array.from({ length: 12 }, () => new Float32Array(samples));
    const alpha = Math.fround(1 - Math.exp(-2 * Math.PI * 120 / frame.sampleRate));
    const f = Math.fround;
    for (let i = 0; i < samples; i++) {
      const l = left[i]!, r = right[i]!;
      const mono = f(f(l + r) * .5), side = f(f(l - r) * .5);
      this.lfeState = f(this.lfeState + f(alpha * f(mono - this.lfeState)));
      channels[0]![i] = l * f(.8); channels[1]![i] = r * f(.8);
      channels[2]![i] = mono * .5; channels[3]![i] = this.lfeState * .25;
      channels[4]![i] = side * f(.34); channels[5]![i] = -side * f(.34);
      channels[6]![i] = side * f(.2); channels[7]![i] = -side * f(.2);
      channels[8]![i] = side * f(.14); channels[9]![i] = -side * f(.14);
      channels[10]![i] = side * f(.1); channels[11]![i] = -side * f(.1);
    }
    return { ...frame, channels, labels: [...ALAC_STEREO_UPMIX_LABELS], rawBedLabels: [...frame.rawBedLabels] };
  }
  /** Always reserve the native 7.1.4 bed; reuse state through a 50 ms toggle. */
  liveSda(frame: DecodedFrameData, enabled: boolean): DecodedFrameData {
    if (!isAlacStereoFrame(frame)) return frame;
    const wet = this.upmix(frame);
    const target = enabled ? 1 : 0;
    let mix = this.liveMix ?? target;
    const step = Math.fround(1 / (frame.sampleRate * .05));
    for (let i = 0; i < frame.channels[0]!.length; i++) {
      mix = Math.fround(mix + Math.max(-step, Math.min(step, target - mix)));
      for (let channel = 0; channel < 12; channel++) {
        const dry = channel < 2 ? frame.channels[channel]![i]! : 0;
        const value = Math.fround(wet.channels[channel]![i]! * SDA_UPMIX_LEVEL_GAIN);
        wet.channels[channel]![i] = mix === 0 ? dry : mix === 1 ? value : Math.fround(dry + Math.fround(Math.fround(value - dry) * mix));
      }
    }
    this.liveMix = mix;
    return wet;
  }
}
