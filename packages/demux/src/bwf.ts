import { decodeDbmdBinauralMetadata, type BinauralRenderMetadata } from "./dbmd.js";
import { parseAdmMetadataStream, type AdmMetadata, type AdmObjectEvent } from "./adm.js";

interface PcmFormat {
  encoding: number; channels: number; sampleRate: number;
  bits: number; blockAlign: number; channelMask: number;
}
export interface BwfMetadata {
  format: PcmFormat; dataOffset: number; dataSize: number; fileSize: number;
  adm?: AdmMetadata; labels: string[]; binaural: BinauralRenderMetadata;
}
export interface BwfAudioTrack {
  codec: string; sampleRate: number; channels: number; durationSec: number; title?: string;
}
export interface BwfPcmFrame {
  codec: string; sampleRate: number; samplePos: number; channels: Float32Array[];
  labels: string[]; rawBedLabels: string[]; events: AdmObjectEvent[];
  objectChannels: { id: number; channel: number }[];
  programLoudness: null; rampDuration: number;
}
export interface BwfDemuxerCallbacks {
  onTrack?: (track: BwfAudioTrack) => void;
  onPcmFrame?: (frame: BwfPcmFrame) => void;
  onBinauralMetadata?: (metadata: BinauralRenderMetadata) => void;
  onError?: (message: string) => void;
}
const MAX_METADATA = 64 * 1024 * 1024;
const text = (bytes: Uint8Array) => new TextDecoder().decode(bytes);
const view = (bytes: Uint8Array) => new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
const missingBinaural = (): BinauralRenderMetadata => ({
  available: false, source: "dbmd", version: null, modeTable: [],
  elementMapping: "unavailable", error: "BWF input has no readable dbmd chunk",
});
function fail(message: string): never { throw new Error("BWF: " + message); }
function safe64(bytes: DataView, offset: number): number {
  const value = Number(bytes.getBigUint64(offset, true));
  if (!Number.isSafeInteger(value)) fail("64-bit chunk exceeds safe file offsets");
  return value;
}
function parseFormat(bytes: Uint8Array): PcmFormat {
  if (bytes.length < 16) fail("truncated fmt chunk");
  const data = view(bytes);
  let encoding = data.getUint16(0, true);
  const channels = data.getUint16(2, true), sampleRate = data.getUint32(4, true);
  const blockAlign = data.getUint16(12, true), bits = data.getUint16(14, true);
  let channelMask = 0;
  if (encoding === 0xfffe) {
    if (bytes.length < 40 || data.getUint16(16, true) < 22) fail("truncated extensible fmt");
    const validBits = data.getUint16(18, true);
    if (validBits !== 0 && validBits !== bits) fail("packed PCM valid-bit depths are unsupported");
    channelMask = data.getUint32(20, true);
    const guidTail = [0, 0, 0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113];
    if (guidTail.some((value, index) => bytes[26 + index] !== value)) fail("unsupported WAVE subformat GUID");
    encoding = data.getUint16(24, true);
  }
  if (!channels || channels > 128 || sampleRate < 8000 || sampleRate > 384000) fail("invalid PCM channel count or sample rate");
  if (!(encoding === 1 && [16, 24, 32].includes(bits)) && !(encoding === 3 && [32, 64].includes(bits))) fail("unsupported PCM encoding/bit depth");
  if (blockAlign !== channels * bits / 8 || data.getUint32(8, true) !== sampleRate * blockAlign) fail("inconsistent PCM alignment or byte rate");
  return { encoding, channels, sampleRate, bits, blockAlign, channelMask };
}
function pcmLabels(format: PcmFormat): string[] {
  if (format.channelMask) {
    const names = ["L", "R", "C", "LFE", "Lb", "Rb", "Lc", "Rc", "Cb", "Ls", "Rs", "Tc", "Tfl", "Tfc", "Tfr", "Tbl", "Tbc", "Tbr"];
    const labels = names.filter((_, bit) => (format.channelMask & (1 << bit)) !== 0);
    if (labels.length !== format.channels || format.channelMask >>> names.length) fail("unsupported PCM channel mask");
    return labels;
  }
  if (format.channels === 1) return ["C"];
  if (format.channels === 2) return ["L", "R"];
  return fail("multichannel PCM requires a channel mask or ADM mapping");
}

