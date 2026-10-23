# Android CPU/GPU cooperative audio

The goal is lower host CPU work under CPU/thermal pressure, while meeting the existing audio buffer deadline. A GPU job need not beat the cold CPU's wall time to be useful. Actual FP32 parity, reduced total host CPU time and a safe audio deadline remain mandatory.

## Production path

CPU FFT, inverse FFT, object/filter state, authored transitions, loudness/spatial balance and output protection stay unchanged. Vulkan performs the existing ordered FP32 complex MAC with no reduced precision. The input packet uses interleaved complex history. GPU coefficients remain resident and are copied only when a convolver's unique identity/revision/transition-row key changes. Reordered/replaced sources cannot reuse a stale filter. Padded inputs and newly configured coefficients are finite zero. Owned packets and GPU allocations remain bounded together to 32 MiB; there is no whole-song or growing per-track cache.

Android's public thermal status and thermal headroom are sampled on a background control executor every ten seconds. Unsupported headroom remains unknown. Measured render utilization is an independent pressure signal; low idle CPU frequency alone is not classified as throttling. Thermal hints are global SoC hints, not proof that only the CPU is hot. No governor, voltage, system thermal setting or vendor-private API is changed.

The requested share rises by 25 percentage points per sustained second of pressure, and falls by 25 points per five sustained cool seconds. Hysteresis avoids rapid route changes. GPU/CPU routing changes the executor, not sample gain. A qualified GPU remains idle when the CPU has adequate headroom.

Shadow calibration uses actual MAC output and totals CPU clock time across the CPU jobs. Candidate host cost includes packet clearing/packing, measured GPU worker CPU time, a conservative double-copy allowance and 15 us coordination reserve. It must save at least 15% host CPU. The whole GPU round trip, packing and copy must fit min(one quarter audio block, 4 ms), with at least three blocks of rendered output reserve. This buffer-based bound replaces the former cold-CPU-speed comparison for this cooperative Vulkan packet only; legacy backends and iOS retain their prior policy.

Precision/driver/deadline failures retain the same block's CPU result. Loss of measurable CPU saving returns to standby and can be recalibrated under pressure. Track/session changes clear old reserve estimates and invalidate pending old calibration/results. Diagnostics expose the requested share, thermal hints, block/reserve budget, CPU reference total and GPU host CPU separately from actual GPU audio batches adopted.

## Checks and scope

- Host spectral/controller suite: 23 checks pass, including cool standby, simulated pressure adoption, recovery, precision failure, deadline isolation and coefficient/source identity changes.
- Xiaomi 13 Pro / Adreno 740: resident FP32 MAC hardware probe passes 9/9 shapes, five executions each with changing history and coefficient updates; measured relative RMS/peak difference is zero.
- Android native facade compiles. Installed-app CPU-saving calibration and actual thermal playback still require validation; the hardware math probe does not establish those results.

## Separate full-FFT experiment

`vulkan_convolution.comp`, `vulkan_convolution_probe.rs` and the C++ probe form a default-off experiment (`vulkan-convolution-probe`). It combines forward FFT, partition MAC and packed stereo inverse FFT in one GPU dispatch. It has not met the strict low-level transient parity gate and must not be adopted for playback. `scripts/build-vulkan-convolution-probe.py` builds it separately. RustFFT-derived arithmetic scheduling retains its MIT notice.

This experiment was originally compared for faster wall time. The user's clarified objective is thermal CPU/GPU cooperation; the production change therefore uses the already bit-matching MAC path with reduced transfer overhead and a CPU-work admission test instead.
