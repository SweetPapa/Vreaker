#!/usr/bin/env python3
"""Prepares Story Bible concept art, pixel-quantized character portraits, and Title Screen
key art for *VELOCITY // BREAKER*.

Produces:
  - raw_assets/concepts/levy_apose_v1.png (padded clean A-pose for TRELLIS.2)
  - raw_assets/concepts/levy_apose_v2.png
  - assets/ui/portraits/levy.png      (128x128 crisp pixel-art portrait, facing right w/ left gauntlet)
  - assets/ui/portraits/mina.png      (128x128 crisp pixel-art portrait)
  - assets/ui/portraits/barnaby.png   (128x128 crisp pixel-art portrait)
  - assets/ui/portraits/vulcanus.png  (128x128 crisp pixel-art portrait)
  - assets/ui/title_key_art.png       (640x360 pixel-art quantized wide Title Screen backdrop)
"""

from __future__ import annotations

import pathlib
from PIL import Image, ImageEnhance, ImageOps

BRAIN_DIR = pathlib.Path(
    "/Users/fterry/.gemini/antigravity-cli/brain/4a32465e-0d6f-43c7-abd3-81f5f97790ee"
)
CONCEPTS_DIR = pathlib.Path("raw_assets/concepts")
PORTRAITS_DIR = pathlib.Path("assets/ui/portraits")
UI_DIR = pathlib.Path("assets/ui")


def pad_apose(src: pathlib.Path, dst: pathlib.Path) -> None:
    with Image.open(src) as img:
        rgb = img.convert("RGB")
        bg = rgb.getpixel((4, 4))
        w, h = rgb.size
        pad_x = int(w * 0.08)
        pad_y = int(h * 0.08)
        out = Image.new("RGB", (w + pad_x * 2, h + pad_y * 2), bg)
        out.paste(rgb, (pad_x, pad_y))
        dst.parent.mkdir(parents=True, exist_ok=True)
        out.save(dst, format="PNG")
        print(f"[prep_ui] wrote {dst} ({out.size})")


def pixelize_portrait(src: pathlib.Path, dst: pathlib.Path, flip_h: bool = False) -> None:
    with Image.open(src) as img:
        rgb = img.convert("RGB")
        if flip_h:
            rgb = ImageOps.mirror(rgb)
        # Center-crop square
        w, h = rgb.size
        side = min(w, h)
        sq = rgb.crop(((w - side) // 2, (h - side) // 2, (w + side) // 2, (h + side) // 2))
        # Downsample cleanly to crisp 128x128 sprite portrait + subtle 64-color pixel palette
        low = sq.resize((128, 128), Image.Resampling.LANCZOS)
        low = ImageEnhance.Contrast(low).enhance(1.06)
        quant = low.quantize(colors=64, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE).convert("RGBA")
        dst.parent.mkdir(parents=True, exist_ok=True)
        quant.save(dst, format="PNG", optimize=True)
        print(f"[prep_ui] wrote {dst} ({quant.size})")


def pixelize_title_key_art(src: pathlib.Path, dst: pathlib.Path) -> None:
    with Image.open(src) as img:
        rgb = img.convert("RGB")
        # Fit exact 16:9 -> 640x360 virtual pixel canvas resolution
        target_ratio = 640 / 360
        w, h = rgb.size
        if w / h > target_ratio:
            new_w = int(h * target_ratio)
            left = (w - new_w) // 2
            rgb = rgb.crop((left, 0, left + new_w, h))
        else:
            new_h = int(w / target_ratio)
            top = (h - new_h) // 2
            rgb = rgb.crop((0, top, w, top + new_h))
        # Downsample to 640x360, keep crisp vibrant colors slightly dimmed so menu card pops
        resized = rgb.resize((640, 360), Image.Resampling.LANCZOS)
        resized = ImageEnhance.Brightness(resized).enhance(0.78)
        resized = ImageEnhance.Contrast(resized).enhance(1.10)
        quant = resized.quantize(colors=96, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE).convert("RGB")
        dst.parent.mkdir(parents=True, exist_ok=True)
        quant.save(dst, format="PNG", optimize=True)
        print(f"[prep_ui] wrote {dst} ({quant.size})")


def main() -> None:
    pad_apose(
        BRAIN_DIR / "levy_apose_v1_1795400972104.jpg",
        CONCEPTS_DIR / "levy_apose_v1.png",
    )
    pad_apose(
        BRAIN_DIR / "levy_apose_v2_1795400983981.jpg",
        CONCEPTS_DIR / "levy_apose_v2.png",
    )
    # Mirror Levy portrait so he faces right AND his glowing cyan Rift gauntlet sits on his LEFT arm!
    pixelize_portrait(
        BRAIN_DIR / "portrait_levy_1795400974530.jpg",
        PORTRAITS_DIR / "levy.png",
        flip_h=True,
    )
    pixelize_portrait(
        BRAIN_DIR / "portrait_mina_1795400983852.jpg",
        PORTRAITS_DIR / "mina.png",
    )
    pixelize_portrait(
        BRAIN_DIR / "portrait_barnaby_1795400992699.jpg",
        PORTRAITS_DIR / "barnaby.png",
    )
    pixelize_portrait(
        BRAIN_DIR / "portrait_vulcanus_1795401004337.jpg",
        PORTRAITS_DIR / "vulcanus.png",
    )
    pixelize_title_key_art(
        BRAIN_DIR / "title_key_art_1795401013221.jpg",
        UI_DIR / "title_key_art.png",
    )


if __name__ == "__main__":
    main()
