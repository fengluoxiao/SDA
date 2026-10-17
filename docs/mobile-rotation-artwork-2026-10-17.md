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


## Cutout-safe side navigation

Fixed 20 pt screen-edge placement can intersect the Dynamic Island/notch. The full-surface dock now uses native SafeAreaView maximum mode on both horizontal edges with 14 pt base padding, plus 6 pt margin around the rail for press-animation overhang: effective selected-side gap = max(14, system inset) + 6. This keeps the approved 20 pt gap where safe, follows device/rotation insets without model-specific values, and does not apply safe-area padding to the edge-aligned artwork. Bottom inset plus 8 pt is unchanged; internal SwiftUI Hosts still ignore container insets to avoid independent shifting. Physical cutout clearance needs device review.


## Landscape mini-player and symbol tint

The floating mini-player now uses a 52 pt capsule with a measured native regular-glass background, retaining the same artwork/text/playback content as the portrait system accessory. The system accessory keeps its own material (no duplicate backdrop). Missing/older native glass uses a translucent fallback. The playback SF Symbol now explicitly receives the requested color through Host seedColor and container foregroundColor/tint; its Host ignores container safe areas. This applies to both mini-player orientations and preserves the main player requested color.

User clarified that the page should move right into the spare right padding, while the rail stays at its approved cutout-safe position. Restore the dock max(14, inset)+6 rule and original reserved lane. For left-side navigation only, the page host uses 30 pt left / 6 pt right padding instead of 18/18: content and mini-player move 12 pt right with unchanged total width. The screen-edge artwork stays independently anchored. UIKit and compatibility hosts share this rule. Changes remain in the Expo Go review stage.


## Align the header with the left-rail content

The header was outside the shifted page host. In left-rail landscape mode its title now includes the same reserved lane and 12 pt shift, and its trailing controls use the page's 6 pt inner edge. Header vertical spacing, the rail, and artwork stay unchanged.


## Separate left/right docking and library bottom backdrop

User confirmed left placement but rejected the inward-shifted right placement, supplying the earlier 12 pt screen-edge reference. Left remains max(14, left safe inset)+6, with the approved page/header shift untouched. Right now disables the dock right inset and uses 6 pt padding + 6 pt rail margin = 12 pt. Bottom placement is unchanged.

Existing tab background/shadow props remain transparent. The library ScrollView now uses a capability-guarded RNSScrollViewMarker to hide its iOS 26 bottom scroll-edge effect, which is separate from the tab glass material. Only the library direct ScrollView is wrapped; content tail padding, native tab/accessory glass and the gray selected segment remain. Tests cover enabled and missing-manager paths. This is a targeted candidate for the reported bottom tint; physical visual confirmation remains pending.


## Expo Go startup regression: remove scroll marker

The supplied error stack maps exactly to the RNSScrollViewMarker hasViewManagerConfig call in the served iOS bundle at line 303170. Expo Go throws from unstable_hasComponent instead of reporting false. Remove the optional marker probe and experimental import entirely, restoring the normal library ScrollView. Tests now throw on unknown manager names rather than treating all capability probes as safe. Retain the accepted left/right placement, header alignment, capsule mini-player and symbol tint. The bottom tint investigation is not considered resolved by this rollback; its unsupported candidate is withdrawn.


## Landscape upper-side actions

With side navigation enabled in iOS landscape, the existing head-tracking and settings actions move from the title row to a vertical stack above the rail on the selected side. Use the same 48 pt lane and horizontal dock rules, 16 pt plus top-safe-area spacing, 44 pt buttons and an 8 pt gap. The existing bottom rail and page layout are unchanged. Only one set of controls is mounted; portrait/disabled side navigation retains the original header row. Docked native button Hosts ignore container safe areas and remove the portrait head-tracking trailing margin, preventing independent native displacement. Settings hides the dock. TypeScript, UI regressions and head-tracking action contracts pass; physical preview remains to be checked.


## Landscape settings transition and library underlay

Device screenshots show the landscape settings header changing position/material during presentation and clipping the first section heading. Landscape now uses a nontranslucent native header with the settings page background and simple_push (no separate native-header transition). The stack reserves header space before the SwiftUI Form; the Host retains normal side/bottom safety. Portrait keeps the existing translucent/default transition. This is a targeted layout fix pending device verification.

The library wrapper previously applied bottom safe-area padding while its ScrollView also used automatic native content insets. This kept list content out of the floating tab/accessory backdrop and exposed a uniform page-colored strip. When using native bottom chrome, the library wrapper no longer reserves that bottom inset; ScrollView automatic content and indicator insets keep the last row reachable while the viewport extends behind the glass. Side-rail and legacy in-page-accessory layouts still retain wrapper bottom protection. Tab fill/shadow remain transparent; no unsupported component probes, added glass tint or full-width panel is used. Tests cover native accessory, no track, sidebar and older-native fallback paths, plus portrait/landscape settings behavior. Device appearance is not claimed verified.


## Reject split landscape header transition

The next device screenshot disproved the previous landscape simple_push/nontranslucent-header candidate: the header covered the full width while settings content was still moving, exposing player artwork at the side. Remove that special animation and opaque native-header override. For landscape only, hide the separate UIKit header and reuse IOSSettingsCompatibilityPage: a single native SwiftUI Host holds the back button, centered title and the original NativeSettingsForm. UIKit pushes/pops the entire surface with its default transition; portrait retains its original system header. The reused page uses maxWidth instead of fixed window width so native safe-area proposals are respected. The return control is icon-only circular glass with the same close callback. Source/runtime adapter checks verify one Host for header and Form, no simple_push, preserved device safe areas and no full-window fixed-width header. This has not been visually validated on an iPhone; no claim of physical acceptance. Other accepted navigation/layout and library changes are preserved.


