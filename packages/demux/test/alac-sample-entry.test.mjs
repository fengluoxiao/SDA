import assert from "node:assert/strict";
import { alacTrackFromMp4Box, parseAlacSampleEntry } from "../src/mp4.ts";

// QuickTime AudioSampleEntry header (20 bytes) followed by the complete
// 36-byte ALAC atom from a real 48 kHz, stereo ALAC M4A track.
const entry = Uint8Array.from([
  0, 0, 0, 0, 0, 0, 0, 0,
  0, 2, 0, 16, 0, 0, 0, 0, 187, 128, 0, 0,
  0, 0, 0, 36, 97, 108, 97, 99,
  0, 0, 0, 0, 0, 0, 16, 0, 0, 24, 40, 10, 14, 2,
  0, 255, 0, 0, 96, 4, 0, 35, 40, 0, 0, 0, 187, 128,
]);

const parsed = parseAlacSampleEntry(entry);
assert.ok(parsed, "recognizes an ALAC sample entry");
assert.equal(parsed.sampleRate, 48_000);
assert.equal(parsed.channels, 2);
assert.equal(parsed.decoderConfig.byteLength, 36);
assert.deepEqual([...parsed.decoderConfig], [...entry.subarray(20)], "preserves the complete ALAC atom");

const truncated = entry.slice();
new DataView(truncated.buffer).setUint32(20, 37);
assert.equal(parseAlacSampleEntry(truncated), null, "rejects an atom that exceeds its sample entry");
assert.equal(parseAlacSampleEntry(entry.subarray(0, 27)), null, "rejects a truncated sample entry");

const track = alacTrackFromMp4Box({
  moov: {
    traks: [{
      tkhd: { track_id: 7 },
      mdia: {
        mdhd: { duration: 6_659_200, timescale: 48_000 },
        minf: { stbl: { stsd: { entries: [{ type: "alac", data: entry }] } } },
      },
    }],
  },
});
assert.ok(track, "discovers an ALAC track from MP4Box's trak/mdia hierarchy");
assert.equal(track.durationSec, 6_659_200 / 48_000, "uses mdia.mdhd container duration");

console.log("ALAC sample entry tests passed");
