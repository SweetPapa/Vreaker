#!/usr/bin/env python3
"""
Builds the complete 20-texture 32-bit pixel-art Stage 2 (*THE CRYO CONDUIT*)
environment suite (`assets/textures/cryo/`), prepares Borealis's crisp 128x128
dialogue portrait (`assets/ui/portraits/borealis.png`), and renders a visual QA
composite preview (`raw_assets/showcases/stage2_cryo_preview.png`).
"""

from __future__ import annotations

import argparse
import math
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
OUT_DIR = ROOT / "assets" / "textures" / "cryo"
PORTRAIT_DIR = ROOT / "assets" / "ui" / "portraits"
SHOWCASE_DIR = ROOT / "raw_assets" / "showcases"


def make_rgba(w: int, h: int) -> np.ndarray:
    return np.zeros((h, w, 4), dtype=np.uint8)


def put(arr: np.ndarray, x: int, y: int, rgba: tuple[int, int, int, int]) -> None:
    h, w, _ = arr.shape
    if 0 <= x < w and 0 <= y < h:
        arr[y, x] = rgba


def rect(arr: np.ndarray, x0: int, y0: int, x1: int, y1: int, rgba: tuple[int, int, int, int]) -> None:
    h, w, _ = arr.shape
    x0, x1 = max(0, x0), min(w, x1)
    y0, y1 = max(0, y0), min(h, y1)
    if x1 > x0 and y1 > y0:
        arr[y0:y1, x0:x1] = rgba


def save_png(name: str, arr: np.ndarray) -> Image.Image:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    im = Image.fromarray(arr)
    im.save(OUT_DIR / name)
    print(f"  [CRYO] {name} ({im.width}x{im.height})")
    return im


