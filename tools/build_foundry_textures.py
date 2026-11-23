#!/usr/bin/env python3
"""Pixel-Art Stage 1 ("SECTOR 01 — THE IRON FOUNDRY") Environment Texture & Parallax Forge
for *VELOCITY // BREAKER*.

Generates a complete, palette-harmonized 32-bit retro-modern pixel-art tileset,
3-layer seamless parallax backdrop, animated hazard/lava strips, and crisp world prop
sprites inside `assets/textures/foundry/` (all nearest-neighbor pixel art calibrated
for the 640x360 virtual canvas + Bevy HDR Bloom!).

Optionally ingests AI-painted raw references (`--far-raw`, `--metal-raw`) and seamlessly
tiles + pixel-quantizes them into `foundry_bg_far.png` and `foundry_tile_metal.png`.
"""

from __future__ import annotations

import argparse
import math
import pathlib
import random

from PIL import Image, ImageDraw, ImageEnhance

OUT_DIR = pathlib.Path("assets/textures/foundry")

# Cohesive 2085 Iron Foundry Pixel-Art Palette (Cool Obsidian Steel vs. Molten Forge Amber vs. Rift Cyan)
STEEL_DEEP = (18, 22, 32, 255)
STEEL_DARK = (30, 36, 49, 255)
STEEL_MID = (48, 57, 74, 255)
STEEL_LIGHT = (74, 86, 108, 255)
STEEL_HIGH = (112, 126, 152, 255)
STEEL_SPEC = (158, 174, 200, 255)

RUST_DARK = (64, 34, 24, 255)
RUST_MID = (108, 52, 28, 255)
COPPER_MID = (164, 96, 44, 255)
COPPER_HI = (218, 146, 72, 255)

AMBER_DEEP = (168, 52, 14, 255)
AMBER_MID = (232, 114, 24, 255)
AMBER_HOT = (255, 190, 56, 255)
AMBER_WHITE = (255, 242, 186, 255)

HAZARD_YEL = (236, 172, 34, 255)
HAZARD_BLK = (22, 24, 30, 255)

CYAN_DARK = (14, 92, 126, 255)
CYAN_MID = (34, 198, 232, 255)
CYAN_HOT = (142, 244, 255, 255)
CYAN_WHITE = (232, 252, 255, 255)

CRIMSON_DARK = (118, 22, 34, 255)
CRIMSON_MID = (228, 42, 62, 255)
CRIMSON_HOT = (255, 136, 146, 255)

EMERALD_DARK = (18, 96, 52, 255)
EMERALD_MID = (42, 214, 118, 255)
EMERALD_HOT = (176, 255, 202, 255)


def make_seamless_horizontal(img: Image.Image, blend_frac: float = 0.14) -> Image.Image:
    """Makes an RGB/RGBA image tile seamlessly along X by cross-fading a wrapped seam."""
    w, h = img.size
    blend_w = max(8, int(w * blend_frac))
    # Shift by w//2 so original outer edges meet in the center, then cross-fade with original center
    shifted = Image.new(img.mode, (w, h))
    half = w // 2
    shifted.paste(img.crop((half, 0, w, h)), (0, 0))
    shifted.paste(img.crop((0, 0, half, h)), (w - half, 0))

    out = shifted.copy()
    src_px = img.load()
    sh_px = shifted.load()
    out_px = out.load()

    # Smoothly blend around x = half (where the original edges meet) using center content of img!
    c0 = half - blend_w // 2
    c1 = half + blend_w // 2
    for x in range(c0, c1):
        t = (x - c0) / max(1, (c1 - c0 - 1))
        # Bell weight peaking at 1.0 right on the seam x = half
        w_orig = 0.5 - 0.5 * math.cos(t * math.tau)
        for y in range(h):
            p_sh = sh_px[x, y]
            p_or = src_px[x, y]
            out_px[x, y] = tuple(int(p_sh[c] * (1.0 - w_orig) + p_or[c] * w_orig) for c in range(len(p_sh)))
    return out


def build_bg_far_from_raw(raw_path: pathlib.Path) -> None:
    """Converts an AI-painted far Foundry cavern image into a crisp, seamless 640x360 pixel-art backdrop."""
    img = Image.open(raw_path).convert("RGB")
    # Crop to 16:9 centered, downsample to 640x360 pixel art, make seamlessly horizontal!
    rw, rh = img.size
    target_aspect = 640.0 / 360.0
    cur_aspect = rw / rh
    if cur_aspect > target_aspect:
        nw = int(rh * target_aspect)
        x0 = (rw - nw) // 2
        img = img.crop((x0, 0, x0 + nw, rh))
    else:
        nh = int(rw / target_aspect)
        y0 = (rh - nh) // 2
        img = img.crop((0, y0, rw, y0 + nh))

    # Downsample to 320x180 then nearest-upscale 2x to 640x360 for chunky authentic 16/32-bit pixel art!
    small = img.resize((320, 180), Image.Resampling.LANCZOS)
    # Atmospheric recession: keep far cavern rich & moody so bright foreground Levy & enemies pop!
    small = ImageEnhance.Brightness(small).enhance(0.76)
    small = ImageEnhance.Contrast(small).enhance(1.12)
    small = ImageEnhance.Color(small).enhance(1.15)
    small = make_seamless_horizontal(small, blend_frac=0.18)
    # Quantize to crisp 48-color retro pixel palette with ZERO noisy dithering
    small = small.quantize(colors=48, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE).convert("RGB")
    final = small.resize((640, 360), Image.Resampling.NEAREST)
    final.save(OUT_DIR / "foundry_bg_far.png")
    print(f"[foundry] built seamless pixel-art foundry_bg_far.png from {raw_path}", flush=True)


