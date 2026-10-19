# SDA app icons

The canonical mark is the existing Windows `apps/desktop/assets/icon.svg` /
`icon.png`. No new symbol design is introduced.

- `app-icon.png`: canonical 1024px PNG flattened onto #202223 (opaque RGB for iOS).
- `splash-icon.png`: unchanged canonical desktop PNG for the existing splash placement.
- `adaptive-icon.png`: original SVG paths on a transparent 1024px canvas, scaled to
  two thirds around the centre for Android adaptive-mask safe placement.
- `monochrome-icon.png`: same paths/padding in white for Android themed icons.

Android launcher/round/adaptive/monochrome resources are generated using the
project-pinned Expo icon generator. `app.json` keeps prebuild output consistent.
The iOS CI verifies the generated AppIcon pixels against the configured source.
