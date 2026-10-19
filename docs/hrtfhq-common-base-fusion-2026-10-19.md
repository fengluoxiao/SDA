# HRTFHQ common base and fusion audit — 2026-10-19
Follow-up: the B gain base is retained. The user subsequently selected the strong common-front clarity addition; see [accepted front clarity](hrtfhq-accepted-front-clarity-2026-10-19.md) for the current shared enhancement default and lifecycle checks.


最终确认：修正同曲增益后，用户明确全部12首选B，包含The Very First Night。B已作为共享渲染器默认配方。此前“待复核”内容保留为过程记录，现已由此次确认取代。打包、提交及推送随后被用户暂缓，本轮只合入代码并修移动端问题。


Latest update — accepted B and fixed-gain export: the user confirmed B (+2/+6 base plus the 4 kHz/+1.5 dB shelf) for the other 11 materials, while explicitly withholding a Very First Night decision because the old long/short exports differed in loudness. The default source recipe is now B. All audition excerpts now share one fixed gain per track/profile/mode, verified with sample-identical overlaps. The historical A–E research and tests below remain provenance, not a claim that the pending song was accepted.


The user selected the net +2 dB main / +6 dB auxiliary HRTFHQ audition for eight of twelve materials, then explicitly requested this as the common base and asked whether the other choices could contribute useful processing to that base. Production does not select a DSP recipe by song, time window or object ID. Ether remains HRTFHQ; the legacy Ether/KU100 recordings are listening references.

## Integrated common base

The enhancement preset now selects the exact archived front-45-to-side-90-degree auxiliary geometry. A spatial-path-only linked +2 dB stage and at most +4 dB auxiliary source lift reproduce the selected net +2/+6 endpoints relative to the same engine baseline. Height blending, LFE exclusion from source lift and the existing source gain slew remain. Source PCM and authored gains remain unchanged. Beds use canonical world-layout weights; objects use world positions. The serial and parallel object mixers receive the same complete layer settings.

The linked +2 dB stage is separate from the user master preamp and from programme loudness balancing. It runs before the existing linked peak guard and inside the spatial branch, so the compatible/plain output route bypasses it. Switching the enhancement off resets the spatial preamp and auxiliary lift to unity/zero; same-family HRTF reloads and track resets retain the existing override rules. Direct API users of auxiliary gain retain the later protected-side geometry until they enable the new enhancement preset.

The former +1.5 dB auxiliary lift, independent rear +3 dB and 4 kHz/+1.5 dB shelf are not stacked on this selected base. The HRTFHQ v3 direct responses, common-ear midrange preparation layer, half-strength retained61 reflection banks, pairwise interpolation clock, codec conditioning and mobile/desktop output controls remain.

## What other choices can contribute

| Chosen reference | Comparison with common base | Integration decision |
|---|---|---|
| Ether I | Its motion-aware residual floor of -15 dB already runs on HRTFHQ shared-depth sends in both mixing paths. | Retain once; no additional attenuation or legacy restore. |
| KU100 for エガクミライ | The previously accepted KU100-derived common-ear midrange presentation is already in the HRTFHQ v3 preparation layer. KU100 additionally changes direction-dependent ear responses and interpolation; a single song preference cannot identify which difference caused its advantage. | Retain the established midrange layer. Keep the selected KU100 audio as a reference; do not blend a second direct HRTF into the base or claim the full preference is solved. |
| Current version for 2step | Its independent high-frequency shelf can be added without changing the selected auxiliary geometry or ratio. Its separate rear gain would raise the already fully lifted rear sources above the selected +6 dB endpoint. | Prepare a base-plus-shelf candidate for all twelve materials; do not automatically add another rear lift. |
| Net +2/+8 for How Love | Same HRTFHQ response and old geometry, with auxiliary extra +6 instead of +4. These are different level targets. | Prepare the stronger auxiliary ratio as a single-variable candidate across all twelve materials. It cannot simultaneously be the +2/+6 default. |

Candidates B (base plus the old high shelf) and C (base with stronger auxiliary) are generic additions/changes to A, not song-specific modes. Each changes one parameter only; they are not simultaneously promoted to defaults. Old 2step and エガクミライ selected references are provided alongside these candidates in matched-loudness mode.

## Validation and listening delivery

The native release suite passed **346 tests, 0 failed**, with 35 offline/fixture tests ignored. The production non-test library check and both enhancement-control scripts passed. All **60 fresh native replays** retained a peak-guard gain of 1 under common diagnostic headroom. All 12 base replays match their independently compiled selected auditions without fitted gain or time alignment: maximum sample error 5.9604645e-08, worst residual -133.208 dB. The 12 stronger-auxiliary comparisons have maximum error 6.7055225e-08, worst residual -133.145 dB. The 23 excerpts produced 235 PCM24 WAV files (including selected historical references); all hashes verified, no skipped references. Maximum PCM24 roundtrip error 1.1897636e-07; maximum 4x oversampled peak -1.500 dBFS; maximum matched-loudness error 0.036780 LU. Browser verification decoded all 235 files and passed actual muted playback, same-position switching, export/persistence, standalone file playback and desktop/mobile layout checks with no page errors. The batch checks the integrated base against the independently compiled earlier +2/+6 audition, and the stronger candidate against the earlier +2/+8 audition, using the same decoded PCM/metadata with no fitted gain or time alignment. Both volume modes use lossless PCM24 exports with loudness and oversampled-peak checks.

Local delivery: C:/Users/legendshop/Downloads/SDA-hrtfhq-base-fusion-20261019/site/index.html. No installer or physical-device listening result is implied by these host checks.

## Follow-up: committed Very First Night repair was missed in the integration regression

