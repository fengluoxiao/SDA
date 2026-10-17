"""Generate native tab item templates: inverse rotation is baked into pixels.
Upright alpha masks retain the three glyph silhouettes from user-supplied
on-device screenshots. No runtime image analysis or icon cache is needed.
"""
from pathlib import Path
from PIL import Image
root = Path(__file__).resolve().parents[1] / "apps/mobile/assets/side-tabs"
for name in ["play", "library", "space"]:
    source = Image.open(root / "sources" / (name + ".png")).convert("RGBA")
    rotated = source.transpose(Image.Transpose.ROTATE_90)
    # The native horizontal item needs 44 pt of intrinsic width. Rotating the
    # bar makes this the vertical item pitch. Pad rather than stretch the glyph.
    for scale, suffix in [(1, ""), (2, "@2x"), (3, "@3x")]:
        glyph = rotated.resize((24 * scale, 24 * scale), Image.Resampling.LANCZOS)
        item = Image.new("RGBA", (44 * scale, 24 * scale), (255, 255, 255, 0))
        # Device reference: glyph center sits ~3 pt below the native selection.
        # Shift left before the bar rotates clockwise, yielding 3 pt upward.
        item.paste(glyph, (7 * scale, 0))
        item.save(root / (name + "-ccw" + suffix + ".png"))
