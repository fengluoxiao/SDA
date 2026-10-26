import { MPEGDecoder } from "mpg123-decoder";
import type { DecodedFrameData } from "@sda/core";

/** Match an MPEG Layer III header after a complete optional ID3v2 tag. */
export function sniffMp3(bytes: Uint8Array): "mp3" | "need-more" | null {
  let offset = 0;
  if (bytes[0] === 0x49 && bytes[1] === 0x44 && bytes[2] === 0x33) {
    if (bytes.length < 10) return "need-more";
    if (bytes[3]! < 2 || bytes[3]! > 4 || bytes.subarray(6, 10).some(value => value > 127)) return null;
    const size = bytes[6]! * 2097152 + bytes[7]! * 16384 + bytes[8]! * 128 + bytes[9]!;
    if (size > 16 * 1024 * 1024) throw new Error("MP3 ID3 tag exceeds 16 MiB");
    offset = 10 + size + (bytes[3] === 4 && (bytes[5]! & 0x10) ? 10 : 0);
    if (bytes.length < offset + 4) return "need-more";
  }
  const a = bytes[offset], b = bytes[offset + 1], c = bytes[offset + 2];
  if (a !== 0xff || b === undefined || c === undefined) return null;
  // Reject reserved version, non-Layer-III, free/reserved bitrate and rate.
  return (b & 0xe0) === 0xe0 && (b & 0x18) !== 0x08 && (b & 6) === 2
    && (c >> 4) > 0 && (c >> 4) < 15 && (c & 12) !== 12 ? "mp3" : null;
}

/** mpg123 runs inside the existing decoder worker, never on the audio thread. */
export class Mp3Decoder {
  private readonly inner = new MPEGDecoder();
  private frames: DecodedFrameData[] = [];
  private errors: string[] = [];
  private samplePos = 0;
  private sampleRate = 0;
  private constructor(private readonly onFormat: (rate: number) => void) {}

  static async create(onFormat: (rate: number) => void): Promise<Mp3Decoder> {
    const decoder = new Mp3Decoder(onFormat);
    await decoder.inner.ready;
    return decoder;
  }

  push(bytes: Uint8Array): void {
    const decoded = this.inner.decode(bytes);
    for (const error of decoded.errors) this.errors.push(`MP3: ${error.message}`);
    if (!decoded.samplesDecoded) return;
    if (this.sampleRate && decoded.sampleRate !== this.sampleRate) throw new Error("MP3 sample rate changed during playback");
    if (!this.sampleRate) { this.sampleRate = decoded.sampleRate; this.onFormat(this.sampleRate); }
    const left = decoded.channelData[0];
    const right = decoded.channelData[1] ?? left;
    if (!left || !right) throw new Error("MP3 decoder returned no PCM channels");
    for (let offset = 0; offset < decoded.samplesDecoded; offset += 4096) {
      const end = Math.min(offset + 4096, decoded.samplesDecoded);
      // Owned copies: transferring these buffers must never detach mpg123 memory.
      this.frames.push({
        codec: "mp3", sampleRate: this.sampleRate, samplePos: this.samplePos,
        channels: [left.slice(offset, end), right.slice(offset, end)],
        labels: ["L", "R"], rawBedLabels: ["L", "R"], events: [], objectChannels: [],
        programLoudness: null, rampDuration: 0,
      });
      this.samplePos += end - offset;
    }
  }
  flush(): void { if (!this.samplePos) this.errors.push("MP3 stream contained no decodable audio"); }
  nextFrame(): DecodedFrameData | undefined { return this.frames.shift(); }
  drainErrors(): string[] { const errors = this.errors; this.errors = []; return errors; }
  free(): void { this.inner.free(); this.frames = []; this.errors = []; }
}