The user identified the historical left/right vocal over-lift at 50–65 seconds. Their recent net +2/+6 selection covered 0–12 seconds and did not constitute acceptance of that problem window. The earlier audit listed the old balance recipe, but the new common-base integration did not include it as a blocking listening regression. This was an integration omission.

Git verification confirms commit `577bda573438c71b977dd6cda57cd96c2a5d68cc`, dated 2026-10-08 19:17:27 +08:00, `fix(ios): ship accepted auxiliary bypass and post-guard trim [skip ci]`, is an ancestor of the current branch. It set auxiliary gain to zero, retained main_restore=2 with master=2 and applied -0.75 dB after the linked guard. Commit `54340e570fac20f907ac7157088ae7ef24795140` (2026-10-11) later replaced the paired main compensation and final trim as part of the bounded-lift default. The committed fix was real; its effective default was superseded.

The old listening record specifically says horizontal main protection alone, and then additional main-only attenuation, failed. Acceptance came after removing auxiliary lift, followed by the final post-guard trim. Therefore adding only a ProtectedSides classifier cannot be described as restoring the accepted fix.

The actual previously accepted PCM24 0–65s recording was found and verified against its stored SHA-256 `c1804e4c3072b95af2f8bd4669a478b1fcecd31109f0d950e540ee63b415ef8a`. It is included only as a matched-loudness historical reference.

Two additional recipes are replayed on all twelve materials, using HRTFHQ throughout:

- D: common-base net +2/+6 endpoints with horizontal front/side protection through 105 degrees and a 105–135 degree transition. This is a **fusion candidate**, not the historically accepted fix.
- E: the exact 577bda5 source/trim recipe on the HRTFHQ v3 family: auxiliary=0, master=2, main_restore=2, post-guard trim=-0.75. This is a **faithful recipe migration**, not a claim of waveform identity to the old KU100 recording. It cannot simultaneously retain the common base's auxiliary +6 endpoint.

| Spatial region | A common base | D protection candidate | E committed repair migration |
|---|---:|---:|---:|
| Horizontal front | +2 dB | +2 dB | 0 dB before guard |
| Horizontal direct side | +6 dB | +2 dB | 0 dB before guard |
| Fully auxiliary height/rear | +6 dB | +6 dB | +2 dB before guard |
| Final post-guard trim | 0 | 0 | -0.75 dB |

Figures are relative to the same underlying engine baseline and exclude unchanged codec-specific compensation. The ordinary peak guard may attenuate actual production playback. A continuous 0–65s comparison and the explicit 50–65s window are now required listening checks. No new song-name, time-window, or object-ID production switch has been introduced. The source-level common base remains unreleased; this regression must not be declared solved solely from passing numerical tests.


Follow-up numerical verification: D and E each replayed on all 12 materials. E on Very First Night matches the independently compiled earlier HRTFHQ balance migration with maximum sample error 1.8626451e-08, residual -133.819 dB, without fitted gain or alignment. Both the 0–65s continuous window and the 50–65s problem window include the verified original accepted reference. Browser checks cover all 235 WAVs, unique selection of 0–12 versus 0–65 windows sharing the same start, and the exact committed-fix deep link. These checks do not replace listening acceptance of the reopened regression.


## Corrected audition gain policy

The earlier exporter calibrated each excerpt independently, in both matched and common modes. For Very First Night B, the first 12 seconds of the 0–65 export were 6.140176 dB below the standalone 0–12 common export (6.104115 dB below in matched mode); correcting the known scalar leaves only PCM24 quantization error. Both excerpts used the same continuous native rendering. This was an export error, not a renderer-path difference. The user explicitly could not judge this song, so its selection remains pending.

The exporter now computes one reference range per track and one fixed gain per track/profile/mode. Very First Night uses 0–65 seconds; every short excerpt is cut from the same continuous source and receives the identical scalar. Matched mode calibrates each profile to the track reference range once; common mode uses one shared gain across the track's fresh profiles. Neither mode re-normalizes a short excerpt. Reference files with multiple windows now also come from one continuous source. Old audio is retained in audio/; the corrected page uses audio-v2/.

All 31 available overlapping comparisons, including 0–12, 26–38 and 50–65 versus 0–65 across profiles and modes, are PCM-sample identical (maximum difference zero). All 235 new WAV hashes are verified. The page excludes a pre-correction Very First Night selection from confirmation/export until the user chooses again; other-song B acceptance is retained. Exported selections carry normalizationVersion=2 and gainScope=track.

Reproducible utilities: scripts/export-hrtfhq-auditions.py --root <delivery> and scripts/verify-audition-fixed-gain.py --root <delivery>, using the audio diagnostic Python environment. These are offline audition tools and add no analysis or file IO to application playback.


Accepted-B production verification: 346 native release tests passed, 35 diagnostic tests ignored; production non-test library check and enhancement-control checks passed. Twelve actual default-path replays match the original selected B without gain fitting or alignment, maximum sample error 1.0430813e-07, worst residual -130.171 dB. The corrected fixed-gain delivery contains 235 verified WAV files and 31 bit-exact PCM overlap comparisons. Browser checks confirm all 235 files load, actual muted playback works, 0–12 and 0–65 remain distinct, and pre-correction Very First Night selections are excluded from confirmation/export. No installer was generated.

## Subsequent user gain correction

After installing the integrated build, the user explicitly requested removal of the independent object-bearing E-AC-3 +2 dB presentation gain because the shared base already supplies net +2 dB. That codec-only stage is now removed; the common spatial +2 dB and auxiliary extra +4 dB remain. Earlier audition figures above used the previous engine baseline and remain historical listening records. Ordinary decoder dialogue/programme loudness handling is unchanged.
