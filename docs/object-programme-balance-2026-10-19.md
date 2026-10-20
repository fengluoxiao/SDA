# Object programme volume balance — 2026-10-19

The AV3A Mixed decoder was connected to the scene renderer, but volume-balance
eligibility still accepted only the verified twelve-channel AV3A bed, stereo
masters and MPEG-H reference output. Mixed AV3A therefore played correctly while
the volume switch bypassed it. E-AC-3 object programmes, including the reported
Happier case, were excluded by the same eligibility rule.

## Shared reference contract

`FrameData.reference_loudness` carries a decoder-owned programme measurement.
Native and browser players accept this explicit capability for any codec. Raw
object channel counts never determine BS.1770 speaker weights. Other object
programmes without a valid reference remain excluded. This adds a reference
contract rather than a list of per-song/per-format gain exceptions.

AV3A Mixed creates a measurement-only fixed 7.1.4 VBAP reference after SRC on the
48 kHz output clock. Authored object gains, position/size/diffusion and ramps are
applied to that reference, and correlated sources sum in speaker channels before
K-weighting. LFE has zero loudness weight. The reference is independent of HRTF,
head pose, rooms and the user-selected enhancement. It is never used as audible
PCM or as the compatibility bed. It is a project reference policy, not a vendor
reference-renderer conformance claim.

E-AC-3 uses its existing complete decoded compatibility speaker bed, with weights
selected from actual speaker roles. Reconstructed object slots are not measured
as speakers. Plain stereo keeps its existing measurement/cache route.

The previously tested native BS.1770 meter now lives in shared core. Golden
vectors still cover the existing sample rates, interpolation, channel weights and
chunk sizes. The measurement is passed as scalar metadata through native and WASM
frames; object PCM is not copied to JS again for another analysis pass.

## Gain, startup and persistence

The existing -18 LUFS target, -1 dBTP protection, attenuation-only policy, source-
clock gain scheduling and one linked post-render programme gain remain in use.
A decoder reference makes the ordinary bounded startup buffer available before
first audible PCM. Live OFF/ON changes the same final stereo path. Source gains,
front/rear ratios, +2 main / max +6 auxiliary and strong vocal clarity remain the
approved rendering recipe.

New object-reference measurements are calculated during ordinary decoding. They
are not persisted as stereo/bed analysis caches. Previously loaded stereo/bed
measurements are discarded when a decoded reference appears, and late cache
updates are ignored. Seek clears reference meter/controller history. Existing
stereo/ALAC/MPEG-H cache behavior remains in place.

## Actual-file checks (offline shared native engine, not a physical phone)

The supplied AV3A file decoded all 9,722 frames / 9,955,328 samples on a 512 KiB
worker. Its interleaved source FNV remains `84f2cecf85884c05`, identical to before
measurement was added. The project reference measured -20.06982 LUFS and -7.47302
dBTP. Its target is therefore 0 dB. Real native stereo ON/OFF energy differences
were below 0.000001 dB in the checked ranges: no attenuation is required by the
existing policy even though eligibility and renderer enablement are now active.

Happier decoded 6,496 frames / 9,977,856 samples. Its compatible speaker reference
measured -17.20186 LUFS / -2.30344 dBTP. The streaming controller's final target
was -0.80593 dB. Actual final stereo ON-minus-OFF energy differences were:

| Source range | Output difference |
| --- | ---: |
| 12–14 s | approximately 0 dB |
| 76–90 s | approximately 0 dB |
| 150–180 s | -0.28758 dB |
| 180–200 s | -1.02101 dB |

These are source-aligned real output comparisons, not gain-status assertions.
Both captures use the same diagnostic master preamp -12 dB and output volume 0.5
to keep the guard out of the gain-ratio test; these are relative differences,
not measurements of app-default absolute loudness.
The cumulative streaming estimate evolves through the song; quiet opening
material does not predict its final full-track loudness. The audio images and
presence settings were not adjusted to create a larger switch difference.

Synthetic hot object scenes additionally verify a nonzero linked attenuation and
live OFF/ON changes at the ordinary HRTF stereo FIFO. Tests cover cancellation of
co-located anti-phase objects, authored gain applied once, LFE weighting, ramp
chunk continuity, reference eligibility across current/future codecs, cache
isolation, JS open/push/flush propagation and unchanged legacy balance vectors.
Private music/PCM is not committed or included in release samples.


2026-10-20: the AV3A scene meter is now the public shared `scene_reference::SceneMeter`; AC-4 Full A-JOC publishes the same reference capability. Sparse declarations and gain-only events retain their spatial state. The decoder reference and programme controller remain format-independent, and the new spatial balance no longer adds a final-mix LUFS normalizer.
