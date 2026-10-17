# Mobile rotation, scene viewport and artwork metadata

Changes prepared for Expo Go UI review; not pushed to trigger a new native build yet.

- The playback artwork and controls now live inside one native navigation screen and use that surface's onLayout dimensions. Portrait/landscape measurements are stored separately. This avoids mixing new window dimensions with an outgoing native layout.
- Horizontal gradient pixels are the lossless90-degree transpose of the existing vertical asset. Both static gradients stay mounted; the JS layout no longer rotates a gradient rectangle during native rotation.
- iOS landscape header moves down14points; right-side controls receive bounded additional top padding only when spare space exists.
- The scene drawable reads the current Fiber store size each frame, not the creation-time snapshot. The360RA camera targets the sphere center. The scene uses remaining page height rather than a screen-based270point minimum, preserving room for metadata and the native bottom accessory.
- Original embedded artwork for エガクミライ is1400x1400. Its display remains square, with the previously accepted uniform zoom/side crop. No vertical source crop was found. Portrait fade start moves from70% to82%; this is more image visibility, not image stretching.
- Native metadata adds optional artworkColor alongside coverUri. iOS samples a bounded ImageIO thumbnail; Android samples the already-decoded bitmap. The16x16 bottom-band sample preserves the existing52% darkening recipe. JS uses the supplied color in the first render. Older native binaries/Expo Go prewarm the fallback and reuse a bounded single-flight memory cache. No new analysis file or playback dependency is introduced.

Validation: TypeScript, existing iOS UI/head-tracking/import checks and native drawable resize regression passed. The gradient transpose was compared pixel-by-pixel. Color-cache tests cover synchronous native priming, duplicate requests, queue priming, retries and eviction. A compiled Kotlin probe executed the real Android Bitmap/Canvas sampler in MuMu and passed lower-band, transparency, alpha and1x1-image cases (explicit JVM exit is required for this MuMu app_process harness; returning from main caused a teardown crash after its assertions). Android instrumentation tests are included for normal app CI. A standalone Swift/ImageIO probe is added to iOS CI; it has not been executed on this Windows host.

Local evidence and preview logs: C:/Users/legendshop/Downloads/SDA-ui-landscape-20261017/. Expo Go preview runs on the current package checkout via Tailscale port8082. A native iOS/Android rotation animation still requires device confirmation; source/geometry checks are not a physical-device visual certification. Native metadata sampling will require the next native build, while Expo Go uses the shared fallback path.


## Follow-up: landscape group spacing and library indicators

The user identified Android's landscape control spacing as the reference. iOS now allocates the metadata/progress, time/transport and transport/volume gaps separately in the same proportions, scaling only whitespace to its measured content viewport. Its previous compact12point cap and hidden trailing gap no longer compress the visible controls. Android's accepted playback layout is unchanged.

The shared library scroll viewport extends to the outer content edge, while its cards retain18point inner padding. The iOS indicator has explicit top/bottom/right insets and theme-aware appearance. Android now reserves the actual74/128point native footer height in the scroll viewport and uses20points of content tail padding; this preserves the last-row scroll range while preventing its indicator from running behind the tabs/mini-player. The native tab adapter is exercised for both footer variants.

TypeScript, iOS UI bounds/spacing tests and Android chrome/scroll-range checks passed. Preview remains on LAN port8083; these follow-up changes are not pushed or packaged before UI review.
