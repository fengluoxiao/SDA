# Genelec Measurement and Room Simulation Research

Verified on 2026-09-06. This research found real loudspeaker impulse responses.
The subsequent implementation is documented in [Room Lab](room-lab.md); it is
not a Genelec GLM calibration.

## Primary Dataset: DIRPAT Genelec 8020

- Publisher: KUG, Institute for Electronic Music and Acoustics (IEM), Graz.
- Authors credited by the database/library: Manuel Brandner, Matthias Frank,
  Daniel Rudrich; AES 144th Convention, 2018, DirPat e-Brief 35.
- [Original repository record](https://phaidra.kug.ac.at/detail/o:68229).
- [Publisher metadata](https://phaidra.kug.ac.at/api/object/o:68229/metadata).
- [Original SOFA download](https://phaidra.kug.ac.at/api/object/o:68229/comp/COMP000003).
- Original filename: `LSPs_HATS_GuitarCabinets_Akustikmessplatz.sofa`.
- Local research download: `D:/SDA/tmp/dirpat-loudspeakers.sofa`.
- Size: 91,234,461 bytes.
- SHA-256: `D0891FE5413D28C4EA94F422AB9683EF0501B6206C1BBA75DACC1EE6F723A7AC`.

The publisher explicitly lists Genelec 8020 and says all listed measurement
objects include impulse response data. HDF5 inspection independently confirms:

| Field | Value |
| --- | --- |
| Convention | SOFA GeneralFIR 1.0 |
| Data.IR dimensions | 12 loudspeakers/sources x 540 directions x 2048 taps |
| Genelec selection | Index 0; file Comment and library index agree |
| Sampling rate | 44,100 Hz |
| IR duration | Approximately 46.44 ms |
| Direction grid | 36 azimuths x 15 polar angles |
| Azimuth coverage | 0 to 350 degrees, 10-degree spacing |
| Polar coverage | 11.25 to 168.75 degrees, 11.25-degree spacing |
| Stored radius | 1 metre |
| Genelec data validity | All finite; 540/540 directions nonzero |
| Absolute peak | Approximately 0.69142049 |
| Data.Delay | Zero throughout |
| Recorded RoomType | IEM Akustikmessplatz PG008 - (5m/4m/2.7m) |

This is the 8020, not a measured 8341/8351 flagship model and not a measured
7.1.4 listening room. The file does not itself establish a perfectly anechoic
measurement. Its finite measurement window and measurement setup need to be
considered before adding synthetic reflections or interpreting low frequencies.

### Rights Metadata Discrepancy

The current publisher page's DC.rights and license link say
[Public Domain Mark 1.0](https://creativecommons.org/publicdomain/mark/1.0/).
The pyroomacoustics dataset index labels the file CC0. The downloaded file's
embedded License field instead says `No license provided, ask the author for
permission`. These are distinct statements; do not relabel them as identical.
Preserve publisher provenance and resolve the discrepancy before asserting a
clean redistribution license for a bundled derivative. No contact was sent to
the authors and no measured audio was added to release assets in this task.

## Compatible Simulation Engine

[pyroomacoustics measured directivities](https://pyroomacoustics.readthedocs.io/en/pypi-release/pyroomacoustics.directivities.html)
supports measured source responses and measured receiver/HRTF responses in room
impulse-response simulation. Its dataset registry explicitly supports
`Genelec_8020` in this file:

- [Dataset registry](https://github.com/LCAV/pyroomacoustics/blob/master/pyroomacoustics/data/sofa_files.json).
- [Dataset documentation and provenance](https://github.com/LCAV/pyroomacoustics/blob/master/pyroomacoustics/datasets/sofa.py).
- [SOFA/DIRPAT reader](https://github.com/LCAV/pyroomacoustics/blob/master/pyroomacoustics/directivities/sofa.py).

Use the library's DIRPAT-specific loader, not generic SOFA coordinate parsing.
The file labels angular units as degrees but contains radians; the library also
repairs a known measurement-position flattening mismatch. Its detection uses
the original file stem, so the local research filename must be mapped back to
the original name before using that automatic path. Preserve relative response
levels between directions, and resample the impulse responses to SDA's 48 kHz
with an actual resampler rather than changing only the sampling-rate field.

## Integration Path

1. Use the Genelec source impulse response for each ray's emission direction,
   with every virtual speaker oriented toward the listening position.
2. Generate a room response using actual propagation delays, distance loss and
   frequency-dependent wall absorption. Use image sources or another supported
   physical simulation method rather than inserting calibration-table values.
3. Apply the selected HRTF to each ray's arrival direction at the listener.
   A mono room reverb applied after stereo HRTF does not preserve reflection
   directions and is not equivalent.
4. Export direct-only and full-room binaural responses for every speaker in the
   selected layout, then use SDA's existing independent-object convolution
   route. Keep objects' decoded audio and motion intact.
5. Derive calibration from the resulting responses and validate timing,
   directional behavior, reflection tails and output headroom. Match comparison
   levels for listening tests; do not use loudness alone to claim improvement.

The room dimensions, wall materials and missing polar-cap interpolation remain
simulation assumptions. A simulated room based on a measured Genelec source is
not a measured personal room, and must not be tagged as such. SDA's current
room-import schema accepts only personal/dummy-head measurements, so generated
profiles need an explicit simulated provenance category before product import.
The current 32,768-tap import ceiling is about 683 ms at 48 kHz; longer simulated
tails require a deliberate supported window or an engine/profile-limit change.

The earlier distance-only preset does not implement this chain and is not a
substitute for it. This research did not change playback or enable that preset.
