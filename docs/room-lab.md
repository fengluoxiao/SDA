# Room Lab

## Native Convolution Performance

The native partitioned convolver stores and multiplies only the unique half of
each real-signal FFT spectrum, reconstructing conjugate bins before inverse
transforms. This preserves the full response length, per-object histories and
sample-wise filter transitions. It does not reduce reflection order or merge
objects into a shared bus. A local 16-source, 8188-tap comparison measured
approximately 2686 to 1312 us/block for static filters, and 8269 to 3773 us/block
when retargeting every source every block. These isolated timings are not a
guarantee for every layout/source count; the 128-sample deadline is 2667 us at
48 kHz, and continuous worst-case motion still exceeds that in this benchmark.
Live playback of the user's 16-source 7.1.4 track with the treated room and
speaker calibration initially averaged about 3999 us/block and lost 186848
output frames in a 12-second window. With this optimization, a later 12-second
window averaged 2325 us/block with no underruns. A subsequent 60-second cinema
run through approximately 1:38-2:38 averaged 2433 us/block with zero new source
or output underruns. The playback segments differ, so these live numbers are
operational checks rather than an identical-input performance benchmark.

Sound paths include a looping propagation animation. All paths share one source
emission time and move at 343 m/s before visual slow motion (25-200x, default
100x). Markers follow accumulated segment lengths, including wall bounces, so
shorter paths arrive first. The animation switch pauses markers in place; this
is a geometry illustration, not a music waveform or an audio-processing control.

The `房间` floating button opens the room simulator, comparison controls, sound
paths and two layout memories. This is a measured-source simulation, not a
Genelec GLM implementation or a measured personal room.

## Audio Model

The generator uses pyroomacoustics 0.10.1 image sources with frequency-dependent
wall absorption, air absorption, inverse-distance propagation and fractional
arrival delays. Every virtual speaker points toward the centered listener.
Speaker distance is the ray intersection with a wall/ceiling inset by 0.25 m,
multiplied by the placement ratio. Layout azimuth/elevation is preserved.

- Source: DIRPAT Genelec 8020, 540 directional measured FIR responses,
  resampled from 44.1 to 48 kHz by polyphase filtering. DIRPAT's coordinate-order
  error is repaired as in the library's specialized reader.
- Receiver: original SADIE II D1 KU100 48 kHz HRIRs, loaded from all available
  measurement directions. Duplicate pole coordinates select the first original
  response. The library selects nearest measured emission/arrival directions.
- Each ray passes through the source's emission-direction FIR and the receiving
  ear's arrival-direction FIR. The generator runs zero-order and selected-order
  simulations separately to produce direct-only and full-room responses.
- All source-measurement FIR samples are retained. Their measurement latency
  and any residual measurement-room contribution are not claimed to be removed.
  This limits equivalence to a perfectly anechoic physical Genelec speaker.
- Configurable room dimensions: length 3–10 m, width 3–8 m, height 2.2–4 m,
  ear height 0.8–1.6 m, radial placement 0.5–1.0, reflection order 1–12.
- Wall presets are explicit **assumed** absorption coefficients at
  125/250/500/1000/2000/4000/8000 Hz. They are not measured wall materials.
- Finite reflection order does not model an infinite diffuse reverberation
  tail, diffraction or low-frequency wave modes. Responses beyond 32,768 taps
  (about 683 ms) are rejected, not silently truncated.

Generated profiles are marked `simulated`, contain source hashes and simulation
parameters, and are stored in the existing local cinema-profile library. Native
playback convolves these responses through the existing independent-object and
speaker-bus paths. Decoded object motion is retained. LFE remains on the existing
low-frequency path; the simulator does not claim a measured Genelec subwoofer.
A fixed-listener BRIR is not a multi-pose measured head-tracking dataset.

## Comparison

Three modes select original KU100, calibrated KU100, or the generated room.
Comparison uses the ordinary KU100 set, neutral cinema settings, and the room
stereo mode. On exit the previous head, calibration/dense preferences, stereo
mode and cinema configuration are restored. A local backup permits recovery
after a page reload. Switching convolution graphs can briefly interrupt audio.

Optional reference matching computes 20 Hz–20 kHz pink-noise response energy
for equal independent feeds into the layout speakers. It attenuates the louder
reference modes to the quietest, with a -40 dB limit, using a separate smoothed
native gain shared by both ears. It does not replace master volume or programme
gain. It is not a perceptual loudness measurement of the current song; source
correlation, monitoring, LFE, headphone EQ and listener perception can differ.

## Sound Paths and Layout Memories

