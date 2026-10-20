# SDA local changes

Upstream: SakuzyPeng/MacinDecode-AC4-Core a23965312a53d3cfb1a63cbae692a788efe46123, crates/macindecode-ac4-decode. Original MIT license retained.

- Standalone manifest keeps the same upstream metadata, bitstream revision and lints.
- Full A-JOC reconstructed-object bound is 128 instead of the upstream product limit of 20. State and coefficient buffers are bounded and heap allocated. Input/downmix, frame and syntax limits are unchanged.
- Regression coverage exercises 24/64/128 outputs, maximum-width reconstruction, invalid 129-output topology, and reset/transaction behavior.

No normative spec tables are redistributed; the existing build process supplies verified tables using MACINDECODE_AC4_SPEC_DIR.