## Center the right-side controls in their reserved lane

User requested equal blank space on both sides of the right-side actions/rail, while preserving approved left placement. Both docks now share SideNavigationDock. Right content ends at rightSafeInset + 48 + 8 pt from the screen; centering a 48 pt rail leaves (rightSafeInset + 8)/2 pt on each side. This moves the right dock inward according to actual device insets instead of another fixed offset. Upper actions use the same centered lane. Left remains exactly max(14, leftInset)+6, with identical top/bottom spacing and content positions. Tests cover five safe-area values for both placements and assert left invariance.


## Restore landscape scrolling header blur

The unified landscape page fixed split transitions but its sequential VStack header kept all scrolling content below the title, losing the portrait header blur. When the already-bundled Expo ZStack view is available, use one native Host with a top-aligned ZStack: NativeSettingsForm behind, fixed 52 pt header above with SwiftUI bar material. The first section owns a 52 pt top inset which scrolls away, so initial controls remain visible and later rows pass beneath the material. No JS scroll tracking, separate header transition, new native manager or image blur is added. Older hosts without ZStack retain the sequential fallback. Portrait still uses the system navigation bar. Adapter checks verify ordering, material and initial inset; iPhone appearance still requires review.


## Library title blur and unobstructed landscape scrollbar

User accepted the bottom underlay and requested only scrollbar clearance plus a blurred library title in both orientations. Right-rail mode now insets the vertical indicator by 56 pt to follow the content edge, rather than running underneath upper side actions. Its top follows measured header height, bottom stays 10 pt because the indicator is horizontally clear of the rail. Left/portrait indicator horizontal placement is preserved.

On the iOS library page, the existing title/actions become an absolute overlay with a decorative native bar-material backdrop. The ScrollView viewport extends behind the header; its content padding includes the measured header height plus the existing 8 pt, keeping initial content clear. Header measurements are cached per orientation, and the backdrop expands through the top/horizontal safe areas while leaving controls in their existing positions. Only already-used Expo Host/VStack primitives are queried. Playback measurements ignore layouts emitted while another tab is active, avoiding the expanded library viewport polluting the playback cache. Error banners retain ordinary header flow. Bottom tab/accessory underlay and accepted dock geometry are untouched. TypeScript, adapter checks and new portrait/landscape indicator/material/fallback checks pass; device visual QA remains pending.


## Device-rejected material headers: real UIKit blur and floating library accessory

User screenshots show the prior SwiftUI background material did not obscure/blur the scrolling text on their Expo Go device. Replace it with guarded expo-blur 57.0.3 (the version listed in this installed Expo SDK bundledNativeModules.json). The library backdrop is now mounted after the native scrolling subtree; landscape settings embeds the same blur behind its native header using the existing RNHostView inside the single transition Host. Missing/throwing Expo view lookup yields an opaque readable fallback, never transparent overlapping text. The live device log reports `[SDA UI] header blur: ExpoBlur native`, confirming the native component path initialized; this does not claim visual acceptance.

The landscape library mini-player was still an in-flow footer, leaving a page-colored strip behind it. It now overlays the ScrollView at the same screen position in both UIKit and compatibility hosts, with 76 pt scroll-content tail clearance. Native portrait accessory behavior is preserved. The scrollbar now sits strictly between the row-content edge and the centered side rail, using the outer provider's real right safe inset. Its bottom clearance is 62 pt when a floating mini-player is shown. Tests cover row-arrow/rail separation across four device insets, transparent footer layout, last-row clearance, native blur/opaque fallback, and settings single-host composition. TypeScript and pinned React/RN/Three/Fiber resolution checks passed; LAN bundle served and a preview reload was requested. No audio code changed.

Dependency setup note: the package-manager operation produced the expected lock entries but hit a shared-hoisted-node_modules temporary-directory error before updating package.json. The new dependency declaration and the single dependency-free package installation were completed explicitly; unrelated lock metadata additions were removed. Pinned mobile dependency checks and the live Metro bundle succeeded afterward.


## Left-only indicator correction and matched header coverage

Right-side indicator placement was accepted and its expression is unchanged. Left-side page content is translated 12 pt right (30/6 padding); the old 3 pt indicator inset therefore placed the indicator 9 pt outside the native screen clip. Use 15 pt in left-side mode only (12 pt translation + original 3 pt inset). Tests verify it lies between the row edge and native clipping boundary for multiple safe insets.

Both header paths already used the same ExpoBlur intensity/tint. Device image sampling found the central header backgrounds differ by roughly 1 RGB level, while both are slightly brighter than the base page. A concrete coverage difference remained: the settings blur stopped at its SwiftUI safe-area bounds, unlike the edge-extended library backdrop. Settings now uses the same inset-aware backdrop, with 0 extra horizontal content padding versus the library's 18 pt. Material strength and accepted right dock/scrollbar behavior are unchanged. Visual equality over differing underlying content is not claimed.


## Real left scrollbar gutter and scroll-edge header behavior

The 15 pt indicator-only adjustment revealed the thumb but left it against the card edge. In left-rail mode only, shrink the ScrollView's trailing negative margin from -18 to -6 so its viewport ends at the native clipping boundary; restore its ordinary 3 pt indicator inset. The existing 18 pt row padding now creates a real 15 pt separation between card edge and indicator. Align the floating library mini-player to the same trailing gutter with a 12 pt right offset. Right-side dock, content and indicator expressions are unchanged.