The sound-path view uses the same room dimensions, listener and speaker points
as the generator. It displays the direct and six first-order reflection paths.
Reported times are geometric propagation times, excluding measured FIR latency.
Clicking a speaker selects its paths without changing focus/mute/solo. Orbit and
zoom are independent of audio. Close the panel for an unobstructed view; use
`返回声场` to return to the ordinary object view.

Two layout slots retain the layout and multi-select mute, solo and focus sets.
Restoration checks the layout, filters channel names to that layout and rejects
conflicting focus versus mute/solo states. Stereo programmes cannot load an
immersive layout. Memories are local browser settings.

## Local Runtime

The simulator runs as a separate, cancellable Python process, outside the audio
thread. Progress is reported per speaker. Only one job runs at a time; jobs have
a ten-minute deadline. Install the pinned dependencies into an isolated Python
environment from `scripts/room-simulator-requirements.txt`, then configure:

```powershell
node scripts/configure-room-simulator.mjs --python C:/path/python.exe --python-path C:/path/site-packages --source C:/data/LSPs_HATS_GuitarCabinets_Akustikmessplatz.sofa --hrtf C:/data/D1.zip
```

The generated `apps/desktop/room-simulator/runtime.json` is machine-local and
ignored by Git. `SDA_ROOM_RUNTIME` can point to another configuration, including
for a packaged application. The ordinary desktop installer does not bundle the
scientific Python environment or DIRPAT archive. Missing runtime dependencies
are reported by the UI instead of substituting fabricated measurements.

This workspace's runtime is configured and the default 7.1.4 response has been
generated and validated. No external author messages were sent. Source rights
metadata retains the discrepancy documented in
[Genelec research](genelec-measured-room-research.md); no clean redistribution
license for a DIRPAT derivative is asserted here.

## Verification

The physical regression generates real responses and checks that reduced
absorption raises reflected energy without changing the direct response, and
that nearer speakers arrive earlier and produce stronger direct sound. Path
segment lengths are checked against propagation time. Native tests check common
ear gain and independent-object/bus consistency. UI checks cover comparison
restoration, layout memories, desktop/mobile controls and nonblank moving 3D.
# Clarity Investigation (2026-09-06)

Revision 2 corrects the measured loudspeaker orientation. Pyroomacoustics
`Rotation3D` applies extrinsic rotations in the specified order. Pitch must be
applied before yaw (`yz`, angles `[-elevation, azimuth]`) to aim the local +X
axis at the listener. Revision 1 applied yaw before pitch; floor speakers were
unaffected, but the front height speakers in 7.1.4 missed the listener by about
75.5 degrees. In the treated 6 x 4 x 2.8 m revision-1 profile, their reflected
energy exceeded direct energy by approximately 2.8 to 3.2 dB. This identifies
an implementation defect, not a characteristic of real Genelec speakers.
After correction, the same front height speakers have reflected-to-direct
energy ratios of -11.0 and -10.8 dB. Floor-speaker responses are unchanged.
These are impulse-response measurements, not proof of subjective clarity.

The reflection stage now applies to the selected comparison mode, including
raw and calibrated KU100. KU100 uses its own measured dry/wet assets and keeps
the existing 0.04 residual weight; it does not load the Genelec simulation.
Full mode preserves the previous KU100 mix, direct removes its residual, and
early uses the same 50 ms boundary/10 ms transition as native cinema processing.
The effect can be subtle, especially with the calibrated room residual.
`scripts/build-ku100-comparison-levels.py` generates the bundled per-layout
KU100 reference energies; rerun it when changing the KU100 assets or mix policy.
The UI computes one downward-only reference across all nine mode/stage pairs
from those energies and the selected room's stored energies. Selecting a stage
keeps the active mode; no stage is highlighted while comparison is inactive.

Newly generated profiles carry `simulation.revision: 2` and a material label.
Existing profiles remain available for comparison and are marked as old in the
UI; they are not silently relabeled or overwritten.

Room comparison now offers direct only, direct plus early reflections, and the
full simulated room. Direct-only preserves the entire order-zero measured
source/HRIR response, with exactly zero added simulation reflections. Early
mode removes only the simulated residual after a complementary 10 ms transition
centered 50 ms after direct onset. It does not remove the measured source tail.
All five reference conditions (raw KU100, calibrated KU100, and three room
stages) share the quietest pink-noise energy reference, with downward-only gain.
The modes preserve both ear filters, their relative timing, and object routing.

The measured on-axis Genelec response contains approximately 1.45% of its
broadband energy more than 5 ms after its peak, and 0.24% after 10 ms. This does
not establish whether that energy is intrinsic decay or measurement-room
contamination, nor exclude audible coloration in specific frequency bands.
No source gating or treble boost is applied without stronger evidence.
`scripts/room-clarity-report.py` reports these tails and per-speaker direct and
reflected energies using the actual measurement files and generated profiles.
