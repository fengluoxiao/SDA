/**
 * MP4 audio extraction via mp4box.js. Covers `ec-3` (E-AC-3 / JOC Atmos),
 * `ac-3`, and `mlpa` (TrueHD-in-MP4, rare). Samples arrive as raw access
 * units ready for @sda/core.
 */

// @ts-ignore — mp4box ships untyped CommonJS.
import MP4Box from "mp4box";

export interface EmbeddedCoverArt {
  bytes: Uint8Array;
  mimeType: "image/jpeg" | "image/png";
}

export interface Mp4AudioTrack {
  trackId: number;
  codec: string; // MP4 sample entry, e.g. "ec-3" or "alac"
  sampleRate: number;
  channels: number;
  /** Complete MP4 `alac` atom required before decoding ALAC packets. */
  decoderConfig?: Uint8Array;
  coverArt?: EmbeddedCoverArt;
  /** Movie duration from the container header (seconds), when known. */
  durationSec?: number;
}

export interface Mp4Packet {
  trackId: number;
  timestampMs: number;
  data: Uint8Array;
}

export interface Mp4DemuxerCallbacks {
  onTrack?: (track: Mp4AudioTrack) => void;
  onPacket?: (packet: Mp4Packet) => void;
  onError?: (message: string) => void;
}

const AUDIO_CODECS = new Set(["ec-3", "ac-3", "ac-4", "mlpa", "dtsc", "dtsh", "dtsl", "dtse"]);
/** Keep EC-3/JOC work bounded: 16 × 1536-sample AUs is about 512 ms at 48 kHz.
 * Large MP4Box extraction batches turn into long worker decode bursts and flood
 * the renderer with object PCM/event messages. */
export const MP4_EXTRACTION_BATCH_SAMPLES = 16;
const atomType = (bytes: Uint8Array, offset: number) => String.fromCharCode(...bytes.subarray(offset, offset + 4));

interface Mp4AlacEntry {
  type: "alac";
  data: Uint8Array;
}

interface Mp4AlacTrack {
  tkhd?: { track_id?: number };
  mdia?: {
    mdhd?: { duration?: number; timescale?: number };
    minf?: { stbl?: { stsd?: { entries?: Mp4AlacEntry[] } } };
  };
}

function readU32(bytes: Uint8Array, offset: number): number {
  return new DataView(bytes.buffer, bytes.byteOffset + offset, 4).getUint32(0);
}

/** mp4box 0.5 retains unknown ALAC sample-entry bytes but does not classify
 * `alac` as audio. Extract its QuickTime header and full nested codec atom. */
export function parseAlacSampleEntry(data: Uint8Array): { sampleRate: number; channels: number; decoderConfig: Uint8Array } | null {
  // AudioSampleEntry: 6 reserved bytes, data-reference index, then version
  // fields; channel count/sample size/sample rate are at 8/10/16.
  if (data.length < 28) return null;
  const channels = new DataView(data.buffer, data.byteOffset + 8, 2).getUint16(0);
  const sampleRate = readU32(data, 16) / 65536;
  if (!Number.isInteger(channels) || channels < 1 || !Number.isFinite(sampleRate) || sampleRate < 1) return null;
  for (let offset = 20; offset + 8 <= data.length;) {
    const size = readU32(data, offset);
    if (size < 8 || offset + size > data.length) return null;
    if (atomType(data, offset + 4) === "alac") {
      return { sampleRate, channels, decoderConfig: data.slice(offset, offset + size) };
    }
    offset += size;
  }
  return null;
}

export function alacTrackFromMp4Box(file: { moov?: { traks?: Mp4AlacTrack[] } }): Mp4AudioTrack | null {
  for (const trak of file.moov?.traks ?? []) {
    const entry = trak.mdia?.minf?.stbl?.stsd?.entries?.[0];
    if (entry?.type !== "alac") continue;
    const parsed = parseAlacSampleEntry(entry.data);
    if (!parsed) return null;
    const track: Mp4AudioTrack = {
      trackId: trak.tkhd?.track_id ?? 0,
      codec: "alac",
      ...parsed,
    };
    const duration = trak.mdia?.mdhd?.duration;
    const timescale = trak.mdia?.mdhd?.timescale;
    if (duration && timescale && Number.isFinite(duration / timescale)) track.durationSec = duration / timescale;
    return track;
  }
  return null;
}

