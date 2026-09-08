import { create, ConverterType } from "@alexanderolsen/libsamplerate-js";
import type { DecodedFrameData, ObjectEvent } from "@sda/core";

/** Adapt decoded PCM and its object timeline to the native renderer's clock. */
export class AlacResampler {
  private converter: Awaited<ReturnType<typeof create>> | null = null;
  private template: DecodedFrameData | null = null;
  private inputSamples = 0;
  private outputSamples = 0;
  private events: ObjectEvent[] = [];

  constructor(private readonly targetRate: number) {}

  async push(frame: DecodedFrameData): Promise<DecodedFrameData | null> {
    if (frame.sampleRate === this.targetRate && !this.converter) return frame;
    if (!["alac", "pcm", "adm"].includes(frame.codec)) {
      throw new Error(`Native ${this.targetRate} Hz output requires sample-clock conversion for ${frame.codec} ${frame.sampleRate} Hz`);
    }
    if (frame.samplePos !== this.inputSamples) throw new Error("PCM resampler received a discontinuous input clock");
    if (this.template && (frame.codec !== this.template.codec || frame.sampleRate !== this.template.sampleRate || frame.labels.join() !== this.template.labels.join())) {
      throw new Error("PCM format changed without reopening the resampler");
    }
    this.template ??= frame;
    const count = frame.channels.length;
    this.converter ??= await create(count, frame.sampleRate, this.targetRate, {
      converterType: ConverterType.SRC_SINC_BEST_QUALITY,
    });
    const samples = frame.channels[0]?.length ?? 0;
    if (!count || frame.channels.some(channel => channel.length !== samples)) throw new Error("Invalid PCM channel lengths");
    const ratio = this.targetRate / frame.sampleRate;
    for (const event of frame.events) {
      const samplePos = Math.round(event.samplePos * ratio);
      this.events.push({ ...event, samplePos, rampDuration: Math.round((event.samplePos + event.rampDuration) * ratio) - samplePos });
    }
    const interleaved = new Float32Array(samples * count);
    for (let i = 0; i < samples; i++) for (let ch = 0; ch < count; ch++) interleaved[i * count + ch] = frame.channels[ch]![i]!;
    this.inputSamples += samples;
    return this.output(this.converter.full(interleaved));
  }

  finish(): DecodedFrameData | null {
    if (!this.converter || !this.template) return null;
    const expected = Math.round(this.inputSamples * this.targetRate / this.template.sampleRate);
    const remaining = expected - this.outputSamples;
    if (remaining < 0) throw new Error("PCM resampler exceeded the programme duration");
    // The wrapper exposes streaming SRC without an end-of-input flag. Feed
    // silence for filter lookahead, then retain only the real programme length.
    const count = this.template.channels.length;
    const padding = Math.ceil(this.template.sampleRate * 0.1);
    const tail = this.converter.full(new Float32Array(padding * count));
    if (tail.length < remaining * count) throw new Error("PCM resampler did not flush its full tail");
    const frame = this.output(tail.subarray(0, remaining * count));
    this.destroy();
    return frame;
  }

  destroy(): void {
    this.converter?.destroy();
    this.converter = null;
    this.template = null;
    this.inputSamples = 0;
    this.outputSamples = 0;
    this.events = [];
  }

  private output(pcm: Float32Array): DecodedFrameData | null {
    if (!pcm.length || !this.template) return null;
    const count = this.template.channels.length;
    const samples = pcm.length / count;
    const channels = Array.from({ length: count }, (_, ch) => {
      const output = new Float32Array(samples);
      for (let i = 0; i < samples; i++) output[i] = pcm[i * count + ch]!;
      return output;
    });
    // SRC retains lookahead. Only release events once their corresponding PCM
    // exists, including events carried by the final flushed samples.
    const end = this.outputSamples + samples;
    const readyEvents = this.events.filter(event => event.samplePos < end);
    this.events = this.events.filter(event => event.samplePos >= end);
    const frame = {
      ...this.template,
      sampleRate: this.targetRate,
      samplePos: this.outputSamples,
      channels,
      events: readyEvents,
      rampDuration: Math.round(this.template.rampDuration * this.targetRate / this.template.sampleRate),
    };
    this.outputSamples += samples;
    return frame;
  }
}
