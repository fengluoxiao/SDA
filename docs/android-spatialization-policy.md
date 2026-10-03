# Android output spatialization policy

SDA renders KU100 binaural stereo itself. Every AAudio output open (including reopen)
requests NEVER spatialization and declares already-spatialized content. The API-32
functions are resolved from libaaudio.so with dlsym so API 26-31 can still load the
library. Missing functions are logged, not advertised as an effective bypass.
Media3 1.5.1's alternate DefaultAudioSink requests NEVER using its AudioAttributes
wrapper before configure. This version does not expose the already-spatialized flag.

This policy affects SDA's stream only. It does not change Bluetooth codecs, HRTF,
room settings, global phone settings or an earbud's internal DSP. Logging records
requested policy / symbol availability, NOT proof that all OEM effects were disabled.

## Manufacturer research (2026-10-02)

- Google: official Pixel help documents spatial audio controls in Settings and
  per-Bluetooth-device settings; head tracking has a separate control.
  https://support.google.com/pixelphone/answer/12967594?hl=en
- OPPO: Enco X3i official product footnote explicitly says Alive Audio is rendered
  on the earbuds, without head tracking. ColorOS users configure it through
  Bluetooth > OPPO Enco X3i > Earbud functions; other supported devices use
  HeyMelody. A host audio-stream attribute cannot be treated as disabling this DSP.
  https://www.oppo.com/en/accessories/enco-x3i/
- Xiaomi: official Xiaomi 14 product page confirms Dolby Atmos support, but does
  not establish a public third-party per-app bypass API or universal setting path.
  https://www.mi.com/global/product/xiaomi-14/
- vivo: no verified public per-app bypass API established in this investigation.
  Do not invent a private settings key or claim all vivo sound effects are disabled.

Android NDK API definitions verified against installed NDK 26.1.10909125 AAudio.h:
setSpatializationBehavior / setIsContentSpatialized are introduced in API 32.
https://developer.android.com/ndk/reference/group/audio

## Validation and limits

Run scripts/test/android-spatialization-policy.test.py. Optionally pass llvm-readelf
and the built shared library to verify no hard imports of API-32 functions.
Native libraries have been compiled for x86_64 and aarch64 (arm64-v8a).
The ARM64 package targets Android API 26 or newer; it is not a physical-device
validation. The current ARM64 native library uses 4 KiB ELF segment alignment;
16 KiB-page device compatibility has not been established.
A successful build is not a listening test or proof of manufacturer compliance.
