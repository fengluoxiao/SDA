# Android LiquidGlass UI parity — 2026-10-18

## Baseline and scope

The accepted iOS/Expo Go UI reference is `e0984ae`. The final integration parent is `98dd1cd` on `fengluoxiao/SDA:feat/android-port`, including its Audio Vivid support. This change ports its layout and interactions to Android using the already integrated Kyant0/AndroidLiquidGlass 1.0.6 sources. Android keeps the existing character-derived Material palette for page backgrounds. The audio pipeline and rendering presets are not changed by the UI patch.

A dedicated verification checkout, `SDA-android-ui-20261018`, contains the current integration parent and this UI patch. The integration checkout has concurrent audio-format work, so verification does not use those uncommitted changes.

## Components and interactions

- Native vertical `LiquidSideTabs`/`LiquidSideTab` are adaptations of the upstream bottom tabs, with upright icons, vertical layout/hit testing and the original backdrop/lens/highlight/content-tint passes. The common capsule owns drag gestures, including the selected overlay. Cancellation restores the committed tab. Original portrait bottom tabs remain available.
- Android phones now use the shared compact landscape player columns. Side navigation is opt-in in settings, with persisted left/right position (right by default). Native Activity orientation is unspecified instead of locked portrait. Top settings, library viewport, scene and mini-player reserve the chosen side and system insets.
- Mini-player can render alone in landscape. Its expand and play/pause actions remain separate. Library content continues behind the glass and has a scrollable tail; Android's actual viewport reserves the side rail because iOS scroll-indicator inset props do not position Android scrollbars.
- Native headers in library and settings blur their captured background after scrolling. Titles remain attached to their pages. Android metadata uses Compose ModalBottomSheet with partial/full expansion, outside/back/swipe dismissal, hosted React content and a themed LiquidButton to finish.
- Settings switches use the upstream `LiquidToggle`, with explicit theme/accent, disabled-state checks, an expanded touch target, accessible switch state/actions and cancellation handling. Busy controls do not write playback settings.
- iOS implementations and accepted docking geometry remain platform-gated. Source-level and component adapter regressions cover them alongside Android.

## Build provenance

Local checked-in JNI binaries predate the shared HRTFHQ engine and correctly fail the packaging guard. The verification checkout stages unmodified native libraries from successful Android CI run `39057126416`, commit `98dd1cd2feeab34eca08a052a45caf1ce1850bbf`, artifact `12110307595`. The x86_64 SHA-256 is `6c4f340d2f53a75e7ed7d87287ef31931fbcb406e87aa43d8d02a85b9e756be9`; ARM64 is `523ba55b53ff0ed204858d70c1a7a4a13a4576c0f99af4f0f0f6062542b78ddc`. This includes the current Audio Vivid engine. The full provenance record is in the local validation directory. CI builds both libraries from source as usual; these local staging files are not part of the UI commit.

Local Windows validation uses a separate frozen-lockfile pnpm 9.15.9 hoisted dependency workspace at `C:/SDA-ui-deps` because the shared pnpm physical paths exceeded Ninja/Windows path limits. Kotlin runs in-process without incremental compilation. A local Gradle init script uses short native build staging directories and `CMAKE_OBJECT_PATH_MAX=240`, without relocating generated RN codegen sources. These environment settings do not change app/CI dependencies. Earlier attempts to relocate all build outputs were withdrawn after codegen/autolinking path errors.

## Validation status

TypeScript, iOS UI checks, native-tab selection/geometry, library header, side docking, Android layout/adapters and settings checks pass. The local x86_64 Release build succeeded and was installed with `adb install -r` to Android 15 / API 35 MuMu instance 0 (`127.0.0.1:16384`). SDA launched successfully. Screenshots show the landscape player, native settings header and glass toggles, and the left vertical rail. No AndroidRuntime/ReactNativeJS error appeared in the checked recent logs. Uiautomator exits 139 on this emulator, so automated accessibility-tree inspection was unavailable; full on-device drag/drawer validation is not claimed.

