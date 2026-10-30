# SDA local patch to truehd 0.6.3

Original package and Apache-2.0 license retained. Correct the five-bit OAMD trim presence mask order (centre bit 4, surround bit 3, height bit 2, top/bottom balance bit 1, listener balance bit 0) before exposing the existing trim structure to the shared SDA renderer. Audio decompression and the upstream TrueHD-specific trim value lookup remain unchanged. E-AC-3 uses its separately specified ETSI lookup.
