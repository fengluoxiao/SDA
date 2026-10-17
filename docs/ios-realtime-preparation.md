# Real-Time Preparation

## Implemented Locally

PCM rings retain 480000 samples per source and exact u64 absolute-clock tags.
Split tag/sample arrays eliminate Slot padding: 5.76 MB instead of 7.68 MB per
source on 64-bit hosts (25% lower backing storage, not total process memory).
Read caches remain source-local. Wrap, late writes, clear and repeated consumption
are tested against the existing semantics and a sparse reference.

RenderCommandQueue now protects queue and byte accounting with one mutex.
The render-side pop uses try_lock: contention defers command processing rather
than blocking the coordinator behind a preempted producer. This is not a
lock-free queue and does not guarantee a hard deadline. Producer operations and
idle waiting now uses a single-consumer park/unpark token rather than a condition
variable that reacquires the producer mutex. The producer still uses a mutex.
PCM/control FIFO ordering
and byte limits remain intact. Concurrent producer/consumer ordering is tested.

These changes form a consolidated candidate. The async-filter-preparation feature
remains off in production until Apple execution and head-pose latency are validated.
The full renderer host suite passed: 274 passed, 27 existing offline tests ignored,
0 failed. Two additional shim-backed Apple adapter tests passed (packing,
roundtrip and concurrent use; not actual Accelerate execution). Windows tests do not establish iOS
thermal/scheduler improvements.

Subsequent extended renderer run: 284 passed, 29 ignored, 0 failed (before the
latest snapshot/queue edits). Latest targeted tests passed for snapshots,
asynchronous source handoff, startup, failure retry and park/unpark queue behavior.
Mobile facade full run: 83 passed, 1 ignored, 0 failed. Its no-HRTF offline whole
song export emitted ring/can_write rejections because its feeding gate only
checks output FIFO; the passing test does not validate lossless whole-song output.
This is recorded as a test limitation, not evidence of production correctness.

## Scheduling Work Still Required

The experimental `async-filter-preparation` feature now separates continuous
object interpolation/FFT filter preparation from convolution. It is not enabled
by the iOS build script. It has one bounded preparation worker (128 queued
mailboxes), per-source latest targets, version-checked results, cancellation and
asset revision snapshots. The old filter remains active until a matching result
arrives. To avoid continuous-motion starvation, result handoff can accept a
monotonically newer completed result from the same asset revision only while its
request is at most 50 ms old. This is a candidate age limit, not an iOS guarantee.
An exact latest target can be accepted after slower preparation, so a stationary
target does not enter an endless expiry/retry cycle.
Older pending targets coalesce and requeue fairly without a render-thread kick.
First filter initialization remains synchronous so startup PCM is not
consumed through a zero filter. Queue contention uses try_lock on the render side.
Jobs are reusable Arc closures, not newly allocated for each pose. Resource
initialization is warmed at source creation when the feature is enabled; disabled
sources allocate no preparation mailbox. Obsolete result destruction is deferred
to the preparation worker. Measurement and position arrays are shared by Arc;
directional snapshots omit unused legacy prepared spectral banks. Cold snapshot creation, configuration changes and
old active-filter destruction can still occur on the render
path; this is NOT ready for a hard-real-time designation. The remaining direct,
speaker and control paths are not separated by this feature.

Mailbox correctness tests cover cancellation, latest target, multi-source progress,
burst coalescing, monotonic result handoff and stale result rejection
and worker panic recovery. Source tests compare settled asynchronous PCM with
the synchronous filter, exercise asset-revision replacement, verify startup output,
retry failed preparation and prove production audio blocks continue while the
preparation worker is deliberately blocked. Two host timing
experiments were run successfully (30 Hz trivial targets and eight moving KU100
sources), but are ignored in ordinary correctness CI. They do not prove iPhone
thermal latency or progress for large scenes/extended diffuse filters. Do not
enable the feature for users merely because these host experiments passed.

The experimental path is only continuous-object filter preparation; direct
objects, buses, control operations and source creation are not all made real-time.
No custom real-time thread policy, dedicated deadline executor or Audio Workgroup
has been installed. The iOS feature remains disabled. No request to turn this
into a production realtime subsystem is reported as fully completed here.

Do not join current ordinary QoS/Rayon threads to a workgroup and label them
real-time. The renderer still prepares filters, performs allocations and handles
control operations. Separate preparation from deadline DSP first, including
ownership-safe prepared-filter handoff and bounded work queues.

For the FIFO producer's asynchronous work cycle, Apple specifies a custom
AudioWorkIntervalCreate workgroup, not blindly joining the device I/O group.
It needs a primary thread that reports start/deadline/finish, explicitly joined
real-time auxiliary workers, and correct leave/cancel on shutdown. Session and
route recreation must not leave workers joined to an expired group. Thread policy
setup and failed join must have checked, observable fallbacks. A dedicated executor
must be validated before replacing Rayon; no such executor is implemented yet.

Accelerate complex DFT is now integrated into the convolution plan layer on Apple
targets. Immutable setups are cached by size/direction; execution uses caller-owned
workspace and does not allocate. Unsupported lengths/setup failures fall back to
RustFFT. The transform remains unnormalized like RustFFT; existing convolution
scaling and packed stereo inverse are unchanged. Apple-only reference/concurrency
tests and all convolution tests are wired into CI. A Windows shim checked adapter
packing and trait/FFI types but is explicitly not an Apple numerical or performance
test. Apple execution has subsequently passed the host validation below. This is not a substitute for
scheduling correctness; custom Audio Workgroups are still not implemented.

## Apple Host Validation

Apple DSP check run 38369334094 passed on ARM64 macOS. Accelerate reference,
roundtrip/concurrency tests, eight production convolution tests, PCM and queue
tests and async handoff tests passed. Release 2048-point FFT medians: RustFFT
forward/inverse 3750 ns; Accelerate forward 2584 ns, inverse 2542 ns. These are
Apple host microbenchmarks, not iPhone thermal performance guarantees.

The production candidate uses Accelerate, packed PCM storage and nonblocking
queue handling. Async preparation remains opt-in, not enabled by the iOS build.
Diagnostics includes the renderer Git revision and backend description. This
candidate does not include dedicated realtime DSP threads or Audio Workgroups
and is not a completed realtime architecture rewrite.

References:

- https://developer.apple.com/documentation/audiotoolbox/understanding-audio-workgroups
- https://developer.apple.com/documentation/audiotoolbox/adding-asynchronous-real-time-threads-to-audio-workgroups
- https://developer.apple.com/documentation/audiotoolbox/analyzing-audio-performance-with-instruments
- https://developer.apple.com/documentation/accelerate/vdsp
