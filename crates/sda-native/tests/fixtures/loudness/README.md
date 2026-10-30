# Fresh stereo reference fixtures

These files contain generated stereo sine tones, with no music or user recordings.

- `quiet-intro-96000.m4a`: ALAC at 96 kHz; 2 quiet seconds followed by 6 loud seconds.
- `quiet-intro-44100.mp3`: the same signal encoded as MP3 at 44.1 kHz.
- `short-48000.m4a`: a complete 0.8-second ALAC signal at 48 kHz, shorter than the legacy 57-block threshold.

The quiet section uses amplitude 0.001 in both channels. The loud section uses amplitude 0.4 left / 0.28 right, at 997 / 701 Hz.

Regenerate with `python scripts/generate-stereo-reference-fixtures.py /path/to/ffmpeg`.
