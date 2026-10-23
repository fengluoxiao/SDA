# Android background playback and MPEG-H programme reference

The user reports audio stutter on a Xiaomi 14 when SDA is playing in the background and other applications' controls are used, without observed overheating. They also requested checking why 群青 is louder than other 360RA material. Thermal throttling is not assumed to be the cause. This work preserves the accepted directional spectral presentation.

## Background execution

The Android path previously used blocking AAudio writes from an ordinary application thread; native render/worker promotion only had an iOS implementation. Android also polled native scene JSON and rebuilt hidden UI every 80 ms, and had no media foreground service or CPU playback lease.

The normal AAudio path now consumes prepared stereo PCM in a data callback. Its shared callback context is allocated before opening, published through read-only OnceLocks, and retained until after stream close. The callback only copies from the SPSC FIFO and updates atomic counters: no decoding, HRTF work, logging, allocation, mutex or disk IO. Unsupported callback stream creation falls back to the blocking backend with explicit diagnostics; opt-in PCM diagnostic builds retain the old capture path.

Hardware buffers start at four granted bursts and can grow one burst on a real hardware xrun, bounded by eight bursts and the reported capacity. A separate monitor performs resizing and logging. Actual backend, burst/buffer sizes, negotiated performance mode, hardware xruns (null if unsupported), software missing frames and render timings are exposed in status. These are not DAC/acoustic latency measurements.

Android render workers request a best-effort negative nice; decoder priority stays below DSP. No hard realtime scheduler class, Qualcomm/MediaTek private API, CPU-number pinning, frequency lock or thermal bypass is requested. The render coordinator participates in DSP instead of merely waiting on the pool: ordinary scenes use two compute participants and dense scenes at most three. The existing rendered-PCM FIFO, its startup threshold, listener control behavior and remote sync are unchanged; the callback and hardware-buffer changes address device-consumer scheduling without adding a large software playback delay.

A mediaPlayback foreground service owns a bounded, renewable partial CPU wake lock while playing. Pause releases the lock; EOF has a short lock-free handover grace for the next playlist item; stop/error/destruction releases the service. Generation tokens reject stale EOF/stop events. Android background JS now reads only cheap playback end/error state at 250 ms; native scene polling and hidden UI updates stop. iOS foreground/background polling behavior stays as before.

## 群青 loudness finding

An independent tap at the source DRC stage showed no sample changes and zero loudness gain for this file. The error was farther downstream: the old stereo programme reference had already passed the upstream output peak limiter, while captured object PCM had not. Full-file measurements were -7.506 LUFS for the limited reference and -1.233 LUFS for the pre-limiter float master, a 6.273 LU discrepancy. Programme correction based on the limited reference was therefore insufficient.

A read-only pre-limiter float master is now shared by native and WASM programme measurement. SDA's own compatible/plain master uses the same float domain, with SDA's existing final true-peak guard; external/system decoder PCM output remains available as before. Source PCM and OAM metadata are unchanged; native and WASM source PCM and float references were verified byte-identical. Only MPEG-H measurement cache keys are versioned, so old limited-reference measurements cannot be reused.

For MPEG-H with volume balance, existing bounded first-output preparation supplies a fixed initial programme correction before PCM is made audible. The correction is not retargeted by later sections; full measurement continues as metadata for subsequent playback. No full-track startup scan, second playback analysis pass or persistent decoded-audio cache is added. This preserves the user's no-live-ducking requirement, with an explicit limitation: an uncached first-play estimate cannot know the whole song's future loudness.

With the requested +2.5 dB makeup retained, actual complete-output measurements of 群青 were:

| State | Integrated LUFS | True peak dBTP |
| --- | ---: | ---: |
| Old complete measurement | -11.5 | -1.2 |
| Correct complete measurement | -17.6 | -4.5 |
| New first-play fixed prefix estimate | -13.2 | -1.2 |

All three cases use constant programme gain. At identical corrected gain, old and Android-policy renders differ by at most 3.875e-7 sample amplitude (difference 131.17 dB below the signal). No source spectral/clarity/bass/IR recipe was retuned. These are output measurements, not a blanket final -18 LUFS certification.

## Verification boundaries

Meaningful checks include callback PCM/clock continuity under burst-size changes and producer stalls, hardware buffer capacity/counter behavior, Android CPU budgets, both mixers and listener states, stale service ownership, real native playback during Settings interaction, raw-source/native-WASM integrity, programme priming/stability, mobile TypeScript and workspace-resolved web/player TypeScript.

Android runtime results are recorded under the dated local diagnostic/release directory. An emulator test cannot establish Xiaomi 14, Qualcomm or MediaTek physical-device behavior. Physical-device performance and listening are not claimed; actual callback/backend/xrun counters are provided for subsequent feedback.
