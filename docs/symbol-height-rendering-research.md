# Symbol III height rendering investigation

Research date: 2026-09-06. Listening window: 186-198 seconds.

## Observations

The listener identified the target instrument in Obj11, Obj14, Obj19 and
Obj21. A level-matched Obj19 test favored calibrated KU100 at its decoded
direction (-90 degrees azimuth, 45 degrees elevation) over the 7.1.4
speaker-filter sum. Raw KU100 was not preferred over calibrated KU100.
Replacing only Obj19 in the complete mix, with its rendered RMS preserved,
did not produce a sufficiently clear effect.

Obj19 PCM RMS is approximately 17-19 dB below the other three objects in
this window. This measures entire objects, not isolated violin stems.
Aggregate metadata-weighted energy equals raw energy in this extraction;
this does not independently validate decoder correctness or event timing.
The dry, static, zero-spread offline tests are not a full live-path reference.
No production rendering fix has been established by these tests.

## Verified public documentation

1. Valve, Virtual Surround Effect:
   https://valvesoftware.github.io/steam-audio/doc/capi/virtual-surround-effect.html
   Describes HRTF rendering at speaker locations. Explicitly identifies
   panning sources to a surround format before binaural rendering as an
   approximation that reduces CPU usage at the cost of spatialization accuracy.
2. Valve, Binaural Effect:
   https://valvesoftware.github.io/steam-audio/doc/capi/binaural-effect.html
   Spatializes a point source at its direction relative to the listener.
   Offers nearest-neighbor and bilinear HRTF interpolation. Interpolation
   has CPU costs; documentation does not promise that it fixes elevation
   audibility of a quiet instrument in a complete mix.
3. Apple, Set up binaural render modes:
   https://support.apple.com/guide/logicpro/binaural-render-modes-lgcp789f000d/mac
   Dolby Off/Near/Mid/Far modes apply to individual bed channels and objects.
   They are distance-model settings, with Mid the default and LFE fixed Off.
   Off still includes the signal without distance modeling. These settings
   have no effect on Apple's renderer. They are not documented as elevation
   gain controls or a general solution to masking.
4. Apple, Spatial Audio Monitoring:
   https://support.apple.com/guide/logicpro/spatial-audio-monitoring-lgcpde5e907c/mac
   Documents head tracking, personalized spatial audio profiles, and music
   versus movie modes. Head tracking keeps the virtual scene fixed as the
   listener moves. This does not expose Apple's DSP or offer its native
   renderer as a Windows implementation.
5. Apple, Dolby Atmos plug-in signal flow:
   https://support.apple.com/guide/logicpro/dolby-atmos-plug-in-signal-flow-lgcpc8111cb7/mac
   Documents separate bed and object input paths and monitoring-dependent
   rendered output. It does not disclose proprietary HRTF processing.

Google search timed out and the attempted Dolby support article returned
an HTTP error. Dolby mode descriptions above are verified through Apple's
official documentation, not a successfully retrieved Dolby support article.

## Engineering implications and next experiment

The native independent-object path currently sums physical-speaker filters
using VBAP amplitudes. Separate object convolution histories preserve object
state but do not remove the spatial approximation of the speaker-filter sum.
Valve's documentation supports comparing this architecture against direct
object HRTF rendering; it does not establish a defect in this particular file.

The next controlled comparison should render ALL active objects at their
decoded directions, preserving source gains and positions, against the
existing layout path. It must not move the horizontal violin-bearing objects
upward. Match overall playback level and retain an unnormalized measurement
report. Validate metadata spread and timing before calling it live parity.

If this improves the complete mix, an optional direct-object mode is a
defensible next implementation. A proposed hybrid could retain layout-based
bed and room-reflection rendering, with object direct sound rendered at its
own direction. That is an engineering proposal, not a documented Apple or
Dolby algorithm, and is not equivalent to physical-speaker reproduction:
layout changes would no longer affect object direct sound in the same way.
Speaker mute, solo, focus, room calibration and transition semantics require
explicit design and regression testing before integrating that mode.

Do not infer that direct rendering alone will make this quiet source salient.
Height boosts, lowering other sources, or moving horizontal content upward
are mix enhancements and must be labeled separately from faithful rendering.

## OpenJOC application comparison

The installed, previously hash-verified official OpenJOC 0.16.0 CLI rendered
the original MP4's extracted access units from the start through 199.008 s.
Default OAMD validation failed with a truncated-bitstream error. Explicit
`--trim-config-count 8` admitted and rendered all 6219 access units without
modifying the executable. This parameter is a compatibility assumption, not
independent proof of the metadata interpretation. Encoded DRC was disabled;
default digital-calibrated dialnorm and OpenJOC internal headroom were retained.
Output: 48000 Hz, float32, 12 channels in reported order
FL, FR, FC, LFE, Lb, Rb, Ls, Rs, TFL, TFR, TBL, TBR.