To match the portrait settings header behavior, the library now drives the shared blur layer opacity from an Animated.ScrollView native scroll value (0–12 pt fades from clear to blurred). Landscape settings observes its native Form geometry and only starts a 120 ms native opacity animation when crossing the top/scrolled boundary. It retains a readable blur until the first native geometry report, rather than risking transparent overlapping text on an unsupported runtime. Returning to the top clears the material's white tint. The original portrait UIKit header is untouched. Tests cover left native bounds/card separation, right invariance, opacity changes, and the shared blur fallback; physical transition appearance remains to be reviewed.


## Compare against the portrait native settings header

A side-by-side crop of the supplied library screenshot and portrait settings reference is saved in the local preview evidence directory as header-reference-comparison.jpg. The earlier uniform systemMaterial backdrop is not equivalent to the native navigation treatment; changing overall opacity alone did not address material character or the abrupt lower boundary. Switch the shared custom header blur to systemChromeMaterialLight/Dark and extend its bottom by 16 pt under a continuous alpha mask. The mask uses existing Expo UI Mask/RNHost primitives behind capability checks and a tiny generated RGBA asset; no dependency/native manager is added. If the mask is unavailable, retain a readable unextended blur. Existing scroll-driven opacity, header geometry, side rails, indicator positions and bottom glass are preserved.

This remains a custom approximation, not the actual UIKit UIScrollEdgeEffect or a claim of pixel parity. TypeScript, module/fallback/mask tests and adapter regressions pass; the LAN bundle contains the new chrome/mask path. Actual device appearance needs review. Apple primary references consulted: UIBlurEffect.Style.systemChromeMaterial, UIScrollEdgeEffect and TN3106 (which recommends avoiding custom navigation backgrounds in iOS 26).


## Replace custom header imitation with native page-owned navigation

User authorized restoring the actual UIKit navigation bar used by portrait settings. New IOSNativePage uses the existing RNSScreen/ScreenStack/HeaderConfig/HeaderSubview primitives to host a real UINavigationController. The library stays within its existing native tab and gets this native header; non-side-mode head-tracking/settings actions are retained in its native trailing slot. Side-mode actions remain in the approved overlay. Native library layout owns the top inset, while the content viewport still handles side cutouts and the accepted list/mini-player gutters.

Settings now uses the same native header in portrait and landscape. The existing outer transition stack keeps both route headers hidden; the settings destination owns an inner native page header and original Form. Thus the whole destination, including its native bar, participates in one push/pop, without simple_push or separate custom title/blur animation. The back action closes the existing settings route, and interactive dismissal retains its original callback. The previous JS BlurView/mask/scroll-opacity helpers, generated alpha image and expo-blur dependency/lock entries are removed. Older binaries without the native primitives retain only the original compatibility form/header.

During live preview, logs confirmed native page header availability and native header-height events for sda-library-content. The user supplied a screenshot showing the native scroll-edge effect, then requested that its landscape width avoid the left action buttons. The left library viewport is now moved inward while compensating its content padding, so cards/covers/mini-player stay at the approved positions. iosLeftLibraryGutter preserves marginLeft + paddingLeft = 38 and clears the action bounds across zero, small and cutout-sized safe insets. Right mode is unchanged.

Validation: mobile TypeScript, iOS UI adapters, native-page capability/actions tests, library bounds/gutters, head-tracking contracts, side-dock geometry and Android chrome checks pass. LAN bundle contains native library/settings pages and excludes the retired custom blur path. Only Expo Go preview was updated; no remote push or native build requested. Transition/rotation appearance still needs device review.


## Extend the left-mode native header effect through the right safe area

Follow-up screenshot identified the remaining uncovered right strip. For left-side navigation on the native library page, extend the ScrollView trailing margin by the outer right safe inset, then add the same amount to row right padding and indicator right inset. The native scroll-edge viewport reaches the physical right edge while card/cover positions and the 15 pt indicator gutter stay unchanged. The floating mini-player remains in its safe content frame. To prevent double automatic indicator safety on this extended viewport, use the actual native header-height callback for the top indicator inset and apply explicit indicator insets only in this left mode. The accepted right-navigation behavior is unchanged. Tests cover edge coverage, row/indicator invariance and forwarding a measured 78 pt header height without duplicating the inset.


## Mirror native-effect bounds for right mode; native sidebar blur

User accepted left-mode native coverage and asked for the remaining right-mode overlap plus blur on the side navigation. Right mode now extends its viewport through the left safe edge and ends it before the right action lane. iosRightLibraryGutter compensates row padding and indicator inset so the accepted card, cover and scrollbar screen coordinates remain identical. Tests cover safe insets 0–62 pt, both opposite screen-edge coverage and action-lane separation. Existing left-mode compensation is retained.

The side rail keeps its existing interaction reproduction but switches its one native SwiftUI capsule glassEffect from clear to regular, providing the native frosted backdrop. No accent tint, second glass layer or custom blur image is added; the neutral selected circle, themed icon, drag behavior and press scaling remain. This is native glass material on the existing custom side rail, not a claim that it is UIKit's system vertical tab bar.


## Clarified scope: full right-side background, not capsule

