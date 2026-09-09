# Built-in Reference Rooms

The installer includes five precomputed simulated profiles: 7.1.4, 2.0, 5.1,
9.1.4 and 9.1.6. Users select Apply built-in room without installing Python,
downloading measurement data or running a generator. Custom generation remains
available when its optional scientific runtime is configured.

## Generation Inputs

All five use the same room so a layout comparison does not also change room
acoustics. These are explicit SDA design assumptions, not parameters measured
in a named studio or certified by Dolby.

| Input | Value |
| --- | --- |
| Length / width / height | 6 / 5 / 3.2 m |
| Listener | 60% of length from rear, centered left/right, ear height 1.2 m |
| Monitor distance | All non-LFE speakers 1.2 m from listener, preserving layout angles |
| Reflection order | 10 |
| Front / ceiling | Fabric-covered 6 pcf rockwool panel, 70% coverage |
| Side / rear walls | 50 mm rockwool, 80 kg/m³, 85% coverage |
| Floor | Carpet 1.35 kg/m² on felt/foam, 100% coverage |
| Uncovered wall area | Double plasterboard with cavity mineral wool |
| Speed of sound | 343 m/s |
| Sample rate | 48 kHz |

The generator preserves SDA layout azimuth/elevation and records every speaker
position and direct/first-reflection path. It computes zero-order and full-order
responses separately. No measured room BRIR is simply renamed as a new layout.
Finite-order image sources omit diffraction, low-frequency wave modes and an
infinite diffuse tail; material coefficients are literature values; placement and coverage remain assumptions rather than measured walls.

## Data Selection

