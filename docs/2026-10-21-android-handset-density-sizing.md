# Android handset sizing and display density

The project owner reported oversized player text and header buttons on a realme GT 7 Pro. The original player shared a 24-unit page title, 20-unit song title and 19-unit artist text with iOS; each glass button was 44 dp across. The artist text was almost the same size as the song title.

## Reference and units

- Android density guidance: https://developer.android.com/training/multiscreen/screendensities
- React Native density and font scale: https://reactnative.dev/docs/pixelratio
- Compose minimum touch targets: https://developer.android.com/develop/ui/compose/accessibility/api-defaults
- realme panel specification: https://www.realme.com/global/realme-gt-7-pro/specs

The official realme specification lists a 6.78-inch, 2780 × 1264 panel, giving approximately 450 PPI from its diagonal. This physical density is not the user's current Android logical density. A resized screenshot's image DPI metadata is not an Android density measurement.

React Native layout measurements and Compose dp controls already account for device density. Player text keeps the platform's font scaling; multiplying these sizes again by PixelRatio, or deriving font sizes from panel PPI, would apply the wrong conversion. No such extra multiplication was found in these controls.

## Change

For Android handset windows only, use a 20-unit page title, 18-unit song title with a 24-unit line height, and 15-unit artist text with a 21-unit line height. The SDA eyebrow is 10 units. Font scaling remains enabled. This is a common logical-size rule, without model IDs, panel PPI, density-based font multiplication, or a system-density override.

The handset glass circles are 36 dp with 20 dp icons inside 48 dp native views. Compose expands the clickable area into that available touch lane. Both header actions use the same compact policy. Non-handset and settings/back-button defaults retain their existing 44 dp presentation. The handset check uses the actual logical window size even if the user opts into the tablet-style player on a phone.

The patch does not change the decoder, audio gains, rendering profile, iOS sizing, playback controls or bottom tabs. The GT 7 Pro's current runtime DPI and font scale remain unmeasured; verification on an emulator is reported separately and is not a claim of physical-device acceptance.

## Theme selector and header spacing

Android theme selection uses the Android framework's android.widget.PopupMenu. The settings page shows only the current color/name; a tap opens the platform menu, anchored at the right disclosure indicator with END gravity. A platform exclusive-checkable group tracks the selected theme. Built-in light/dark framework themes control appearance, and public API29+ setForceShowIcon retains color swatches. API26–28 keep the framework's default icon visibility. Selecting closes the menu and persists the existing theme preference. Without the native module, the existing selection dialog remains available.

This is distinct from the earlier Compose Material3 DropdownMenu implementation. The framework revision does not set a custom popup background and does not use hidden-API reflection. The invisible anchor does not capture backdrop frames; dismissal also follows native Back, outside-touch and view-detach behavior.

References: https://developer.android.com/reference/android/widget/PopupMenu and https://developer.android.com/reference/android/view/Menu#setGroupCheckable(int,boolean,boolean)

Remove the compact header action's extra 8 dp trailing margin. The two 48 dp touch lanes are adjacent, giving 48 dp between their centers and approximately 12 dp between the 36 dp circles, while preserving non-overlapping touch targets.

## Popup focus and current-theme propagation

A focused popup does not hide the activity window. Chrome now preserves its render node/composition when input focus is lost, and visible backdrop sampling no longer depends on input focus. Returning focus still refreshes interaction state. This fixes the disappearing settings-header blur for focusable popup windows generally.

The framework popup receives the current application primary control color. Its colorAccent/colorControlActivated/colorControlNormal overlays are generated at build time from android-material-schemes.json for all themes and both light/dark modes, avoiding a second manual palette. Platform popup geometry/background are retained. The resolved activated color is checked against the bridge's current primary color.

The native anchor keeps a fill-size layout node and does not recreate its empty composition on popup focus transitions. A new trigger click always reaches the framework; a delayed JS expanded/accessibility update cannot suppress reopening. Native request tokens still prevent a palette-only rerender from reopening a dismissed menu.

The application base theme declares android:textColor as black. Popup-only palette overlays explicitly supply textColor/textColorPrimary from the palette ink, and textColorSecondary from muted, so platform menu labels do not inherit black in dark mode. Both light and dark popup foreground colors are tested from installed screenshots.
