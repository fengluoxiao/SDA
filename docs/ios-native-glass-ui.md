# iOS native controls and Liquid Glass (October 3, 2026)

SDA keeps Expo 52 / React Native 0.76 and adds three iOS-only Expo view
managers to the existing SdaCore pod. No audio, import or signing change.

- GlassButtonControl: UIKit UIButton, SF Symbols and `.glass()` / 
  `.prominentGlass()` on iOS 26; gray/filled native buttons on iOS 16–25.
- GlassTabsControl: interactive regular UIGlassEffect in a native
  UIVisualEffectView; three native buttons report page changes to JS. The effect
  is installed only once the mounted view has a nonzero layout. Previous iOS
  versions use systemChromeMaterial; Reduce Transparency uses an opaque surface.
- MaterialSurfaceControl: systemMaterial for the settings sheet, with an opaque
  accessibility fallback. Large content cards/3D canvas are not made glass.
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
Separate scene/player/settings launches save native-material reports and
screenshots. The previous audio regression and nonblank 7.1.4 image checks stay
required. Reports alone are not proof of appearance: inspect all screenshots.
Actual AirPods listening, physical touch/VoiceOver/Reduce Transparency and old
physical iOS releases require device checks; SDK 27 is not validated when absent.
