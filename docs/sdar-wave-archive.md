# SDAR preserved WAV archive

Authored PCM, sample-clock object automation and render resources remain separate from archival ADM-BWF bytes. Playback reads `sdrm` and bounded ID3 assets. It does not inflate the original ADM XML. No object, trajectory or room resource is discarded to reduce the archive.

New intermediate BCH2 files advertise `waveEnvelope: "chunked-zlib-v1"` in HEAD, followed by sealed RNDM (render JSON), ID3-space (original ID3 or empty), a chunked archive and the existing AUD2/index records. The archive consists of WVHC (original pre-PCM header), WVTC (post-PCM tail) records, then an empty WVEN terminator. Header chunks precede tail chunks. Every input block is at most 1 MiB and independently zlib-compressed and sealed with the existing escaping/CRC format. Each record uses the existing four-character ID, 16 hex length digits, newline and body framing. Chunk lengths are checked against file boundaries before seeking or allocation. Restoration inflates and writes one block at a time; its memory does not grow with raw ADM size.

Final lossless SDAR ISO files advertise `masterEnvelope: {format: "chunked-zlib-v1", box: "sdwe", chunkBytes: 1048576}` in sdaf. An extended-size sdwe box preserves the archive records directly. sdrm and sdiz carry the render JSON and compressed original ID3 separately for playback. M4A delivery retains the same playback metadata and one audio track, without the unused WAV archival envelope. Existing WAVH/WVTZ and sdwh/sdwt exports remain readable.

The per-document and asset decoding limits still apply to playback metadata. They do not impose a 64/256 MiB aggregate limit on new raw ADM archives. A larger original header or tail produces additional records. Filesystem capacity, integer/file-boundary checks and supported RIFF/RF64/PCM topology remain applicable.

`apps/desktop/test/sdar-wave-archive.test.cjs` covers small ADM, 66 MiB and 270 MiB aggregate ADM, header/tail placement, bounded records, SDAR/M4A parsing, byte-exact WAV restoration and corrupt archive rejection. The separate object export test covers independent object lanes and automation/seek fidelity.

ADM import also streams original chunk checksums and records `storage: "master-envelope"` provenance. It preserves those chunks in WAV and lossless SDAR instead of duplicating raw XML in ID3 assets; M4A carries the full normalized playback scene and provenance checksums, without the editable original ADM archive. No raw-ADM-size gate is used by import.
