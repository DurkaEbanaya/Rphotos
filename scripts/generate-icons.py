#!/usr/bin/env python3
"""Reproduce the original Rphotos icon at native OS sizes; requires Pillow.

Geometry mirrors assets/rphotos.svg. No upstream/Microsoft artwork is reused.
Run from any directory: python3 scripts/generate-icons.py
"""
from pathlib import Path
from PIL import Image, ImageDraw


def render(size):
    # Supersample so small tray/taskbar sizes retain smooth geometry.
    scale = max(2, 1024 // size)
    factor = size * scale / 512
    image = Image.new("RGBA", (size * scale, size * scale))
    draw = ImageDraw.Draw(image)

    def box(coords):
        return tuple(round(v * factor) for v in coords)

    draw.rounded_rectangle(box((24, 24, 488, 488)), radius=round(104 * factor), fill="#0078d7")
    draw.rectangle(box((108, 112, 358, 344)), fill="#80c8ff")
    draw.rectangle(box((144, 148, 404, 400)), fill="white")
    draw.rectangle(box((164, 168, 384, 320)), fill="#dcefff")
    draw.ellipse(box((308, 186, 352, 230)), fill="#0078d7")
    draw.polygon([box(p) for p in ((164, 320), (236, 224), (292, 292), (332, 252), (384, 320))], fill="#0078d7")
    draw.line(box((172, 364, 276, 364)), fill="#0078d7", width=round(16 * factor))
    return image.resize((size, size), Image.Resampling.LANCZOS)


if __name__ == "__main__":
    assets = Path(__file__).resolve().parent.parent / "assets"
    render(256).save(assets / "rphotos.png")
    render(256).save(assets / "rphotos.ico", sizes=[(n, n) for n in (16, 24, 32, 48, 64, 128, 256)])
    render(1024).save(assets / "Rphotos.icns")
