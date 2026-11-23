#!/usr/bin/env python3
"""
Builds the 32-bit pixel-art VFX sprite suite (`assets/textures/vfx/`) and the
20 studio-grade multi-layered 44.1 kHz DSP sound effects (`assets/audio/sfx/`)
for *VELOCITY // BREAKER*, eliminating blocky placeholder rectangles and raw
single-oscillator beeps.
"""

from __future__ import annotations

import math
import os
import wave
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
VFX_DIR = ROOT / "assets" / "textures" / "vfx"
SFX_DIR = ROOT / "assets" / "audio" / "sfx"
SHOWCASE_DIR = ROOT / "raw_assets" / "showcases"

SR = 44_100


# =============================================================================
# 1. 32-BIT PIXEL-ART VFX TEXTURE GENERATOR
# =============================================================================


def make_canvas(w: int, h: int) -> np.ndarray:
    return np.zeros((h, w, 4), dtype=np.uint8)


def blend_px(arr: np.ndarray, x: int, y: int, rgba: tuple[int, int, int, int]) -> None:
    h, w, _ = arr.shape
    if not (0 <= x < w and 0 <= y < h):
        return
    sr, sg, sb, sa = rgba
    if sa <= 0:
        return
    if sa >= 255 or arr[y, x, 3] == 0:
        arr[y, x] = [sr, sg, sb, sa]
        return
    a = sa / 255.0
    da = arr[y, x, 3] / 255.0
    out_a = a + da * (1.0 - a)
    if out_a <= 1e-5:
        return
    arr[y, x, 0] = int(min(255, (sr * a + arr[y, x, 0] * da * (1.0 - a)) / out_a))
    arr[y, x, 1] = int(min(255, (sg * a + arr[y, x, 1] * da * (1.0 - a)) / out_a))
    arr[y, x, 2] = int(min(255, (sb * a + arr[y, x, 2] * da * (1.0 - a)) / out_a))
    arr[y, x, 3] = int(min(255, out_a * 255.0))


def save_vfx(name: str, arr: np.ndarray) -> Image.Image:
    VFX_DIR.mkdir(parents=True, exist_ok=True)
    img = Image.fromarray(arr, mode="RGBA")
    img.save(VFX_DIR / name)
    return img


def build_buster_pellet() -> Image.Image:
    """18x10 tapered glowing Rift Buster plasma dart pointing right (+X)."""
    w, h = 18, 10
    arr = make_canvas(w, h)
    cy = (h - 1) * 0.5
    for y in range(h):
        for x in range(w):
            # Normalized x: 0 (tail) -> 1 (nose)
            nx = x / (w - 1)
            dy = abs(y - cy)
            # Teardrop/dart envelope: narrow tail, plump glowing head, pointed tip
            if nx > 0.72:
                half_h = 3.8 * math.sqrt(max(0.0, 1.0 - ((nx - 0.72) / 0.28) ** 2))
            else:
                half_h = 1.0 + 2.8 * (nx / 0.72) ** 0.75
            if dy <= half_h + 0.6:
                radial = dy / max(half_h, 0.25)
                if radial < 0.42 and nx > 0.30:
                    # White-hot plasma core
                    blend_px(arr, x, y, (245, 254, 255, 255))
                elif radial < 0.78 and nx > 0.14:
                    # Bright electric cyan body
                    blend_px(arr, x, y, (85, 232, 255, 240))
                else:
                    # Soft outer cyan corona
                    a = int(max(0.0, 1.0 - (radial - 0.55) / 0.65) * (0.45 + 0.55 * nx) * 210)
                    blend_px(arr, x, y, (28, 168, 255, a))
    # Forked trailing energy wisps
    for tx, ty, a in [(0, 4, 130), (1, 3, 120), (1, 6, 120), (2, 4, 190), (2, 5, 190)]:
        blend_px(arr, tx, ty, (70, 220, 255, a))
    return save_vfx("vfx_buster_pellet.png", arr)


