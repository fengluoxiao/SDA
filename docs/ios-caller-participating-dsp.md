# Caller-Participating DSP

iOS object excitation, directional convolution, legacy object processing and
speaker bus convolution now use caller-participating dispatch. The render
coordinator processes one disjoint half; one auxiliary task processes the other.
Borrowed tasks complete before the scope returns, including error/panic cases.
Windows default dispatch is unchanged. The caller-participating-dsp feature lets
host tests execute this path without pretending to run an iOS scheduler.

This still uses the existing dedicated Rayon pool and an end-of-stage join. It
does not implement Audio Workgroups, hard-real-time policy, or eliminate all
blocking. It avoids handing the whole stage away while the coordinator waits.
Two bus vectors are processed separately; this is not one fused whole-block job.

Local tests passed for caller participation, delayed auxiliary execution, scoped
error completion, five legacy object output tests, hardware mixing parity,
spatial-layer parity, moving cue parity, bus diffusion, and eight convolution
tests. No audio buffer, HRTF response, filter length or sound setting changed.
Actual iOS animation/thermal dropout improvement remains unverified.

Run the fast Apple DSP workflow before building a new app. Its result cannot
replace physical-device scheduling verification.
