#!/usr/bin/env python3
"""Batch auto-rigger, Blender pixel-art sprite baker, and `ffmpeg` showcase reel generator
for the complete *VELOCITY // BREAKER* Story Bible character roster (`docs/story-bible.md`).

Maps each TRELLIS.2 HQ `.glb` directly into its zero-code Bevy sprite slot:
  - levy                -> assets/sprites/hero      (96x96)
  - vulcanus            -> assets/sprites/boss      (144x144)
  - strider_drone       -> assets/sprites/rogue     (96x96)
  - slag_enforcer       -> assets/sprites/barbarian (96x96)
  - arc_weaver          -> assets/sprites/mage      (96x96)
  - phantom_interceptor -> assets/sprites/assassin  (96x96)

Usage:
    python3 tools/rig_and_bake_roster.py --only vulcanus
    python3 tools/rig_and_bake_roster.py --all
"""

from __future__ import annotations

import argparse
import pathlib
import subprocess
import sys

ROSTER = {
    "levy": {
        "raw_glb": "raw_assets/characters/levy_hq.glb",
        "rigged_glb": "raw_assets/characters/levy_rigged.glb",
        "preset": "levy",
        "slot": "hero",
        "size": 96,
        "ortho_scale": 3.55,
        "quantize": 56,
    },
    "vulcanus": {
        "raw_glb": "raw_assets/characters/vulcanus_hq.glb",
        "rigged_glb": "raw_assets/characters/vulcanus_rigged.glb",
        "preset": "vulcanus",
        "slot": "boss",
        "size": 144,
        "ortho_scale": 4.20,
        "quantize": 56,
    },
    "strider_drone": {
        "raw_glb": "raw_assets/characters/strider_drone_hq.glb",
        "rigged_glb": "raw_assets/characters/strider_drone_rigged.glb",
        "preset": "strider",
        "slot": "rogue",
        "size": 96,
        "ortho_scale": 3.55,
        "quantize": 48,
    },
    "slag_enforcer": {
        "raw_glb": "raw_assets/characters/slag_enforcer_hq.glb",
        "rigged_glb": "raw_assets/characters/slag_enforcer_rigged.glb",
        "preset": "enforcer",
        "slot": "barbarian",
        "size": 96,
        "ortho_scale": 3.85,
        "quantize": 48,
    },
    "arc_weaver": {
        "raw_glb": "raw_assets/characters/arc_weaver_hq.glb",
        "rigged_glb": "raw_assets/characters/arc_weaver_rigged.glb",
        "preset": "arc_weaver",
        "slot": "mage",
        "size": 96,
        "ortho_scale": 3.60,
        "quantize": 48,
    },
    "phantom_interceptor": {
        "raw_glb": "raw_assets/characters/phantom_interceptor_hq.glb",
        "rigged_glb": "raw_assets/characters/phantom_interceptor_rigged.glb",
        "preset": "phantom",
        "slot": "assassin",
        "size": 96,
        "ortho_scale": 3.55,
        "quantize": 48,
    },
}


def process_character(key: str, skip_existing_rig: bool = False) -> None:
    cfg = ROSTER[key]
    raw_glb = pathlib.Path(cfg["raw_glb"])
    rigged_glb = pathlib.Path(cfg["rigged_glb"])
    out_dir = pathlib.Path("assets/sprites") / cfg["slot"]
    showcase_mp4 = pathlib.Path("raw_assets/showcases") / f"{key}_showcase.mp4"
    showcase_gif = pathlib.Path("raw_assets/showcases") / f"{key}_showcase.gif"

    if not raw_glb.exists():
        raise FileNotFoundError(f"Missing TRELLIS.2 raw GLB for '{key}': {raw_glb}")

    if not (skip_existing_rig and rigged_glb.exists()):
        print(f"\n=== [1/3] RIGGING {key.upper()} -> {rigged_glb} ===", flush=True)
        subprocess.run(
            [
                "blender", "--background",
                "--python", "tools/rig_character_blender.py",
                "--",
                "--input", str(raw_glb),
                "--out", str(rigged_glb),
                "--preset", cfg["preset"],
                "--preview-dir", f"/tmp/{key}_rig_preview",
            ],
            check=True,
        )

    print(f"\n=== [2/3] BAKING {key.upper()} -> {out_dir} ({cfg['size']}x{cfg['size']}) ===", flush=True)
    subprocess.run(
        [
            "blender", "--background",
            "--python", "tools/sprite_baker/bake_sprites.py",
            "--",
            "--input", str(rigged_glb),
            "--out", str(out_dir),
            "--name", cfg["slot"],
            "--size", str(cfg["size"]),
            "--equip", "all",
            "--ortho-scale", str(cfg["ortho_scale"]),
            "--quantize", str(cfg["quantize"]),
        ],
        check=True,
    )

    print(f"\n=== [3/3] FFMPEG SHOWCASE {key.upper()} -> {showcase_mp4} ===", flush=True)
    scale = 2 if cfg["size"] >= 144 else 3
    subprocess.run(
        [
            sys.executable, "tools/make_showcase_reel.py",
            "--sprite", str(out_dir),
            "--out", str(showcase_mp4),
            "--gif", str(showcase_gif),
            "--scale", str(scale),
        ],
        check=True,
    )


def main() -> None:
    parser = argparse.ArgumentParser(description="Rig, bake, and showcase Velocity Breaker Story Bible roster.")
    parser.add_argument("--only", nargs="+", choices=list(ROSTER.keys()), help="Specific characters to process.")
    parser.add_argument("--all", action="store_true", help="Process all characters with existing raw GLBs.")
    parser.add_argument("--skip-existing-rig", action="store_true", help="Reuse existing rigged GLB.")
    args = parser.parse_args()

    if args.only:
        targets = args.only
    elif args.all:
        targets = [k for k, v in ROSTER.items() if pathlib.Path(v["raw_glb"]).exists()]
    else:
        targets = ["levy"]

    for key in targets:
        process_character(key, skip_existing_rig=args.skip_existing_rig)
    print(f"\n[roster] DONE processing: {', '.join(targets)}", flush=True)


if __name__ == "__main__":
    main()
