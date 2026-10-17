# Android LiquidGlass UI parity — 2026-10-18

## Baseline and scope

The accepted iOS/Expo Go UI baseline is `e0984ae`. This change ports its layout and interactions to Android using the already integrated Kyant0/AndroidLiquidGlass 1.0.6 sources. Android keeps the existing character-derived Material palette for page backgrounds. The audio pipeline and rendering presets are not changed by the UI patch.

A dedicated verification checkout, `SDA-android-ui-20261018`, contains that baseline and only this UI patch. The integration checkout has concurrent audio-format work, so verification does not use those uncommitted changes.

## Components and interactions

- Native vertical `LiquidSideTabs`/`LiquidSideTab` are adaptations of the upstream bottom tabs, with upright icons, vertical layout/hit testing and the original backdrop/lens/highlight/content-tint passes. The common capsule owns drag gestures, including the selected overlay. Cancellation restores the committed tab. Original portrait bottom tabs remain available.
- Android phones now use the shared compact landscape player columns. Side navigation is opt-in in settings, with persisted left/right position (right by default). Native Activity orientation is unspecified instead of locked portrait. Top settings, library viewport, scene and mini-player reserve the chosen side and system insets.
- Mini-player can render alone in landscape. Its expand and play/pause actions remain separate. Library content continues behind the glass and has a scrollable tail; Android's actual viewport reserves the side rail because iOS scroll-indicator inset props do not position Android scrollbars.
- Native headers in library and settings blur their captured background after scrolling. Titles remain attached to their pages. Android metadata uses Compose ModalBottomSheet with partial/full expansion, outside/back/swipe dismissal, hosted React content and a themed LiquidButton to finish.
- Settings switches use the upstream `LiquidToggle`, with explicit theme/accent, disabled-state checks, an expanded touch target, accessible switch state/actions and cancellation handling. Busy controls do not write playback settings.
- iOS implementations and accepted docking geometry remain platform-gated. Source-level and component adapter regressions cover them alongside Android.

## Build provenance

Local checked-in JNI binaries predate the shared HRTFHQ engine and correctly fail the packaging guard. The verification checkout stages the native binaries from successful Android CI run `39050894996`, commit `e0984ae9b47a27fe214c81f3e8deecc0a4c7c9dc`. Artifact `12107948148` contains that baseline APK. The unmodified x86_64 renderer SHA-256 is `c9dc15075974b199e45cb86d9401a43134e3b64ffb82e0593672e1766d1ea6f6`; ARM64 is `d57a1f95f3ad5e191aff2fb6a7832883a94df4a51476816c262efdfa4e7dd16d`. The full provenance record is in the local validation directory.

Local Windows build uses non-incremental, in-process Kotlin compilation to avoid cross-drive incremental-cache errors. Generated prefab/native build paths are relocated to `C:/SDA-ui-build` with a local Gradle init script to avoid pnpm physical paths exceeding Windows process path limits. These local environment settings do not alter the app or CI dependencies.

## Validation status

TypeScript, iOS UI checks, native-tab selection/geometry, library header, side docking, Android layout/adapters and settings checks pass. Native build, installed-app interaction and screenshot verification are in progress; this document must be updated with their actual results before declaring device validation complete.
