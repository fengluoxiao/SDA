import type { DecodedFrameData } from "@sda/core";

/** The conventional 7.1.4 bed order used by the SDA renderer. */
export const ALAC_STEREO_UPMIX_LABELS = [
  "L", "R", "C", "LFE", "Ls", "Rs", "Lb", "Rb", "Tfl", "Tfr", "Trl", "Trr",
] as const;

export function isAlacStereoFrame(frame: Pick<DecodedFrameData, "codec" | "channels" | "labels" | "objectChannels" | "events">): boolean {
  return frame.codec === "alac"
    && frame.channels.length === 2
    && frame.labels.length === 2
    && frame.objectChannels.length === 0
    && frame.events.length === 0
    && ["L", "Left", "FrontLeft"].includes(frame.labels[0]!)
    && ["R", "Right", "FrontRight"].includes(frame.labels[1]!);
}

/**
 * Conservative pseudo-upmix for decoded ALAC stereo. It preserves the direct
 * L/R image, derives centre from mono, and limits surround/height to the
 * decorrelated L-R component. This is a listening transform, not Atmos data.
 */
export class AlacStereoUpmixer {
  private lfeState = 0;
  private nextSample: number | null = null;

  reset(): void {
    this.lfeState = 0;
    this.nextSample = null;
  }

  upmix(frame: DecodedFrameData): DecodedFrameData {
    if (!isAlacStereoFrame(frame)) return frame;
    const left = frame.channels[0]!;
    const right = frame.channels[1]!;
    const samples = Math.min(left.length, right.length);
    if (this.nextSample !== frame.samplePos) this.lfeState = 0;
    this.nextSample = frame.samplePos + samples;

    const channels = Array.from({ length: ALAC_STEREO_UPMIX_LABELS.length }, () => new Float32Array(samples));
    const cutoff = 120;
    const alpha = 1 - Math.exp(-2 * Math.PI * cutoff / frame.sampleRate);
    for (let sample = 0; sample < samples; sample++) {
      const l = left[sample]!;
      const r = right[sample]!;
      const mono = (l + r) * 0.5;
      const side = (l - r) * 0.5;
      this.lfeState += alpha * (mono - this.lfeState);

      channels[0]![sample] = l * 0.8;
      channels[1]![sample] = r * 0.8;
      channels[2]![sample] = mono * 0.5;
      channels[3]![sample] = this.lfeState * 0.25;
      channels[4]![sample] = side * 0.34;
      channels[5]![sample] = -side * 0.34;
      channels[6]![sample] = side * 0.2;
      channels[7]![sample] = -side * 0.2;
      channels[8]![sample] = side * 0.14;
      channels[9]![sample] = -side * 0.14;
      channels[10]![sample] = side * 0.1;
      channels[11]![sample] = -side * 0.1;
    }
    return {
      ...frame,
      channels,
      labels: [...ALAC_STEREO_UPMIX_LABELS],
      // Retain the decoded source labels so the UI can identify this as an
      // ALAC stereo master rather than an authored immersive bed.
      rawBedLabels: [...frame.rawBedLabels],
    };
  }
}