The user withdrew the capsule-backdrop change and clarified the entire right-side background should blur. Restore SideNavigationRail and its gesture/theme tests exactly to the accepted regular-glass version. IOSSideNavigationBackground now provides a single native ExpoBlur visual-effect view across the full-height right control region, underneath both top actions and the bottom rail. Its inner edge meets the native library viewport boundary computed from the actual safe inset; it leaves the accepted scrollbar untouched. It is noninteractive and only mounted for right-side landscape navigation while settings is closed. Left mode and the capsule itself are unchanged. The SDK-pinned expo-blur dependency is used only for this region; native library/settings headers remain UIKit-owned. Tests check region bounds, layers, multiple safe insets and missing-view fallback.


## Final preview revision: transparent side regions and larger settings back button

User required preview/release parity, then explicitly cancelled side-region blur in favor of transparency. Withdraw the unvalidated release-only SdaScrollEdge candidate and remove its module registration, JS adapter and Swift source from this working revision. Remove the full-height side-background BlurView component and expo-blur dependency/lock entries. Both side regions now have no overlay; capsule appearance and accepted native title bars stay unchanged. Settings back uses large native control size, 22 pt semibold symbol and a 44x44 frame. Library action rows use the same RN presentation in Expo Go and standalone, avoiding a SdaGlassButton-dependent visual switch. Mock artwork remains development-only. Changes await user preview approval: no commit, push or native build is performed for this revision.


## Match side capsule appearance to the upper action buttons

The screenshot showed a darker lower capsule than the settings/tracking buttons. Its glass Host forced dark appearance on immersive playback, whereas the upper action Hosts inherited system appearance. Remove only the capsule Host override so all side-control materials inherit the same appearance. Keep the separate symbol Host contrast, theme accent, selection feedback, gestures and dock geometry. Side-region background remains transparent. This uses the same Expo UI path in preview and release; physical appearance awaits preview confirmation.

Follow-up: inactive side-rail icons use black in system light mode and white in system dark mode, with the selected icon retaining the theme accent. Pass system appearance into the rail instead of treating immersive artwork as dark mode.


## Transparent library bottom region (corrected)

The user meant removing the bottom background, not adding a cover. Withdraw the mistaken safe-area cover. In sidebar mode the library viewport now extends to the bottom edge without a reserved footer strip; all wrappers remain transparent. Only the floating mini-player retains the device bottom-safe inset, with the same extra inset added to scroll-tail clearance so the last row remains reachable. Both sidebar positions share this path in Expo Go and release. Pending physical preview confirmation.


## Approved release request — 2026-10-17

User approved the transparent bottom-region preview and authorized commit, push and iOS packaging. Preview/release UI parity is now a standing requirement for future bug fixes and optimizations, recorded in apps/mobile/AGENTS.md. iOS CI runs the native-page, library-layout, side-background and dock checks alongside the existing UI, theme and gesture checks. The withdrawn standalone-only scroll-edge module is not registered or shipped; only Expo Go mock data is omitted in production. The separate dirty native-renderer probe is excluded from this UI release commit. Installed-release visual confirmation is still a separate device check.


## Right dock cutout safety in either landscape rotation

Device feedback found that the right dock overlapped the Dynamic Island when rotated onto that side. The old (rightInset + 8) / 2 formula halved the cutout inset. The shared UI now preserves the full current right inset plus 4 pt, including clearance for the pressed capsule/selection expansion. Upper actions use the same dock. Library scrollbar/native edge bounds follow the available gap while card positions remain unchanged. Left docking and zero-right-inset positions are unchanged. Regression checks cover swapped left/right insets, both placements, press expansion and library bounds; mobile TypeScript and UI checks pass. Expo Go preview is updated for physical rotation review before another UI release.


## Side-rail selected appearance matched to the portrait reference

The user supplied portrait/native and landscape/custom screenshots: the portrait selection has a darker neutral fill and deeper accent, while the vertical selection looked nearly absent with a brighter icon. The rail now draws an explicit neutral selection above the single glass backdrop and below symbols. Light mode uses black at 34% for the selection and a same-hue/saturation shade (55% of original HSL lightness) for the selected icon; dark mode retains the bright theme accent and neutral light fill. The saved theme colors and portrait UIKit bar are unchanged. This is a custom visual approximation for Expo Go/release parity, not access to UIKit's internal adaptive tint. All themes share the rule, including held/dragged selection. Gesture/theme checks and TypeScript pass; physical screenshot matching awaits user review. The cutout-safety fix remains in this preview.


## Limit selection deepening to light appearance over dark content

Follow-up screenshots showed that applying the deeper selection to the light library was incorrect. The rail now receives the actual page backdrop (artwork fade on playback, page background elsewhere). Only light system appearance over a dark backdrop applies the deeper theme shade and darker neutral fill. Light pages/bright artwork retain the original accent and subtle gray selection; dark system appearance retains its previous bright accent and white inactive icons. All theme/background combinations are covered by the existing gesture/component tests.

The right dock stays at the full safe inset plus 4 pt. Increase the page-to-rail gap from 4 to 16 pt by reserving 12 additional points in the playback controls, library rows/scrollbar, space page and floating mini-player. Share the spacing constant between UIKit and compatibility hosts. Left mode, artwork position and transparent side/bottom regions remain unchanged. UI/type/layout tests pass. Confirmed the existing Tailscale server at 100.74.135.31:8084 serves this integration checkout and both corrections. Preview-only pending visual approval; no push or new build.


## Observe physical cutout side and restore opposite-side geometry

