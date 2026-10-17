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


## Optional iOS landscape side navigation

The user requested a settings switch rather than automatic replacement. Added Settings > Interface > Landscape side navigation, default off, saved in the app's UI preference file. The flag applies only to iOS landscape; portrait keeps its bottom tabs. The current UIKit Tabs.Host and all page keys stay mounted while the native bottom bar/accessory is hidden. A compact left rail controls the existing selected-page state, with one in-page mini-player on library/space. The rail overlays the existing content without reserving horizontal space or shifting immersive artwork. It is48x140points with44point accessible touch targets. On supported iOS26 hosts, both the capsule and selected item use native clear Liquid Glass, with decorative native SwiftUI symbols and a separate RN touch surface; there is no opaque accent-filled selection circle. Older non-UIKit compatibility hosts reuse the same pages in a manual rail arrangement.

TypeScript and UI adapter tests passed, including bottom-bar/accessory visibility and preserved selected screen. Preference tests cover default-off, persisted reload, serialized writes, invalid data, and late disk reads not overriding a user toggle. Preview is updated on port8083; physical-device visual confirmation remains outstanding. This is SDA's opt-in side navigation, not a claim of a verified device-specific iPhone Duo native API.

The user rejected the initial oversized reserved-column rail. The current overlay width leaves page geometry unchanged, uses box-none hit testing outside the actual rail, and reduces icons to20points. The settings gate and persistence are unchanged.


## Right-bottom navigation, drag selection and equal columns

At the user's request the48x140point rail now docks at the lower right, within the bottom safe area plus8points; horizontal safe insets remain owned by the enclosing home view. Empty dock space uses box-none hit testing. The main right-hand controls, scene metadata and mini-player leave room for the overlay, and the library can scroll its final rows above it.

A300ms hold enables drag preview through the three tabs; releasing commits one selection. Taps still work; an early swipe, outside release, interruption, multiple touches or rotation cancels selection. A common RN gesture surface operates over the native Liquid Glass visuals, with independent accessible tab actions. Native SwiftUI buttons alone did not implement this cross-tab gesture.

Landscape columns now have explicit50% widths with internal padding rather than a screen-height-wide cover plus a remainder column. The cover is a contained square computed from the measured tab viewport, centered in the left half, with its original aspect ratio. Existing portrait geometry is unchanged. Geometry/bounds, gesture-state, native-tab/safe-dock and TypeScript checks passed; native device gesture/visual confirmation remains for the Expo Go preview.


## Native-like press and scrub feedback

The user clarified that successful tab changes alone are insufficient: the original bottom tab bar lifts its glass highlight under pressure, tracks the finger and settles after release. The side rail now keeps one continuously positioned native Liquid Glass selection surface. Pressing springs it to1.18scale, movement changes its continuous Yposition rather than swapping circles at discrete indices, and release springs it to the selected segment and scale1. Cancelled gestures settle back to the current tab. Animation uses the native driver; page selection is still committed only on release. This is an interaction reproduction using native glass material, not an assertion that the side rail itself is UIKit/SwiftUI's system TabView.

Actual Expo Go client logs now show local-coordinate drags reaching all three tab targets. Earlier window-coordinate/button-ownership issues were removed. The initial touch target remains48x140; only an already-active drag gets a wider tracking corridor and bounded endpoint overshoot. A decorative image stack has no competing native Button gesture recognizer. The model and the component's responder/animation adapter are both tested, including deliberately inconsistent page/window coordinates. Native-device animation appearance remains for user confirmation.

Right-side layout is now consistent across playback, library cards, space canvas/labels and mini-player: redundant inset stacking and the extra library footer padding were removed. The dock stays within the parent's horizontal safe area with4extra points, and bottom safe area with8extra points.


## Landscape theme propagation

The independent side-rail SwiftUI hosts now explicitly receive the selected theme as seedColor. The selected SF Symbol inherits both foregroundColor and tint from its native container, while inactive symbols follow light/dark appearance. The moving native glass selection gets the same accent with20%alpha; the outer capsule remains neutral glass. This avoids an opaque accent disk and does not reset gesture/selection state when the theme changes. All five theme accents were exercised across three selected tabs and both appearances in the actual component adapter.
