# Stereo to 7.1.4 WAV export

Desktop export runs an independent decoder worker. Supported sources are ordinary
ALAC, MP3 and PCM WAV stereo. Existing multichannel or object programmes are
rejected. The export starts from the beginning and includes the complete song;
playback position, pause, file switches and listening volume do not change it.

The shared mobile/desktop upmix matrix and its fixed -2 dB calibration produce
12 channels at 48 kHz. Files use 24-bit PCM WAVEFORMATEXTENSIBLE with speaker mask
0x2d63f. On disk the order is FL, FR, FC, LFE, BL, BR, SL, SR, TFL, TFR, TBL, TBR;
back channels are reordered from SDA's internal side-before-back bed. This is a
channel bed. Export snapshots the current playback volume-balance and spatial-balance switches
at the start of each job. One export action writes exactly one final WAV with
both enabled effects baked into its audio. The export panel only reports these
settings; it does not create alternate versions or separate channel files. HRTF, virtual room, headphone EQ
and device volume remain outside the export. No proprietary/object-based master format is generated.

A bounded floating-point disk spool permits offline whole-track reference
measurement. Volume balance measures the original stereo master at 48 kHz,
selects one nonpositive -18 LUFS programme gain with the source true-peak bound,
and includes the same +2.5 dB calibration when enabled. This complete-track
reference is deterministic and does not depend on the playback cursor/cache.

Spatial balance calls the native renderer in a device-free offline mode. Its
source-domain controller, world-direction membership, spectral redistribution
and common clarity shelf are the same Rust kernels as playback, applied to the
12 bed channels before HRTF. Protected fronts and LFE retain their source-domain
baseline. No separate JavaScript imitation or binaural mix is used.

Both balances are rendered to a second bounded spool. A fresh true-peak scan
after both operations determines final headroom before quantization. One linked nonpositive gain leaves -1 dBTP headroom with a small
rounding margin. PCM24 uses TPDF dither and is measured again before publication.
There is no section-dependent gain envelope or independent channel normalization.
RIFF is used for ordinary files; RF64/ds64 is selected automatically above 4 GiB.

Save As confirms destination replacement. The source and its hard-link aliases
cannot be overwritten. A private staging file is flushed before atomic rename;
errors or cancellation preserve an existing destination and remove temporary
files. The application cancels and waits for the export worker during shutdown.

Build the offline worker with `node scripts/build-upmix-export.mjs`. Desktop dev,
build, pack and beforePack invoke this build; the Node codec artifacts are staged
beside the existing performance decoder. Tests cover ALAC/MP3/WAV decode, SRC and
gapless clocks, canonical channel placement, PCM24 readback, hot input protection,
RF64 sizes, cancellation, destination preservation, source aliases and IPC ownership.

# Desktop listening level correction

The monitor master switch now gates the saved hardware simulation submodule in
render settings. A disabled monitor previously allowed a saved +3 dB input drive
and hardware routing to remain active, also bypassing the normal output-dynamics
and stereo bass paths. The stored hardware parameters remain available when the
monitor is explicitly enabled. Default mobile monitor settings follow the same
bypass path; hardware simulation remains an explicit desktop listening effect.

Export protocol v1 requires a native readiness handshake; an older sidecar is
rejected instead of silently exporting unprocessed audio or opening another
playback session. Cancellation closes the helper control pipe and removes all
intermediate files. The settings snapshot is immutable for a running export.
