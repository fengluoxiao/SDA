# Desktop loudness audit — October 26, 2026

## Verified stereo fix and startup cost

Ordinary ALAC stereo and its SDA upmix now get a freshly measured full-source
reference before starting playback. References are scoped to one playback;
no desktop loudness result is read from or written to a persistent cache.
Output SRC, HRTF and the approved peak/spatial protection remain in playback.
WAV export measures the same original source at its own sample rate.

For `BADDY BADDY`, the old desktop selected a quiet intro at -20.2184 LUFS and
locked 0 dB programme correction. Complete-source metering at 48 kHz measured
-5.62593 LUFS. The optimized source-rate measurement is -5.64809 LUFS and selects
-12.35191 dB, with the existing separate +2.5 dB calibration when enabled.
This is a 0.02216 dB difference in reference level, rather than changing the
listening resampler or output audio quality.

One measured baseline took 44.577 s: 32.700 s SRC, 6.878 s metering, 3.363 s
decoding plus other work. The final production source-rate analysis took
12.690 s on the same real source. These are local observations, not timing
promises for every file or device.

## Other formats: checked, not yet comprehensively repaired

- E-AC-3 Atmos: decoder supplies a complete compatible speaker reference.
- MPEG-H / 360RA: decoder supplies the pre-limiter stereo programme tap.
- Audio Vivid: verified 7.1.4 beds are metered as speaker beds; object scenes
  provide a fixed 7.1.4 scene reference.
- AC-4: decoder supplies a fixed 7.1.4 scene reference.

These desktop routes still select the first usable reference (57 valid
400 ms blocks, 100 ms hop; typically about 6 s of programme) with a bounded
7 s startup gate. A controlled quiet-intro reference test on the actual
player scheduling methods reproduced 0 dB selected gain versus -10 dB for
its complete reference for E-AC-3, MPEG-H, AV3A and AC-4. This demonstrates the
policy weakness; it does not establish a specific loudness error for every
real song or a physical output difference between a PC and phone.

TrueHD object/multichannel frames currently have no `reference_loudness`
attached in `truehd_pipeline.rs`, and the worker does not infer one from
unrendered objects. That is a separate eligibility gap. In addition, MP4
container aliases (`ec-3`, `ac-4`, `mha1`/`mhm1`) can reach the native programme
codec gate without canonicalization; makeup recognition must be audited with
those aliases before claiming uniform output calibration.

A future fix must use a complete, freshly computed neutral programme reference
for each source format. Never meter arbitrary raw object channels as speaker
channels, add late automatic programme attenuation, or revive a stored
loudness cache. Do not treat this stereo optimization as a complete repair
of the immersive routes above.

## Development protocol failure

A stale Electron main process rejected a new worker's reference-policy name
while Vite had already updated renderer/worker code. Compatible measurement
policies now share explicit measurement protocol 1, and incompatible results
produce a restart-oriented error instead of a generic analysis failure.
The old main process was fully exited and the new running window verified
successful real-song playback with the selected programme correction.
