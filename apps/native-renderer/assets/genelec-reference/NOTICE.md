# Genelec 8351B reference-room provenance

These fixed coefficients are integrated into SDA Spatial, together with the
retained short residual and the current direct HRTF. They combine numerical normalized directivity
from Genelec's publicly provided 8351B EASE GLL with an explicitly assumed
6 x 5 x 3.2 metre room and the existing shared HRTFHQ receiving responses.
It is not a measured personal room, a Genelec GLM profile, an Aural ID profile,
or a complete absolute 8351B/subwoofer transfer measurement.

Source: https://www.genelec.com/simulation-files
Original GLL: https://assets.ctfassets.net/4zjnzn055a4v/1WcRAwuQu6hUCHa9UT990V/8e16d8615c0befd4d3ceded84f178aae/Genelec_8351B.gll
GLL SHA256: 41f01aa835cdcf03c57b81ad5eccbbfcd60eb37a21ce7ac32b770ffec67c743d
Manufacturer metadata: Genelec, copyright 2023; all tone controls off.
The original GLL binary, pictures and manufacturer logos are not bundled here.

The normalized magnitudes were exported offline and reconstructed on the
meridian-major grid with shared poles. They are retained in
8351b-directivity.json.gz. The nominal source bandwidth is 50–16000 Hz.
Minimum-phase reconstruction, out-of-band endpoint continuation, room size,
wall absorption and two-order image propagation are simulation assumptions.
The existing selected direct HRTF is retained; only the separate reflected
component changes. Main-channel bass is not collapsed into mono, and LFE
continues to use the existing independent path.

The offline generator is scripts/build-genelec-reference.py. Generated fixed
coefficients are not music recordings or a playback audio cache. Source and
coefficient hashes are recorded in manifest.json. Original shared-HRTF
provenance and notices remain applicable to the receiving responses.

The simulation is an independent SDA reference option, not a Genelec endorsed
product or a claim of reproducing the physical low-frequency sensation of an
83xx monitor/subwoofer system through headphones.
