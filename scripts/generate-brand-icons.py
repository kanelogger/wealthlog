"""Generate Wealthlog brand assets from the approved piggy-bank artwork.

The approved source image is user-supplied artwork and is not committed to the
repository. Provide its path explicitly:

    python3 scripts/generate-brand-icons.py path/to/piggy-bank-v2.png

Preserve the selected image's composition, rounded tile and transparency.
"""

import sys
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
PUBLIC = ROOT / "apps/frontend/public"
NATIVE = ROOT / "apps/tauri/icons"
RESAMPLE = Image.Resampling.LANCZOS


def main():
    if len(sys.argv) != 2:
        raise SystemExit("usage: python3 scripts/generate-brand-icons.py <source-image>")
    artwork = Image.open(sys.argv[1]).convert("RGBA")
    if artwork.width != artwork.height:
        raise ValueError("Brand source must be square")

    # The user selected this exact image, including its existing rounded tile.
    # Do not redraw it, add a second mask, or change the pig's proportions.
    artwork.resize((256, 256), RESAMPLE).save(PUBLIC / "logo.png", optimize=True)
    native = artwork.resize((1024, 1024), RESAMPLE)
    native.save(NATIVE / "icon.png", optimize=True)
    native.save(NATIVE / "icon.icns", format="ICNS")
    native.save(
        NATIVE / "icon.ico", format="ICO",
        sizes=[(n, n) for n in (16, 24, 32, 48, 64, 128, 256)],
    )

    # Touch/PWA icons require an opaque background. Composite alpha onto ivory
    # explicitly, so transparent pixels never become a black rectangle.
    opaque = Image.new("RGBA", artwork.size, "#fffdf7")
    opaque.alpha_composite(artwork)
    opaque = opaque.convert("RGB")
    for filename, pixels in (
        ("app-icon-192.png", 192),
        ("app-icon-512.png", 512),
        ("apple-touch-icon.png", 180),
    ):
        opaque.resize((pixels, pixels), RESAMPLE).save(PUBLIC / filename, optimize=True)
    maskable = Image.new("RGB", (512, 512), "#fffdf7")
    # All identifying features fit in the central 80%-diameter safe circle.
    content_size = 360
    offset = (512 - content_size) // 2
    maskable.paste(opaque.resize((content_size, content_size), RESAMPLE), (offset, offset))
    maskable.save(PUBLIC / "app-icon-maskable-512.png", optimize=True)


if __name__ == "__main__":
    main()
