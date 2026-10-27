# Resource compression dependencies

miniz_oxide 0.8.9 and adler2 2.0.1 were vendored from their official crates.io release archives. Upstream source and license files are retained. Cargo manifests are limited to the runtime allocation/std features used here, with a local dependency path. Original manifests are retained as Cargo.toml.orig.

These libraries compress embedded metadata/resources, not SDAR audio. The independent audio codecs are implemented in src/lossless.

miniz_oxide: MIT OR Zlib OR Apache-2.0. adler2: 0BSD OR MIT OR Apache-2.0. See each directory for full notices.
