# Native per-object HRTF performance

The native renderer keeps independent object convolution histories. Large ADM
masters use shared immutable speaker-filter spectra, reusable object-filter
buffers, and a persistent Rayon pool (up to four workers, with a serial fallback
for small source counts or unavailable workers). Filter updates and convolution
finish before the next output block is published; source summation order stays
unchanged. WASAPI continues consuming only the stereo FIFO.

The x86-64 spectral multiply-accumulate uses runtime-detected AVX, with a scalar
fallback. It uses separate multiply/add operations rather than changing rounding
with fused multiply-add. Objects skip filter construction only after their full
convolution tail has drained and their new input is exactly silent. Their last
silent direction is restored before an audible transition, including motion and
speaker focus changes during silence.

The native partition is now 1024 frames. At 48 kHz this gives 21.33 ms per render
block and 42.67 ms across the two convolution stages, 32 ms more fixed latency
than the previous 256-frame implementation. Source metadata and VBAP motion still
advance at their authored sample positions with a 128-frame routing quantum.
Per-object filter targets/crossfades update once per convolution block. Changing
partition size therefore changes latency and filter-transition granularity; it
does not produce sample-identical output to the old partition size. No object
count, sample rate, or room impulse-response length is reduced.

## Measurements

Local release-build measurements on 2026-09-08, using the same first 20 seconds of
NaturesFuryADM.wav (48 kHz, 24-bit, 10 bed tracks, 108 objects), real PCM, and ADM
metadata. File reads and PCM conversion are outside the render timer. The first
20 blocks are warm-up. No audio device playback is involved.

| Implementation | Block frames | Mean render time | Block budget | Render time / audio time |
| --- | ---: | ---: | ---: | ---: |
| Previous code | 256 | 9.588 ms | 5.333 ms | 180% |
| Optimized, original partition | 256 | 6.204 ms | 5.333 ms | 116% |
| Optimized, intermediate partition | 512 | 8.349 ms | 10.667 ms | 78% |

The 512-frame run across the whole 108.67-second file averaged 11.158 ms per
block (105% of real time), so it was insufficient for the later dense sections.
The final 1024-frame implementation rendered that entire file with a mean of
18.541 ms per block against a 21.333 ms budget (87% of real time); p95 was
24.212 ms and the maximum was 100.901 ms. All output samples were finite.
The existing FIFO absorbs short scheduling spikes; offline measurements do not establish zero underruns
under every desktop load. Simultaneous dense motion of every object or longer
imported room responses can still exceed a machine's capacity.

## Reproduction

From the repository root, prepare metadata with the production BWF parser:

```powershell
node scripts/prepare-adm-native-benchmark.mjs "path/to/master.wav"
$env:SDA_ADM_BENCHMARK = (Resolve-Path tmp/adm-performance.json).Path
$env:SDA_ADM_BENCHMARK_SECONDS = '20'
cargo test --manifest-path apps/native-renderer/Cargo.toml --release --locked --offline benchmark_adm_file -- --ignored --nocapture --test-threads=1
```

The file benchmark accepts 48 kHz, 24-bit ADM PCM. Metadata JSON refers to the
local WAV; neither audio nor metadata needs to be committed. Increase the seconds
limit to cover the whole file. Run benchmarks without another build or benchmark
competing for the CPU.

```powershell
cargo test --manifest-path apps/native-renderer/Cargo.toml --release --locked --offline -- --test-threads=2
cargo test --manifest-path apps/native-renderer/Cargo.toml --release --locked --offline benchmark_adm_direct_objects -- --ignored --nocapture
cargo test --manifest-path apps/native-renderer/Cargo.toml --release --locked --offline benchmark_adm_full_engine -- --ignored --nocapture
```

Regression coverage includes scalar/AVX agreement at partial-vector boundaries,
serial/batched rendering during motion and focus changes, silent re-entry,
long measured-room tails, bed/object separation, mode switches, and authored
metadata timing.