The actual APK passed `verify-android-render-experiment.py`, matches the x86_64 native library SHA above, includes the Backdrop license and excludes preview mock markers. APK SHA-256: `a5e5684d7cb3004c3c50db95b3585ac72823f37c70b2b7fda74c957904ab474c`.

The user subsequently requested local Android builds and MuMu delivery only, with no further commits/pushes/remote builds. Commit `30d5d87` had already been pushed to `feat/android-port` before that restriction. Its two CI runs had already ended in failure when checked, so no active run needed cancellation. The navigation-preference test still evaluated the new Android gate without providing `hasAndroidChrome`; that test harness was corrected locally and now covers Android with/without the native chrome, iOS and web. This test fix and the final validation notes remain uncommitted, per the user's instruction. No CI rerun was requested.


## Local follow-up: shared settings/tab material

User identified a material mismatch between settings and navigation. Buttons had transparent fill with 2dp blur; tabs had a 40% neutral fill and 8dp blur. Added one shared light/dark neutral surface and blur constant for bottom/side navigation and settings/back controls. The settings button now receives the system glass appearance separately from its foreground-contrast flag, so white player icons do not incorrectly select a dark surface in light mode. Button geometry, native gestures and icon/theme tint remain as before.

TypeScript, Android chrome/settings regressions and the new foreground-vs-system appearance matrix pass. Local Release build succeeded, then adb update/install and launch succeeded on Android 15 MuMu. The light-mode screenshot `glass-unified-light.png` shows the matching base material. Dark mode is covered by parameter tests; a physical dark-mode screenshot was not taken in this checkpoint. Updated APK SHA-256: `98c254cccedc000aa9003279e89bc8972f49bd7744680be89e64d0ea89b6b97d`. All follow-up changes are local and uncommitted; no CI was triggered.


## Local follow-up: system-appearance settings icon

Settings/back icons now follow the system appearance directly: dark glyphs in light mode and light glyphs in dark mode, including the immersive player. Removed the caller foreground override from AndroidGlassSettingsButton and its callers; the shared glass material remains unchanged. TypeScript, Android chrome/settings and iOS UI regressions pass. Local Release build succeeded, followed by update/install on Android 15 MuMu. Actual screenshots `settings-icon-light.png` and `settings-icon-dark.png` verify both appearances; the original system night mode was restored to `no`. Updated APK SHA-256: `e2702aeb1e80a545837aacb6bdc3260997a6f346edb91a1dcb14e8ebdd35b727`. No commit, push or CI trigger was performed.


## Local follow-up: actual system-bar underlay

The previous transparent window still contained a top/bottom SafeAreaView spacer. The settings scroll viewport therefore never reached the status bar, and its native blur only covered the title. Native Android chrome now consumes left/right safe edges at the root; header controls reserve the top inset inside one full-height native blur surface, and scroll content extends underneath it. Playback/scene headers retain their original visible positions through per-header inset padding. Bottom tab/mini controls reserve navigation insets individually; scroll-tail padding accounts for the same inset, preserving the list's original maximum scroll range.

The native window and theme request transparent status/navigation bars, disable navigation contrast scrims, and clear the navigation divider. Navigation-key appearance follows the active page's contrast through the native module. Selection modals use translucent status/navigation windows too. iOS edge handling remains unchanged.

TypeScript, Android UI/settings, iOS UI, navigation-preference and surface-resume checks pass. The Android layout matrix covers top/bottom insets of 0/24/48dp, unchanged scroll range, floating-control clearance and full status+title background height. Local Release build and installation on Android 15 MuMu succeeded. Actual scrolling screenshots show the status area responding to underlying content: the old sampled top background had one flat color; the new sampled area changes between scrolling frames. MuMu reports `qemu.hw.mainkeys=1` and hides the native navigation bar, so actual three-button/gesture-bar visual coverage is not claimed on this emulator. Those bottom paths have window/configuration and inset/layout verification.

APK SHA-256: `a3101e9cd5aed85d84b8d8fd50e9ea92264debf497344eaf22eb0b5841d7f1c5`. This follow-up is local and uncommitted; no push or CI trigger.
