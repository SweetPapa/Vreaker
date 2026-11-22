#!/usr/bin/env python3
"""Packs per-frame diffuse/normal/emission PNGs into aligned pixel-art sprite sheets
and emits a Bevy-compatible `.ron` SpriteSheetManifest.

Pure Python + Pillow (no bpy dependency) so it can be run standalone or invoked by
`bake_sprites.py`.
"""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path
from typing import Any

from PIL import Image


def clean_diffuse_alpha(img: Image.Image, alpha_threshold: int) -> tuple[Image.Image, Image.Image]:
    """Enforces hard binary pixel-art alpha and returns (clean_rgba, binary_mask)."""
    rgba = img.convert("RGBA")
    px = rgba.load()
    w, h = rgba.size
    mask = Image.new("L", (w, h), 0)
    mpx = mask.load()

    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if a >= alpha_threshold:
                px[x, y] = (r, g, b, 255)
                mpx[x, y] = 255
            else:
                px[x, y] = (0, 0, 0, 0)
                mpx[x, y] = 0
    return rgba, mask


def clean_normal_frame(img: Image.Image, mask: Image.Image) -> Image.Image:
    """Applies the shared hard alpha mask and neutralizes transparent normal pixels."""
    rgba = img.convert("RGBA")
    px = rgba.load()
    mpx = mask.load()
    w, h = rgba.size

    for y in range(h):
        for x in range(w):
            if mpx[x, y] == 0:
                px[x, y] = (128, 128, 255, 0)
            else:
                r, g, b, _ = px[x, y]
                px[x, y] = (r, g, max(b, 128), 255)
    return rgba


def clean_emission_frame(img: Image.Image, mask: Image.Image, noise_floor: int = 8) -> Image.Image:
    """Applies the shared hard alpha mask and clamps near-black emission noise."""
    rgba = img.convert("RGBA")
    px = rgba.load()
    mpx = mask.load()
    w, h = rgba.size

    for y in range(h):
        for x in range(w):
            if mpx[x, y] == 0:
                px[x, y] = (0, 0, 0, 0)
            else:
                r, g, b, _ = px[x, y]
                if r < noise_floor and g < noise_floor and b < noise_floor:
                    px[x, y] = (0, 0, 0, 255)
                else:
                    px[x, y] = (r, g, b, 255)
    return rgba


def quantize_diffuse_sheet(sheet: Image.Image, colors: int) -> Image.Image:
    """Quantizes opaque pixels across the entire diffuse atlas with zero dithering
    so the palette stays locked across all animation frames.
    """
    if colors <= 0:
        return sheet

    rgba = sheet.convert("RGBA")
    alpha = rgba.getchannel("A")
    rgb = rgba.convert("RGB")
    quantized = rgb.quantize(
        colors=colors,
        method=Image.Quantize.MEDIANCUT,
        dither=Image.Dither.NONE,
    ).convert("RGBA")

    # Restore exact hard binary alpha mask and zero RGB on transparent pixels.
    qpx = quantized.load()
    apx = alpha.load()
    w, h = quantized.size
    for y in range(h):
        for x in range(w):
            if apx[x, y] == 0:
                qpx[x, y] = (0, 0, 0, 0)
            else:
                r, g, b, _ = qpx[x, y]
                qpx[x, y] = (r, g, b, 255)
    return quantized


def fmt_float(val: float) -> str:
    """Formats a float cleanly for RON output (always includes a decimal point)."""
    rounded = round(float(val), 2)
    if rounded == -0.0:
        rounded = 0.0
    text = f"{rounded:.2f}".rstrip("0")
    if text.endswith("."):
        text += "0"
    return text


def fmt_opt_vec2(pt: list[float] | tuple[float, float] | None) -> str:
    if pt is None:
        return "None"
    return f"Some(({fmt_float(pt[0])}, {fmt_float(pt[1])}))"


