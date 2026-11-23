#!/usr/bin/env python3
"""Uses Pillow + `ffmpeg` to assemble a synchronized multi-character Story Bible showcase
video (`raw_assets/showcases/velocity_breaker_roster.mp4` + `.gif`) and high-res 6-character
pixel-art roster contact sheet (`raw_assets/showcases/roster_sheet.png`).

Usage:
    python3 tools/make_roster_reel.py
"""

from __future__ import annotations

import pathlib
import re
import shutil
import subprocess
import tempfile

from PIL import Image, ImageDraw

CHARACTERS = [
    ("LEVY (HERO)", "assets/sprites/hero", "assets/ui/portraits/levy.png", (55, 225, 255)),
    ("IRON TITAN: VULCANUS", "assets/sprites/boss", "assets/ui/portraits/vulcanus.png", (255, 140, 38)),
    ("STRIDER DRONE MK-II", "assets/sprites/rogue", None, (255, 68, 86)),
    ("SLAG ENFORCER", "assets/sprites/barbarian", None, (255, 198, 48)),
    ("ARC WEAVER", "assets/sprites/mage", None, (198, 108, 255)),
    ("PHANTOM INTERCEPTOR", "assets/sprites/assassin", None, (255, 76, 220)),
]

REEL_CLIPS = [
    ("idle", 1),
    ("run", 1),
    ("telegraph", 1),
    ("jab", 1),
    ("jab_2", 1),
    ("f_smash", 1),
    ("sword_spin", 1),
    ("buster", 1),
]


def parse_manifest(ron_path: pathlib.Path) -> tuple[tuple[int, int], int, dict[str, tuple[int, int]]]:
    text = ron_path.read_text(encoding="utf-8")
    m_size = re.search(r"frame_size:\s*\((\d+),\s*(\d+)\)", text)
    m_cols = re.search(r"columns:\s*(\d+)", text)
    assert m_size and m_cols
    fw, fh = int(m_size.group(1)), int(m_size.group(2))
    cols = int(m_cols.group(1))
    anims: dict[str, tuple[int, int]] = {}
    for m in re.finditer(r'"([a-z0-9_]+)":\s*AnimationDef\(\s*start:\s*(\d+),\s*end:\s*(\d+)', text):
        anims[m.group(1)] = (int(m.group(2)), int(m.group(3)))
    return (fw, fh), cols, anims