/** Scan chunk headers with bounded range reads, without loading the audio data. */
export async function readBwfMetadata(
  readRange: (offset: number, length: number) => Promise<Uint8Array>, fileSize: number,
): Promise<BwfMetadata> {
  if (!Number.isSafeInteger(fileSize) || fileSize < 12) fail("invalid file size");
  const read = async (offset: number, length: number) => {
    if (offset + length > fileSize) fail("chunk extends past end of file");
    const bytes = new Uint8Array(length);
    // Desktop IPC deliberately caps each read at 1 MiB.
    for (let consumed = 0; consumed < length;) {
      const count = Math.min(1024 * 1024, length - consumed);
      const part = await readRange(offset + consumed, count);
      if (part.length !== count) fail("truncated read at " + (offset + consumed));
      bytes.set(part, consumed);
      consumed += count;
    }
    return bytes;
  };
  const header = await read(0, 12);
  if (!BwfDemuxer.sniffs(header)) fail("not a RIFF/RF64/BW64 WAVE file");
  const tag = text(header.subarray(0, 4));
  let end = tag === "RIFF" ? view(header).getUint32(4, true) + 8 : fileSize;
  if (end > fileSize || end < 12) fail("invalid RIFF size");
  let format: PcmFormat | undefined, axml: { offset: number; length: number } | undefined, chna: Uint8Array | undefined;
  let dataOffset = -1, dataSize = 0, binaural = missingBinaural();
  let ds64Data: number | undefined;
  const sizes = new Map<string, number[]>(), seen = new Set<string>();
  let offset = 12;
  while (offset + 8 <= end) {
    const chunk = await read(offset, 8), id = text(chunk.subarray(0, 4));
    let length = view(chunk).getUint32(4, true);
    if (length === 0xffffffff) {
      const replacement = id === "data" ? ds64Data : sizes.get(id)?.shift();
      if (replacement === undefined) fail("missing ds64 length for " + id);
      length = replacement;
    }
    if (!Number.isSafeInteger(offset + 8 + length) || offset + 8 + length > end) fail("invalid chunk size: " + id);
    if (["fmt ", "axml", "chna", "dbmd", "ds64", "data"].includes(id)) {
      if (seen.has(id)) fail("multiple chunks are unsupported: " + id);
      seen.add(id);
      if (id !== "data" && id !== "axml" && length > MAX_METADATA) fail("metadata exceeds 64 MiB: " + id);
      if (id === "data") { dataOffset = offset + 8; dataSize = length; }
      else if (id === "axml") axml = { offset: offset + 8, length };
      else {
        const bytes = await read(offset + 8, length);
        if (id === "fmt ") format = parseFormat(bytes);
        if (id === "chna") chna = bytes;
        if (id === "dbmd") binaural = decodeDbmdBinauralMetadata(bytes);
        if (id === "ds64") {
          if (length < 28) fail("truncated ds64 chunk");
          const data = view(bytes);
          end = safe64(data, 0) + 8; ds64Data = safe64(data, 8);
          if (end > fileSize || end < offset + 8 + length) fail("invalid ds64 RIFF size");
          const count = data.getUint32(24, true);
          if (28 + count * 12 > length) fail("truncated ds64 size table");
          for (let i = 0; i < count; i++) {
            const position = 28 + i * 12, key = text(bytes.subarray(position, position + 4));
            const entries = sizes.get(key) ?? [];
            entries.push(safe64(data, position + 4)); sizes.set(key, entries);
          }
        }
      }
    }
    offset += 8 + length + (length % 2);
  }
  if (offset !== end) fail("truncated chunk header or padding");
  if (tag !== "RIFF" && !seen.has("ds64")) fail("RF64/BW64 requires ds64");
  if (!format || dataOffset < 0) fail("missing fmt or data chunk");
  if (dataSize % format.blockAlign) fail("partial PCM sample frame");
  if (!!axml !== !!chna) fail("ADM requires both axml and chna chunks");
  const xmlRange = axml;
  const source = async function* () {
    for (let consumed = 0; consumed < xmlRange!.length;) {
      const length = Math.min(64 * 1024, xmlRange!.length - consumed);
      yield await read(xmlRange!.offset + consumed, length);
      consumed += length;
    }
  };
  const adm = axml && chna ? await parseAdmMetadataStream(source, chna, format.sampleRate, format.channels, { overlapPolicy: "latest-start", interpolationPolicy: "clamp" }) : undefined;
  return { format, dataOffset, dataSize, fileSize, adm, labels: adm?.labels ?? pcmLabels(format), binaural };
}