def build_bg_mid() -> None:
    """Builds a rich, hand-shaded 640x360 transparent mid-parallax pixel-art layer:
    massive riveted steel support towers, cross-braced crane trusses, industrial coolant pipes,
    hanging slag chains with warm amber forge lanterns, and distant catwalk railings."""
    w, h = 320, 180
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    col_outline = (10, 13, 20, 255)
    col_dark = (22, 27, 38, 255)
    col_mid = (36, 43, 58, 255)
    col_hi = (56, 66, 86, 255)
    col_warm = (92, 54, 34, 255)

    # 1. Overhead heavy gantry crane bridge (y = 14..32)
    d.rectangle([0, 14, w - 1, 30], fill=col_dark)
    d.line([0, 14, w - 1, 14], fill=col_hi)
    d.line([0, 15, w - 1, 15], fill=col_mid)
    d.line([0, 29, w - 1, 29], fill=col_warm)
    d.line([0, 30, w - 1, 30], fill=col_outline)
    # Diagonal X-truss windows cut out along gantry
    for gx in range(0, w, 16):
        d.rectangle([gx + 2, 17, gx + 13, 27], fill=(0, 0, 0, 0))
        d.line([gx + 2, 17, gx + 13, 27], fill=col_mid, width=2)
        d.line([gx + 13, 17, gx + 2, 27], fill=col_dark, width=2)
        d.point((gx + 1, 16), fill=col_hi)
        d.point((gx + 14, 16), fill=col_hi)

    # 2. Horizontal background coolant pipe run (y = 44..52) with brackets & glowing cyan gauges
    d.rectangle([0, 46, w - 1, 51], fill=col_dark)
    d.line([0, 46, w - 1, 46], fill=col_hi)
    d.line([0, 47, w - 1, 47], fill=col_mid)
    d.line([0, 51, w - 1, 51], fill=col_outline)
    for px in range(12, w, 40):
        d.rectangle([px, 44, px + 5, 53], fill=col_mid, outline=col_outline)
        d.rectangle([px + 2, 47, px + 3, 49], fill=CYAN_MID)

    # 3. Massive vertical steel foundry columns spaced evenly every 80px (seamless at w=320!)
    for cx in (20, 100, 180, 260):
        # Main I-beam shaft
        d.rectangle([cx, 0, cx + 18, h - 1], fill=col_dark)
        d.rectangle([cx + 1, 0, cx + 3, h - 1], fill=col_mid)
        d.line([cx + 1, 0, cx + 1, h - 1], fill=col_hi)
        d.rectangle([cx + 15, 0, cx + 17, h - 1], fill=col_warm)
        d.line([cx, 0, cx, h - 1], fill=col_outline)
        d.line([cx + 18, 0, cx + 18, h - 1], fill=col_outline)
        # Recessed center flange + horizontal stiffener plates + rivets
        for ry in range(8, h, 24):
            d.rectangle([cx + 3, ry, cx + 15, ry + 4], fill=col_mid)
            d.line([cx + 3, ry, cx + 15, ry], fill=col_hi)
            d.line([cx + 3, ry + 4, cx + 15, ry + 4], fill=col_outline)
            d.point((cx + 5, ry + 2), fill=col_hi)
            d.point((cx + 13, ry + 2), fill=col_hi)
            # Warm amber beacon slit every second plate
            if (ry // 24) % 2 == 1:
                d.rectangle([cx + 7, ry + 11, cx + 11, ry + 13], fill=AMBER_MID)
                d.rectangle([cx + 8, ry + 11, cx + 10, ry + 12], fill=AMBER_HOT)

        # Angled diagonal gusset braces joining column to top gantry
        for k in range(12):
            d.line([cx - 12 + k, 30, cx, 42 - k], fill=col_dark)
            d.line([cx + 18, 42 - k, cx + 30 - k, 30], fill=col_dark)
        d.line([cx - 12, 30, cx, 42], fill=col_hi)
        d.line([cx + 18, 42, cx + 30, 30], fill=col_warm)

    # 4. Mid-level industrial catwalk bridge with detailed safety railings (y = 114..126)
    d.rectangle([0, 121, w - 1, 127], fill=col_dark)
    d.line([0, 121, w - 1, 121], fill=col_hi)
    d.line([0, 122, w - 1, 122], fill=col_mid)
    d.line([0, 126, w - 1, 126], fill=col_warm)
    d.line([0, 127, w - 1, 127], fill=col_outline)
    # Railing top bar & posts
    d.line([0, 113, w - 1, 113], fill=col_mid)
    d.line([0, 117, w - 1, 117], fill=col_dark)
    for rx in range(0, w, 10):
        d.line([rx, 113, rx, 121], fill=col_mid)

    # 5. Hanging heavy foundry chains + glowing amber forge lanterns between columns
    for hx, drop_y, is_cyan in [(60, 74, False), (140, 92, True), (220, 68, False), (300, 86, False)]:
        for cy in range(31, drop_y, 4):
            d.rectangle([hx - 1, cy, hx + 1, cy + 2], outline=col_hi, fill=col_dark)
        # Iron lantern cage
        d.rectangle([hx - 4, drop_y, hx + 4, drop_y + 8], fill=col_dark, outline=col_outline)
        d.line([hx - 4, drop_y, hx + 4, drop_y], fill=col_hi)
        if is_cyan:
            d.rectangle([hx - 2, drop_y + 2, hx + 2, drop_y + 6], fill=CYAN_MID)
            d.rectangle([hx - 1, drop_y + 3, hx + 1, drop_y + 5], fill=CYAN_WHITE)
        else:
            d.rectangle([hx - 2, drop_y + 2, hx + 2, drop_y + 6], fill=AMBER_MID)
            d.rectangle([hx - 1, drop_y + 3, hx + 1, drop_y + 5], fill=AMBER_WHITE)

    # 6. Lower foundry floor bulkhead apron with recessed molten furnace vents (y = 158..179)
    d.rectangle([0, 158, w - 1, h - 1], fill=col_dark)
    d.line([0, 158, w - 1, 158], fill=col_warm)
    d.line([0, 159, w - 1, 159], fill=col_mid)
    for vx in range(6, w, 40):
        d.rectangle([vx, 163, vx + 26, 175], fill=col_outline, outline=col_mid)
        for vy in (165, 168, 171, 173):
            col = AMBER_HOT if vy >= 171 else AMBER_MID
            d.line([vx + 2, vy, vx + 24, vy], fill=col)

    final = img.resize((640, 360), Image.Resampling.NEAREST)
    final.save(OUT_DIR / "foundry_bg_mid.png")
    print("[foundry] built detailed pixel-art foundry_bg_mid.png", flush=True)


def build_bg_near() -> None:
    """Builds a 3rd atmospheric near-midground parallax layer (640x360 transparent) with
    colossal dark industrial silhouette support girders, hanging slag buckets, and glowing conduit cables."""
    w, h = 320, 180
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    sil_dark = (14, 17, 25, 225)
    sil_mid = (24, 30, 42, 235)
    sil_rim = (78, 48, 30, 230)
    sil_cool = (42, 56, 78, 225)

    # Two massive distant diagonal mega-truss pillars per 320px repeat (widely spaced so gameplay stays super clean!)
    for bx in (52, 212):
        # Diagonal heavy truss beam rising right-to-left in background
        for y in range(0, h):
            x = int(bx + math.sin(y * 0.0) + (y * 0.14)) % w
            d.rectangle([x, y, (x + 14) % w if x + 14 < w else w - 1, y], fill=sil_dark)
            if x + 1 < w:
                d.point((x + 1, y), fill=sil_cool)
            if x + 13 < w:
                d.point((x + 13, y), fill=sil_rim)
        # Cross-rivet bands
        for y in range(18, h, 36):
            x = int(bx + y * 0.14)
            if x + 15 < w:
                d.rectangle([x - 1, y, x + 15, y + 4], fill=sil_mid)
                d.line([x - 1, y, x + 15, y], fill=sil_cool)
                d.rectangle([x + 5, y + 1, x + 9, y + 3], fill=(210, 96, 22, 210))

    # Sagging overhead high-voltage foundry cables along upper sky
    for y_base, sag, col in [(10, 16, sil_mid), (14, 22, sil_dark)]:
        for x in range(w):
            t = (x % 160) / 160.0
            y = int(y_base + math.sin(t * math.pi) * sag)
            if 0 <= y < h:
                d.point((x, y), fill=col)
                if y + 1 < h:
                    d.point((x, y + 1), fill=sil_dark)

    final = img.resize((640, 360), Image.Resampling.NEAREST)
    final.save(OUT_DIR / "foundry_bg_near.png")
    print("[foundry] built 3rd-layer parallax foundry_bg_near.png", flush=True)


def build_tile_metal(raw_path: pathlib.Path | None = None) -> None:
    """Builds a crisp, richly weathered 64x64 seamless riveted Foundry steel plating tile."""
    if raw_path is not None and raw_path.exists():
        img = Image.open(raw_path).convert("RGB")
        # Crop square & downsample to crisp 64x64 pixel art
        s = min(img.size)
        x0 = (img.width - s) // 2
        y0 = (img.height - s) // 2
        img = img.crop((x0, y0, x0 + s, y0 + s)).resize((64, 64), Image.Resampling.LANCZOS)
        img = ImageEnhance.Brightness(img).enhance(0.72)
        img = ImageEnhance.Contrast(img).enhance(1.18)
        img = img.quantize(colors=24, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE).convert("RGBA")
        # Reinforce crisp pixel-perfect 32x32 panel seams & rivets so tiling is 100% locked!
        d = ImageDraw.Draw(img)
        for ox in (0, 32):
            for oy in (0, 32):
                d.rectangle([ox, oy, ox + 31, oy + 31], outline=STEEL_DEEP)
                d.line([ox + 1, oy + 1, ox + 30, oy + 1], fill=STEEL_LIGHT)
                d.line([ox + 1, oy + 1, ox + 1, oy + 30], fill=STEEL_MID)
                for rx, ry in [(ox + 4, oy + 4), (ox + 26, oy + 4), (ox + 4, oy + 26), (ox + 26, oy + 26)]:
                    d.rectangle([rx, ry, rx + 2, ry + 2], fill=STEEL_DEEP)
                    d.rectangle([rx, ry, rx + 1, ry + 1], fill=STEEL_HIGH)
        img.save(OUT_DIR / "foundry_tile_metal.png")
        print(f"[foundry] built hybrid AI+pixel foundry_tile_metal.png from {raw_path}", flush=True)
        return

    rng = random.Random(2085)
    img = Image.new("RGBA", (64, 64), STEEL_DARK)
    d = ImageDraw.Draw(img)

    # Subtle brushed steel noise + warm rust wear patches
    for y in range(64):
        for x in range(64):
            n = rng.random()
            if n < 0.14:
                img.putpixel((x, y), STEEL_DEEP)
            elif n < 0.28:
                img.putpixel((x, y), STEEL_MID)
            elif n > 0.96:
                img.putpixel((x, y), RUST_DARK)

    # 2x2 beveled steel armor plates with crisp highlights, dark grooves & recessed rivets
    for ox in (0, 32):
        for oy in (0, 32):
            d.rectangle([ox, oy, ox + 31, oy + 31], outline=STEEL_DEEP)
            d.line([ox + 1, oy + 1, ox + 30, oy + 1], fill=STEEL_LIGHT)
            d.line([ox + 1, oy + 1, ox + 1, oy + 30], fill=STEEL_MID)
            d.line([ox + 1, oy + 30, ox + 30, oy + 30], fill=STEEL_DEEP)
            d.line([ox + 30, oy + 1, ox + 30, oy + 30], fill=STEEL_DEEP)
            # Inner recessed detail panel
            d.rectangle([ox + 6, oy + 8, ox + 25, oy + 23], outline=STEEL_DEEP)
            d.line([ox + 7, oy + 24, ox + 25, oy + 24], fill=STEEL_MID)
            # Corner bolts
            for rx, ry in [(ox + 3, oy + 3), (ox + 26, oy + 3), (ox + 3, oy + 26), (ox + 26, oy + 26)]:
                d.rectangle([rx, ry, rx + 2, ry + 2], fill=STEEL_DEEP)
                d.rectangle([rx, ry, rx + 1, ry + 1], fill=STEEL_HIGH)
                d.point((rx, ry), fill=STEEL_SPEC)

    img.save(OUT_DIR / "foundry_tile_metal.png")
    print("[foundry] built procedural pixel-art foundry_tile_metal.png", flush=True)


def build_tile_trim_top() -> None:
    """64x12 tiled heavy beveled Foundry floor deck trim with amber-black hazard chevrons
    and glowing cyan Rift conduit rail along the top walkway lip."""
    img = Image.new("RGBA", (64, 12), STEEL_DARK)
    d = ImageDraw.Draw(img)

    # Top gleaming metallic walkway edge
    d.line([0, 0, 63, 0], fill=STEEL_SPEC)
    d.line([0, 1, 63, 1], fill=STEEL_HIGH)
    d.line([0, 2, 63, 2], fill=STEEL_LIGHT)

    # Recessed glowing cyan conduit track (y = 3..4)
    d.line([0, 3, 63, 3], fill=CYAN_DARK)
    d.line([0, 4, 63, 4], fill=CYAN_MID)
    for x in range(4, 64, 16):
        d.line([x, 4, x + 7, 4], fill=CYAN_HOT)

    # Industrial amber-black diagonal hazard stripe band (y = 6..9)
    d.line([0, 5, 63, 5], fill=STEEL_DEEP)
    for y in range(6, 10):
        for x in range(64):
            stripe = ((x + y) // 6) % 2
            col = HAZARD_YEL if stripe == 0 else HAZARD_BLK
            if stripe == 0 and y == 6:
                col = AMBER_HOT
            img.putpixel((x, y), col)
    d.line([0, 10, 63, 10], fill=STEEL_DEEP)
    d.line([0, 11, 63, 11], fill=(10, 12, 18, 255))

    img.save(OUT_DIR / "foundry_tile_trim_top.png")


def build_tile_wall() -> None:
    """32x64 tiled vertical armored bulkhead pillar for wall-jump shafts & fortress walls."""
    img = Image.new("RGBA", (32, 64), STEEL_DARK)
    d = ImageDraw.Draw(img)

    # Vertical fluted steel ribs + left/right wall-grip ridges
    for x in range(32):
        if x in (0, 31):
            col = STEEL_DEEP
        elif x in (1, 30):
            col = STEEL_HIGH
        elif x in (2, 29):
            col = STEEL_LIGHT
        elif x in (6, 25):
            col = STEEL_DEEP
        elif x in (7, 24):
            col = STEEL_MID
        elif 13 <= x <= 18:
            # Central vertical copper furnace conduit pipe
            col = COPPER_HI if x == 14 else (COPPER_MID if x in (15, 16) else RUST_DARK)
        else:
            col = STEEL_DARK
        d.line([x, 0, x, 63], fill=col)

    # Horizontal armor clamp collars every 16px
    for y in range(0, 64, 16):
        d.rectangle([1, y, 30, y + 3], fill=STEEL_MID)
        d.line([1, y, 30, y], fill=STEEL_HIGH)
        d.line([1, y + 3, 30, y + 3], fill=STEEL_DEEP)
        # Wall-grip cyan/amber LED strip along edges
        d.rectangle([2, y + 6, 3, y + 10], fill=AMBER_MID)
        d.rectangle([28, y + 6, 29, y + 10], fill=AMBER_MID)

    img.save(OUT_DIR / "foundry_tile_wall.png")


def build_platform_oneway() -> None:
    """64x12 tiled open-truss steel industrial catwalk platform with warm amber lip."""
    img = Image.new("RGBA", (64, 12), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    # Solid grated top rail (y = 0..4)
    d.line([0, 0, 63, 0], fill=AMBER_HOT)
    d.line([0, 1, 63, 1], fill=AMBER_MID)
    d.rectangle([0, 2, 63, 4], fill=STEEL_MID)
    d.line([0, 2, 63, 2], fill=STEEL_HIGH)
    d.line([0, 4, 63, 4], fill=STEEL_DEEP)

    # Open diagonal steel truss webbing underneath (y = 5..10)
    d.line([0, 10, 63, 10], fill=STEEL_MID)
    d.line([0, 11, 63, 11], fill=STEEL_DEEP)
    for tx in range(0, 64, 16):
        d.line([tx, 5, tx, 10], fill=STEEL_HIGH)
        d.line([tx + 1, 5, tx + 1, 10], fill=STEEL_DARK)
        d.line([tx + 1, 5, tx + 8, 10], fill=STEEL_LIGHT)
        d.line([tx + 8, 10, tx + 15, 5], fill=STEEL_MID)
        d.point((tx + 8, 9), fill=STEEL_SPEC)

    img.save(OUT_DIR / "foundry_platform_oneway.png")


def build_platform_moving() -> None:
    """64x16 tiled cyber-foundry mag-lev cargo shuttle platform with glowing cyan hover jets."""
    img = Image.new("RGBA", (64, 16), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    # Sleek beveled top deck with glowing cyan kinetic rail
    d.line([0, 0, 63, 0], fill=CYAN_WHITE)
    d.line([0, 1, 63, 1], fill=CYAN_MID)
    d.rectangle([0, 2, 63, 9], fill=STEEL_MID)
    d.line([0, 2, 63, 2], fill=STEEL_HIGH)
    d.line([0, 9, 63, 9], fill=STEEL_DEEP)
    # Recessed chassis panels & cyan energy core pips
    for cx in range(4, 64, 16):
        d.rectangle([cx, 4, cx + 10, 7], fill=STEEL_DEEP)
        d.rectangle([cx + 2, 5, cx + 8, 6], fill=CYAN_MID)
        d.rectangle([cx + 4, 5, cx + 6, 5], fill=CYAN_WHITE)
    # Under-chassis hover thruster nozzles + plasma exhaust plumes (y = 10..15)
    for nx in (8, 28, 48):
        d.rectangle([nx, 10, nx + 7, 12], fill=STEEL_DARK, outline=STEEL_DEEP)
        d.rectangle([nx + 1, 13, nx + 6, 14], fill=CYAN_MID)
        d.rectangle([nx + 2, 14, nx + 5, 15], fill=CYAN_HOT)

    img.save(OUT_DIR / "foundry_platform_moving.png")


def build_breakable_wall() -> None:
    """32x64 scorched cracked iron bulkhead glowing with unstable molten orange fractures."""
    img = Image.new("RGBA", (32, 64), (42, 28, 26, 255))
    d = ImageDraw.Draw(img)
    rng = random.Random(77)
    for y in range(64):
        for x in range(32):
            v = rng.random()
            if v < 0.25:
                img.putpixel((x, y), RUST_DARK)
            elif v < 0.40:
                img.putpixel((x, y), STEEL_DARK)
    # Heavy scorched border plates
    d.rectangle([0, 0, 31, 63], outline=STEEL_DEEP)
    d.line([1, 1, 1, 62], fill=RUST_MID)
    d.line([30, 1, 30, 62], fill=RUST_MID)
    # Jagged glowing molten magma fracture network across center!
    cracks = [
        [(2, 10), (11, 16), (18, 12), (29, 20)],
        [(3, 30), (14, 34), (20, 27), (29, 32)],
        [(4, 48), (12, 52), (22, 44), (29, 54)],
        [(14, 15), (16, 33), (13, 50)],
    ]
    for path in cracks:
        for a, b in zip(path, path[1:]):
            d.line([a, b], fill=AMBER_DEEP, width=4)
            d.line([a, b], fill=AMBER_MID, width=2)
            d.line([a, b], fill=AMBER_WHITE, width=1)
    img.save(OUT_DIR / "foundry_breakable_wall.png")


def build_boost_and_jump_pads() -> None:
    """Builds `foundry_boost_pad.png` (64x12) and `foundry_jump_pad.png` (48x14)."""
    # 1. Boost Pad: bolted steel rail + blazing cyan forward chevron arrows!
    bp = Image.new("RGBA", (64, 12), (0, 0, 0, 0))
    d = ImageDraw.Draw(bp)
    d.rectangle([0, 4, 63, 11], fill=STEEL_DARK, outline=STEEL_DEEP)
    d.line([1, 4, 62, 4], fill=STEEL_HIGH)
    d.rectangle([2, 2, 61, 6], fill=CYAN_DARK)
    # Four blazing forward chevrons (>>>)
    for cx in (6, 20, 34, 48):
        for k in range(5):
            d.line([(cx + k, 1 + k // 2), (cx + k + 5, 4)], fill=CYAN_HOT)
            d.line([(cx + k, 7 - k // 2), (cx + k + 5, 4)], fill=CYAN_MID)
        d.line([(cx + 3, 2), (cx + 8, 4)], fill=CYAN_WHITE)
        d.line([(cx + 3, 6), (cx + 8, 4)], fill=CYAN_WHITE)
    bp.save(OUT_DIR / "foundry_boost_pad.png")

    # 2. Jump Pad: hydraulic twin-piston spring platform with amber-cyan bounce plate!
    jp = Image.new("RGBA", (48, 14), (0, 0, 0, 0))
    d = ImageDraw.Draw(jp)
    # Heavy bolted base anchor
    d.rectangle([2, 10, 45, 13], fill=STEEL_DARK, outline=STEEL_DEEP)
    d.line([3, 10, 44, 10], fill=STEEL_LIGHT)
    # Coiled copper hydraulic pistons
    for px in (9, 22, 35):
        d.rectangle([px, 5, px + 4, 10], fill=COPPER_MID, outline=RUST_DARK)
        d.line([px + 1, 5, px + 1, 10], fill=COPPER_HI)
        d.line([px, 7, px + 4, 7], fill=STEEL_DEEP)
    # Glowing pneumatic amber top bounce deck + cyan center core
    d.rectangle([4, 1, 43, 5], fill=AMBER_MID, outline=AMBER_DEEP)
    d.line([5, 1, 42, 1], fill=AMBER_WHITE)
    d.line([5, 2, 42, 2], fill=AMBER_HOT)
    d.rectangle([16, 2, 31, 4], fill=CYAN_HOT)
    d.rectangle([19, 2, 28, 3], fill=CYAN_WHITE)
    jp.save(OUT_DIR / "foundry_jump_pad.png")


def build_spikes_and_turret() -> None:
    """Builds `foundry_spikes.png` (32x16 tiled) and `foundry_turret.png` (24x24)."""
    # 1. Superheated plasma-tipped iron spikes (4 razor spikes per 32px tile)
    sp = Image.new("RGBA", (32, 16), (0, 0, 0, 0))
    d = ImageDraw.Draw(sp)
    d.rectangle([0, 12, 31, 15], fill=STEEL_DARK, outline=STEEL_DEEP)
    d.line([0, 12, 31, 12], fill=STEEL_HIGH)
    for sx in (4, 12, 20, 28):
        d.polygon([(sx - 3, 12), (sx, 1), (sx + 3, 12)], fill=STEEL_MID, outline=STEEL_DEEP)
        d.polygon([(sx - 2, 11), (sx, 2), (sx, 11)], fill=STEEL_LIGHT)
        # Superheated glowing crimson-hot tip!
        d.polygon([(sx - 1, 6), (sx, 0), (sx + 1, 6)], fill=CRIMSON_MID)
        d.point((sx, 1), fill=AMBER_WHITE)
        d.point((sx, 2), fill=CRIMSON_HOT)
    sp.save(OUT_DIR / "foundry_spikes.png")

    # 2. 2085 Directorate armored wall defense turret (facing +X right; Bevy flips X for left!)
    tr = Image.new("RGBA", (24, 24), (0, 0, 0, 0))
    d = ImageDraw.Draw(tr)
    # Rear mounting flange
    d.rectangle([0, 3, 5, 20], fill=STEEL_MID, outline=STEEL_DEEP)
    d.line([1, 4, 1, 19], fill=STEEL_HIGH)
    # Armored octagonal turret body
    d.polygon([(4, 5), (15, 5), (18, 8), (18, 15), (15, 18), (4, 18)], fill=STEEL_DARK, outline=STEEL_DEEP)
    d.line([5, 6, 14, 6], fill=STEEL_HIGH)
    d.line([5, 7, 14, 7], fill=STEEL_LIGHT)
    # Twin cooling vents
    d.line([7, 14, 13, 14], fill=STEEL_DEEP)
    d.line([7, 16, 13, 16], fill=STEEL_DEEP)
    # Reinforced plasma cannon barrel pointing right (+X)
    d.rectangle([16, 8, 23, 14], fill=STEEL_MID, outline=STEEL_DEEP)
    d.line([17, 9, 22, 9], fill=STEEL_SPEC)
    d.rectangle([22, 9, 23, 13], fill=CRIMSON_MID)
    # Crimson optical sensor eye
    d.rectangle([9, 9, 13, 12], fill=CRIMSON_DARK)
    d.rectangle([10, 10, 12, 11], fill=CRIMSON_HOT)
    tr.save(OUT_DIR / "foundry_turret.png")


def build_checkpoint_and_collectibles() -> None:
    """Builds crisp pixel sprites for Checkpoint Pylon, Energy Core, 1700s Brass Keepsake Relic, and Heal Canister."""
    # 1. Checkpoint Resonance Pylon pedestal (20x44; glowing diamond orb sits on top in Bevy!)
    cp = Image.new("RGBA", (20, 44), (0, 0, 0, 0))
    d = ImageDraw.Draw(cp)
    # Wide bolted pedestal base
    d.rectangle([1, 38, 18, 43], fill=STEEL_DARK, outline=STEEL_DEEP)
    d.line([2, 38, 17, 38], fill=STEEL_HIGH)
    # Tapered steel pylon tower
    d.polygon([(4, 38), (6, 14), (13, 14), (15, 38)], fill=STEEL_MID, outline=STEEL_DEEP)
    d.line([6, 15, 5, 37], fill=STEEL_HIGH)
    d.line([13, 15, 14, 37], fill=STEEL_DARK)
    # Twin copper stabilizer prongs holding the floating crystal at top
    d.polygon([(3, 16), (6, 12), (6, 18)], fill=COPPER_MID, outline=RUST_DARK)
    d.polygon([(16, 16), (13, 12), (13, 18)], fill=COPPER_MID, outline=RUST_DARK)
    # Vertical energy conduit core along center
    d.rectangle([9, 17, 10, 35], fill=CYAN_DARK)
    for y in (20, 25, 30):
        d.rectangle([8, y, 11, y + 1], fill=STEEL_LIGHT)
    cp.save(OUT_DIR / "foundry_checkpoint.png")

    # 2. Rift Energy Core (16x16 glowing cyan faceted crystal)
    core = Image.new("RGBA", (16, 16), (0, 0, 0, 0))
    d = ImageDraw.Draw(core)
    d.polygon([(8, 1), (14, 8), (8, 14), (2, 8)], fill=CYAN_DARK, outline=(10, 48, 72, 255))
    d.polygon([(8, 2), (13, 8), (8, 13), (3, 8)], fill=CYAN_MID)
    d.polygon([(8, 4), (11, 8), (8, 11), (5, 8)], fill=CYAN_HOT)
    d.polygon([(8, 5), (10, 8), (8, 9), (6, 8)], fill=CYAN_WHITE)
    core.save(OUT_DIR / "foundry_core.png")

    # 3. 1700s Brass Keepsake Relic (20x20 ornate golden pocket-watch / locket with warm glow!)
    rel = Image.new("RGBA", (20, 20), (0, 0, 0, 0))
    d = ImageDraw.Draw(rel)
    # Top watch bow ring & crown
    d.ellipse([7, 1, 12, 5], outline=AMBER_HOT, width=1)
    d.rectangle([8, 4, 11, 6], fill=COPPER_HI)
    # Ornate round 1700s brass locket casing
    d.ellipse([3, 5, 16, 18], fill=COPPER_MID, outline=RUST_DARK)
    d.ellipse([4, 6, 15, 17], fill=AMBER_MID, outline=AMBER_HOT)
    # Enamel clock face + filigree hands
    d.ellipse([6, 8, 13, 15], fill=AMBER_WHITE)
    d.line([9, 11, 9, 8], fill=RUST_DARK)
    d.line([9, 11, 12, 11], fill=RUST_DARK)
    d.point((6, 7), fill=(255, 255, 255, 255))
    rel.save(OUT_DIR / "foundry_relic.png")

    # 4. Nano-Med Heal Canister (16x20 sleek armored emerald cryo-vial)
    hl = Image.new("RGBA", (16, 20), (0, 0, 0, 0))
    d = ImageDraw.Draw(hl)
    # Steel top & bottom endcaps
    d.rectangle([3, 1, 12, 4], fill=STEEL_LIGHT, outline=STEEL_DEEP)
    d.rectangle([3, 15, 12, 18], fill=STEEL_MID, outline=STEEL_DEEP)
    # Glowing emerald glass cylinder
    d.rectangle([2, 4, 13, 15], fill=EMERALD_DARK, outline=STEEL_DEEP)
    d.rectangle([3, 5, 12, 14], fill=EMERALD_MID)
    d.line([4, 5, 4, 14], fill=EMERALD_HOT)
    # Bright white medical cross
    d.rectangle([7, 7, 8, 12], fill=CYAN_WHITE)
    d.rectangle([5, 9, 10, 10], fill=CYAN_WHITE)
    hl.save(OUT_DIR / "foundry_heal.png")


def build_lava_and_fixtures() -> None:
    """Builds `foundry_lava_strip.png` (128x28), `foundry_molten_cascade.png` (24x128),
    and `foundry_lamp_fixture.png` (28x14) for dramatic environmental storytelling!"""
    # 1. Shimmering molten slag river strip (128x28 seamless horizontal)
    lava = Image.new("RGBA", (128, 28), (0, 0, 0, 0))
    d = ImageDraw.Draw(lava)
    for y in range(28):
        for x in range(128):
            wave = math.sin(x * math.tau / 32.0) * 1.8 + math.cos(x * math.tau / 64.0) * 1.2
            surface_y = 6.0 + wave
            if y < surface_y - 3.0:
                continue
            elif y < surface_y:
                # Subtle heat glow rim right above lava crest
                alpha = int(110 * (1.0 - (surface_y - y) / 3.0))
                lava.putpixel((x, y), (255, 120, 28, alpha))
            elif y < surface_y + 2.2:
                lava.putpixel((x, y), AMBER_WHITE)
            elif y < surface_y + 5.5:
                lava.putpixel((x, y), AMBER_HOT)
            elif y < surface_y + 11.0:
                swirl = math.sin((x + y * 3) * math.tau / 42.0)
                lava.putpixel((x, y), AMBER_HOT if swirl > 0.55 else AMBER_MID)
            else:
                crust = math.sin((x - y * 2) * math.tau / 32.0)
                lava.putpixel((x, y), AMBER_MID if crust > 0.4 else AMBER_DEEP)
    lava.save(OUT_DIR / "foundry_lava_strip.png")

    # 2. Pouring background liquid-metal crucible waterfall (24x128 seamless vertical)
    cas = Image.new("RGBA", (24, 128), (0, 0, 0, 0))
    for y in range(128):
        for x in range(24):
            dx = abs(x - 11.5)
            wobble = math.sin(y * math.tau / 32.0) * 1.1
            dist = dx + wobble * 0.4
            if dist > 8.5:
                continue
            elif dist > 6.5:
                cas.putpixel((x, y), (210, 74, 16, 130))
            elif dist > 4.2:
                cas.putpixel((x, y), (242, 124, 24, 215))
            elif dist > 1.8:
                streak = math.sin((y + x * 4) * math.tau / 24.0)
                cas.putpixel((x, y), AMBER_WHITE if streak > 0.55 else AMBER_HOT)
            else:
                cas.putpixel((x, y), AMBER_WHITE)
    cas.save(OUT_DIR / "foundry_molten_cascade.png")

    # 3. Industrial hanging overhead lamp hood (28x14)
    lamp = Image.new("RGBA", (28, 14), (0, 0, 0, 0))
    d = ImageDraw.Draw(lamp)
    d.rectangle([12, 0, 15, 4], fill=STEEL_MID, outline=STEEL_DEEP)
    d.polygon([(3, 10), (8, 4), (19, 4), (24, 10)], fill=STEEL_DARK, outline=STEEL_DEEP)
    d.line([8, 5, 19, 5], fill=STEEL_HIGH)
    d.rectangle([6, 10, 21, 13], fill=(255, 246, 224, 255))
    lamp.save(OUT_DIR / "foundry_lamp_fixture.png")


def main() -> None:
    parser = argparse.ArgumentParser(description="Build crisp pixel-art Stage 1 Foundry textures.")
    parser.add_argument("--far-raw", type=pathlib.Path, default=None, help="Optional AI far cavern raw image.")
    parser.add_argument("--metal-raw", type=pathlib.Path, default=None, help="Optional AI metal surface raw image.")
    args = parser.parse_args()

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    if args.far_raw is not None and args.far_raw.exists():
        build_bg_far_from_raw(args.far_raw)
    build_bg_mid()
    build_bg_near()
    build_tile_metal(args.metal_raw)
    build_tile_trim_top()
    build_tile_wall()
    build_platform_oneway()
    build_platform_moving()
    build_breakable_wall()
    build_boost_and_jump_pads()
    build_spikes_and_turret()
    build_checkpoint_and_collectibles()
    build_lava_and_fixtures()
    print(f"[foundry] ALL Stage 1 pixel-art environment textures built in {OUT_DIR}!", flush=True)


if __name__ == "__main__":
    main()
