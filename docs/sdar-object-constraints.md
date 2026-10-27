# Authored object spatial constraints

`object-spatial-constraints-v1` is a required SDA interchange feature whenever an authored object timeline contains `zoneExclusion` or a true `horizontalOnly`. Readers must reject unknown required features rather than dropping render parameters. Files without these constraints retain their existing feature contract.

The normalized event retains the full exclusion array and horizontal-layer flag alongside position, gain, extent and diffusion. Cartesian zones use normalized three-axis min/max values in [-1,1]. Polar zones use azimuth/elevation min/max values; rear-seam azimuth wrapping is valid, elevation remains ordered. Invalid types, dimensions, non-finite or out-of-domain bounds are rejected before playback.

SDAR/M4A preserve this metadata independent of audio compression. WAV retains both its original ADM and normalized SDA metadata. Seek recovery carries the target constraint state through interpolated position/gain recovery. The mobile sda-core event contract preserves these fields through serde and FrameRouter; changes to constraints alone must never be coalesced as redundant. Empty/false defaults keep existing decoder JSON behavior.

The existing renderer implements the speaker exclusions and horizontal-layer routing; this change enables delivery and transport, without adding a second DSP approximation. Tests cover schema validation, missing required features, seek preservation, native conversion and constraint-only updates, and full WAV/SDAR/M4A export roundtrips with both Cartesian and wrapped polar zones.