/** Extract iTunes `covr` artwork from mp4box's unparsed `ilst` children. */
function embeddedCoverArt(file: { moov?: { udta?: { meta?: { ilst?: { data?: Uint8Array } } } } }): EmbeddedCoverArt | undefined {
  const ilst = file.moov?.udta?.meta?.ilst?.data;
  if (!ilst) return undefined;
  for (let offset = 0; offset + 8 <= ilst.length;) {
    const size = new DataView(ilst.buffer, ilst.byteOffset + offset, 4).getUint32(0);
    if (size < 16 || offset + size > ilst.length) break;
    if (atomType(ilst, offset + 4) === "covr") {
      const childOffset = offset + 8;
      const childSize = new DataView(ilst.buffer, ilst.byteOffset + childOffset, 4).getUint32(0);
      if (childSize >= 16 && childOffset + childSize <= offset + size && atomType(ilst, childOffset + 4) === "data") {
        const bytes = ilst.slice(childOffset + 16, childOffset + childSize);
        const jpeg = bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff;
        const png = bytes[0] === 0x89 && bytes[1] === 0x50 && bytes[2] === 0x4e && bytes[3] === 0x47;
        if (jpeg || png) return { bytes, mimeType: jpeg ? "image/jpeg" : "image/png" };
      }
    }
    offset += size;
  }
  return undefined;
}

export class Mp4Demuxer {
  private file: ReturnType<typeof MP4Box.createFile>;
  private offset = 0;
  private wantedTrackId: number | null = null;
  /** MP4Box keeps sample payloads until this cursor is released. */
  private deliveredSamples = 0;
  private cb: Mp4DemuxerCallbacks;

  constructor(cb: Mp4DemuxerCallbacks = {}) {
    this.cb = cb;
    this.file = MP4Box.createFile();
    this.file.onError = (e: unknown) => this.cb.onError?.(String(e));
    this.file.onReady = (info: { audioTracks: Array<{ id: number; codec: string; duration?: number; timescale?: number; movie_duration?: number; movie_timescale?: number; audio: { sample_rate: number; channel_count: number } }> }) => {
      const candidates: Mp4AudioTrack[] = [];
      const alac = alacTrackFromMp4Box(this.file as unknown as { moov?: { traks?: Mp4AlacTrack[] } });
      if (alac) candidates.push(alac);
      for (const t of info.audioTracks) {
        if (!AUDIO_CODECS.has(t.codec)) continue;
        const track: Mp4AudioTrack = {
          trackId: t.id,
          codec: t.codec,
          sampleRate: t.audio.sample_rate,
          channels: t.audio.channel_count,
        };
        const dur = t.duration && t.timescale ? t.duration / t.timescale
          : t.movie_duration && t.movie_timescale ? t.movie_duration / t.movie_timescale
          : undefined;
        if (dur && Number.isFinite(dur)) track.durationSec = dur;
        candidates.push(track);
      }
      for (const track of candidates) {
        const coverArt = embeddedCoverArt(this.file);
        if (coverArt) track.coverArt = coverArt;
        this.cb.onTrack?.(track);
        // Extract the first supported track only.
        if (this.wantedTrackId === null && track.trackId > 0) {
          this.wantedTrackId = track.trackId;
          this.file.setExtractionOptions(track.trackId, null, { nbSamples: MP4_EXTRACTION_BATCH_SAMPLES });
          this.file.start();
        }
      }
    };
    this.file.onSamples = (trackId: number, _user: unknown, samples: Array<{ cts: number; timescale: number; data: Uint8Array }>) => {
      for (const s of samples) {
        this.cb.onPacket?.({
          trackId: this.wantedTrackId ?? 0,
          timestampMs: (s.cts / s.timescale) * 1000,
          data: s.data,
        });
      }
      // onPacket consumes each access unit synchronously. Release only after
      // the whole callback has returned from that consumer so MP4Box can drop
      // compressed payloads instead of retaining the entire M4A/MP4 in memory.
      this.deliveredSamples += samples.length;
      this.file.releaseUsedSamples(trackId, this.deliveredSamples);
    };
  }

  static sniffs(bytes: Uint8Array): boolean {
    // .... ftyp
    return (
      bytes.length >= 8 &&
      bytes[4] === 0x66 && bytes[5] === 0x74 && bytes[6] === 0x79 && bytes[7] === 0x70
    );
  }

  push(chunk: Uint8Array): void {
    const buf = chunk.buffer.slice(chunk.byteOffset, chunk.byteOffset + chunk.byteLength) as ArrayBuffer & { fileStart: number };
    buf.fileStart = this.offset;
    this.offset += chunk.byteLength;
    this.file.appendBuffer(buf);
  }

  flush(): void {
    this.file.flush();
  }
}