The user clarified that cutout avoidance must apply only when the selected side rail is on the cutout side, then extended that requirement to left docking. Safe-area numbers may be symmetric across a 180-degree rotation, so add the SDK-compatible expo-screen-orientation 57.0.2 module in both Expo Go JS and the release dependency/lock. Observe interface-orientation events and refresh on foreground; do not lock orientation, poll, or add a release-only bridge. Expo orientation 3 maps to UIInterfaceOrientation.landscapeLeft (Home button left, portrait top/cutout right); 4 maps to landscapeRight (Home button right, cutout left). Verified against the official module Swift enum and Apple's UIInterfaceOrientation documentation. Initial/unknown orientation keeps conservative protection, and stale async reads cannot overwrite newer events.

Right same-side mode retains full safe inset plus 4 and the 20 pt content reservation gap; opposite-side mode restores the exact old (inset+8)/2 position, 8 pt gap and old library indicator formula. Left same-side mode retains its approved max(14,inset)+6 position; opposite-side mode moves toward the edge with a 20 pt minimum. Header/content/mini-player follow the left displacement rather than leaving a dead content lane. Geometry tests include equal left/right safe insets, direct 180-degree rotation and either sidebar setting.

For the reported missing playback selection, paint the neutral selected background directly on the non-collapsible animated overlay above glass and below the explicitly transparent symbol host. Keep the background-aware palette rule; left playback with missing artwork uses its actual #303034 backdrop instead of the right-side fade color. UI, gesture, orientation, layout and type checks pass. No claim of exact on-device parity: at verification both preview manifest requests timed out (8084 status also timed out), so this version has not been verified as delivered to the phone. Previous process restart attempts were blocked by automatic approval. No commit, push or new build for these pending UI changes.

Dependency handling note: pnpm 11 rewrote unrelated lock sections; that rewrite was discarded and the lock regenerated using CI's pnpm 9.15.9. Shared preview node_modules linking hit an EPERM on an existing TypeScript folder; no recursive removal/reinstall was attempted. The added official orientation package resolves locally, pinned mobile JS dependency checks and TypeScript pass, and its manifest specifier matches the generated lock entry.


## Playback selected state: fix RGB backdrop parsing

New matched-background portrait/landscape screenshots exposed the actual palette bug: sideNavigationAppearance only parsed hex, but both native ArtworkColor metadata and the GL sampler provide rgb(r,g,b). Consequently the live sampled dark-green playback background did not activate either the darker selected fill or deeper theme icon. Accept bounded RGB triples (including whitespace) as well as hex and classify equivalent colors identically. Keep the approved light-page and system-dark rules. Fix the Expo-only fixture's prepared color from #3f4e3f to equivalent rgb(63,78,63), which the production metadata validator accepts.

Regressions now cover all five accents, RGB/hex equivalence in both appearances, actual rgb(63,78,63), invalid RGB, real preview metadata validation and the rendered selection overlay. Mobile TypeScript, gesture/theme, artwork-cache and transparent-background checks pass. Tailscale 8084 is responding again and serves the corrected RGB parser, fixture and selected-fill code from this checkout. This establishes delivery of the fix, not pixel-identical physical appearance. Pending user visual approval; no push or build.


## Native segmented picker vertical-presentation trial

User asked to try vertical use of the segmented control. Add IOSVerticalSegmentedTabs using the existing Expo UI native Picker with segmented style, large control size and unmodified theme tint. Rotate its 140x48 Host by 90 degrees inside the existing 48x140 dock and counter-rotate each SF Symbol by -90 degrees. The native Picker owns selection and input; there is no RN PanResponder or custom selected-circle overlay in this path. Keep CustomSideNavigationRail recoverable and available to older/unsupported hosts. The same capability-based path runs in Expo Go and release; no dev-only visual branch is added. Dock orientation/cutout logic and page content geometry remain as previously previewed.

TypeScript plus native-picker adapter, geometry, existing gesture and transparent-background tests pass. Tests prove JS wiring and transformed bounds, not native label rendering or physical drag behavior. Confirmed Tailscale 8084 serves the trial and accepts reload (HTTP 200). The key remaining on-device checks are whether SwiftUI preserves the inverse symbol transforms in segmented labels, whether rotated native hit testing/dragging works, and whether glass selection matches the requested reference. This is a rotation experiment, not an officially provided vertical segmented style. No commit/push/build before visual acceptance.


## Vertical Picker feedback: upright symbols and background review

Physical feedback found that the segmented image labels did not preserve inverse rotation or theme tint. Replace the Picker labels with blank tagged Text and keep the real rotated control for physical input/glass/selection. Draw three upright SF Symbols in a separate unrotated, touch-transparent native Host, explicitly applying the existing background-aware selected palette. Provide named RN accessibility tabs while hiding blank native labels from VoiceOver. No custom selected-circle layer is reintroduced. Adapter/theme/gesture checks and mobile TypeScript pass; Tailscale 8084 bundle contains this correction and accepted reload (HTTP 200).

The user additionally reports a background problem without a screenshot of this Picker revision. The RN/Host wrappers are transparent, but the native segmented control still owns its own material/selected backing. No unsupported claim that this matches the previous capsule; obtain the current device screenshot before changing that layer. No commit/push/build.


## Neutral native Picker backing preview