def generate_ron_manifest(meta: dict[str, Any], columns: int, rows: int) -> str:
    """Builds the `SpriteSheetManifest` RON document matching `SPEC.MD`."""
    frame_w, frame_h = meta["frame_size"]
    lines: list[str] = [
        "SpriteSheetManifest(",
        f"    frame_size: ({frame_w}, {frame_h}),",
        f"    columns: {columns},",
        f"    rows: {rows},",
        "    animations: {",
    ]

    for anim_name, anim in meta["animations"].items():
        start = int(anim["start"])
        end = int(anim["end"])
        fps = fmt_float(anim["fps"])
        looping = "true" if anim["looping"] else "false"
        hitboxes = anim.get("active_hitboxes", [])

        if not hitboxes:
            lines.append(
                f'        "{anim_name}": AnimationDef(start: {start}, end: {end}, fps: {fps}, looping: {looping}),'
            )
        else:
            lines.append(f'        "{anim_name}": AnimationDef(')
            lines.append(
                f"            start: {start}, end: {end}, fps: {fps}, looping: {looping},"
            )
            lines.append("            active_hitboxes: [")
            for hb in hitboxes:
                ox, oy = hb["offset"]
                lines.append(
                    "                FrameHitbox("
                    f"frame: {int(hb['frame'])}, "
                    f"offset: ({fmt_float(ox)}, {fmt_float(oy)}), "
                    f"radius: {fmt_float(hb['radius'])}, "
                    f"damage: {fmt_float(hb['damage'])}, "
                    f"base_kb: {fmt_float(hb['base_kb'])}, "
                    f"kb_scale: {fmt_float(hb['kb_scale'])}, "
                    f"angle_deg: {fmt_float(hb['angle_deg'])}"
                    "),"
                )
            lines.append("            ],")
            lines.append("        ),")

    lines.append("    },")

    bone_sockets: dict[str, dict[str, Any]] = meta.get("bone_sockets", {})
    if bone_sockets:
        lines.append("    bone_sockets: {")
        for frame_idx in sorted((int(k) for k in bone_sockets.keys())):
            sockets = bone_sockets[str(frame_idx)]
            lines.append(
                f"        {frame_idx}: BoneFrameSockets("
                f"hand_r: {fmt_opt_vec2(sockets.get('hand.R'))}, "
                f"hand_l: {fmt_opt_vec2(sockets.get('hand.L'))}, "
                f"foot_r: {fmt_opt_vec2(sockets.get('foot.R'))}, "
                f"foot_l: {fmt_opt_vec2(sockets.get('foot.L'))}, "
                f"buster_muzzle: {fmt_opt_vec2(sockets.get('buster_muzzle'))}, "
                f"chest: {fmt_opt_vec2(sockets.get('chest'))}"
                "),"
            )
        lines.append("    },")

    lines.append(")")
    lines.append("")
    return "\n".join(lines)


