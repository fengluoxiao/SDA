# Android CPU/GPU cooperative audio

The goal is lower host CPU work under CPU/thermal pressure, while meeting the existing audio buffer deadline. A GPU job need not beat the cold CPU's wall time to be useful. Actual FP32 parity, reduced total host CPU time and a safe audio deadline remain mandatory.

## Production path

CPU FFT, inverse FFT, object/filter state, authored transitions, loudness/spatial balance and output protection stay unchanged. Vulkan performs the existing ordered FP32 complex MAC with no reduced precision. The input/output packet uses uniquely issued, Arc-backed persistent Vulkan mappings. No extra CPU-to-GPU or GPU-to-CPU memcpy is performed. Separate context lifetime tokens keep mappings valid, old tokens are dropped before reconfiguration, and retired contexts are closed only by their creating compute worker. A dead worker can retain at most its one context until process exit rather than freeing GPU resources on a render thread.

The input packet uses interleaved complex history; only active rows cross the boundary, retired rows/parts are zeroed once, and unused output/reference buffers are not cleared every block. GPU coefficients remain resident and are copied only when a convolver's unique identity/revision/transition-row key changes. Reordered/replaced sources cannot reuse a stale filter. Padded inputs and newly configured coefficients are finite zero. Output is row-local interleaved stereo complex data, allowing direct positive-frequency memcpy into the CPU convolver. The inverse reconstructs the negative half, so it need not be cleared. A GPU atomic flag validates nonfinite results before host use without a whole-output CPU scan. NaN rejection and flag reset are checked in hardware. Owned packets and GPU allocations remain bounded together to 32 MiB; there is no whole-song or growing per-track cache.

Android's public thermal status and thermal headroom are sampled on a background control executor every ten seconds. Unsupported headroom remains unknown. Measured render utilization is an independent pressure signal; where public sysfs is readable, a median performance-core frequency ceiling below 85% together with active render work is another CPU-budget signal (not proof of thermal cause); low idle CPU frequency alone is not classified as throttling. Thermal hints are global SoC hints, not proof that only the CPU is hot. No governor, voltage, system thermal setting or vendor-private API is changed.

The GPU driver worker uses the allowed performance-core set. During requested sharing its priority matches the decoder (-4), below the render workers; shadow work remains background-priority. Its real CPU clock cost is still included in admission.

The requested share rises by 25 percentage points per sustained second of pressure, and falls by 25 points per five sustained cool seconds. Hysteresis avoids rapid route changes. GPU/CPU routing changes the executor, not sample gain. A qualified GPU remains idle when the CPU has adequate headroom.

Shadow calibration uses actual MAC output and totals CPU clock time across the CPU jobs. Candidate host cost includes packet clearing/packing, measured GPU worker CPU time, a conservative double-copy allowance and 15 us coordination reserve. It must save at least 15% host CPU. The whole GPU round trip, packing and copy must fit min(one quarter audio block, 4 ms), with at least three blocks of rendered output reserve. This buffer-based bound replaces the former cold-CPU-speed comparison for this cooperative Vulkan packet only; legacy backends and iOS retain their prior policy.

Precision/driver/deadline failures retain the same block's CPU result. Cooperative deadline misses have a bounded cooldown/requalification retry; non-deadline driver or precision failures remain disabled. Late results never become a new block's output. CPU costs are smoothed, brief spikes require three consecutive misses before standby, and one real CPU/GPU parity reference is re-sampled every 128 rendering blocks during active sharing. Loss of measurable CPU saving returns to standby and can be recalibrated under pressure. Track/session changes clear old reserve estimates and invalidate pending old calibration/results. Diagnostics expose the requested share, thermal hints, block/reserve budget, CPU reference total and GPU host CPU separately from actual GPU audio batches adopted.

## Checks and scope

- Host spectral/controller suite: 26 checks pass, including cool standby, simulated pressure adoption, recovery, precision failure, deadline isolation and coefficient/source identity changes.
- Xiaomi 13 Pro / Adreno 740: resident FP32 MAC hardware probe passes 10/10 shapes, five executions each with changing history and coefficient updates; measured relative RMS/peak difference is zero.
- The original installed cooperative build played the user track to natural EOF without callback missing frames. Zero-copy installed-app validation remains pending; no real temperature decrease has been claimed. The separate physical cooperation probe uses production modules, synthetic input and simulated policy hints; it must not be presented as a real overheating trace.

## Separate full-FFT experiment

`vulkan_convolution.comp`, `vulkan_convolution_probe.rs` and the C++ probe form a default-off experiment (`vulkan-convolution-probe`). It combines forward FFT, partition MAC and packed stereo inverse FFT in one GPU dispatch. It has not met the strict low-level transient parity gate and must not be adopted for playback. `scripts/build-vulkan-convolution-probe.py` builds it separately. RustFFT-derived arithmetic scheduling retains its MIT notice.

This experiment was originally compared for faster wall time. The user's clarified objective is thermal CPU/GPU cooperation; the production change therefore uses the already bit-matching MAC path with reduced transfer overhead and a CPU-work admission test instead.

## Reproducible physical cooperative probe

Build the `cooperative_spectral_probe` example for Android with `pcm-diagnostic`; run with source count (up to128), optionally `limited`. The default leaves CPU affinity policy intact and supplies a simulated pressure hint inside that probe process. `limited` temporarily restricts only that probe process to efficiency cores and restores its original thread masks; it is a resource-budget simulation, not system thermal throttling. No audio device is opened and no global thermal setting is changed. Every resulting PCM sample is compared with an independent production CPU convolver, including actually adopted GPU blocks. A new render session checks context retirement.

Recorded intermediate mapped run:53 GPU blocks adopted under simulated pressure, maximum PCM numeric difference0, no orphaned contexts, and worker-side context retirement observed. A native deadline then correctly caused CPU fallback; timeout classification was subsequently made recoverable within the unchanged audio deadline. Final records distinguish failed pre-optimization trials from successful GPU use.
