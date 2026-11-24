#!/usr/bin/env python3
"""Draws the BalanceCraft icon and writes every size the packages need.

The icon is original artwork made for BalanceCraft: a balance on a green tile,
drawn with Pillow shapes (no fonts, no third-party art). Outputs, committed:

  assets/app-icon/balancecraft.svg        reference drawing (same geometry)
  assets/app-icon/hicolor/<n>x<n>/apps/it.mauromazzieri.balancecraft.png
  assets/app-icon/balancecraft-1024.png   source of the macOS .icns (iconutil, at packaging time)
  assets/app-icon/balancecraft.ico        Windows (16-256)
  assets/app-icon/window-64.rgba          raw RGBA for the window icon (include_bytes!)

Run from the repository root: python3 tools/icons.py
"""
from pathlib import Path
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "app-icon"
APP_ID = "it.mauromazzieri.balancecraft"
S = 1024  # design grid
BG = (46, 107, 78, 255)
FG = (255, 255, 255, 255)
ACCENT = (242, 201, 76, 255)


def draw(size_px: int, margin: float = 0.0) -> Image.Image:
    """The icon at `size_px`, drawn at 4x and reduced for smooth edges.
    `margin`: transparent border as a fraction of the side (macOS grid)."""
    k = 4
    big = S * k
    img = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    m = int(big * margin)
    side = big - 2 * m
    u = side / S  # design unit

    def p(x, y):
        return (m + x * u, m + y * u)

    d.rounded_rectangle([p(0, 0), p(S, S)], radius=int(220 * u), fill=BG)
    w = lambda n: max(1, int(n * u))
    # post and base
    d.rectangle([p(496, 250), p(528, 790)], fill=FG)
    d.rounded_rectangle([p(352, 780), p(672, 830)], radius=w(24), fill=FG)
    # beam and pivot
    d.rounded_rectangle([p(212, 284), p(812, 316)], radius=w(16), fill=FG)
    d.ellipse([p(478, 238), p(546, 306)], fill=ACCENT)
    # strings and pans
    for cx in (262, 762):
        d.line([p(cx, 300), p(cx - 110, 560)], fill=FG, width=w(14))
        d.line([p(cx, 300), p(cx + 110, 560)], fill=FG, width=w(14))
        d.chord([p(cx - 150, 470), p(cx + 150, 650)], start=0, end=180, fill=FG)
    return img.resize((size_px, size_px), Image.LANCZOS)


SVG = f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {S} {S}">
  <!-- BalanceCraft icon: reference drawing of tools/icons.py (MIT OR Apache-2.0). -->
  <rect width="{S}" height="{S}" rx="220" fill="#2E6B4E"/>
  <g fill="#FFFFFF">
    <rect x="496" y="250" width="32" height="540"/>
    <rect x="352" y="780" width="320" height="50" rx="24"/>
    <rect x="212" y="284" width="600" height="32" rx="16"/>
    <path d="M112 560 A150 90 0 0 0 412 560 Z"/>
    <path d="M612 560 A150 90 0 0 0 912 560 Z"/>
  </g>
  <g stroke="#FFFFFF" stroke-width="14">
    <path d="M262 300 L152 560 M262 300 L372 560 M762 300 L652 560 M762 300 L872 560"/>
  </g>
  <circle cx="512" cy="272" r="34" fill="#F2C94C"/>
</svg>
"""


def mobile() -> None:
    """The phone apps' icons; Apple's has no transparency and no rounded corners."""
    res = ROOT / "mobile" / "android" / "app" / "src" / "main" / "res"
    for name, n in (("mdpi", 48), ("hdpi", 72), ("xhdpi", 96), ("xxhdpi", 144), ("xxxhdpi", 192)):
        d = res / f"mipmap-{name}"
        d.mkdir(parents=True, exist_ok=True)
        draw(n, margin=0.04).save(d / "ic_launcher.png", optimize=True)
    ios = ROOT / "mobile" / "ios" / "BalanceCraft" / "Assets.xcassets" / "AppIcon.appiconset"
    square = Image.new("RGB", (S, S), BG[:3])
    icon = draw(S)
    square.paste(icon, mask=icon.getchannel("A"))
    square.save(ios / "icon-1024.png", optimize=True)


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "balancecraft.svg").write_text(SVG)
    for n in (16, 24, 32, 48, 64, 128, 256, 512):
        d = OUT / "hicolor" / f"{n}x{n}" / "apps"
        d.mkdir(parents=True, exist_ok=True)
        draw(n).save(d / f"{APP_ID}.png", optimize=True)
    # Apple's grid leaves a transparent margin around the tile.
    draw(1024, margin=100 / 1024).save(OUT / "balancecraft-1024.png", optimize=True)
    draw(256).save(OUT / "balancecraft.ico", sizes=[(n, n) for n in (16, 20, 24, 32, 40, 48, 64, 128, 256)])
    (OUT / "window-64.rgba").write_bytes(draw(64).tobytes())
    mobile()


if __name__ == "__main__":
    main()