User requested continued background work. Remove the character seedColor and tint modifier from the native Picker/touch Host, while retaining explicit colors on the independent upright symbol layer. Make the outer view explicitly transparent. Do not add another glass layer or claim a clear RN wrapper removes the Picker's internal backing. Native segmented selection and physical touch handling remain intact. Tests assert that the track no longer receives character tint and the icon theme remains; mobile TypeScript and side-background checks pass. Confirmed Tailscale 8084 serves the neutral-track version and reload returned HTTP 200. Exact physical background matching still awaits user feedback; the segmented control's native resting material remains. No commit/push/build.


## Correct selected symbol color inheritance in the vertical Picker

User clarified that the active icon remained black. Keep the native backing unchanged. Match the already-used IOSPlaybackSymbol pattern: each upright symbol now has its own VStack with foregroundColor, tint, font and fixed slot frame, and the Image also receives the resolved color. This avoids relying solely on image-level color/frame modifiers. Tests now validate inherited container color for all five themes, each selected tab, both appearances and light/dark page backgrounds rather than just inspecting Image.color. TypeScript passes. Tailscale served the native-container foreground/tint code; physical rendering still requires user confirmation. No commit/push/build.


## Replace segmented backing with native TabBar; preview icon while scrubbing

User accepted upright/theme-colored icons but rejected the segmented backing as unlike portrait, then requested the same selected color while dragging over another item before release. Withdraw IOSVerticalSegmentedTabs and its test (saved outside the repository in the preview evidence folder). Add IOSVerticalTabBar using react-native-screens Tabs.Host/Tabs.Screen, the same native UITabBarController family used in portrait. Extract the unchanged portrait appearance into ios-tab-appearance.ts and share it. A transparent 20x20 image template leaves native bar item geometry/accessibility intact while the accepted upright SF Symbols remain in the independent, touch-transparent colored layer. The native bar owns actual selection/gesture and hosts only three empty views, not additional player instances.

Add createNativeSideTabPreview as a passive touch observer: measure the unrotated wrapper in window coordinates, follow the latest finger even if measurement completes late, color hover ?? selected with the same selected palette, restore on exit/cancel/multitouch/end, and never commit navigation. Native onTabSelected remains the only commit path. Upper/lower dock positioning and cutout logic are unchanged. JS tests cover shared appearances, original portrait configuration, hover/selected icon colors, preview-without-commit, native commits, delayed measurement, cancellation and capability fallback. TypeScript passes. Tailscale 8084 serves the new component/observer and reload returned HTTP 200.

This remains a physical UI trial: a full native tab controller rotated into 140x48 can have intrinsic layout/gesture limitations not covered by JS tests. Verify actual bar bounds, blank-template loading and native touch propagation on the device before claiming background or scrub parity. No commit/push/build.


## Retain native TabBar and correct inset layout / native touch preview

Device feedback showed the small rotated UITabBar did not align with fixed upright symbols and the RN touch observer did not recolor while scrubbing. The initial restoration of the Picker was superseded by the user's explicit instruction to keep the native TabBar and fix its placement. Active path is again IOSVerticalTabBar, not Picker.

Keep a fixed visible 140x48 unrotated stage, but provide extra native-controller width/height for its own insets and offset the leading inset before rotation. SafeAreaListener on the selected placeholder page feeds native inset changes back to the bounded compensation model; initial values use device insets. A thin-space title retains an invisible nonempty native item label. This compensation is a new candidate for the screenshot misalignment, not a claim that JS tests establish UIKit's exact floating-material bounds.

Replace the failed RN onTouch stream with react-native-gesture-handler 2.32.0 (the SDK 57 bundled version): simultaneous native/manual recognizers, cancelsTouchesInView false, JS touch down/move/up/cancel callbacks, and no manual activation or navigation commit. Hover icon color uses the same selected palette; native onTabSelected is the commit path. The recognizer is hosted in a local GestureHandlerRootView and the same dependencies/code are included in release. UI/type/gesture/layout tests and the Tailscale bundle compile pass. Confirm native event delivery and actual bar alignment on the phone before declaring either resolved.

Dependency lock was generated in an isolated manifest-only workspace using pnpm 9.15.9; the shared preview node_modules tree was not reinstalled. Official package tarballs were integrity-checked when materializing missing preview dependencies. Native TabBar/foreground changes remain uncommitted and unbuilt.


## Put glyphs inside native tab items instead of an external overlay

User correctly identified that icons were still outside the native bar. Remove the SwiftUI symbol overlay, custom selected-color calculation and manual/RN/GestureHandler preselection path from IOSVerticalTabBar. Each native Tabs.Screen now has its own nonempty templateSource icon. UIKit owns glyph position, selection tint and native drag highlight together with the background. The empty placeholder icon is removed. The three glyph silhouettes were retained as transparent alpha templates from user-provided device screenshots and pre-rotated counter-clockwise; the rotated native bar displays them upright. No runtime image generation or analysis is introduced. Source masks, 1x/2x/3x images and a reproducible generator are included with provenance.

Remove the unused react-native-gesture-handler dependency and regenerate the lock in the isolated workspace. Existing side/cutout geometry and portrait native appearance are retained. Tests decode the generated PNGs using Node built-ins, verify visible alpha and inverse rotation, and verify that all three actual native items own distinct image sources and the chosen theme tint. They also assert the absence of external glyph/hover layers. Mobile TypeScript passes. Confirmed Tailscale 8084 serves the native image assets and no vertical-tab-upright-symbols overlay; reload returned HTTP 200. Physical native-bar sizing still requires device review; no commit/push/build.


## Increase native item occupancy without scaling the glyphs

