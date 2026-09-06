# Cinema Room Rendering

SDA's cinema controls are an explicit virtual-room and calibration tool. They do
not implement Trinnov Optimizer/WaveForming, Dirac ART, Dolby Surround or DTS
Neural:X. No brand-specific sound is claimed. Existing independent object PCM,
codec motion, layouts, source monitoring and headphone correction remain active.

## Controls

The `影院` floating panel has room, speaker-calibration and measurement-report
views. Changes are drafts until Apply succeeds. Settings and the selected room
are restored after restarting the native renderer or replacing the track.
Processing is disabled by default; enabled neutral settings retain the existing
HRTF/BRIR response. Applying settings rebuilds the convolution graph and can
briefly interrupt playback. This is not a continuous automation interface.

- Direct sound: -24 to +6 dB.
- Early reflections and late tail: -40 to +6 dB independently.
- Early/late boundary: 10 to 100 ms, with complementary 10 ms transition windows.
- Per speaker: -24 to +6 dB trim, 0 to 20 ms common-ear delay, and low/high shelves
  at 120 Hz and 4 kHz (up to +/-6 dB). LFE supports trim and delay only.
- Optional LR4 bass management: 40 to 160 Hz crossover, with independently
  trimmed redirected bass. The two second-order Butterworth sections on each
  branch give a fourth-order Linkwitz-Riley split. This does not boost source LFE
  or apply a second LFE low-pass to redirected bass. 2.0 disables redirection.

Original stereo bypasses spatial calibration/room processing and bass management.
Dry and room stereo retain the stereo-comparison behavior. Final headphone FIR,
EQ, volume/programme gain and peak protection remain shared. Extra gain may reach
the peak guard; this is not automatic loudness matching or bit-perfect playback.

## Response Model

Built-in calibrated assets provide direct-window and full-room responses. The
reflection component is `room - direct`; early and late controls are time windows
on that residual, not a claim of perfect physical source separation. Existing
built-in room weight remains in effect. With an imported room, Room mode uses the
full supplied measured room response, while Dry uses its direct window.

Both ears receive the same calibration delay and gain, preserving the supplied
interaural timing and amplitude ratio. Independent objects use the same calibrated
speaker filters as the bus path, combined with their actual layout gains and with
independent convolution histories. Filter lengths include delay and EQ tails;
they are padded uniformly so object movement cannot truncate a longer response.

## Importing Measurements

The importer accepts a self-contained SDA v1 JSON room profile. It checks 48 kHz,
finite bounded samples, complete channel coverage, matching angles, source and
license metadata. Stored profiles use SHA-256 identities and are rechecked before
loading. Metadata describes provenance; it is not proof of ownership or a claim
that a measurement has been independently verified.

Each profile is tied to one layout and contains one response for every non-LFE
speaker. A layout mismatch falls back to the entire selected built-in HRTF set,
with a visible status in the panel, rather than mixing partial subjects. The LFE
path remains a separate low-frequency output, not an imported directional BRIR.

Prepare measured stereo WAV files with:

```powershell
node scripts/prepare-room-profile.mjs measurements.json room-profile.json
```

Example input structure (all files must be real measurements):

```json
{
  "name": "My measured room",
  "source": "Measurement session and equipment description",
  "license": "Your actual permission or license",
  "measurement": "personal",
  "layout": "2.0",
  "speakers": [
    {"name":"FrontLeft","azimuth":30,"elevation":0,"file":"left-speaker.wav"},
    {"name":"FrontRight","azimuth":-30,"elevation":0,"file":"right-speaker.wav"}
  ]
}
```

Supported WAV input is stereo 48 kHz PCM16/24/32 or float32, 512 to 32768 frames
(up to about 683 ms). Longer room measurements require an intentional window
before conversion; the importer never silently truncates or resamples them.
The converter makes a common-ear direct window ending 4 ms after the later
ear's threshold onset, with a 1 ms fade. Inspect this window for early reflections
and noisy measurements before trusting its calibration report. No per-ear
normalization, time realignment or invented late reverberation is performed.

The generated JSON has `version`, `name`, `source`, `license`, `measurement`,
`sampleRate`, `layout`, and `speakers`. Each speaker has its name/angles,
`onsetSample`, and equal-length `directLeft`, `directRight`, `roomLeft`, and
`roomRight` sample arrays. Proprietary Realiser PRIR files and SOFA containers
are not decoded by this importer; use legitimately exported response WAVs.

## Reports and Limits

The report lists threshold arrival, interaural delay, relative direct energy and
peak amplitude. Suggestions align average ear arrival to the latest speaker and
attenuate direct-response energy to the quietest speaker. Bounds are reported;
they do not amplify quiet channels or claim calibrated SPL. The report can be
exported and suggestions previewed before Apply.

This does not measure the user's room or ears remotely, perform automatic
phase inversion/full room correction, or reproduce tactile bass. A single-pose
BRIR is not a complete multi-pose personalized head-tracking measurement. Actual
personalization needs the user's response measurements and headphone calibration.

References: [Trinnov Optimizer](https://www.trinnov.com/en/technologies/active-acoustics/optimizer/),
[Trinnov WaveForming](https://www.trinnov.com/en/technologies/active-acoustics/waveforming/),
[Smyth Realiser](https://smyth-research.com/#Technology).

## Genelec Reference and One-Click Save

Genelec's [official GLM description](https://www.genelec.com/glm) explains that
AutoCal uses a reference microphone to measure the acoustic environment and
adjust each monitor's level, distance delay, subwoofer crossover phase and room
response EQ. A layout name such as 7.1.4 does not determine these values. No
universal calibration table for this virtual room was found in the official
material reviewed on 2026-09-06. The software does not bundle or claim Genelec
GLM measurements, speaker emulation or proprietary AutoCal filters.

The speaker calibration tab offers an explicitly labeled equal-distance neutral
baseline: 0 dB trim, 0 ms added delay, and 0 dB shelves for every visible layout
speaker. Current virtual layouts place all speakers at the same distance.
This is a neutral starting point, not acoustic room correction. The button is
disabled for unequal-distance layouts. An imported compatible measurement also
exposes a one-click save for its existing alignment suggestions.

Both save actions enable cinema processing and apply/save the current draft
only after native acceptance; other draft controls and the selected room are
retained. A rejection leaves the draft and saved configuration unchanged.

### Simulated Room Calibration

The simulated-room save button derives a table for the current speaker layout
from a 6 m front/back by 4 m wide by 2.8 m high rectangular room. The listener
is centered horizontally at 1.2 m ear height. Each speaker direction intersects
the wall or ceiling; its ray length is the assumed distance. With sound speed
343 m/s, added delay is `(maximumDistance - distance) / 343` seconds and trim is
`20 log10(distance / maximumDistance)` dB. Thus nearer hypothetical speakers
are delayed and attenuated to match the farthest speaker. Values are rounded
to 0.01 ms/dB; both shelves remain zero. LFE follows the same idealized distance
assumption, without a claim of subwoofer crossover phase alignment.

This is a simulated calibration preset, not Genelec GLM data, measured room EQ,
or a room impulse-response simulation. Applying its trims to the existing BRIR
does not first insert the hypothetical room's distance attenuation and travel
times; it is an auditionable preset, not proof of correcting that BRIR. It does
not move the 3D speaker meshes or replace object directions. Existing room
responses and other draft controls are retained.
