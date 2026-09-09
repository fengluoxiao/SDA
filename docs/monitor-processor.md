# Digital Monitor Processor

SDA's Monitor panel represents the digital controller between the decoded
speaker feeds and the virtual active monitors. It is separate from Room's
measurements, acoustics and calibration. The existing layout determines output
assignment; this release does not provide arbitrary patching or upmixing.

## Hardware Reference

Reviewed 2026-09-09: [Trinnov D-MON official specifications](https://www.trinnov.com/en/products/d-mon/).
The manufacturer lists per-channel gain, polarity and delay, monitor profiles,
31-band/FIR EQ, 18-input/16-bus summing, and up to 9.3.6 systems. Its stated
native processing rate is 96 kHz up to 12 channels and 48 kHz above 12 channels.
The listed converter resolution is 24 bits/96 kHz; D/A SNR is 118 dB A-weighted,
and processing precision is 64-bit floating point.

These are reference hardware specifications, not claims about SDA. SDA retains
its 48 kHz f32 DSP, up to 16 layout outputs and a separate LFE path. It does not
implement Optimizer, analogue converters, EUCON, arbitrary hardware routing,
31-band EQ, or a measured D-MON transfer function. SNR alone cannot determine a
noise spectrum or distortion model. No synthetic converter noise, saturation,
jitter or "amplifier warmth" is added. A transparent digital model is the
appropriate starting point for feeding existing speaker and binaural models.

## Implemented Controls

- Monitor attenuation: -80 to 0 dB; DIM: -40 to 0 dB (default -20); master mute.
- Per output: -24 to +6 dB trim, 0 to 20 ms delay, polarity inversion and mute.
- Optional 40-160 Hz fourth-order Linkwitz-Riley bass management and -24 to
  +6 dB redirected-bass trim. Source LFE keeps its existing separate path.
- Explicit bypass, drafts, Apply, undo and persistent restore across tracks
  and renderer restarts. Apply replaces the graph through the existing FIFO
  reheat transition, rather than providing continuous real-time automation.

The ranges above are SDA design limits, not undocumented D-MON limits. Defaults
are neutral and bypassed. There is no claim of calibrated headphone SPL.

Channel gain, polarity and delay are composed into speaker convolution filters.
For linear time-invariant controls this is equivalent to applying them before
the room/HRTF filter. Both ears receive identical channel controls, preserving
the relative ear timing. Independent objects and the speaker bus use the same
filters. LFE has its own gain, polarity and delay. Master attenuation is applied
before the final linked guard, including original-stereo listening. Original
stereo continues to bypass virtual-speaker and bass-management processing.

The internal legacy `cinema` protocol remains for compatibility. Its nested
`monitor` settings have an independent enable flag. Desktop validation migrates
legacy bass management once and clears its old enable flag. Room comparison
retains monitor settings; Room's former panel is reachable via Room > profiles
and calibration. No extra room response is inserted by the monitor processor.

## Built-in Configurations

Reviewed 2026-09-09. The presets are compiled into the web bundle and require
no network connection or external data at installation or runtime.

- Transparent monitoring: neutral per-channel gain, delay and polarity, with
  bass redirection disabled. D-MON's published channel-control architecture
  is the reference, not a claimed hardware transfer-function measurement.
- 80 Hz bass management: Neumann's official KH 750 DSP page specifies
  "80 Hz fixed", "24 dB/oct; 4th order", and bypassable bass management:
  https://www.neumann.com/en-en/products/monitors/kh-750-dsp .
  SDA uses its existing LR4 filter; the public specifications do not establish
  identical hardware phase response. This preset is not KH 750 DSP emulation.

Both presets use SDA's existing -20 dB DIM default and 0 dB redirected-bass
trim; these are SDA choices, not values measured from either reference device.
Per-output trim and delay reset to zero and polarity to normal. These are
uncalibrated neutral values, not invented room measurements or absolute SPL.
Master level, enable, DIM on/off, master mute and existing channel mutes are
retained. Loading changes a draft only; Apply uses the existing renderer path.

Output names come from the active layout, including 2.0, 5.1, 7.1.4, 9.1.4
and 9.1.6. Bass-management presets are rejected when no LFE output exists.
The reference subwoofer is stereo hardware; applying these filter settings
to SDA's multichannel layouts is an SDA extension.

## Room-Derived Output Alignment

The Outputs tab can generate a monitor draft from the currently applied
built-in room. The catalog already contains analysis of each direct HRIR pair:
mean left/right onset time and summed left/right response energy.
For channel i, effective arrival is raw arrival plus the Room delay; effective
energy in dB is raw energy plus the Room gain. The monitor applies:

- delay[i] = max(effective arrivals) - effective arrival[i]
- trim[i] = min(effective energies) - effective energy[i]

Existing monitor trims are replaced, not accumulated. Already-aligned Room
calibration yields approximately zero residual correction. A common delay and
gain are applied to both ears; per-ear timing/level differences are preserved.
This matches aggregate direct-response energy, not measured SPL or a hardware
frequency response. Direction-dependent HRIR energy is part of that metric.

LFE has no corresponding room impulse response and is retained unchanged.
Polarity defaults to normal; mute, master level, bass management and processor
enable remain unchanged. Nonzero Room shelf EQ, missing channels, layout
mismatches and out-of-range compensation reject generation.

Apply rechecks the room/profile/layout context against the loaded draft. These
are stored snapshot settings, not a continuously tracking calibration mode:
after changing the room or its calibration, generate alignment again. The
feature does not alter room data, start playback, or silently enable processing.

## Verification

Native tests cover bypass, polarity, gain, common-ear delay, DIM and validation.
Desktop tests cover idempotent bass migration and room/monitor independence.
Existing direct-object/bus, bass crossover, callback and device-rate tests remain
part of the native regression suite. No audio is played automatically for tests.
