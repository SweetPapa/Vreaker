#!/usr/bin/env python3
"""Builds crisp nearest-neighbor animated MP4 & GIF character showcase reels using `ffmpeg`
directly from baked 3-pass sprite sheets (`_diffuse.png`, `_normal.png`, `_emission.png`, `.ron`).

Shows a 3-panel triptych (`DIFFUSE | NORMAL MAP | EMISSION BLOOM`) upscaled 4x nearest-neighbor
cycling cleanly through signature gameplay animations (`idle`, `run`, `jab`, `f_smash`,
`up_special`, `sword_spin`, `buster`, `taunt`) so every silhouette & glow reads clearly!

Usage:
    python3 tools/make_showcase_reel.py --sprite assets/sprites/hero --out /tmp/levy_showcase.mp4
"""

from __future__ import annotations

import argparse
import pathlib
import re
import shutil
import subprocess
import tempfile

from PIL import Image, ImageDraw

SHOWCASE_CLIPS = [
    ("idle", 2),
    ("run", 2),
    ("sprint", 2),
    ("jab", 1),
    ("jab_2", 1),
    ("jab_3", 1),
    ("f_smash", 1),
    ("up_special", 1),
    ("sword_spin", 2),
    ("buster", 1),
    ("taunt", 1),
]


def parse_ron_manifest(ron_path: pathlib.Path) -> tuple[tuple[int, int], int, dict[str, tuple[int, int, float]]]:
    text = ron_path.read_text(encoding="utf-8")
    m_size = re.search(r"frame_size:\s*\((\d+),\s*(\d+)\)", text)
    m_cols = re.search(r"columns:\s*(\d+)", text)
    if not m_size or not m_cols:
        raise ValueError(f"Could not parse header in {ron_path}")
    fw, fh = int(m_size.group(1)), int(m_size.group(2))
    cols = int(m_cols.group(1))
    anims: dict[str, tuple[int, int, float]] = {}
    for m in re.finditer(
        r'"([a-z0-9_]+)":\s*AnimationDef\(\s*start:\s*(\d+),\s*end:\s*(\d+),\s*fps:\s*([0-9.]+)',
        text,
    ):
        anims[m.group(1)] = (int(m.group(2)), int(m.group(3)), float(m.group(4)))
    return (fw, fh), cols, anims


def crop_frame(sheet: Image.Image, idx: int, fw: int, fh: int, cols: int) -> Image.Image:
    x = (idx % cols) * fw
    y = (idx // cols) * fh
    return sheet.crop((x, y, x + fw, y + fh))


def build_reel(sprite_dir: pathlib.Path, out_mp4: pathlib.Path, out_gif: pathlib.Path | None, scale: int) -> None:
    name = sprite_dir.name
    ron_path = sprite_dir / f"{name}.ron"
    (fw, fh), cols, anims = parse_ron_manifest(ron_path)

    diff = Image.open(sprite_dir / f"{name}_diffuse.png").convert("RGBA")
    norm = Image.open(sprite_dir / f"{name}_normal.png").convert("RGBA")
    emis = Image.open(sprite_dir / f"{name}_emission.png").convert("RGBA")

    pw, ph = fw * scale, fh * scale
    header_h = 28
    canvas_w = pw * 3 + 24
    canvas_h = ph + header_h + 12

    tmp = pathlib.Path(tempfile.mkdtemp(prefix="vbreaker_reel_"))
    try:
        frame_num = 0
        for anim_key, loops in SHOWCASE_CLIPS:
            if anim_key not in anims:
                continue
            start, end, _fps = anims[anim_key]
            for _ in range(loops):
                for idx in range(start, end + 1):
                    canvas = Image.new("RGB", (canvas_w, canvas_h), (12, 16, 26))
                    draw = ImageDraw.Draw(canvas)
                    draw.rectangle([0, 0, canvas_w, header_h], fill=(20, 28, 44))
                    draw.text((10, 8), f"{name.upper()}  |  CLIP: {anim_key.upper()}  [#{idx:03d}]", fill=(55, 225, 255))
                    draw.text((pw + 18, 8), "NORMAL MAP", fill=(180, 195, 230))
                    draw.text((pw * 2 + 24, 8), "HDR EMISSION", fill=(255, 180, 60))

                    panels = [
                        (crop_frame(diff, idx, fw, fh, cols), 6),
                        (crop_frame(norm, idx, fw, fh, cols), pw + 12),
                        (crop_frame(emis, idx, fw, fh, cols), pw * 2 + 18),
                    ]
                    for panel_img, px in panels:
                        # Subtle checkerboard / dark studio cell backdrop
                        draw.rectangle([px, header_h + 4, px + pw, header_h + 4 + ph], fill=(16, 20, 32), outline=(42, 58, 84))
                        up = panel_img.resize((pw, ph), Image.Resampling.NEAREST)
                        canvas.paste(up, (px, header_h + 4), up)

                    canvas.save(tmp / f"f_{frame_num:04d}.png")
                    frame_num += 1

        if frame_num == 0:
            raise RuntimeError(f"No frames exported from {sprite_dir}")

        out_mp4.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(
            [
                "ffmpeg", "-y",
                "-framerate", "14",
                "-i", str(tmp / "f_%04d.png"),
                "-c:v", "libx264",
                "-pix_fmt", "yuv420p",
                "-crf", "16",
                str(out_mp4),
            ],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
        )
        print(f"[showcase] wrote {out_mp4} ({frame_num} frames)")

        if out_gif is not None:
            out_gif.parent.mkdir(parents=True, exist_ok=True)
            subprocess.run(
                [
                    "ffmpeg", "-y",
                    "-framerate", "14",
                    "-i", str(tmp / "f_%04d.png"),
                    "-vf", "split[s0][s1];[s0]palettegen=max_colors=128[p];[s1][p]paletteuse=dither=none",
                    str(out_gif),
                ],
                check=True,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.PIPE,
            )
            print(f"[showcase] wrote {out_gif}")
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def main() -> None:
    parser = argparse.ArgumentParser(description="Build ffmpeg showcase MP4/GIF from baked sprite sheets.")
    parser.add_argument("--sprite", required=True, type=pathlib.Path, help="Baked sprite dir (e.g. assets/sprites/hero).")
    parser.add_argument("--out", required=True, type=pathlib.Path, help="Output .mp4 path.")
    parser.add_argument("--gif", type=pathlib.Path, default=None, help="Optional output .gif path.")
    parser.add_argument("--scale", type=int, default=3, help="Nearest-neighbor integer upscale (default: 3).")
    args = parser.parse_args()
    build_reel(args.sprite.resolve(), args.out.resolve(), args.gif.resolve() if args.gif else None, args.scale)


if __name__ == "__main__":
    main()