def build_buster_charged() -> Image.Image:
    """36x22 roaring Rift Buster Charged comet blast pointing right (+X)."""
    w, h = 36, 22
    arr = make_canvas(w, h)
    cy = (h - 1) * 0.5
    for y in range(h):
        for x in range(w):
            nx = x / (w - 1)
            dy = y - cy
            ady = abs(dy)
            # Sculpted comet head + undulating plasma wake
            if nx >= 0.66:
                u = (nx - 0.66) / 0.34
                half_h = 9.2 * math.sqrt(max(0.0, 1.0 - u * u))
            else:
                wave_mod = math.sin(nx * 14.0 + (1.0 if dy >= 0 else -1.0) * 1.2) * 1.4
                half_h = (2.2 + 7.2 * (nx / 0.66) ** 0.7) + wave_mod * (1.0 - nx)
            if half_h <= 0.1:
                continue
            r = ady / half_h
            if r <= 1.08:
                # Inner golden-white piercing core near nose
                core_dx = (x - 24.5) / 8.5
                core_dy = dy / 4.2
                core_d = math.hypot(core_dx, core_dy)
                if core_d < 0.78:
                    blend_px(arr, x, y, (255, 253, 232, 255))
                elif core_d < 1.12 or (r < 0.40 and nx > 0.22):
                    blend_px(arr, x, y, (185, 248, 255, 255))
                elif r < 0.78:
                    a = int((0.35 + 0.65 * nx) * 245)
                    blend_px(arr, x, y, (42, 212, 255, a))
                else:
                    a = int(max(0.0, 1.0 - (r - 0.72) / 0.38) * (0.25 + 0.75 * nx) * 200)
                    blend_px(arr, x, y, (22, 130, 245, a))
    # Crisp helical energy rings wrapping the plasma comet
    for rx in [12, 18, 24]:
        for y in range(2, h - 2):
            dy = abs(y - cy)
            if 3.0 <= dy <= 8.2:
                blend_px(arr, rx + int(math.sin(y * 0.5) * 1.2), y, (215, 252, 255, 195))
    return save_vfx("vfx_buster_charged.png", arr)


def build_turret_bolt() -> Image.Image:
    """14x14 glowing spherical crimson-rose plasma bolt."""
    w, h = 14, 14
    arr = make_canvas(w, h)
    cx, cy = (w - 1) * 0.5, (h - 1) * 0.5
    for y in range(h):
        for x in range(w):
            d = math.hypot(x - cx, y - cy) / 6.5
            if d < 0.34:
                blend_px(arr, x, y, (255, 242, 245, 255))
            elif d < 0.62:
                blend_px(arr, x, y, (255, 92, 122, 250))
            elif d < 0.92:
                a = int((1.0 - (d - 0.60) / 0.36) * 225)
                blend_px(arr, x, y, (215, 26, 68, a))
            elif d < 1.08:
                a = int((1.0 - (d - 0.90) / 0.20) * 110)
                blend_px(arr, x, y, (165, 14, 48, a))
    return save_vfx("vfx_turret_bolt.png", arr)


def build_arcane_orb() -> Image.Image:
    """18x18 sculpted violet-magenta arcane star-orb with bright diamond core & rune ring."""
    w, h = 18, 18
    arr = make_canvas(w, h)
    cx, cy = (w - 1) * 0.5, (h - 1) * 0.5
    for y in range(h):
        for x in range(w):
            dx, dy = x - cx, y - cy
            d = math.hypot(dx, dy) / 8.2
            ang = math.atan2(dy, dx)
            star = 0.14 * math.cos(ang * 4.0)
            sd = d - star
            if abs(dx) + abs(dy) <= 3.1:
                blend_px(arr, x, y, (252, 232, 255, 255))
            elif sd < 0.55:
                blend_px(arr, x, y, (212, 82, 255, 250))
            elif 0.70 < d < 0.88 and (int((ang + math.pi) * 6.0) % 2 == 0):
                blend_px(arr, x, y, (238, 152, 255, 210))
            elif sd < 0.95:
                a = int((1.0 - (sd - 0.52) / 0.45) * 210)
                blend_px(arr, x, y, (148, 32, 224, a))
    return save_vfx("vfx_arcane_orb.png", arr)


def build_magma_wave() -> Image.Image:
    """30x22 cresting molten furnace shockwave flame moving right (+X)."""
    w, h = 30, 22
    arr = make_canvas(w, h)
    for y in range(h):
        ny = 1.0 - y / (h - 1)  # 0=bottom, 1=top
        for x in range(w):
            nx = x / (w - 1)  # 0=back, 1=front crest
            # Curved breaking wave silhouette
            crest_x = 0.72 + 0.22 * math.sin(ny * math.pi * 0.85) - 0.14 * (ny ** 2)
            tail_x = 0.04 + 0.52 * (ny ** 1.35) + 0.07 * math.sin(ny * 13.0)
            if tail_x <= nx <= crest_x:
                span = max(0.08, crest_x - tail_x)
                rel_front = (nx - tail_x) / span  # 0=tail -> 1=leading crest
                if rel_front > 0.78 and ny < 0.86:
                    blend_px(arr, x, y, (255, 248, 186, 255))
                elif rel_front > 0.45:
                    blend_px(arr, x, y, (255, 148, 36, 250))
                elif rel_front > 0.16:
                    blend_px(arr, x, y, (224, 62, 18, 225))
                else:
                    blend_px(arr, x, y, (148, 24, 10, 165))
    # Detached trailing ember sparks
    for ex, ey in [(2, 18), (5, 14), (8, 10), (12, 6), (4, 19), (10, 15)]:
        blend_px(arr, ex, ey, (255, 196, 62, 220))
    return save_vfx("vfx_magma_wave.png", arr)


