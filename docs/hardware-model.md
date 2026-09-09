# Electrical signal-chain model

The optional hardware model is independent of room processing and monitor enable.
Defaults are generic engineering assumptions, not measured device specifications.
It is not a commercial amplifier or sound-card replica.

Spatial playback sums sources into virtual speaker buses, then applies the
electrical model before HRTF and room convolution. Direct object HRTF is temporarily
superseded while hardware is enabled; the user's preference is retained. Stereo
original mode processes its left/right outputs. The LFE/bass sum has its own chain.
All chains use the same settings and eight samples of FIR delay (0.167 ms at 48 kHz),
plus the amplifier pole's frequency-dependent phase. Disabled processing is exact bypass.

The model includes signed PCM quantization, full-scale DAC clipping, 4x FIR
interpolation, one-pole amplifier bandwidth, voltage/current clipping into a
resistive load, output-impedance division, and FIR decimation. A 0 dBFS sine maps
to the configured line RMS voltage before amplifier gain. Output is normalized by
the nominal line-voltage/amplifier-gain product: increasing gain reduces electrical
headroom, rather than adding automatic headphone loudness. Input drive is not
compensated. Monitor trims, calibration and listening attenuation remain downstream
digital controls, not amplifier gain or physical SPL calibration.

The 33-tap Blackman FIRs limit high-frequency bandwidth and reduce, but do not
eliminate, nonlinear aliasing. This is an engineering approximation, not a
transparent DAC reconstruction filter. Noise, jitter, slew limiting, reactive
speaker impedance, power supply sag, thermal protection, crosstalk, and measured
device transfer functions are not modeled. Hardware defaults remain off.

Room comparisons retain current monitor/hardware changes when switching or
restoring room modes. Only generating room-derived alignment is unavailable
during temporary comparison, because its inputs depend on the selected room.

## Parameter provenance correction (2026-09-09)

Input gain defaults to 0 dB in the UI, desktop validation and native engine.
It is a relative gain, not an absolute dBFS level. Existing explicit user values
are preserved; the panel offers a reset to 0 dB. Changing this gain does not
establish a hardware calibration or eliminate amplifier clipping.

D-MON is only a functional reference for monitor controls. Its official page
specifies 24-bit DAC conversion and 118 dB A-weighted D/A SNR, but does not
justify the generic amplifier voltage, current, impedance or bandwidth values.
The panel labels those values as custom approximations. A specification-based AHB2 preset is now available; it is not a measured replica.
The AHB2 official page was successfully retrieved after the earlier TLS failures.
ADI-2 DAC specifications remain unverified and are not used.


## Benchmark AHB2 specification presets (2026-09-09)

Source: https://benchmarkmedia.com/products/benchmark-ahb2-power-amplifier

| Mode | Input sensitivity / matching source RMS | Voltage gain |
| --- | --- | --- |
| High | 2 V | 23 dB |
| Mid | 4 V | 17 dB |
| Low | 9.8 V | 9.2 dB |

All presets use stereo operation into 8 ohms, rated at 100 W per channel
(both driven), and the published 29 A peak current limit. The 1 kHz damping
factor of 254 gives a constant approximation of 8/254 ohms output impedance.
The rated load voltage is sqrt(2*100*8)=40 V peak. The model's internal limit
is 40*(1+1/254) V to account for its output divider. This is a rating-derived
clipping approximation, NOT the supply rail voltage or a measured clip onset.
Other loads and bridged operation are not covered by these presets.

The published response is better than 0.1 Hz–200 kHz (+0/-3 dB). The 200 kHz
entry drives the existing simplified one-pole approximation at 192 kHz internal
rate; it does NOT recreate the manufacturer's transfer function or reproduce
ultrasonic bandwidth in SDA's 48 kHz output. Frequency-dependent impedance,
THX feed-forward correction, noise, protection and power-supply behavior remain
unmodeled. These limitations are exposed in the UI's source disclosure.

AHB2 is an analogue amplifier. The source's DAC precision is retained separately;
24 bits is an SDA default, not an AHB2 spec. Matching voltage is a source setup,
not a claim about the user's physical sound card. Input gain resets to 0 dB;
loading a preset preserves hardware enable, monitor attenuation, mute and DIM.
Edits that depart from the preset are shown as custom. The existing explicit
Apply action commits settings; opening the panel does not change playback.

Validation covers desktop persistence constraints, 100 W terminal-power mapping,
all three gain/sensitivity pairs and offline normalized 1 kHz output within
0.15 dB of input. Tests do not open an audio device.

Input gain has 0 (default), -3, -6, -12, +3 and +6 dB shortcuts plus numeric
entry (-60 to +12 dB). It is independent of AHB2 identity and gain mode.
Changing it does not mark the amplifier specifications as custom.