User reported that the native items are vertically crowded. The template image canvas was 24x24 points, leaving the native horizontal bar to size content from very small items before rotation. Regenerate each image as 44x24 points with 10 pt transparent padding on both sides of the existing 24-point glyph canvas. After the parent rotates, that padding becomes vertical space; the glyph pixels themselves are unchanged. No outer icon overlay, fake selected layer or manual drag coloring is restored.

PNG tests verify 1x/2x/3x canvas sizes, transparent added margins and byte-exact preservation of the 3x glyph alpha after inverse rotation. TypeScript passes. Confirmed all three assets in the live Tailscale bundle report width 44 / height 24 and reload returned HTTP 200. An attempt to read actual device layout through the inspector did not obtain measurements before the device disconnected; no claim of measured native floating-pill height is made. Temporary diagnostic global removed. Physical spacing remains pending user review; no commit/push/build.


## Remove child-inset feedback that shrinks the native controller

User reported no visible height change after wider icon canvases. Do not treat image dimensions as verification of the floating bar's physical length. Remove SafeAreaListener/reportedInsets from IOSVerticalTabBar: the child page's already-adjusted content insets must not overwrite the enclosing controller's device inset reservation. Size the controller from the parent screen insets and explicitly pass pixel width/height with flex:0 to Tabs.Host, preserving 140 points after the reserved horizontal insets.

Tests now assert that child pages have no size-changing callback and cover parent left/right insets 0/44/59/62 and bottom 0/21/34. TypeScript and native-item asset tests pass. Live Tailscale bundle was checked for explicit controller dimensions and absence of the shrinking feedback; reload returned HTTP 200. Inspector briefly connected but did not yield reliable device layout data before disconnection, so the native floating-pill bounds remain unverified. No commit/push/build.


## Reduce the working native side-bar length

User confirmed the height fix took effect but found the bar too tall. Reduce the visible layout length from 140 to 120 points using a shared NATIVE_SIDE_TAB_LENGTH constant for the controller width, rotated stage and outer frame. Recompute rotation offsets from that length. Keep the dock's bottom anchor, width, glyph pixels, native backgrounds and selection handling unchanged. Native item/layout regression checks pass; reduced-height preview awaits user confirmation.


## Match old visible bounds instead of equating host length with pill length

The user supplied the still-too-long native result and the approved old rail. Both images are 2774x1280. The cropped reference pill is approximately 417 px high, the new pill approximately 484 px, with its bottom about 63 px lower. Relative to the old 140 pt rail this is about 163 pt visible height and 21 pt excessive bottom extension. The previous claim that changing the host constant to 120 had shortened the visible pill was not established and is withdrawn.

Restore the outer visible footprint to the old 140 pt so the dock's bottom anchor and center match the reference. In the native controller compensation, subtract the already-reserved bottom-safe strip from each side clearance before extending the long axis. On the 59/59/21 case this changes controller width from 238 to 216 and leading offset from -59 to -38; the visible outer frame changes from 120 to 140, lifting its center by 10 pt at the fixed bottom anchor. This candidate is calibrated to the measured reference/current difference, not a claim of access to private UIKit floating-pill bounds. Template glyph sizes, native item ownership, theme and interaction remain unchanged. Regression coverage includes side insets smaller than the bottom inset and does not allow negative reserves. Physical appearance still awaits verification; no commit/push/build.


## Center native item glyphs by removing invisible titles

Device feedback showed every glyph shifted toward the top of its segment. The native items still had a thin-space title from the earlier placeholder experiment. Remove the title prop entirely, so react-native-screens passes isTitleUndefined and UIKit receives a nil title. A whitespace title still participates in inline icon/title layout; after the 90-degree rotation that reserved space displaces the icon vertically. Named accessibility labels remain. Controller geometry, glyph assets, native background and tint are unchanged. Regression checks require undefined titles for all items. Device centering still awaits confirmation; no commit/push/build.


## Final optical centering adjustment

User reported a small remaining center offset after removing titles. Measured the selected region and blue glyph in codex-clipboard-9f241582-2d41-40fc-aaa0-53e1cca56e89.jpg: glyph center is approximately 8.5 screenshot pixels below the selection center (about 2.85 pt), with only -0.5 px horizontal difference. Move the template glyph 3 pt toward the native horizontal leading edge before the bar's clockwise rotation, yielding a 3 pt upward adjustment on screen. The 44x24 item canvas, glyph scale, native hit targets, whole-bar bounds and theme remain unchanged. Generator padding becomes 7/13 instead of 10/10; PNG regression checks retain exact source glyph alpha and enforce the new offset at every scale. No commit/push/build.


## Resume after rotating while backgrounded

User reproduced 1–3 seconds of inconsistent orientation when leaving in portrait and returning in landscape, or the reverse. IOSPlayer always preferred the last surfaceSize over current window metrics; it never invalidated that measured size for background/foreground changes. MobileObjectScene also pinned Canvas to the previous JS viewport width/height.

Replace that cache with usePlayerSurfaceLayout and a revision-guarded layout model. Leaving active clears the old measurement; returning reads current Dimensions and schedules a fresh native root measurement on the next frame, even if onLayout does not fire. Window changes invalidate old pending measurements. onLayout only requests a fresh measurement rather than consuming potentially queued event sizes. Same-revision live native measurements may supersede delayed Dimensions events. Foreground rotation retains the previous measured-surface policy until the native root changes. No fixed delays, fade masks, navigation remounts or audio changes.

