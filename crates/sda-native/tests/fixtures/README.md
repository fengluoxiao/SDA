ios-stereo-tones.eac3 is a generated 1-second stereo test signal, not a song.
48 kHz E-AC-3, 192 kb/s, left 440 Hz and right 880 Hz at amplitude 0.1.
Generated with FFmpeg lavfi aevalsrc; no copyrighted recording is used.

ios-stereo-tones.m4a contains the same signal, remuxed with FFmpeg -c:a copy -f mp4. It is used by the opt-in simulator probe to exercise AVAssetReader compressed packet extraction and AVAudioEngine callback playback.
