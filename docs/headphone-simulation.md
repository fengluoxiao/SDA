# Headphone Timbre Simulation

The UI selects a playback headphone and a target headphone. The desktop renderer
now synthesizes a new transfer function, rather than playing the target's
correction FIR directly. For published correction magnitudes C, the simulation is
C_playback / C_target, with 1 kHz normalization. Same-model conversion is exact
identity. Correction-profile preamps are excluded; the derived FIR receives its
own shared stereo level normalization against 20 Hz–20 kHz pink-noise energy.
Normalization is bounded to -12…+6 dB; the native final linked look-ahead peak
guard protects programme peaks. Reference-energy matching is not song-specific
LUFS or a guarantee of equal perceived loudness. It replaces maximum-band-boost
attenuation, which made whole songs unnecessarily quiet.

This is a target-relative timbre approximation, not an acoustic headphone clone.
The bundled AutoEq results use different measurement rigs and reference targets,
so their differences are not a common-rig raw frequency-response ratio. In
particular, in-ear/over-ear comparisons retain this limitation. Fit, ANC, adaptive
processing, distortion, leakage and spatial presentation are not reproduced.

The default playback option is Universal Headphones (uncalibrated). It assumes a
neutral playback response and leaves the actual headphone's colouration in the
result; no playback-model adaptation is required. A measured playback profile is
optional. The previous AirPods Pro 3 reference option migrates to this identical
generic reference without changing its transfer function. Explicit model
calibration selections are retained. No AirPods Pro 3 measurement is claimed.

DSP uses fft.js (MIT), log-frequency smoothing, +/-12 dB limits, conservative
low/high-frequency taper, and real-cepstrum minimum-phase reconstruction at 48 kHz
(16384-point FFT, 8192-tap FIR). Left/right share output headroom attenuation.
Filter synthesis occurs on selection, not on the native audio callback. The FIR
replaces the final stereo headphone FIR; it is not added as a second stage.

Room/monitor/hardware exclusion remains in force. The new simulation selection
uses a separate persisted key so an old correction selection is not silently
reinterpreted as a simulation after upgrade. Imported inputs must still be
correction profiles; they are not arbitrary raw frequency-response files.
