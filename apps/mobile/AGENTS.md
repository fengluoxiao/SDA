# Mobile UI release contract

- The user-approved Expo Go preview is the UI acceptance reference. iOS release builds must use the same layout, colors, native controls, navigation, safe-area behavior and interactions on the same supported OS/device/settings.
- This applies to all future bug fixes, optimizations and packaging work. Do not select a different visual implementation merely because SdaEngine/SdaGlass or another standalone-only module is available. Do not add release-only UI effects that were absent from the approved preview.
- Expo Go mock tracks/artwork/metadata are development-only and must never ship as runtime sample data. Real audio/native playback may differ by capability; those differences must not silently replace the approved UI.
- Preserve the approved UIKit page headers, transparent landscape side/bottom regions, safe floating mini-player, and system light/dark inactive tab icons with theme-colored selected icons.
- For UI changes, run the relevant mobile UI/layout checks and TypeScript checks. Verify production bundles exclude mock fixtures. Device visual review is required for intentional appearance changes; passing code tests alone is not visual acceptance.
- Show intentional UI changes in Expo Go before submitting a release build unless the user has explicitly authorized that change/build in the current task. Never claim device parity was visually verified without testing the installed release.