def build_bg_far(raw_path: Path) -> Image.Image:
    """640x360 seamless horizontally-tiled pixel-art Cryo Conduit far cathedral server hall."""
    raw = Image.open(raw_path).convert("RGB")
    # Crop/resize cleanly to 640x360, then seamlessly mirror-blend edges for invisible horizontal wrap
    im = raw.resize((640, 360), Image.Resampling.LANCZOS)
    arr = np.array(im, dtype=np.float32)
    # Crossfade a 48px wrap band across the seam so ParallaxLayer tiles with zero visible seam
    seam = 48
    left = arr[:, :seam, :].copy()
    right = arr[:, -seam:, :].copy()
    w = np.linspace(0.0, 1.0, seam, dtype=np.float32)[None, :, None]
    blended = right * (1.0 - w) + left * w
    arr[:, :seam // 2, :] = blended[:, seam // 2 :, :]
    arr[:, -seam // 2 :, :] = blended[:, : seam // 2, :]
    # Subtle atmospheric sapphire recess so foreground ice platforms & fighters pop!
    arr[:, :, 0] *= 0.80
    arr[:, :, 1] *= 0.86
    arr[:, :, 2] *= 0.94
    q = Image.fromarray(np.clip(arr, 0, 255).astype(np.uint8)).quantize(colors=64, method=Image.Quantize.MEDIANCUT).convert("RGBA")
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    q.save(OUT_DIR / "cryo_bg_far.png")
    print("  [CRYO] cryo_bg_far.png (640x360)")
    return q


def build_bg_mid() -> Image.Image:
    """640x360 transparent mid-parallax frosted server racks, liquid-nitrogen coolant tubes & frosted catwalks."""
    w, h = 640, 360
    arr = make_rgba(w, h)
    # Frosted steel server pillars every 160px
    for px in [36, 196, 356, 516]:
        rect(arr, px, 0, px + 26, h, (20, 32, 52, 232))
        rect(arr, px, 0, px + 3, h, (44, 72, 108, 240))
        rect(arr, px + 23, 0, px + 26, h, (12, 18, 32, 245))
        # Blinking server telemetry LED strips & frosty horizontal braces
        for y in range(18, h, 28):
            rect(arr, px + 4, y, px + 22, y + 4, (32, 50, 78, 240))
            led_col = (70, 215, 255, 225) if (y // 28) % 2 == 0 else (140, 235, 255, 195)
            rect(arr, px + 7, y + 1, px + 11, y + 3, led_col)
            rect(arr, px + 14, y + 1, px + 18, y + 3, (48, 125, 180, 180))
        # Glowing vertical liquid-nitrogen pipe flanking each server monolith
        nx = px + 30
        rect(arr, nx, 0, nx + 12, h, (18, 42, 68, 210))
        rect(arr, nx + 2, 0, nx + 10, h, (56, 152, 206, 180))
        rect(arr, nx + 4, 0, nx + 7, h, (148, 234, 255, 195))
        for y in range(12, h, 44):
            rect(arr, nx - 1, y, nx + 13, y + 5, (38, 58, 86, 245))
            # Rising cryo bubbles inside glass tube
            rect(arr, nx + 5, (y + 18) % h, nx + 7, (y + 20) % h, (220, 252, 255, 215))

    # Mid-height frosted overhead gantry beam + server bridge + hanging icicles
    for gy in [78, 242]:
        rect(arr, 0, gy, w, gy + 10, (24, 38, 62, 235))
        rect(arr, 0, gy, w, gy + 2, (110, 185, 232, 220))
        for x in range(0, w, 24):
            # Truss diagonals & hanging frost icicles
            for k in range(10):
                put(arr, (x + k) % w, gy + k, (38, 60, 92, 225))
            ic_len = 6 + ((x * 7) % 13)
            for iy in range(ic_len):
                iw = max(1, int((1.0 - iy / ic_len) * 3))
                rect(arr, x + 6, gy + 10 + iy, x + 6 + iw, gy + 11 + iy, (145, 224, 255, 200))
    return save_png("cryo_bg_mid.png", arr)


def build_bg_near() -> Image.Image:
    """640x360 near-parallax frosted mega-arch silhouette girders & crystalline icicle fringe."""
    w, h = 640, 360
    arr = make_rgba(w, h)
    # Diagonal heavy server-hall support trusses
    for sx in [90, 410]:
        for y in range(h):
            cx = int(sx + (y - 180) * 0.16)
            rect(arr, cx, y, cx + 22, y + 1, (14, 22, 38, 215))
            rect(arr, cx, y, cx + 2, y + 1, (54, 108, 152, 210))
            if y % 36 < 4:
                rect(arr, cx + 3, y, cx + 19, y + 1, (30, 52, 82, 220))
    # Overhead sagging frosted power conduits & upper icicle cluster silhouettes
    for x in range(w):
        sag = int(math.sin((x % 320) / 320.0 * math.pi) * 22.0)
        rect(arr, x, 30 + sag, x + 1, 33 + sag, (16, 26, 44, 215))
        rect(arr, x, 30 + sag, x + 1, 31 + sag, (72, 140, 188, 185))
    return save_png("cryo_bg_near.png", arr)


def build_tile_metal(raw_path: Path) -> Image.Image:
    """64x64 seamless frosted dark-titanium 2085 server armor tiles."""
    raw = Image.open(raw_path).convert("RGB")
    im = raw.resize((64, 64), Image.Resampling.LANCZOS)
    q = im.quantize(colors=40, method=Image.Quantize.MEDIANCUT).convert("RGBA")
    arr = np.array(q)
    # Crisp dark bevel border so tiled blocks lock seamlessly on 64x64 grid
    arr[0, :, :3] = np.clip(arr[0, :, :3].astype(int) + 18, 0, 255)
    arr[-1, :, :3] = (arr[-1, :, :3] * 0.55).astype(np.uint8)
    arr[:, 0, :3] = (arr[:, 0, :3] * 0.72).astype(np.uint8)
    arr[:, -1, :3] = (arr[:, -1, :3] * 0.55).astype(np.uint8)
    return save_png("cryo_tile_metal.png", arr)


def build_tile_trim_top() -> Image.Image:
    """64x12 frosted titanium deck trim with glacial-cyan coolant rail & ice-blue frost edge."""
    w, h = 64, 12
    arr = make_rgba(w, h)
    rect(arr, 0, 0, w, 2, (205, 245, 255, 255))  # Crisp frost specular lip
    rect(arr, 0, 2, w, 4, (84, 206, 248, 255))   # Glacial cyan conduit line
    rect(arr, 0, 4, w, 9, (34, 52, 78, 255))     # Frosted brushed titanium fascia
    rect(arr, 0, 9, w, 12, (16, 26, 42, 255))   # Deep under-shadow
    for x in range(0, w, 16):
        # Diagonal arctic-blue / slate server caution chevrons
        for dy in range(4):
            for dx in range(6):
                put(arr, (x + dx + dy) % w, 5 + dy, (58, 138, 198, 255))
        put(arr, x + 2, 1, (245, 254, 255, 255))
    return save_png("cryo_tile_trim_top.png", arr)


def build_tile_ice_top() -> Image.Image:
    """64x14 shimmering low-friction MIRROR-ICE cap with sparkling crystalline highlights & icicles!"""
    w, h = 64, 14
    arr = make_rgba(w, h)
    rect(arr, 0, 0, w, 2, (238, 252, 255, 255))   # Gleaming mirror-ice top polish!
    rect(arr, 0, 2, w, 6, (132, 232, 255, 250))   # Translucent sapphire-cyan crystal body
    rect(arr, 0, 6, w, 9, (68, 178, 238, 240))    # Deep glacial blue ice core
    # Diagonal mirror specular reflection streaks
    for x in range(0, w, 16):
        for k in range(7):
            put(arr, (x + k) % w, min(8, 1 + k), (235, 252, 255, 245))
            put(arr, (x + k + 1) % w, min(8, 1 + k), (185, 244, 255, 220))
    # Jagged pixel-art hanging ice teeth underneath the mirror-ice cap!
    for x in range(w):
        tooth = 1 + int(abs(math.sin(x * 0.45)) * 4.2)
        for dy in range(tooth):
            y = 9 + dy
            if y < h:
                col = (182, 242, 255, 235) if dy == 0 else (92, 198, 245, 210)
                put(arr, x, y, col)
    return save_png("cryo_tile_ice_top.png", arr)


def build_tile_wall() -> Image.Image:
    """32x64 vertical fluted cryogenic server bulkhead with glowing coolant tube & wall-grip LEDs."""
    w, h = 32, 64
    arr = make_rgba(w, h)
    rect(arr, 0, 0, w, h, (30, 44, 68, 255))
    rect(arr, 0, 0, 3, h, (112, 206, 246, 255))
    rect(arr, w - 3, 0, w, h, (112, 206, 246, 255))
    # Center glowing nitrogen coolant channel
    rect(arr, 12, 0, 20, h, (18, 30, 50, 255))
    rect(arr, 14, 0, 18, h, (68, 188, 242, 255))
    rect(arr, 15, 0, 17, h, (195, 246, 255, 255))
    for y in range(0, h, 16):
        rect(arr, 3, y, w - 3, y + 3, (56, 78, 110, 255))
        rect(arr, 3, y + 3, w - 3, y + 4, (14, 22, 38, 255))
        rect(arr, 5, y + 7, 9, y + 10, (92, 228, 255, 255))
        rect(arr, 23, y + 7, 27, y + 10, (92, 228, 255, 255))
    return save_png("cryo_tile_wall.png", arr)


def build_platform_oneway() -> Image.Image:
    """64x12 crystalline frosted titanium server-rack catwalk with glowing cyan rail."""
    w, h = 64, 12
    arr = make_rgba(w, h)
    rect(arr, 0, 0, w, 2, (206, 246, 255, 255))
    rect(arr, 0, 2, w, 4, (64, 198, 246, 255))
    rect(arr, 0, 4, w, 6, (34, 54, 82, 255))
    rect(arr, 0, 10, w, 12, (24, 40, 64, 255))
    for x in range(0, w, 16):
        rect(arr, x, 5, x + 2, 11, (68, 102, 138, 255))
        for k in range(6):
            put(arr, (x + 2 + k) % w, 5 + k, (52, 86, 126, 235))
            put(arr, (x + 14 - k) % w, 5 + k, (52, 86, 126, 235))
    return save_png("cryo_platform_oneway.png", arr)


def build_platform_moving() -> Image.Image:
    """64x16 sleek white-cobalt cryo mag-lev data gondola with glowing glacial hover vents."""
    w, h = 64, 16
    arr = make_rgba(w, h)
    rect(arr, 2, 0, w - 2, 3, (224, 250, 255, 255))
    rect(arr, 0, 3, w, 10, (44, 70, 106, 255))
    rect(arr, 2, 4, w - 2, 8, (76, 112, 156, 255))
    rect(arr, 4, 10, w - 4, 13, (24, 42, 68, 255))
    for tx in [8, 20, 36, 48]:
        rect(arr, tx, 11, tx + 8, 15, (86, 232, 255, 255))
        rect(arr, tx + 2, 12, tx + 6, 14, (230, 252, 255, 255))
        rect(arr, tx + 2, 5, tx + 6, 7, (118, 236, 255, 255))
    return save_png("cryo_platform_moving.png", arr)


def build_breakable_ice() -> Image.Image:
    """32x64 shatterable glacial-blue crystal ice bulkhead with glowing frost fracture veins."""
    w, h = 32, 64
    arr = make_rgba(w, h)
    for y in range(h):
        for x in range(w):
            facet = ((x // 8 + y // 8) % 2) * 18
            base_r = 58 + facet
            base_g = 142 + facet
            base_b = 212 + min(43, facet)
            if x in (0, w - 1) or y in (0, h - 1):
                put(arr, x, y, (28, 64, 112, 255))
            else:
                put(arr, x, y, (base_r, base_g, base_b, 245))
    # Jagged glowing white-cyan stress fracture cracks showing SMASHABLE/MELTABLE ice!
    cracks = [
        (4, 8, 18, 20), (18, 20, 11, 36), (11, 36, 26, 48), (26, 48, 14, 60),
        (18, 20, 27, 15), (11, 36, 3, 42),
    ]
    for x0, y0, x1, y1 in cracks:
        steps = max(abs(x1 - x0), abs(y1 - y0))
        for s in range(steps + 1):
            t = s / max(1, steps)
            cx = int(round(x0 + (x1 - x0) * t))
            cy = int(round(y0 + (y1 - y0) * t))
            rect(arr, cx - 1, cy - 1, cx + 2, cy + 2, (145, 236, 255, 255))
            put(arr, cx, cy, (245, 254, 255, 255))
    return save_png("cryo_breakable_wall.png", arr)


def build_boost_pad() -> Image.Image:
    """64x12 cryo superconductor speed rail with blazing ice-white/cyan `>>>` chevrons."""
    w, h = 64, 12
    arr = make_rgba(w, h)
    rect(arr, 0, 2, w, h, (22, 38, 62, 255))
    rect(arr, 0, 0, w, 2, (112, 224, 255, 255))
    for cx in range(6, w - 6, 14):
        for dy in range(4):
            put(arr, cx + dy, 3 + dy, (220, 252, 255, 255))
            put(arr, cx + dy + 1, 3 + dy, (90, 234, 255, 255))
            put(arr, cx + dy, 9 - dy, (220, 252, 255, 255))
            put(arr, cx + dy + 1, 9 - dy, (90, 234, 255, 255))
    return save_png("cryo_boost_pad.png", arr)


def build_jump_pad() -> Image.Image:
    """48x14 pressurized liquid-nitrogen pneumatic cryo spring pad."""
    w, h = 48, 14
    arr = make_rgba(w, h)
    rect(arr, 4, 10, w - 4, 14, (26, 42, 66, 255))
    rect(arr, 10, 5, 16, 10, (124, 212, 246, 255))
    rect(arr, 32, 5, 38, 10, (124, 212, 246, 255))
    rect(arr, 2, 0, w - 2, 5, (72, 196, 248, 255))
    rect(arr, 4, 0, w - 4, 2, (232, 252, 255, 255))
    rect(arr, 18, 2, 30, 5, (148, 242, 255, 255))
    return save_png("cryo_jump_pad.png", arr)


def build_spikes() -> Image.Image:
    """32x16 razor crystalline sub-zero icicle spikes."""
    w, h = 32, 16
    arr = make_rgba(w, h)
    rect(arr, 0, 13, w, 16, (28, 52, 86, 255))
    for sx in [0, 8, 16, 24]:
        for y in range(13):
            half = max(0, int((y / 12.0) * 3.8))
            cx = sx + 4
            for x in range(cx - half, cx + half + 1):
                if y < 4:
                    col = (238, 252, 255, 255)
                elif x <= cx:
                    col = (128, 224, 255, 255)
                else:
                    col = (56, 142, 214, 255)
                put(arr, x, y, col)
    return save_png("cryo_spikes.png", arr)


def build_turret() -> Image.Image:
    """24x24 frosted frost-white & sapphire 2085 Cryo security turret."""
    w, h = 24, 24
    arr = make_rgba(w, h)
    rect(arr, 2, 4, 18, 20, (48, 72, 106, 255))
    rect(arr, 4, 6, 16, 18, (158, 194, 228, 255))
    rect(arr, 14, 9, 24, 15, (42, 66, 98, 255))
    rect(arr, 21, 10, 24, 14, (112, 236, 255, 255))
    rect(arr, 7, 9, 13, 15, (20, 34, 56, 255))
    rect(arr, 9, 11, 11, 13, (124, 244, 255, 255))
    return save_png("cryo_turret.png", arr)


def build_checkpoint() -> Image.Image:
    """20x44 crystalline frosted titanium resonance pylon."""
    w, h = 20, 44
    arr = make_rgba(w, h)
    rect(arr, 2, 38, 18, 44, (36, 56, 84, 255))
    rect(arr, 5, 14, 15, 38, (54, 84, 122, 255))
    rect(arr, 8, 16, 12, 36, (108, 224, 255, 255))
    rect(arr, 3, 10, 17, 15, (156, 214, 246, 255))
    return save_png("cryo_checkpoint.png", arr)


def build_nitrogen_strip() -> Image.Image:
    """128x28 billowing sub-zero liquid-nitrogen fog wave horizon."""
    w, h = 128, 28
    arr = make_rgba(w, h)
    for x in range(w):
        crest = int(6 + 4.0 * math.sin(x / w * math.tau * 2.0) + 2.0 * math.cos(x / w * math.tau * 5.0))
        for y in range(crest, h):
            d = y - crest
            if d < 2:
                put(arr, x, y, (206, 246, 255, 220))
            elif d < 7:
                put(arr, x, y, (98, 206, 248, 215))
            elif d < 15:
                put(arr, x, y, (42, 132, 206, 225))
            else:
                put(arr, x, y, (22, 74, 142, 235))
    return save_png("cryo_lava_strip.png", arr)


def build_nitrogen_cascade() -> Image.Image:
    """24x128 pouring translucent liquid-nitrogen cryogenic waterfall."""
    w, h = 24, 128
    arr = make_rgba(w, h)
    for y in range(h):
        for x in range(w):
            cx = abs(x - (w - 1) * 0.5)
            wob = math.sin(y * 0.22 + x * 0.35) * 1.4
            if cx <= 9.5 + wob:
                if cx < 3.0:
                    put(arr, x, y, (212, 248, 255, 220))
                elif cx < 6.5:
                    put(arr, x, y, (102, 212, 250, 195))
                else:
                    put(arr, x, y, (46, 136, 208, 145))
    return save_png("cryo_molten_cascade.png", arr)


def build_lamp_fixture() -> Image.Image:
    """28x14 frosted titanium cryo light-bar hood."""
    w, h = 28, 14
    arr = make_rgba(w, h)
    rect(arr, 11, 0, 17, 4, (44, 66, 98, 255))
    rect(arr, 4, 4, 24, 9, (64, 94, 132, 255))
    rect(arr, 2, 9, 26, 12, (148, 216, 248, 255))
    rect(arr, 6, 12, 22, 14, (224, 250, 255, 255))
    return save_png("cryo_lamp_fixture.png", arr)


def prep_borealis_portrait(raw_path: Path) -> None:
    PORTRAIT_DIR.mkdir(parents=True, exist_ok=True)
    im = Image.open(raw_path).convert("RGBA")
    im = im.resize((128, 128), Image.Resampling.LANCZOS)
    im.save(PORTRAIT_DIR / "borealis.png")
    print(f"  [UI]   assets/ui/portraits/borealis.png (128x128)")


def build_preview(bg_far: Image.Image, bg_mid: Image.Image, tile: Image.Image, ice_top: Image.Image, break_ice: Image.Image, nitro: Image.Image, casc: Image.Image) -> None:
    SHOWCASE_DIR.mkdir(parents=True, exist_ok=True)
    canvas = Image.new("RGBA", (640, 360), (10, 18, 34, 255))
    canvas.alpha_composite(bg_far)
    canvas.alpha_composite(bg_mid)
    # Pouring nitrogen waterfall
    for y in range(0, 360, 128):
        canvas.alpha_composite(casc, (490, y))
    # Mirror-ice server deck
    for x in range(32, 420, 64):
        canvas.alpha_composite(tile, (x, 260))
        canvas.alpha_composite(ice_top, (x, 258))
    canvas.alpha_composite(break_ice, (340, 196))
    for x in range(0, 640, 128):
        canvas.alpha_composite(nitro, (x, 332))
    out = canvas.resize((1280, 720), Image.Resampling.NEAREST)
    out.save(SHOWCASE_DIR / "stage2_cryo_preview.png")
    print(f"  [QA]   {SHOWCASE_DIR / 'stage2_cryo_preview.png'}")


def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("--far-raw", required=True, type=Path)
    p.add_argument("--tile-raw", required=True, type=Path)
    p.add_argument("--portrait-raw", required=True, type=Path)
    args = p.parse_args()

    bg_far = build_bg_far(args.far_raw)
    bg_mid = build_bg_mid()
    build_bg_near()
    tile = build_tile_metal(args.tile_raw)
    build_tile_trim_top()
    ice_top = build_tile_ice_top()
    build_tile_wall()
    build_platform_oneway()
    build_platform_moving()
    break_ice = build_breakable_ice()
    build_boost_pad()
    build_jump_pad()
    build_spikes()
    build_turret()
    build_checkpoint()
    nitro = build_nitrogen_strip()
    casc = build_nitrogen_cascade()
    build_lamp_fixture()
    prep_borealis_portrait(args.portrait_raw)
    build_preview(bg_far, bg_mid, tile, ice_top, break_ice, nitro, casc)


if __name__ == "__main__":
    main()