Reproduction scripts and reports are under `tmp/symbol-306`:
`extract-reference.mjs`, `openjoc-compare.py`, `openjoc-performance.json`,
and `openjoc-comparison.json`. The raw output is `openjoc-714.wav`.

Within a bounded +/-4096-sample alignment search around 186 seconds,
the right-front-top waveform aligned at -545 samples relative to the saved
SDA PCM window and correlated at 0.999999959. The result is not at the search
boundary. This strongly supports the same waveform reaching that speaker
up to scale in this window; it is not a complete object-by-object validation.

Summed top-channel energy relative to summed horizontal-channel energy:

- SDA static zero-spread VBAP reconstruction: -23.452 dB.
- OpenJOC speaker output: -28.868 dB.

OpenJOC is approximately 5.416 dB lower by this aggregate top-to-horizontal
measure. The result does not support a top-layer loss unique to SDA relative
to this reference application. Different spatial projection, trim handling,
and internal headroom remain confounds. It does not establish Dolby fidelity
or violin-only levels, and the implementations may share specification errors.

Both channel outputs were convolved with the same calibrated KU100 dry
speaker filters at SDA speaker positions, excluding LFE, and matched using
one whole-mix RMS scalar. Finite samples and peak headroom were checked.
Listening clips: `reference-sda-listen.wav` and
`reference-openjoc-listen.wav`, each 12 seconds. No production change was made.

## Subsequent single-speaker localization defect

The listener subsequently reported incorrect localization even with just one
overhead speaker soloed. Live logs and settings identified `hrtf-dense-raw`,
with cinema disabled, headphone compensation cleared, flat three-band EQ,
and the low-frequency diagnostic shelf enabled. Earlier dry comparisons used
the standard calibrated set; they did not represent this live asset selection.

The native renderer used its active dense object set for physical speaker
filters as well. The dense KU100 grid has 30-degree azimuth spacing in the
45-degree elevation ring and omits the physical +/-45 and +/-135 degree
speaker anchors. Nearest selection therefore displaced those speakers to an
adjacent measurement. The web renderer already distinguishes physical speaker
IRs from dense-only fills.

Native dense sets now load their matching standard speaker set (`hrtf` or
`hrtf-raw`) for physical speaker filters. Object-direction lookups retain the
dense grid. Cinema profile precedence and the selected calibration mode remain.
This fixes an identified direction-selection defect, not a demonstrated cure
for every reported perceptual issue.

Validation: 73 native tests passed, including calibrated/raw dense speaker
parity over 7.1.4 and 9.1.6, overhead Solo/focus invariance, and an independent
time-domain impulse oracle for the complete neutral native output chain at
wet weights 0 and 0.04. The long existing authored-bed tour was excluded from
this run; no claim is made that it passed.

## Real-device monitor and subject comparisons

The listener confirmed a hard-left / hard-right control separates normally.
A pure calibrated KU100 control was lateralized, but right-front-top elevation
was weak. That distinguishes weak elevation from a proven mono-output defect.

`tmp/symbol-306/live-mode-comparison.py` launches the deployed native executable
in a temporary process, uses the actual framed command/PCM transport, declares
one TopFrontRight bed, and captures the default Realtek headphone endpoint's
WASAPI loopback. It uses the same 300 Hz-12 kHz signal as the offline control,
dry calibrated KU100, neutral EQ, no headphone compensation or room profile.
Normal, Solo and focus right-minus-left energies were 8.8968, 8.8998 and
8.9038 dB respectively; zero-lag ear correlations were about -0.31. These
conditions did not collapse to mono. Focus capture reported one discontinuity,
so it is not a sample-exact timing reference. This tests one active source,
not masking by background sources or every live application setting.
The temporary process was closed and the existing app settings were preserved.

`build-subject-comparison.py` generated level-matched dry right-front-top clips
for SADIE D1 KU100, D2, H3 and H13, each using its complete subject's exact
(-45,45) entry. Each clip uses one joint-ear RMS gain, with no per-ear changes.
Files are `subject-ku100-height.wav`, `subject-d2-height.wav`,
`subject-h3-height.wav`, and `subject-h13-height.wav`. No preferred subject is
inferred from numeric energy; the listener must evaluate elevation.
The proposed same-azimuth horizontal comparison was not generated because
the standard sets do not contain the exact (-45,0) entry.
