# iOS research checkpoint package

User explicitly paused vocal research and requested commit, push and iOS packaging.

## Included runtime state

- Existing unified feat/android-port branch retains merged AirPods head tracking, Atmos +2dB linked preguard presentation gain, accepted bounded spatial enhancement and existing direction-independent12–35ms cue window.
- Head tracking repeated-setting is idempotent; subscription starts before warm pose replay; pending calibration has an explicit status.
- Device audio route/sample-rate diagnostics are read-only; no playback prescan or analysis cache.
- No H13 default replacement, no FFmpeg downmix injection, no source/ID/time-specific vocal replacement, no rejected phase/EQ/front-reflection policy enabled.

## Research archived, not simultaneously enabled

Commit includes offline diagnostic scripts and reports/documentation for core downmix, JOC/metadata parity, object latency, pair interference, spatial cue experiments, phase-branch analysis, H13 complete-subject diagnostic and motion/three-song comparisons. Test-only controls do not enter release playback. Copyrighted song PCM and audition WAVs remain local, not committed or bundled into the app. Existing unrelated Android/desktop changes remain unstaged.

The request to include research is a checkpoint of implementation plus investigation, not a claim that mutually incompatible/rejected trials form one verified universal repair. Vocal/space listening goal is paused, not complete.

## Latest downmix comparisons

OpenJOC expert trim8, observed-vendor-compat, DRC disabled, reconstruction-only experimental speaker bridge generated5.1 and7.1.4; excluded separate Base fullband to avoid unresolved double counting. LFE measured zero; all four7.1.4 height channels nonzero. FFmpeg automatic matrix converts to stereo. Neither file is an official Dolby oracle. Crop starts at140*48000+609 and lasts40sec; audition static gain to−18LUFS with4xpeak safety. Local evidence:diagnostic-714/51-listening-report.json.

SDA latest audition used original decoded objects/metadata, bundled KU100/spatial cues, enhancement enabled, balance disabled, fixed head pose, ordinary master0dB. Full-prefix guard minimum.913474;140–180sec raw−12.9286LUFS, offline audition static−5.0714dB to−18LUFS. Not device capture. Evidence:sda-current-listening-report.json.

## Validation

Native test-only stale reflection helper references were caught by compilation and corrected before release. Rejected front reduction remains confined to its test body; temporal preservation test calls the actual production temporal helper. Portable iOS assets/headtracking/session/enhancement/temporal source checks executed. Windows cannot run xcrun or actual simulator tests: Apple checks remain required in the iOS CI workflow. Build/delivery outcome must be recorded from the actual exact-head workflow, not inferred from local checks.