/** PCM playback uses preflight metadata; metadata-only consumers may scan a stream. */
export class BwfDemuxer {
  private position = 0;
  private samplePos = 0;
  private eventIndex = 0;
  private pending: Uint8Array = new Uint8Array();
  private failed = false;
  private started = false;
  private scanRemaining = 12;
  private scanId = "";
  private scanPad = 0;
  private binauralFound = false;
  constructor(private cb: BwfDemuxerCallbacks = {}, private metadata?: BwfMetadata) {}
  static sniffs(bytes: Uint8Array): boolean {
    return bytes.length >= 12 && ["RIFF", "RF64", "BW64"].includes(text(bytes.subarray(0, 4))) && text(bytes.subarray(8, 12)) === "WAVE";
  }
  push(chunk: Uint8Array): void {
    if (this.failed) return;
    try {
      if (!this.metadata) {
        if (this.cb.onPcmFrame) fail("PCM playback requires seekable metadata preflight");
        this.scanMetadata(chunk); return;
      }
      const metadata = this.metadata;
      if (!this.started) {
        this.started = true;
        this.cb.onBinauralMetadata?.(metadata.binaural);
        this.cb.onTrack?.({ codec: metadata.adm ? "adm" : "pcm", sampleRate: metadata.format.sampleRate,
          channels: metadata.format.channels, durationSec: metadata.dataSize / metadata.format.blockAlign / metadata.format.sampleRate, title: metadata.adm?.title });
      }
      const start = Math.max(0, metadata.dataOffset - this.position);
      const end = Math.min(chunk.length, metadata.dataOffset + metadata.dataSize - this.position);
      this.position += chunk.length;
      if (end <= start) return;
      let bytes = chunk.subarray(start, end);
      if (this.pending.length) {
        const combined = new Uint8Array(this.pending.length + bytes.length);
        combined.set(this.pending); combined.set(bytes, this.pending.length); bytes = combined;
      }
      const align = metadata.format.blockAlign;
      // 118-channel masters otherwise generate a desktop IPC round trip for
      // every ~15 ms read. Aggregate PCM while retaining sample-exact events.
      const samples = metadata.adm ? 4096 : 1024;
      let offset = 0;
      while (offset + samples * align <= bytes.length) {
        this.decode(bytes.subarray(offset, offset + samples * align), samples);
        offset += samples * align;
      }
      this.pending = bytes.slice(offset);
    } catch (error) { this.failed = true; throw error; }
  }
  flush(): void {
    if (this.failed) return;
    if (this.metadata) {
      const align = this.metadata.format.blockAlign;
      if (this.pending.length % align) fail("truncated PCM data");
      if (this.pending.length) {
        this.decode(this.pending, this.pending.length / align);
        this.pending = new Uint8Array();
      }
      if (this.pending.length || this.samplePos * this.metadata.format.blockAlign !== this.metadata.dataSize) fail("truncated PCM data");
    } else if (!this.binauralFound) this.cb.onBinauralMetadata?.(missingBinaural());
  }
  private decode(bytes: Uint8Array, samples: number): void {
    const metadata = this.metadata!, { format, adm } = metadata, data = view(bytes);
    const channels = Array.from({ length: format.channels }, () => new Float32Array(samples));
    const width = format.bits / 8;
    for (let channel = 0; channel < channels.length; channel++) {
      const output = channels[channel]!;
      for (let sample = 0, offset = channel * width; sample < samples; sample++, offset += format.blockAlign) {
        let value: number;
        if (format.encoding === 3) value = width === 4 ? data.getFloat32(offset, true) : data.getFloat64(offset, true);
        else if (width === 2) value = data.getInt16(offset, true) / 32768;
        else if (width === 3) value = (data.getUint8(offset) | data.getUint8(offset + 1) << 8 | data.getInt8(offset + 2) << 16) / 8388608;
        else value = data.getInt32(offset, true) / 2147483648;
        if (!Number.isFinite(value)) fail("non-finite PCM sample");
        output[sample] = value;
      }
    }
    const events: AdmObjectEvent[] = [];
    while (adm && this.eventIndex < adm.events.length && adm.events[this.eventIndex]!.samplePos < this.samplePos + samples) events.push(adm.events[this.eventIndex++]!);
    this.cb.onPcmFrame?.({ codec: adm ? "adm" : "pcm", sampleRate: format.sampleRate, samplePos: this.samplePos,
      channels, labels: metadata.labels, rawBedLabels: adm?.rawBedLabels ?? metadata.labels,
      events, objectChannels: adm?.objectChannels ?? [], programLoudness: null, rampDuration: 1 });
    this.samplePos += samples;
  }
  private scanMetadata(chunk: Uint8Array): void {
    let offset = 0;
    while (offset < chunk.length && !this.binauralFound) {
      if (this.scanRemaining) {
        const count = Math.min(this.scanRemaining, chunk.length - offset);
        if (this.scanId === "dbmd") {
          const next = new Uint8Array(this.pending.length + count);
          next.set(this.pending); next.set(chunk.subarray(offset, offset + count), this.pending.length); this.pending = next;
        }
        offset += count; this.scanRemaining -= count;
        if (this.scanRemaining) continue;
        if (this.scanId === "dbmd") {
          this.binauralFound = true; this.cb.onBinauralMetadata?.(decodeDbmdBinauralMetadata(this.pending));
          this.pending = new Uint8Array(); return;
        }
        this.scanId = "";
        if (this.scanPad) { this.scanRemaining = this.scanPad; this.scanPad = 0; continue; }
      }
      const count = Math.min(8 - this.pending.length, chunk.length - offset);
      const next = new Uint8Array(this.pending.length + count);
      next.set(this.pending); next.set(chunk.subarray(offset, offset + count), this.pending.length);
      this.pending = next; offset += count;
      if (this.pending.length < 8) continue;
      this.scanId = text(this.pending.subarray(0, 4)); this.scanRemaining = view(this.pending).getUint32(4, true);
      if (this.scanId === "dbmd" && this.scanRemaining > MAX_METADATA) fail("dbmd exceeds 64 MiB");
      if (this.scanRemaining === 0xffffffff) fail("RF64 metadata scan requires seekable input");
      this.scanPad = this.scanRemaining % 2; this.pending = new Uint8Array();
    }
  }
}
