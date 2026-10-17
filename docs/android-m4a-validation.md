# Android M4A / MP4 E-AC-3 input

The Android document picker accepts `.m4a`, `.mp4`, `.eac3`, and `.ec3`.
For containers, `Eac3Input` selects an `audio/eac3` or `audio/eac3-joc`
track using Android `MediaExtractor`. It passes compressed access-unit bytes
to the existing Rust decoder, without MediaCodec decoding or transcoding.
AAC/ALAC-only and encrypted tracks are not supported by this input path.
Raw E-AC-3 streams retain their existing path.

The extractor has a bounded 1 MiB packet buffer, independent of song length.
Closing/stopping playback releases the extractor; initialization failures
close the opened input. Pausing does not count toward the feed/drain timeout.

## Instrumentation tests

Build `:sda-core:assembleDebugAndroidTest`, install the resulting test APK,
and run `app.sda.mobile.sda.test/androidx.test.runner.AndroidJUnitRunner`.
The tests verify raw byte preservation and rejection of non-E-AC-3 audio.

The optional real-file test expects `sample.m4a` in the test application's
external files directory and an `eac3Sha256` instrumentation argument. Compute
that hash over concatenated `ec-3` access units from the desktop MP4Box demuxer,
not over the whole container. The test deliberately splits access units into
991-byte reads and checks repeated EOF reads as well as the complete hash.
Do not commit copyrighted media fixtures.

Example for the locally supplied `1-01 doll.m4a`:

```text
adb shell mkdir -p /sdcard/Android/data/app.sda.mobile.sda.test/files
adb push "1-01 doll.m4a" /sdcard/Android/data/app.sda.mobile.sda.test/files/sample.m4a
adb shell am instrument -w -e eac3Sha256 f09a8a0a5923bd6fd599ae63fafc47f5585278a2a278b239ff0e91a6b91c25fa app.sda.mobile.sda.test/androidx.test.runner.AndroidJUnitRunner
```

Desktop inspection: `ec-3`, 48 kHz, 768 kbps, 5,599 access units,
17,200,128 compressed audio bytes, 179.168 seconds. The Android release build
is validated separately by selecting the original M4A in the system picker,
checking moving object positions and the audio clock, pausing/resuming, and
allowing playback to reach EOF.
