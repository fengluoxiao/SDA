# iOS background playback / lock-screen investigation

The user reports audio cutting out while the device is locked. Exact timing,
route and device-level cause still require a physical-device reproduction.
Do not claim that a simulator background check proves physical locking.

## Existing behavior checked

- `UIBackgroundModes` already contains `audio`.
- Both production routes activate `AVAudioSession` with category `playback`.
- The React AppState handler does not pause on background entry.
- Sustained decoding/output is native, not driven by React polling.
- Playlist advancement is still triggered by the React poller. The separate
  issue of no next song while JS is suspended is NOT fixed by these changes.

## Hardening

- A finite UIKit background task covers preparation before the first consumed
  audio frame. End it on first consumption, pause, stop, error or expiration.
  There is no silent keepalive, endless assertion or fake background mode.
  An expired preparation is cancelled with an explicit error.
- Imported *owned* copies that inherited complete/while-open file protection are
  changed to complete-until-first-authentication before opening. External Files
  provider originals and unrelated files are never modified. Protection is not
  disabled globally; import-copy behavior remains enabled.
- Recover an unexpectedly stopped AVAudioEngine on lifecycle/route/configuration
  events and in the native feed loop. Never override a deliberate pause, finished
  track or real audio-session interruption. Reentrancy is guarded.
- Keep bounded local diagnostics for lifecycle, protected-data transitions,
  interruptions, route changes, engine restart, preparation expiry and feed errors.
  Expose the trace via `SdaEngine.playbackDiagnostics`; retain a JSON copy in app
  caches. No periodic PCM trace or automatic upload.
- Open BOTH playback and analysis inputs before `playUri` returns: JS releases
  extracted MHAS files immediately afterward. Reopening the pathname later would
  fail after unlinking; the two open descriptors remain valid. This also fixes a
  flaw found while reviewing the new complete-track normalization scan.

## CI regression probe

Launch a Release app, play real MPEG-H fixtures through KU100 or system 7.1.4,
then genuinely put SDA in the background by launching Settings. Require the
native consumed clock to advance by at least eight seconds while SDA has received
its actual background notification, with the preparation assertion already ended.
Also require protection-policy assertions and playback/analysis after unlink.
CoreSimulator may not implement file protection despite accepting the setter;
reports explicitly mark actual conversion as unverified in that case. A supported
protection attribute must still show the expected conversion.
The report always states `physicalLockVerified: false`.

Pending physical-device checks: a mid-song device lock, a locked uncached
normalization start, AirPods/Bluetooth route changes, interruptions and a whole
track/next-track transition. If the actual device still fails, retrieve its local
playback diagnostics to distinguish suspension, protected reads, session events
and output failure; do not hide a failure by unconditionally restarting audio.

## Mid-track process termination follow-up

The user clarified that playback stops mid-song and the background process
appears to have been killed, not merely failing to advance the playlist.
Physical-device Jetsam/termination logs are still required to identify the cause.

- Drain an autorelease pool on every native feeder iteration and every full-track
  analysis chunk; a long-lived queue task must not retain Cocoa temporaries until EOF.
- Advance the object snapshot from the native feeder once per second independent
  of React polling. Consumed metadata is coalesced; future events are preserved.
- Pause the iOS 3D animation loop while inactive/background or offscreen; retain
  the scene and camera for foreground return. Android rendering stays unchanged.
- Persist memory-warning notifications in the bounded playback trace. These
  warnings alone do not prove Jetsam, and cannot capture a crash after termination.
