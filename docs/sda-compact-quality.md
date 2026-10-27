# SDA compact reconstruction quality

SBC2 retains its existing packet/decoder contract. The corrected encoder closes quantization around exactly the reconstructed references available to the decoder. Cancelling a cross-channel predictor using original reference samples is invalid after those references have been quantized; recursive cross filters can otherwise turn small reference errors into near-full-scale pulses in quiet stems. Existing affected deliveries require re-export from their original source.

Predictors for compact encoding are selected using actual closed-loop coding cost, not only an unquantized residual estimate. The search retains an independent-channel fallback. Lossless SBL3 prediction and exact PCM reconstruction remain unchanged.

Every source lane/block is checked against an RMS reconstruction-error budget: error energy must be no greater than source energy times1e-6 (60 dB SNR), with a two-PCM24-unit absolute RMS allowance for extremely quiet signals. A separate quantizer-step cap bounds isolated errors to at most four times that relative RMS allowance (or the absolute floor). Quiet channels therefore cannot borrow a loud channel's quantization step.

The configured bitrate remains a target. If the protected reconstruction cannot fit the nominal packet budget, compact mode uses quality-constrained variable bitrate and reports the actual audio bitrate in the existing transport manifest. `compactQuality` records the policy and bounds. It must not silently trade away this protection to force many independent object stems into an insufficient target bitrate.

Regression coverage includes a strong recursive cross-reference model, quiet independent lanes, per-block/per-channel decoded waveform error checks in delivery tests, lossless exact identity, and comparisons with the original multichannel PCM. Metadata/decode success alone is not audio-quality validation.
