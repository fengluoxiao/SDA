"""Regenerate synthetic quiet-intro and complete-short-source fixtures."""
from pathlib import Path
import subprocess
import sys

if len(sys.argv) != 2:
    raise SystemExit("Usage: python scripts/generate-stereo-reference-fixtures.py /path/to/ffmpeg")
root = Path(__file__).resolve().parents[1]
destination = root / "crates/sda-native/tests/fixtures/loudness"
destination.mkdir(parents=True, exist_ok=True)
quiet = "aevalsrc='if(lt(t,2),0.001,0.4)*sin(2*PI*997*t)'|'if(lt(t,2),0.001,0.28)*sin(2*PI*701*t)':s={rate}:d=8"
short = "aevalsrc=0.4*sin(2*PI*997*t)|0.28*sin(2*PI*701*t):s=48000:d=0.8"
for name, rate, codec, signal in [
    ("quiet-intro-96000.m4a", 96000, "alac", quiet),
    ("quiet-intro-44100.mp3", 44100, "libmp3lame", quiet),
    ("short-48000.m4a", 48000, "alac", short),
]:
    args = [sys.argv[1], "-y", "-hide_banner", "-nostdin", "-loglevel", "error", "-f", "lavfi", "-i", signal.format(rate=rate), "-map", "0:a:0", "-c:a", codec]
    args += ["-sample_fmt", "s32p"] if codec == "alac" else ["-b:a", "192k"]
    subprocess.run(args + ["-map_metadata", "-1", str(destination / name)], check=True)
