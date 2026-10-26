import assert from "node:assert/strict";
import { readFileSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { execFileSync } from "node:child_process";
import { Mp3Decoder, sniffMp3 } from "../src/mp3-decoder.ts";
import { AlacResampler } from "../src/alac-resampler.ts";
const ffmpeg = process.env.SDA_TEST_FFMPEG;
assert(ffmpeg, "Set SDA_TEST_FFMPEG to a local FFmpeg executable");
const directory = mkdtempSync(join(tmpdir(), "sda-mp3-"));
async function decode(bytes, chunkSize) {
  const formats = [], decoder = await Mp3Decoder.create(rate => formats.push(rate));
  const channels = [[], []]; let samples = 0;
  const drain = () => {
    let frame;
    while ((frame = decoder.nextFrame())) {
      assert.equal(frame.codec, "mp3"); assert.equal(frame.samplePos, samples);
      assert(frame.channels[0].length <= 4096);
      frame.channels.forEach((plane, i) => channels[i].push(...plane));
      samples += frame.channels[0].length;
    }
    assert.deepEqual(decoder.drainErrors(), []);
  };
  try {
    for (let offset = 0; offset < bytes.length; offset += chunkSize) { decoder.push(bytes.subarray(offset, offset + chunkSize)); drain(); }
    decoder.flush(); drain();
    assert.deepEqual(formats, [44100]);
    return channels;
  } finally { decoder.free(); }
}
async function main() {
try {
  for (const mono of [false, true]) {
    const file = join(directory, mono ? "mono.mp3" : "stereo.mp3");
    execFileSync(ffmpeg, ["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i",
      mono ? "sine=frequency=997:sample_rate=44100:duration=1.2" : "aevalsrc=0.2*sin(2*PI*997*t)|0.1*sin(2*PI*431*t):s=44100:d=1.2",
      "-c:a", "libmp3lame", "-q:a", "3", file]);
    const bytes = new Uint8Array(readFileSync(file)); assert.equal(sniffMp3(bytes), "mp3");
    const whole = await decode(bytes, bytes.length), chunks = await decode(bytes, 137);
    assert.deepEqual(chunks, whole, "chunk boundaries must not change PCM, duration, or channel order");
    assert(whole[0].length >= 44100 && whole[0].length < 44100 * 1.3);
    if (mono) assert.deepEqual(whole[0], whole[1], "mono must stay centered");
    else {
      const rms = values => Math.sqrt(values.reduce((s, x) => s + x*x, 0) / values.length);
      assert(Math.abs(rms(whole[0])/rms(whole[1])-2) < 0.1, "authored stereo level ratio must survive decoding");
    }
    const src = new AlacResampler(48000); let output = 0;
    const collect = frame => { if (frame) { assert.equal(frame.samplePos, output); output += frame.channels[0].length; } };
    for (let start = 0; start < whole[0].length; start += 4096) collect(await src.push({
      codec:"mp3",sampleRate:44100,samplePos:start,channels:whole.map(ch=>Float32Array.from(ch.slice(start,start+4096))),
      labels:["L","R"],rawBedLabels:["L","R"],events:[],objectChannels:[],programLoudness:null,rampDuration:0,
    }));
    collect(src.finish()); assert.equal(output, Math.round(whole[0].length*48000/44100));
  }
  assert.equal(sniffMp3(Uint8Array.of(0x49,0x44,0x33,4,0,0,0,0,1,0)), "need-more");
  for (const invalid of [[0xff,0xf1,0x90,0], [0xff,0xeb,0x90,0], [0xff,0xfb,0xf0,0], [0xf8,0x72,0x6f,0xba]]) assert.equal(sniffMp3(Uint8Array.from(invalid)), null);
  console.log("MP3 stereo/mono, VBR streaming, gapless clock, strict sniffing and 48 kHz SRC passed");
} finally { rmSync(directory, {recursive:true,force:true}); }

}
main().catch(error => { console.error(error); process.exitCode = 1; });
