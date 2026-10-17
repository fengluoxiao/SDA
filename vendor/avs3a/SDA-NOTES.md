# SDA AV3A decoder source

Upstream: https://github.com/1254qwer/avs3a-rust
Pinned revision: 59213e47a27ffa6ba9211bac1f5a6c6fd182e9d8

Retain upstream LICENSE and assets/README.md (including its normative-asset provenance notices). This is a source decoder, not an embedded Windows/Android vendor binary. Upstream full-length tests/fixtures and user music are not shipped as runtime assets. A three-frame public excerpt is retained under packages/core/tests/fixtures for regression tests.

Local changes: correct the missing LICENSE.md manifest path to LICENSE; use the same sequential neural channel stages on wasm32, where native Rayon threads are unavailable; native targets retain the original bounded thread pool. No synthesis, channel order, PRNG advancement or audio gains changed. See scripts/test-audiovivid.mjs and packages/core/src/av3a_pipeline.rs for integration regressions.
