# Android local player UI

The native UI follows `apps/desktop/remote-web/index.html` and `app.css`: green dark/light palettes, rounded cards, globe placeholder, circular controls, player/list/scene pages, and a top-right sound settings panel. `RemotePlayer.tsx` adapts those controls to the local Android engine.

The list currently contains the selected local file. Desktop connection, pairing, room authoring, and remote host controls are not exposed as nonfunctional buttons. KU100 rendering switches and output telemetry use the native engine. Volume is reapplied when a new engine is started. The space page uses the shared ADM coordinates and a native 3D scene.

Playback progress uses the consumed audio clock divided by the selected E-AC-3 track's container duration. It is a read-only progress bar, matching the remote page's HTML progress element. Missing duration is displayed as `--:--`, never estimated from the decode-ahead buffer. Android MediaExtractor resources are released after metadata reads.

Validation: TypeScript check and Android release build; MuMu local M4A playback, 61-direction KU100 with 15 object convolvers, and live actual-direction/basic-object switching. Decoder regression tests and compressed-access-unit integrity instrumentation are documented separately in android-m4a-validation.md.

Metadata: Android MediaMetadataRetriever reads embedded title, artist/album artist, album, year, track number and cover art. Artwork is decoded with subsampling to at most 1024 pixels and cached by its content hash. Missing tags fall back to the filename and globe. Duration still comes from the selected E-AC-3 track. The supplied fixture's MP4 tags independently contain title `doll`, artist `陈康堤`, album `doll - Single`, and a 2,782,489-byte embedded cover.
