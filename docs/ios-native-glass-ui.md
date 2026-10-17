# iOS native controls and Liquid Glass (October 3, 2026)

SDA keeps Expo 52 / React Native 0.76 and adds five iOS-only Expo view
managers to the existing SdaCore pod. No audio, import or signing change.

- GlassButtonControl: UIKit UIButton, SF Symbols and `.glass()` / 
  `.prominentGlass()` on iOS 26; gray/filled native buttons on iOS 16–25.
- GlassTabsControl: interactive regular UIGlassEffect in a native
  UIVisualEffectView; three native buttons report page changes to JS. The effect
  is installed only once the mounted view has a nonzero layout. Previous iOS
  versions use systemChromeMaterial; Reduce Transparency uses an opaque surface.
- MaterialSurfaceControl: systemMaterial for the settings sheet, with an opaque
  accessibility fallback. Large content cards/3D canvas are not made glass.
- All three pages use the iOS grouped palette and consistent content spacing.
  Library rows and rendering/room selections are native plain UIButton rows with
  SF Symbols, selected state, subtitle and full VoiceOver labels. Content is not
  covered by unnecessary glass. Import, replay and stop use native glass actions.
- Playback mode uses a native UIMenu, with existing mode IDs and persistence.
- Volume uses UISlider with SF speaker symbols. Native tracking start/end/cancel
  feeds the existing scroll locks; frequent React updates cannot reset the thumb
  while tracking. Progress remains read-only (the engine does not support seeking).
- Distance mapping uses UIStepper (0.25–4 m, 0.05 m increments). The existing
  near-field handler and output-route disabled states are preserved.
- Settings switches remain React Native Switch, which is already UISwitch on iOS.
  The sheet retains the proven Modal/drag/scroll implementation, rather than
  rewriting navigation/audio lifecycle. This is a native-control integration,
  not a wholesale SwiftUI rewrite.
- Native button accessibility labels/selection/disabled states are owned by
  UIKit. The visualizer gesture locks and the settings header drag remain intact.
- Android and older installed binaries/Expo Go retain the prior JS controls.

Apple documentation checked during implementation:
- UIGlassEffect (introduced iOS 26.0)
- UIGlassEffect.isInteractive
- UIButton.Configuration.glass / prominentGlass (introduced iOS 26.0)
Expo native-view adapter and GlassView sources were checked for view bridging
and the delayed effect-installation requirement.

CI typechecks the UIKit-only implementation against the installed SDK with an
 iOS 16 deployment target, then builds all Expo adapters in the full app.
Separate scene/player/library/settings launches save native-material reports and
screenshots. The previous audio regression and nonblank 7.1.4 image checks stay
required. Reports alone are not proof of appearance: inspect all screenshots.
Actual AirPods listening, physical touch/VoiceOver/Reduce Transparency and old
physical iOS releases require device checks; SDK 27 is not validated when absent.