def pack_atlas(
    frames_dir: Path,
    meta_path: Path,
    out_dir: Path,
    name: str,
    columns: int = 8,
    quantize_colors: int = 48,
    alpha_threshold: int = 128,
) -> dict[str, Path]:
    """Packs rendered diffuse/normal/emission frames into aligned atlases + `.ron` manifest."""
    meta = json.loads(meta_path.read_text(encoding="utf-8"))
    frame_count = int(meta["frame_count"])
    frame_w, frame_h = (int(meta["frame_size"][0]), int(meta["frame_size"][1]))
    if frame_count <= 0:
        raise ValueError("Cannot pack atlas with zero frames.")

    cols = max(1, min(columns, frame_count))
    rows = max(1, math.ceil(frame_count / cols))
    atlas_w = cols * frame_w
    atlas_h = rows * frame_h

    diffuse_sheet = Image.new("RGBA", (atlas_w, atlas_h), (0, 0, 0, 0))
    normal_sheet = Image.new("RGBA", (atlas_w, atlas_h), (128, 128, 255, 0))
    emission_sheet = Image.new("RGBA", (atlas_w, atlas_h), (0, 0, 0, 0))

    for idx in range(frame_count):
        fname = f"frame_{idx:04d}.png"
        d_path = frames_dir / "diffuse" / fname
        n_path = frames_dir / "normal" / fname
        e_path = frames_dir / "emission" / fname

        for p in (d_path, n_path, e_path):
            if not p.exists():
                raise FileNotFoundError(f"Missing rendered pass frame: {p}")

        with Image.open(d_path) as d_raw:
            if d_raw.size != (frame_w, frame_h):
                d_raw = d_raw.resize((frame_w, frame_h), Image.Resampling.NEAREST)
            d_clean, mask = clean_diffuse_alpha(d_raw, alpha_threshold)

        with Image.open(n_path) as n_raw:
            if n_raw.size != (frame_w, frame_h):
                n_raw = n_raw.resize((frame_w, frame_h), Image.Resampling.NEAREST)
            n_clean = clean_normal_frame(n_raw, mask)

        with Image.open(e_path) as e_raw:
            if e_raw.size != (frame_w, frame_h):
                e_raw = e_raw.resize((frame_w, frame_h), Image.Resampling.NEAREST)
            e_clean = clean_emission_frame(e_raw, mask)

        gx = (idx % cols) * frame_w
        gy = (idx // cols) * frame_h
        diffuse_sheet.paste(d_clean, (gx, gy))
        normal_sheet.paste(n_clean, (gx, gy))
        emission_sheet.paste(e_clean, (gx, gy))

    if quantize_colors > 0:
        diffuse_sheet = quantize_diffuse_sheet(diffuse_sheet, quantize_colors)

    out_dir.mkdir(parents=True, exist_ok=True)
    diffuse_out = out_dir / f"{name}_diffuse.png"
    normal_out = out_dir / f"{name}_normal.png"
    emission_out = out_dir / f"{name}_emission.png"
    manifest_out = out_dir / f"{name}.ron"

    diffuse_sheet.save(diffuse_out, format="PNG", optimize=True)
    normal_sheet.save(normal_out, format="PNG", optimize=True)
    emission_sheet.save(emission_out, format="PNG", optimize=True)
    manifest_out.write_text(generate_ron_manifest(meta, cols, rows), encoding="utf-8")

    return {
        "diffuse": diffuse_out,
        "normal": normal_out,
        "emission": emission_out,
        "manifest": manifest_out,
    }


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Pack rendered 3-pass sprite frames into TextureAtlas sheets + RON manifest."
    )
    parser.add_argument("--frames-dir", required=True, type=Path, help="Directory with diffuse/normal/emission subdirs.")
    parser.add_argument("--meta", required=True, type=Path, help="Path to frames_meta.json emitted by bake_sprites.py.")
    parser.add_argument("--out", required=True, type=Path, help="Output directory (e.g. assets/sprites/hero).")
    parser.add_argument("--name", default=None, help="Artifact base name (defaults to --out folder basename).")
    parser.add_argument("--columns", type=int, default=8, help="Atlas column count (default: 8).")
    parser.add_argument("--quantize", type=int, default=48, help="Max diffuse palette colors (0 = disable, default: 48).")
    parser.add_argument("--alpha-threshold", type=int, default=128, help="Hard pixel-art alpha cutoff (default: 128).")
    args = parser.parse_args()

    out_dir = args.out.resolve()
    name = args.name or out_dir.name
    outputs = pack_atlas(
        frames_dir=args.frames_dir.resolve(),
        meta_path=args.meta.resolve(),
        out_dir=out_dir,
        name=name,
        columns=args.columns,
        quantize_colors=args.quantize,
        alpha_threshold=args.alpha_threshold,
    )
    for key, path in outputs.items():
        print(f"[pack_atlas] wrote {key:8s}: {path}")


if __name__ == "__main__":
    main()