def build_emitter_nozzle() -> Image.Image:
    """32x12 armored 2085 industrial plasma beam emitter pedestal with glowing aperture."""
    w, h = 32, 12
    arr = make_canvas(w, h)
    for y in range(h):
        for x in range(w):
            # Trapezoidal armored base wider at bottom, emitter crown at top
            inset = int((1.0 - y / (h - 1)) * 4.0)
            if x < inset or x >= w - inset:
                continue
            if y == h - 1 or x == inset or x == w - inset - 1:
                blend_px(arr, x, y, (22, 28, 40, 255))
            elif y <= 2:
                # Glowing plasma aperture ring at top
                if 7 <= x <= w - 8:
                    col = (255, 215, 120, 255) if y == 0 else (255, 118, 42, 255)
                    blend_px(arr, x, y, col)
                else:
                    blend_px(arr, x, y, (84, 98, 122, 255))
            elif y in (6, 7):
                # Caution hazard chevron band
                stripe = ((x + y) // 4) % 2 == 0
                col = (212, 144, 34, 255) if stripe else (30, 34, 44, 255)
                blend_px(arr, x, y, col)
            else:
                # Brushed gunmetal steel body with specular bevel
                shade = 68 if x < w // 2 else 52
                if y == 3:
                    shade += 24
                blend_px(arr, x, y, (shade, shade + 8, shade + 22, 255))
    # Bolts
    for bx in [6, 25]:
        blend_px(arr, bx, 9, (132, 148, 174, 255))
    return save_vfx("vfx_emitter_nozzle.png", arr)


def build_respawn_platform() -> Image.Image:
    """48x20 sleek winged holographic cyan Smash Angel Respawn pedestal."""
    w, h = 48, 20
    arr = make_canvas(w, h)
    cx = (w - 1) * 0.5
    for y in range(h):
        for x in range(w):
            dx = abs(x - cx)
            # Top hover deck (rows 2..6)
            if 2 <= y <= 6 and dx <= 21.0 - max(0, y - 3) * 2.5:
                if y == 2:
                    blend_px(arr, x, y, (220, 252, 255, 255))
                elif y == 3:
                    blend_px(arr, x, y, (68, 224, 255, 255))
                else:
                    blend_px(arr, x, y, (34, 62, 92, 245))
            # Winged light vanes flanking deck
            elif 4 <= y <= 9 and 12.0 <= dx <= 22.0 - (y - 4) * 1.4:
                blend_px(arr, x, y, (72, 216, 255, 215))
            # Central thruster core & downward light cone (rows 7..19)
            elif 7 <= y <= 18:
                cone_w = max(1.0, 9.0 * (1.0 - (y - 7) / 13.0))
                if dx <= cone_w:
                    core = 1.0 - dx / max(cone_w, 0.5)
                    v_fade = 1.0 - (y - 7) / 13.0
                    a = int(core * v_fade * 210)
                    if y <= 9 and dx <= 4.0:
                        blend_px(arr, x, y, (225, 252, 255, 235))
                    else:
                        blend_px(arr, x, y, (48, 214, 255, a))
    return save_vfx("vfx_respawn_platform.png", arr)


def build_beacon_crystal() -> Image.Image:
    """16x22 faceted luminous diamond resonance crystal (white/neutral so Sprite tint colors it!)."""
    w, h = 16, 22
    arr = make_canvas(w, h)
    cx, cy = (w - 1) * 0.5, (h - 1) * 0.5
    for y in range(h):
        for x in range(w):
            nx = (x - cx) / 6.2
            ny = (y - cy) / 9.4
            d = abs(nx) + abs(ny)
            if d <= 1.0:
                # Faceted diamond planes: bright upper-left highlight, crisp lower-right shadow
                if d < 0.38:
                    v = 255
                elif nx < 0 and ny < 0:
                    v = 248
                elif nx >= 0 and ny < 0:
                    v = 212
                elif nx < 0 and ny >= 0:
                    v = 196
                else:
                    v = 158
                if d > 0.86:
                    v = 255
                blend_px(arr, x, y, (v, v, v, 255))
            elif d <= 1.24:
                a = int((1.0 - (d - 1.0) / 0.25) * 120)
                blend_px(arr, x, y, (255, 255, 255, a))
    return save_vfx("vfx_beacon_crystal.png", arr)


def build_shadow_oval() -> Image.Image:
    """32x10 smooth pixel-dithered dark elliptical contact shadow."""
    w, h = 32, 10
    arr = make_canvas(w, h)
    cx, cy = (w - 1) * 0.5, (h - 1) * 0.5
    for y in range(h):
        for x in range(w):
            nx = (x - cx) / 14.5
            ny = (y - cy) / 4.2
            d = math.hypot(nx, ny)
            if d <= 1.0:
                a = int((1.0 - d * d) ** 1.2 * 230)
                blend_px(arr, x, y, (8, 12, 22, a))
    return save_vfx("vfx_shadow_oval.png", arr)


def build_smoke_puff() -> Image.Image:
    """16x16 soft pixel-art billowy dust/smoke cloud (white so Sprite tint colors it)."""
    w, h = 16, 16
    arr = make_canvas(w, h)
    lobes = [(7.5, 8.0, 5.4), (4.8, 9.2, 3.8), (10.6, 8.8, 4.0), (7.8, 5.6, 3.8)]
    for y in range(h):
        for x in range(w):
            best = 0.0
            for lx, ly, lr in lobes:
                d = math.hypot(x - lx, y - ly) / lr
                if d < 1.0:
                    best = max(best, 1.0 - d * d)
            if best > 0.06:
                shade = int(198 + 57 * min(1.0, best * 1.15 - (y / h) * 0.25))
                alpha = int(min(1.0, best * 1.45) * 235)
                blend_px(arr, x, y, (shade, shade, shade, alpha))
    return save_vfx("vfx_smoke_puff.png", arr)


def build_star_spark() -> Image.Image:
    """12x12 crisp 4-point pixel-art starburst spark (white so HDR Sprite tint blooms it!)."""
    w, h = 12, 12
    arr = make_canvas(w, h)
    cx, cy = (w - 1) * 0.5, (h - 1) * 0.5
    for y in range(h):
        for x in range(w):
            dx = abs(x - cx)
            dy = abs(y - cy)
            # Sharp 4-pointed star cross + bright center diamond
            cross = max(
                max(0.0, 1.0 - dx / 5.5) * max(0.0, 1.0 - dy / 1.6),
                max(0.0, 1.0 - dy / 5.5) * max(0.0, 1.0 - dx / 1.6),
            )
            core = max(0.0, 1.0 - math.hypot(dx, dy) / 2.8)
            val = min(1.0, max(cross, core))
            if val > 0.08:
                blend_px(arr, x, y, (255, 255, 255, int(val * 255)))
    return save_vfx("vfx_star_spark.png", arr)


def build_p1_marker() -> Image.Image:
    """14x14 crisp bordered glowing cyan P1 downward chevron indicator."""
    w, h = 14, 14
    arr = make_canvas(w, h)
    cx = (w - 1) * 0.5
    for y in range(h):
        for x in range(w):
            dx = abs(x - cx)
            # Top accent bar (rows 1..3)
            if 1 <= y <= 3 and dx <= 5.5:
                col = (14, 24, 38, 255) if (y == 1 or dx >= 4.8) else (220, 252, 255, 255)
                blend_px(arr, x, y, col)
            # Downward chevron triangle (rows 5..12)
            elif 5 <= y <= 12:
                half_w = (12.4 - y) * 0.80
                if dx <= half_w + 1.0:
                    if dx > half_w - 0.3 or y == 5 or y == 12:
                        blend_px(arr, x, y, (14, 24, 38, 255))
                    elif dx <= half_w * 0.45 and y <= 9:
                        blend_px(arr, x, y, (220, 252, 255, 255))
                    else:
                        blend_px(arr, x, y, (52, 224, 255, 255))
    return save_vfx("vfx_p1_marker.png", arr)


def generate_all_vfx_textures() -> None:
    builders = [
        ("vfx_buster_pellet.png", build_buster_pellet),
        ("vfx_buster_charged.png", build_buster_charged),
        ("vfx_turret_bolt.png", build_turret_bolt),
        ("vfx_arcane_orb.png", build_arcane_orb),
        ("vfx_magma_wave.png", build_magma_wave),
        ("vfx_emitter_nozzle.png", build_emitter_nozzle),
        ("vfx_respawn_platform.png", build_respawn_platform),
        ("vfx_beacon_crystal.png", build_beacon_crystal),
        ("vfx_shadow_oval.png", build_shadow_oval),
        ("vfx_smoke_puff.png", build_smoke_puff),
        ("vfx_star_spark.png", build_star_spark),
        ("vfx_p1_marker.png", build_p1_marker),
    ]
    imgs: list[tuple[str, Image.Image]] = []
    for name, fn in builders:
        imgs.append((name, fn()))
        print(f"  [VFX] {name}")

    # Build a crisp 4x contact sheet on dark Foundry slate for instant visual QA
    SHOWCASE_DIR.mkdir(parents=True, exist_ok=True)
    sheet = Image.new("RGBA", (640, 260), (14, 18, 28, 255))
    draw = ImageDraw.Draw(sheet)
    for idx, (name, im) in enumerate(imgs):
        col = idx % 4
        row = idx // 4
        cx = col * 160 + 80
        cy = row * 84 + 38
        scaled = im.resize((im.width * 3, im.height * 3), Image.Resampling.NEAREST)
        sheet.alpha_composite(scaled, (cx - scaled.width // 2, cy - scaled.height // 2))
        draw.text((col * 160 + 8, row * 84 + 68), name.replace("vfx_", "").replace(".png", ""), fill=(165, 200, 230, 255))
    sheet.save(SHOWCASE_DIR / "vfx_sprites_sheet.png")
    print(f"  [QA]  {SHOWCASE_DIR / 'vfx_sprites_sheet.png'}")


# =============================================================================
# 2. STUDIO-GRADE MULTI-LAYERED 44.1 kHz DSP WAV SFX GENERATOR
# =============================================================================


def write_wav(name: str, samples: np.ndarray) -> None:
    SFX_DIR.mkdir(parents=True, exist_ok=True)
    # Soft analog saturation & normalize peak cleanly to -1.5 dB
    x = np.tanh(samples * 1.15)
    peak = float(np.max(np.abs(x))) if len(x) > 0 else 1.0
    if peak > 1e-5:
        x = x * (0.86 / peak)
    pcm = np.clip(x * 32767.0, -32768.0, 32767.0).astype(np.int16)
    with wave.open(str(SFX_DIR / name), "wb") as wf:
        wf.setnchannels(1)
        wf.setsampwidth(2)
        wf.setframerate(SR)
        wf.writeframes(pcm.tobytes())


def rng(seed: int) -> np.random.Generator:
    return np.random.default_rng(seed)


def lowpass(x: np.ndarray, cutoff_hz: float) -> np.ndarray:
    rc = 1.0 / (2.0 * math.pi * max(20.0, cutoff_hz))
    dt = 1.0 / SR
    alpha = dt / (rc + dt)
    out = np.empty_like(x)
    y = 0.0
    for i, s in enumerate(x):
        y += alpha * (float(s) - y)
        out[i] = y
    return out


def highpass(x: np.ndarray, cutoff_hz: float) -> np.ndarray:
    return x - lowpass(x, cutoff_hz)


def bandpass(x: np.ndarray, low_hz: float, high_hz: float) -> np.ndarray:
    return lowpass(highpass(x, low_hz), high_hz)


def sweep_phase(f0: float, f1: float, n: int, curve: float = 1.0) -> np.ndarray:
    u = np.linspace(0.0, 1.0, n, dtype=np.float64)
    freqs = f0 + (f1 - f0) * (u ** curve)
    return np.cumsum(2.0 * math.pi * freqs / SR)


def add_metallic_tail(x: np.ndarray, delay_ms: float = 28.0, decay: float = 0.34) -> np.ndarray:
    d = int(SR * delay_ms / 1000.0)
    out = x.copy()
    if d < len(out):
        out[d:] += x[:-d] * decay
    if 2 * d < len(out):
        out[2 * d :] += x[: -2 * d] * (decay * decay)
    return out


def generate_all_sfx() -> None:
    specs: list[tuple[str, np.ndarray]] = []

    # 1. slash_light.wav: razor-fast Rift Saber air-slice + crisp plasma shimmer
    n = int(SR * 0.13)
    t = np.linspace(0.0, 1.0, n)
    noise = bandpass(rng(101).standard_normal(n), 1100.0, 5600.0)
    whoosh_env = np.sin(np.pi * np.clip(t * 1.4, 0.0, 1.0)) ** 1.4 * np.exp(-t * 5.2)
    blade = np.sin(sweep_phase(980.0, 260.0, n, 0.65)) * 0.45 + np.sin(sweep_phase(2200.0, 640.0, n, 0.7)) * 0.25
    specs.append(("slash_light.wav", (noise * 1.15 + blade * 0.65) * whoosh_env))

    # 2. slash_heavy.wav: weighty high-energy Rift Cleave + metallic plasma crack
    n = int(SR * 0.24)
    t = np.linspace(0.0, 1.0, n)
    noise = bandpass(rng(102).standard_normal(n), 600.0, 4200.0)
    env = np.sin(np.pi * np.clip(t * 1.35, 0.0, 1.0)) * np.exp(-t * 3.8)
    sub = np.sin(sweep_phase(240.0, 62.0, n, 0.55)) * np.exp(-t * 5.0)
    plasma = np.sin(sweep_phase(1350.0, 180.0, n, 0.6)) * 0.55 + np.sin(sweep_phase(2800.0, 410.0, n, 0.75)) * 0.30
    specs.append(("slash_heavy.wav", add_metallic_tail((noise * 1.0 + plasma * 0.75) * env + sub * 0.9, 19.0, 0.28)))

    # 3. land.wav: heavy 1700s steel-boot deck impact thud + short iron clank
    n = int(SR * 0.09)
    t = np.linspace(0.0, 1.0, n)
    thud = np.sin(sweep_phase(155.0, 44.0, n, 0.5)) * np.exp(-t * 9.5)
    clank = bandpass(rng(103).standard_normal(n), 320.0, 1800.0) * np.exp(-t * 14.0)
    specs.append(("land.wav", thud * 1.1 + clank * 0.55))

    # 4. spin_rev.wav: electric turbine Sonic spin-dash rev charge
    n = int(SR * 0.12)
    t = np.linspace(0.0, 1.0, n)
    ph = sweep_phase(210.0, 740.0, n, 1.2)
    rev = (0.6 * np.sin(ph) + 0.35 * np.sin(ph * 2.0) + 0.2 * np.sin(ph * 3.0)) * np.exp(-t * 2.4) * (1.0 - np.exp(-t * 35.0))
    whir = bandpass(rng(104).standard_normal(n), 900.0, 3200.0) * 0.22 * np.exp(-t * 3.0)
    specs.append(("spin_rev.wav", rev + whir))

    # 5. shield_block.wav: holographic hex-crystal clang + shimmering harmonic deflect ring
    n = int(SR * 0.22)
    t = np.linspace(0.0, 1.0, n)
    hit = bandpass(rng(105).standard_normal(n), 1800.0, 6500.0) * np.exp(-t * 24.0)
    bell = (
        np.sin(2 * math.pi * 880.0 * t * 0.22) * 0.55
        + np.sin(2 * math.pi * 1320.0 * t * 0.22) * 0.45
        + np.sin(2 * math.pi * 2110.0 * t * 0.22) * 0.35
    ) * np.exp(-t * 6.5)
    specs.append(("shield_block.wav", add_metallic_tail(hit * 0.8 + bell, 16.0, 0.32)))

    # 6. air_dodge.wav: crisp phase-shift wind-snap whoosh
    n = int(SR * 0.14)
    t = np.linspace(0.0, 1.0, n)
    whoosh = bandpass(rng(106).standard_normal(n), 950.0, 4800.0) * np.sin(math.pi * t) ** 0.9 * np.exp(-t * 3.5)
    tone = np.sin(sweep_phase(920.0, 260.0, n, 0.5)) * 0.32 * np.exp(-t * 6.5)
    specs.append(("air_dodge.wav", whoosh * 1.1 + tone))

    # 7. counter_boom.wav: iconic Smash Bros. golden crystal counter ping + shockwave detonation!
    n = int(SR * 0.42)
    t = np.linspace(0.0, 1.0, n)
    ping = (
        np.sin(2 * math.pi * 1760.0 * t * 0.42) * 0.6
        + np.sin(2 * math.pi * 2637.0 * t * 0.42) * 0.45
        + np.sin(2 * math.pi * 3520.0 * t * 0.42) * 0.3
    ) * np.exp(-t * 9.0)
    boom_t = np.clip((t - 0.08) / 0.92, 0.0, 1.0)
    boom_gate = (t >= 0.08).astype(np.float64)
    boom = np.sin(sweep_phase(190.0, 38.0, n, 0.55)) * np.exp(-boom_t * 4.2) * boom_gate
    crack = bandpass(rng(107).standard_normal(n), 450.0, 4800.0) * np.exp(-boom_t * 7.5) * boom_gate
    specs.append(("counter_boom.wav", add_metallic_tail(ping * 0.95 + boom * 1.15 + crack * 0.75, 24.0, 0.36)))

    # 8. ground_slam.wav: heavy seismic tremor impact + foundry steel rattle
    n = int(SR * 0.34)
    t = np.linspace(0.0, 1.0, n)
    sub = np.sin(sweep_phase(150.0, 34.0, n, 0.48)) * np.exp(-t * 4.5)
    crunch = lowpass(rng(108).standard_normal(n), 1450.0) * np.exp(-t * 6.5)
    clang = bandpass(rng(109).standard_normal(n), 1600.0, 4200.0) * np.exp(-t * 14.0)
    specs.append(("ground_slam.wav", add_metallic_tail(sub * 1.25 + crunch * 0.85 + clang * 0.45, 26.0, 0.30)))

    # 9. side_blitz.wav: electric horizontal lightning dash zip + plasma crackle
    n = int(SR * 0.20)
    t = np.linspace(0.0, 1.0, n)
    zip_s = np.sin(sweep_phase(360.0, 1420.0, n, 0.75)) * 0.65 + np.sin(sweep_phase(720.0, 2400.0, n, 0.9)) * 0.35
    crackle = highpass(rng(110).standard_normal(n), 2200.0) * (0.5 + 0.5 * np.sin(2 * math.pi * 42.0 * t * 0.20))
    env = np.sin(math.pi * np.clip(t * 1.2, 0.0, 1.0)) * np.exp(-t * 3.8)
    specs.append(("side_blitz.wav", (zip_s + crackle * 0.65) * env))

    # 10. enemy_blink.wav: sci-fi arcane phase-warp shimmer
    n = int(SR * 0.18)
    t = np.linspace(0.0, 1.0, n)
    ph = sweep_phase(1280.0, 310.0, n, 0.65)
    fm = np.sin(ph + 2.4 * np.sin(2 * math.pi * 24.0 * t * 0.18))
    env = np.sin(math.pi * t) ** 0.7 * np.exp(-t * 3.2)
    specs.append(("enemy_blink.wav", add_metallic_tail(fm * env, 22.0, 0.34)))

    # 11. boss_laser_charge.wav: menacing rising 2085 thermal cannon turbine whine + electric surge!
    n = int(SR * 0.46)
    t = np.linspace(0.0, 1.0, n)
    ph = sweep_phase(180.0, 1480.0, n, 1.85)
    harm = 0.6 * np.sin(ph) + 0.35 * np.sin(ph * 2.0) + 0.22 * np.sin(ph * 3.0)
    buzz = bandpass(rng(111).standard_normal(n), 1400.0, 5200.0) * (0.2 + 0.6 * t)
    trem = 0.75 + 0.25 * np.sin(2 * math.pi * (16.0 + 38.0 * t) * t * 0.46)
    env = np.where(t < 0.88, np.sqrt(t / 0.88), np.maximum(0.0, (1.0 - t) / 0.12))
    specs.append(("boss_laser_charge.wav", (harm + buzz * 0.35) * trem * env))

    # 12. cyclone_spin.wav: 360-degree dual-pulse Hurricane Sword Spin doppler whoosh!
    n = int(SR * 0.36)
    t = np.linspace(0.0, 1.0, n)
    doppler = 0.55 + 0.45 * np.sin(2 * math.pi * 3.5 * t)
    air = bandpass(rng(112).standard_normal(n), 800.0, 4800.0) * doppler
    hum = (np.sin(sweep_phase(340.0, 820.0, n, 0.8)) * 0.5 + np.sin(sweep_phase(680.0, 1640.0, n, 0.8)) * 0.3) * doppler
    env = np.sin(math.pi * t) ** 0.65
    specs.append(("cyclone_spin.wav", (air * 0.9 + hum * 0.65) * env))

    # 13. smash_release.wav: razor golden metallic shine + explosive air-splitting whip!
    n = int(SR * 0.26)
    t = np.linspace(0.0, 1.0, n)
    glint = (np.sin(2 * math.pi * 1568.0 * t * 0.26) + 0.6 * np.sin(2 * math.pi * 2349.0 * t * 0.26)) * np.exp(-t * 14.0)
    whip = np.sin(sweep_phase(720.0, 110.0, n, 0.5)) * np.exp(-t * 4.8)
    snap = bandpass(rng(113).standard_normal(n), 700.0, 5400.0) * np.exp(-t * 8.5)
    specs.append(("smash_release.wav", add_metallic_tail(glint * 0.6 + whip * 0.9 + snap * 0.85, 18.0, 0.28)))

    # 14. fast_fall.wav: crisp arcade star-sparkle zip downward
    n = int(SR * 0.08)
    t = np.linspace(0.0, 1.0, n)
    zip_d = np.sin(sweep_phase(1350.0, 480.0, n, 0.7)) * np.exp(-t * 8.0)
    spark = np.sin(2 * math.pi * 2600.0 * t * 0.08) * np.exp(-t * 18.0) * 0.45
    specs.append(("fast_fall.wav", zip_d + spark))

    # 15. arcane_erupt.wav: violet plasma pillar explosion + crackling upward energy surge
    n = int(SR * 0.34)
    t = np.linspace(0.0, 1.0, n)
    surge = np.sin(sweep_phase(220.0, 1160.0, n, 0.7)) * 0.6 + np.sin(sweep_phase(140.0, 48.0, n, 0.5)) * 0.8
    fizz = bandpass(rng(114).standard_normal(n), 1100.0, 6200.0) * np.exp(-t * 4.5)
    env = np.exp(-t * 3.8) * (1.0 - np.exp(-t * 45.0))
    specs.append(("arcane_erupt.wav", add_metallic_tail((surge + fizz * 0.75) * env, 24.0, 0.34)))

    # 16. jump_pad.wav: hydraulic pneumatic piston release + springy upward launch boing
    n = int(SR * 0.24)
    t = np.linspace(0.0, 1.0, n)
    piston = highpass(rng(115).standard_normal(n), 1800.0) * np.exp(-t * 16.0) * 0.65
    spring = (np.sin(sweep_phase(190.0, 860.0, n, 0.65)) * 0.75 + np.sin(sweep_phase(380.0, 1720.0, n, 0.65)) * 0.35) * np.exp(-t * 4.2)
    specs.append(("jump_pad.wav", piston + spring))

    # 17. checkpoint.wav: radiant ascending 3-note Rift resonance pylon chord!
    n = int(SR * 0.42)
    out = np.zeros(n, dtype=np.float64)
    for idx, freq in enumerate([523.25, 659.25, 1046.50]):  # C5 -> E5 -> C6
        start = int(idx * SR * 0.065)
        seg_n = n - start
        st = np.linspace(0.0, seg_n / SR, seg_n)
        tone = (np.sin(2 * math.pi * freq * st) * 0.65 + np.sin(2 * math.pi * freq * 2.0 * st) * 0.25) * np.exp(-st * 7.5)
        out[start:] += tone
    specs.append(("checkpoint.wav", add_metallic_tail(out, 32.0, 0.36)))

    # 18. collect_core.wav: crystalline high-tech diamond chime (E6 -> B6)
    n = int(SR * 0.16)
    out = np.zeros(n, dtype=np.float64)
    for idx, freq in enumerate([1318.5, 1975.5]):
        start = int(idx * SR * 0.032)
        seg_n = n - start
        st = np.linspace(0.0, seg_n / SR, seg_n)
        out[start:] += (np.sin(2 * math.pi * freq * st) * 0.7 + np.sin(2 * math.pi * freq * 2.0 * st) * 0.22) * np.exp(-st * 18.0)
    specs.append(("collect_core.wav", add_metallic_tail(out, 20.0, 0.28)))

    # 19. collect_relic.wav: triumphant 4-note shimmering 1700s keepsake arpeggio!
    n = int(SR * 0.56)
    out = np.zeros(n, dtype=np.float64)
    for idx, freq in enumerate([587.33, 739.99, 880.00, 1174.66]):  # D5 F#5 A5 D6
        start = int(idx * SR * 0.068)
        seg_n = n - start
        st = np.linspace(0.0, seg_n / SR, seg_n)
        tone = (
            np.sin(2 * math.pi * freq * st) * 0.62
            + np.sin(2 * math.pi * freq * 2.0 * st) * 0.26
            + np.sin(2 * math.pi * freq * 3.0 * st) * 0.12
        ) * np.exp(-st * 5.8)
        out[start:] += tone
    specs.append(("collect_relic.wav", add_metallic_tail(out, 38.0, 0.38)))

    # 20. wall_break.wav: explosive iron bulkhead shatter + tumbling metallic debris!
    n = int(SR * 0.38)
    t = np.linspace(0.0, 1.0, n)
    boom = np.sin(sweep_phase(170.0, 32.0, n, 0.45)) * np.exp(-t * 4.6)
    shatter = bandpass(rng(116).standard_normal(n), 380.0, 5200.0) * np.exp(-t * 5.4)
    clatter = np.zeros(n, dtype=np.float64)
    for k, delay_s in enumerate([0.04, 0.09, 0.15, 0.21]):
        s0 = int(delay_s * SR)
        m = min(int(0.06 * SR), n - s0)
        if m > 0:
            mt = np.linspace(0.0, 1.0, m)
            f = 420.0 + k * 195.0
            clatter[s0 : s0 + m] += (np.sin(2 * math.pi * f * mt * 0.06) + bandpass(rng(200 + k).standard_normal(m), 900.0, 3600.0) * 0.6) * np.exp(-mt * 11.0) * 0.35
    specs.append(("wall_break.wav", add_metallic_tail(boom * 1.2 + shatter * 0.9 + clatter, 28.0, 0.30)))

    for name, buf in specs:
        write_wav(name, buf)
        print(f"  [SFX] {name} ({len(buf)/SR:.2f}s @ {SR}Hz)")


if __name__ == "__main__":
    print("Building 32-bit pixel-art VFX textures...")
    generate_all_vfx_textures()
    print("Building 20 studio-grade 44.1 kHz DSP SFX WAVs...")
    generate_all_sfx()
    print("Done!")