Gate the space scene's active rendering on current layout readiness and let Canvas flex with its native parent; keep the existing Canvas/camera/asset state mounted. Tests cover both background rotation directions, same-orientation return, delayed Dimensions, stale measurement callbacks, ignored background measurements, coalesced frame reads and unmount cleanup. Existing UI, drawable, cutout and playback-polling regressions plus TypeScript pass. Tailscale bundle contains the fix and reload returned HTTP 200. Device validation of the reported transition is pending; no commit/push/build. This addresses app layout synchronization, not the OS's app-switcher snapshot animation.


## 2026-10-18: correct the resume regression and tab-state feedback

User reported worse transitions with duplicate/mismatched headers, page content and selected tabs. The prior recovery patch was incomplete: it immediately cleared measured geometry on inactive/active transitions, then allowed a live read with the old orientation to replace the newly selected window orientation. It also used measure(), whose installed RN renderer implementation includes transforms. Native tab callbacks were unconditionally routed to the app even when emitted for implicit or programmatic selection, allowing the secondary controller's initialization to change the actual page.

Keep a committed display size while inactive, record window changes without reflowing the snapshot, and accept two matching untransformed native layout reads on resume before committing one new display size. Prefer Fabric offsetWidth/offsetHeight; use measureLayout relative to the same view as a transform-free compatibility fallback. Never call measure() for presentation sizing. Reject old epochs and out-of-order reads; retain normal foreground resize behavior. Retries are bounded animation-frame reads, not fixed waits or fade masks. Freeze the visible cutout-side choice until layout readiness returns.

Both native tab controllers now acknowledge monotonic provenance but only active, user-origin events may invoke app navigation. Implicit/programmatic acknowledgements and stale events cannot select a different page. Tests cover the failing old/new/old sequence, frozen inactive geometry, transform-free read APIs, reversed callback order, same/opposite-orientation resumes and initialization/sync events that previously fed back into app state. Existing UI, native-icon, drawable and playback-polling checks plus TypeScript pass. Live Tailscale bundle verified and full reload requested (HTTP 200). Device validation remains pending. No commit/push/build and no change to audio or the approved side-bar appearance.


## Foreground rotation and correct preview reload transport

User accepted background restoration but reported that switching inside the app still failed; this turn explicitly interpreted that as foreground portrait/landscape rotation. Extend the existing stable-layout handoff to active orientation changes regardless of event order (Dimensions first or root layout first). Set settling and invalidate layout readiness on that transition so the established cutout-side freeze and scene-render gate also apply. Same-orientation normal resizing stays on its previous path. Regression checks cover both directions, native-layout-first ordering and quickly reversing rotation while rejecting stale reads. Background-resume tests remain passing. No tab visual/selection behavior was changed in this turn.

Critical correction: POST /reload on this Expo CLI returns an HTML fallback page with HTTP 200; it does NOT prove or trigger a full native reload. Earlier notes that inferred reload from that HTTP status were incorrect. Hot Refresh can preserve the layout controller stored in useRef, so subsequent tests may still execute old controller logic without a real reload. Use the installed CLI's actual local WebSocket message protocol instead: connect to ws://127.0.0.1:8084/message, request {version:2,id:"before",target:"server",method:"getpeers"}, then broadcast {version:2,method:"reload"}. The intended backend/local-client route validates local socket/origin; no security setting is changed. This turn found two connected preview clients and received Metro's reload broadcast echo. The socket IDs remained unchanged afterward, so this confirms broadcast, not device reload completion. Do not reuse POST /reload as a verification method.

Served bundle contains the foreground rotation update; TypeScript and both resume/foreground regression scenarios pass. No commit/push/build. User device confirmation of foreground rotation is pending.

## 2026-10-18: tab switching (user clarification; not device rotation)

The user clarified that the remaining animation problem occurs when selecting tabs. Found a selection-dependent structural change: the shared Tabs.Host used to sit beneath a conditional outer header, and the outer safe-area edges changed to [] for the library only. Library gutters also depended on whether the library was currently selected. During native tab transitions this resized the whole host and changed the incoming/outgoing page geometry.

Move playback/space headers into their own native tab pages, with stable per-page top/side safe areas. The shared UIKit host now always occupies the same outer safe-area envelope. The library retains its native navigation header and its existing gutters even while another tab is selected. Header titles/colors are derived from the owning page index. Compatibility and Android paths retain their external header. Keep the accepted background-resume handling, native side-bar icons/geometry and native selection semantics unchanged.

Added regression coverage for repeated 0/1/2/1/0 selections in portrait and both side-bar positions, asserting stable host/page/header geometry. Updated existing native viewport assertions for the page-owned safe-area wrapper. Native-tab, mobile UI, library header, surface recovery, vertical native bar, side dock/gesture/cutout checks and TypeScript pass. Served Tailscale development bundle contains the change. Production minified bundle compiles and excludes the three preview fixture markers. Metro getpeers returned one iOS-role peer; the proper WebSocket reload broadcast was echoed. No inspector target was connected, so no on-device animation validation is claimed. Keep this preview pending user review; no commit/push/build in this turn.

## 2026-10-18: accepted UI release

User confirmed tab switching is now acceptable and authorized committing all accepted UI work and building iOS. The approved native side tabs, theme/drag colors, cutout-aware dock placement, stable background/foreground layout handling and page-owned tab headers are the release UI reference. Existing preview/release parity contract remains in force; preview fixtures stay development-only. Supplementary native-page/background/artwork/theme/playback/queue/session/drawable/head-tracking/resolver/diagnostics checks all pass. The unrelated offline Atmos probe edit remains outside this UI release commit.
