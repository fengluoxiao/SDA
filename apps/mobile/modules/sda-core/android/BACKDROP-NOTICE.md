# Backdrop Catalog Source

The Kotlin files under `src/main/java/com/kyant/backdrop/catalog/` are derived
from Kyant0/AndroidLiquidGlass tag 1.0.6, distributed under Apache-2.0.

Source: https://github.com/Kyant0/AndroidLiquidGlass/tree/1.0.6/catalog

The complete upstream license is in BACKDROP-LICENSE.txt.

Local modifications: explicit SDA theme/accent parameters and stable external
selection observation. The host provides its existing native background bridge.

LiquidButton is also derived from the same tag. Local additions provide enabled
state, an accessibility action label, configurable height and content padding
for SDA's song area and separate playback control.

LiquidSideTabs and LiquidSideTab are vertical adaptations of the same 1.0.6
BottomTabs/BottomTab sources. Layout, hit testing, dragging and the tint capture
pass use vertical coordinates; native icons remain upright. Cancellation restores
the committed selection. LiquidToggle is copied from tag 1.0.6 with explicit
theme/accent, enabled state, fresh callbacks and accessibility switch actions.
The source license continues to apply to all adapted files.


## Community LiquidMenu demo

SdaSpatialContextMenu.kt adapts Aarav90-cpu's LiquidMenu.kt / LiquidMorphDemo
snippet posted in AndroidLiquidGlass issue 70 (2025-12-20):
https://github.com/Kyant0/AndroidLiquidGlass/issues/70

The exact retrieved snippet is archived with the local preview delivery as
LiquidMenu.upstream.kt. This is a community demo, not an official Backdrop
component. The original spring constants, continuous-corner morph, 12dp blur,
24dp/32dp chromatic lens, 20% white fill, 40% white border are retained; SDA adapts the menu geometry to the supplied iOS reference. Per the user's follow-up,
blue option fills are removed and option labels/checkmarks are black in light
mode and white in dark mode. Options are left aligned with a reserved checkmark
slot, so selected and unselected labels share the same starting position. SDA supplies the live background, a 44dp trigger, two real audio
choices, checked semantics, a focusable overlay and dismissal/lifecycle handling.
The older com.kyant.capsule continuous shape API is adapted to the installed
com.kyant.shapes RoundedRectangle(Continuous) API.

The approved iOS menu reference is used for compact SDA layout: 224dp width,
two 44dp option rows with 8dp vertical insets (104dp expanded height), 17sp
regular labels and 16dp horizontal insets. Demo spring/lens/morph remain.
