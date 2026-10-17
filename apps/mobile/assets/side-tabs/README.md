# Native side-tab template icons

These transparent templates belong to the real UIKit tab items. There is no independent icon overlay. The artwork is rotated 90 degrees counter-clockwise so the clockwise-rotated native bar displays upright glyphs. UIKit applies the current tab tint and native highlighted/selected state.

The upright alpha masks preserve the glyph silhouettes from user-provided on-device references: play and music/list from codex-clipboard-edce2074-4f53-4c59-bc25-d5aaa0cb330c.jpg, and space/cube from codex-clipboard-380f96d2-dc24-4d61-8148-a39b014cee3f.jpg. Full screenshots are not included. sources/*.png are reproducible 3x alpha-mask inputs; only *-ccw PNG variants are imported by the app.

Run `python scripts/generate-side-tab-icons.py` to regenerate the 1x/2x/3x 44×24-point templates. `scripts/test-ios-vertical-tab-bar.mjs` checks nonempty glyph alpha and exact inverse rotation of the 3x images. Each template has 7 points of transparent leading padding and 13 points of trailing padding around the original 24-point glyph canvas. This shifts the visible glyph upward by 3 points after clockwise rotation, matching the device reference selection center. This gives the rotated native item 44 points of vertical occupancy without stretching the glyph. No runtime image analysis or asset generation is used.