def crop_idx(sheet: Image.Image, idx: int, fw: int, fh: int, cols: int) -> Image.Image:
    x = (idx % cols) * fw
    y = (idx // cols) * fh
    return sheet.crop((x, y, x + fw, y + fh))


def build_composite_reel() -> None:
    out_dir = pathlib.Path("raw_assets/showcases")
    out_dir.mkdir(parents=True, exist_ok=True)

    loaded = []
    for label, sp_dir_str, port_path, accent in CHARACTERS:
        sp_dir = pathlib.Path(sp_dir_str)
        name = sp_dir.name
        ron = sp_dir / f"{name}.ron"
        if not ron.exists():
            continue
        (fw, fh), cols, anims = parse_manifest(ron)
        diff = Image.open(sp_dir / f"{name}_diffuse.png").convert("RGBA")
        emis = Image.open(sp_dir / f"{name}_emission.png").convert("RGBA")
        port = Image.open(port_path).convert("RGBA") if (port_path and pathlib.Path(port_path).exists()) else None
        loaded.append((label, accent, fw, fh, cols, anims, diff, emis, port))

    # Build a crisp 3x2 animated grid (cell = 288x320) -> 900x692 even-dimension video canvas
    cell_w, cell_h = 288, 316
    pad = 12
    title_h = 44
    canvas_w = cell_w * 3 + pad * 4
    canvas_h = title_h + cell_h * 2 + pad * 3
    if canvas_w % 2:
        canvas_w += 1
    if canvas_h % 2:
        canvas_h += 1

    # Build synchronized clip sequence (all characters play the same clip name together!)
    seq: list[tuple[str, int]] = []
    for clip_name, loops in REEL_CLIPS:
        for _ in range(loops):
            for step in range(8):
                seq.append((clip_name, step))

    tmp = pathlib.Path(tempfile.mkdtemp(prefix="vbreaker_roster_"))
    try:
        for f_i, (clip_name, step) in enumerate(seq):
            canvas = Image.new("RGB", (canvas_w, canvas_h), (10, 14, 22))
            draw = ImageDraw.Draw(canvas)
            draw.rectangle([0, 0, canvas_w, title_h], fill=(16, 22, 36))
            draw.line([0, title_h, canvas_w, title_h], fill=(55, 225, 255), width=2)
            draw.text(
                (16, 15),
                f"VELOCITY // BREAKER  •  STORY BIBLE 2085 ROSTER SHOWCASE   |   ANIMATION: {clip_name.upper()}",
                fill=(55, 225, 255),
            )

            for idx, (label, accent, fw, fh, cols, anims, diff, emis, port) in enumerate(loaded[:6]):
                gx = idx % 3
                gy = idx // 3
                x0 = pad + gx * (cell_w + pad)
                y0 = title_h + pad + gy * (cell_h + pad)
                draw.rectangle([x0, y0, x0 + cell_w, y0 + cell_h], fill=(18, 24, 36), outline=accent, width=2)
                draw.rectangle([x0 + 2, y0 + 2, x0 + cell_w - 2, y0 + 26], fill=(14, 18, 28))
                draw.text((x0 + 10, y0 + 8), label, fill=accent)

                start, end = anims.get(clip_name, anims["idle"])
                span = max(1, end - start + 1)
                frame_id = start + (step % span)

                scale = 2 if fw >= 144 else 3
                sw, sh = fw * scale, fh * scale
                fd = crop_idx(diff, frame_id, fw, fh, cols).resize((sw, sh), Image.Resampling.NEAREST)
                fe = crop_idx(emis, frame_id, fw, fh, cols).resize((sw, sh), Image.Resampling.NEAREST)

                # Composite bright additive-style glow preview over diffuse for showcase clarity
                px_x = x0 + (cell_w - sw) // 2
                px_y = y0 + 26 + (cell_h - 26 - sh) // 2
                canvas.paste(fd, (px_x, px_y), fd)
                # Blend non-black emission pixels brightly on top
                e_px = fe.load()
                c_px = canvas.load()
                for yy in range(sh):
                    for xx in range(sw):
                        er, eg, eb, ea = e_px[xx, yy]
                        if ea > 0 and (er + eg + eb) > 24:
                            tx, ty = px_x + xx, px_y + yy
                            if 0 <= tx < canvas_w and 0 <= ty < canvas_h:
                                cr, cg, cb = c_px[tx, ty]
                                c_px[tx, ty] = (min(255, cr + er), min(255, cg + eg), min(255, cb + eb))

                if port is not None:
                    p_small = port.resize((52, 52), Image.Resampling.NEAREST)
                    canvas.paste(p_small, (x0 + cell_w - 60, y0 + 32), p_small)
                    draw.rectangle([x0 + cell_w - 61, y0 + 31, x0 + cell_w - 8, y0 + 84], outline=accent)

            frame_path = tmp / f"f_{f_i:04d}.png"
            canvas.save(frame_path)
            if f_i == 26:  # jab peak impact frame with glowing 2D SlashSmear across all 6 fighters!
                canvas.save(out_dir / "roster_sheet.png")

        mp4_out = out_dir / "velocity_breaker_roster.mp4"
        gif_out = out_dir / "velocity_breaker_roster.gif"
        subprocess.run(
            [
                "ffmpeg", "-y",
                "-framerate", "12",
                "-i", str(tmp / "f_%04d.png"),
                "-c:v", "libx264",
                "-pix_fmt", "yuv420p",
                "-crf", "16",
                str(mp4_out),
            ],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
        )
        subprocess.run(
            [
                "ffmpeg", "-y",
                "-framerate", "12",
                "-i", str(tmp / "f_%04d.png"),
                "-vf", "split[s0][s1];[s0]palettegen=max_colors=160[p];[s1][p]paletteuse=dither=none",
                str(gif_out),
            ],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
        )
        print(f"[roster_reel] wrote {mp4_out}, {gif_out}, and {out_dir / 'roster_sheet.png'}")
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    build_composite_reel()
