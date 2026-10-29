# Synthetic ALAC regression fixtures

These short files contain generated 24-bit sine waves, not recorded music. Left is 440 Hz at 0.25 amplitude; right is 880 Hz at 0.125 amplitude. Most contain `sample_rate / 8 + 317` input frames, including a partial final packet. The `exact` fixture contains 12,288 frames (an exact multiple of the resampler block); `short` contains 32 frames, less than the filter look-ahead. The MP3 fixture verifies the shared resampler still preserves gapless duration and waveform. The mono fixture tests rejection by the stereo upmix path.

The `entry1` and `entry44100` files intentionally use placeholder or misleading MP4 sample-entry rates. Their ALAC magic cookies and compressed PCM retain the real rates in the filenames. The decoder must use the ALAC signal specification before constructing its 48 kHz resampler.

Regenerate from the repository root with Python and FFmpeg:

```text
python scripts/generate-alac-regression-fixtures.py /path/to/ffmpeg
```
