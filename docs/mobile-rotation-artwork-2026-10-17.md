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

The independent side-rail SwiftUI hosts now explicitly receive the selected theme as seedColor. The selected SF Symbol inherits both foregroundColor and tint from its native container, while inactive symbols follow light/dark appearance. The selected symbol carries the accent. The moving glass selection and outer capsule remain neutral glass; the selection is composited above the symbols so it can sample their colors. This avoids an opaque accent disk and does not reset gesture/selection state when the theme changes. All five theme accents were exercised across three selected tabs and both appearances in the actual component adapter.


## Reference-matched material ordering and whole-bar press response

The user supplied the original bottom-tab press reference. The moving native glass selection is now drawn above the icon layer, rather than underneath it. No flat accent tint is applied to the glass; only the active icon uses the theme. The complete visual rail springs to1.04scale on press, together with the1.18selection lift, and both settle to1on release/cancel. The touch plane and page geometry do not scale. Duplicate settle requests after the controlled selection callback no longer restart the same spring. Tests cover press/release scales, stacking order and all theme variants; on-device optical appearance still needs visual confirmation.


## Device regression rollback

The user reported an opaque black circle covering the icons after the above-symbol glass composition. That composition is rejected. The moving native material has been returned below the symbol layer, its previous translucent theme tint restored, and symbol stacking is explicit. The whole-bar press response, drag selection and safe docking remain. Do not claim that a separate glass Host placed above symbols reproduces the system tab bar's optical refraction; device output contradicted the earlier assumption. Tests now enforce unobscured symbol stacking, while final visual confirmation still belongs to the device preview.


## Remove the failing moving glass surface

Device feedback showed that restoring layer order alone still left a dark disk. The independently moving native glass Host is therefore removed entirely. Only the stationary outer capsule uses native Liquid Glass. The moving selection is a neutral semitransparent gray RN view below the symbols (white14% in dark appearance;118/118/128 at18% in light appearance), as requested by the user. It never takes the accent tint. The active symbol remains themed. Press/scrub/settle and whole-bar feedback remain, but no optical-refraction claim is made for the selection fill. Tests explicitly require exactly one stationary glass effect and a neutral selection across all themes and selections.


## Small rightward dock adjustment

Removed the dock's extra 4 pt right padding in both UIKit and compatibility hosts. The rail moves 4 pt right within the existing horizontal safe area; page widths, bottom spacing and touch bounds are unchanged.


## Screen-edge landscape dock

The previous 4 pt change left the outer horizontal safe-area inset in place. The rail now mounts as a sibling of the page SafeAreaView, on the full presentation surface, with 12 pt right spacing and the existing bottom inset plus 8 pt spacing. The native and compatibility hosts suppress their internal dock when externally docked. Content geometry stays unchanged, the full touch target stays inside the presentation surface, and settings hide the floating dock. Device preview confirmation remains required.


## Align native layers at the screen edge

Device feedback after external docking showed the neutral RN selection to the right of the native capsule and symbols. Both SwiftUI Hosts lacked ignoreSafeArea, allowing container insets to alter their contents independently of the RN selection and touch plane. Set ignoreSafeArea="container" on both fixed 48x140 Hosts; only the outer dock handles screen positioning and bottom safety. No per-layer translation is used. Regression checks enforce the shared native frame and inset policy; physical alignment still requires Expo Go device confirmation.


## Configurable dock side and cover preview

The approved dock uses 20 pt horizontal clearance. Settings > Interface now offers left/right placement, default right, persisted alongside the enable switch. Both positions move the whole rail, with native container inset suppression preserved. Preference tests cover older settings, invalid values, reload and late-read changes independently per field.

Expo Go development mode without SdaEngine now loads a 12-row UI fixture with the original 1400x1400 エガクミライ cover and its sampled #3f4e3f bottom color. The fixture is UI-only (no audio decoding). Its image import sits behind the development/Expo Go gate. Verified the LAN iOS development bundle contains it and the minified production-mode bundle excludes expo-artwork-preview-, preview://artwork/ and egaku-cover. Native packaging has not been triggered; device UI review remains pending.


## Left-side clearance and immersive landscape artwork

Device feedback requested the Android reference artwork framing. Landscape artwork now uses a full-surface-height square anchored at x=0, instead of a smaller square centered in the safe-area left column. Image aspect ratio and the horizontal color fade are retained; control columns remain 50/50 and portrait framing is unchanged. The playback control column only reserves extra right spacing when the rail is on the right. Library/space clearance and mini-player margin now follow the selected side; the right scrollbar no longer reserves rail height for a left rail. TypeScript and layout/adapter regression checks pass; Expo Go device appearance remains subject to review.
