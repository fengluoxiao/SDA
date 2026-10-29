"""Generate short, synthetic ALAC fixtures; requires FFmpeg on PATH or as argv[1]."""
from pathlib import Path
import math, struct, subprocess, sys, tempfile, wave

OUT = Path(__file__).resolve().parents[1] / "crates/sda-native/tests/fixtures/alac"
CASES = [(44100, 2, None), (48000, 2, None), (88200, 2, 1), (96000, 2, 1),
         (96000, 2, 44100), (192000, 2, 1), (44100, 1, None)]

def boxes(data, start, end):
    while start < end:
        size, kind = struct.unpack_from(">I4s", data, start)
        assert size >= 8 and start + size <= end
        yield kind, start, start + 8, start + size
        start += size

def sample_entry(data):
    start, end = 0, len(data)
    for kind in [b"moov", b"trak", b"mdia", b"minf", b"stbl", b"stsd"]:
        _, _, start, end = next(box for box in boxes(data, start, end) if box[0] == kind)
    assert struct.unpack_from(">I", data, start + 4)[0] == 1
    entry = start + 8
    assert data[entry + 4:entry + 8] == b"alac"
    return entry

def main():
    OUT.mkdir(parents=True, exist_ok=True)
    ffmpeg = sys.argv[1] if len(sys.argv) > 1 else "ffmpeg"
    cases = [(rate, channels, entry_rate, rate // 8 + 317, "") for rate, channels, entry_rate in CASES]
    cases += [(96000, 2, 1, 12288, "-exact"), (96000, 2, 1, 32, "-short")]
    for rate, channels, entry_rate, frames, suffix in cases:
        name = f"tone-{rate}-{channels}ch" + (f"-entry{entry_rate}" if entry_rate else "") + suffix + ".m4a"
        with tempfile.TemporaryDirectory() as temp:
            wav = Path(temp) / "tone.wav"
            pcm = bytearray()
            for frame in range(frames):
                for channel in range(channels):
                    value = (0.25 if channel == 0 else 0.125) * math.sin(2 * math.pi * (440 if channel == 0 else 880) * frame / rate)
                    pcm.extend(round(value * (1 << 23)).to_bytes(3, "little", signed=True))
            with wave.open(str(wav), "wb") as writer:
                writer.setparams((channels, 3, rate, frames, "NONE", "not compressed"))
                writer.writeframes(pcm)
            path = OUT / name
            subprocess.run([ffmpeg, "-v", "error", "-y", "-i", str(wav), "-map_metadata", "-1", "-c:a", "alac", str(path)], check=True)
            if rate == 44100 and channels == 2 and entry_rate is None:
                subprocess.run([ffmpeg, "-v", "error", "-y", "-i", str(wav), "-map_metadata", "-1", "-c:a", "libmp3lame", "-b:a", "320k", str(OUT / "tone-44100-2ch.mp3")], check=True)
            if entry_rate:
                data = bytearray(path.read_bytes())
                struct.pack_into(">I", data, sample_entry(data) + 32, entry_rate << 16)
                path.write_bytes(data)
            print(name, frames, "input frames", path.stat().st_size, "bytes")

if __name__ == "__main__":
    main()