Checked 2026-09-09 against the [University of York publisher page](https://www.york.ac.uk/sadie-project/database.html).
SADIE II provides D1 KU100 directional HRIR measurements in original 48 kHz,
24-bit WAV and explicitly licenses its database under Apache-2.0. The publisher
states its HRIRs were measured with Genelec 8010 speakers at 1.2 m. The separate
50-point BRIR measurements use Genelec 8030/40 at 1.5 m; those room responses are
not used here because the requested generation inputs define a different room.

The existing verified D1 archive is reused after SHA-256 validation. Measured
receiver responses are used for each ray direction, including duplicate-pole
removal and the library's nearest-direction selection. The publisher has diffuse-field equalized, time-aligned and windowed these
HRIRs (SADIE II paper section 3.2.3); SDA preserves the published amplitudes and
filter latency, not an asserted uncorrected measurement-system response. An ideal omnidirectional emitter adds no
second speaker response. This is not a claim to emulate a flagship loudspeaker.

The previously downloaded DIRPAT Genelec 8020 file has conflicting embedded and
publisher license statements, so it is excluded from these redistributed assets.
The [Dolby studio directory](https://professional.dolby.com/music/dolby-atmos-music-studios/)
does not provide the full geometry/material/IR data needed to reconstruct those
studios. No public source establishes a universally "best" dataset. SADIE is
chosen for traceable measured receiver data, compatibility and clear licensing.

## Packaging And Integrity

`apps/desktop/builtin-rooms/catalog.json` holds summaries, generation inputs,
raw SHA-256 identities and compressed-file checksums. Gzip responses and license
notices are included by electron-builder, including in app.asar. Native code
cannot open asar paths, so the desktop process verifies/decompresses the selected
profile into its user-data cache. Corrupt caches are rebuilt; corrupt packaged
archives are rejected. Built-ins cannot be deleted through the profile API.
User-created profiles stay in their separate writable library.

Reproduce from the configured scientific runtime:

```powershell
node scripts/build-builtin-rooms.mjs
node apps/desktop/test/builtin-rooms.test.mjs
```

Ordinary packaging consumes the checked-in generated assets; it does not run
the simulator or require the research archive. The old local generated profiles
were explicitly removed at the user's request, not by an automatic migration
that deletes other users' data.

Historical revision 3 verification on 2026-09-09: all five generated profiles passed validation and
cache-corruption recovery tests. An electron-builder Windows unpacked build was
launched with isolated user data and no configured Python runtime. Each bundled
profile was extracted from app.asar and accepted by the packaged native renderer;
built-in deletion was rejected and the audio clock remained at zero. Desktop
1280 px and mobile 390 px UI checks covered persistent Apply, matching layout,
locked built-in parameters, custom mode and nonblank animated sound paths.

## Revision 4: physical reference and material provenance

Generated with a relative digital source convention: unity ideal source and
published directional HRIR at a numerical propagation reference of 1 metre;
pressure scales as 1/r and time as r/343. The original 1.2 m acquisition radius
is provenance, not a second attenuation/delay or a makeup factor. This is not
an absolute Pa/volt or SPL calibration. No automatic gain is added.

Materials are pinned in `scripts/room-materials.json`, including the upstream
file SHA-256 and URL. pyroomacoustics attributes its table to the annex of
Vorländer, *Auralization*, Springer 1st edition (2008). Available choices are
50 mm / 80 kg/m³ rockwool, double 13 mm plasterboard on a steel frame with
50 mm cavity mineral wool, and average hard surface. Values are energy
absorption coefficients, not pressure multipliers; the simulator handles the
reflection conversion. Whole-surface coverage remains an explicit design
assumption, not a measured studio. Full-frequency coefficient tables and the
reference conditions are exposed in the UI and retained through import.

Old profiles remain readable; their coefficients are not silently rewritten.
Regenerating old `treated/living/reflective` inputs maps to the three explicit
material IDs and records revision 4. The new bundled catalog exposes all five
regenerated layouts; prior cached responses are not deleted.

Physics regression checks cover the half-amplitude / propagation-time change
when distance doubles, unchanged direct arrival with reflections enabled,
material-dependent reflection energy, preserved original HRIR scaling, and
geometry against image-source path lengths. A named real-room match remains
unclaimed: the SADIE BRIR uses a different loudspeaker array and room, so its
reverberation cannot serve as a numerical truth target for this design room.

The pyroomacoustics 0.10.1 default 10 Hz whole-RIR zero-phase highpass is explicitly
disabled. Its reverse pass lets the reflection tail influence direct samples. The library
documents ISM positive-DC artifacts, so the generator retains correction using
a causal second-order 10 Hz Butterworth, identically for direct and full RIRs.
8192 trailing zero samples capture its decay. This is a documented numerical
correction, not loudness normalization or an extra wall absorption coefficient.
The independent impulse tests failed with the default filter and pass with it
disabled: distance doubling gives 0.5 amplitude and adding reflections preserves
the entire direct arrival prefix. No makeup gain was used to obtain this result.

## Revision 5: near-field studio control room

The current catalog uses the geometry and per-surface treatment listed above.
Coverage is an SDA design choice. Energy absorption coefficients are mixed by
area, then passed to the simulator separately for the six surfaces. This is an
effective uniform surface model, not a spatial arrangement of individual panels
or a diffuser simulation. The floor table only provides bands through 4 kHz;
8 kHz explicitly holds the 4 kHz value. The aggregate material coefficients in
metadata are descriptive only; the solver uses the six individual arrays.

[EBU Tech 3276](https://tech.ebu.ch/docs/tech/tech3276.pdf) supplies the early
reflection screening reference (first 15 ms, at least 10 dB below direct in
1–8 kHz) and a nominal reverberation reference of 0.25*(V/100)^(1/3) seconds.
[Genelec monitor placement](https://www.genelec.com/monitor-placement) explains
symmetry and boundary reflection cancellations. Neither source mandates this
1.2 m distance or these coverage percentages. This intentionally dry near-field
design is not an EBU-compliant reference room: the stereo base is below its
2–4 m range and some estimated reverberation bands are below its recommendation.

Each profile records Eyring diffuse-field estimates and first-order geometric
early-reflection screening. These are not measured T60 values or proof that the
complete binaural response meets EBU limits: they omit higher-order overlap,
desk reflections, source directivity and low-frequency modes. Bundled emitters
remain ideal omnidirectional sources, not an emulation of a named studio monitor.
The custom editor exposes listening distance and hides the unused radial ratio
for studio mode. Old user profiles remain unchanged and readable.

Revision 5 verification (2026-09-09): seven physics regressions passed, followed
by the updated studio geometry/material checks after treatment refinement.
All five final archives passed content/hash validation, cache recovery and
packaging-inclusion checks. TypeScript, production web build, room validation
and IPC tests passed. The visible development Electron window was restarted;
the final 7.1.4 preset was accepted by native, persisted and displayed after a
reload without changing the latest cinema/monitor/hardware settings. No media
was played and no exclusive-device probe was run. The new revision was not
repackaged into a fresh installer during this check.
