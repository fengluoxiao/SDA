import type { DecodedFrameData } from "@sda/core";

/** Bound transport overhead without changing the codec clock or object events. */
export class FrameBatcher {
  private frames: DecodedFrameData[] = [];
  private samples = 0;

  constructor(private readonly emit: (frame: DecodedFrameData) => void) {}

  push(frame: DecodedFrameData): void {
    const first = this.frames[0];
    if (first && (
      frame.samplePos !== first.samplePos + this.samples
      || frame.codec !== first.codec
      || frame.sampleRate !== first.sampleRate
      || frame.channels.length !== first.channels.length
      || JSON.stringify(frame.labels) !== JSON.stringify(first.labels)
      || JSON.stringify(frame.rawBedLabels) !== JSON.stringify(first.rawBedLabels)
      || JSON.stringify(frame.programLoudness) !== JSON.stringify(first.programLoudness)
      || frame.objectChannels.length > 0
    )) this.flush();
    this.frames.push(frame);
    this.samples += frame.channels[0]?.length ?? 0;
    if (this.samples >= frame.sampleRate * 0.02) this.flush();
  }

  flush(): void {
    const first = this.frames[0];
    if (!first) return;
    const frames = this.frames;
    const count = this.samples;
    this.frames = [];
    this.samples = 0;
    if (frames.length === 1) {
      this.emit(first);
      return;
    }
    const channels = first.channels.map((_, ch) => {
      const pcm = new Float32Array(count);
      let offset = 0;
      for (const frame of frames) {
        pcm.set(frame.channels[ch]!, offset);
        offset += frame.channels[ch]!.length;
      }
      return pcm;
    });
    this.emit({ ...first, channels, events: frames.flatMap(frame => frame.events) });
  }
}
