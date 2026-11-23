#!/usr/bin/env python3
"""Headless Blender (bpy) 3-pass pixel-art sprite baker for *VELOCITY // BREAKER*.

Usage:
    blender --background --python tools/sprite_baker/bake_sprites.py -- \
        --input external_assets/kaykit_adventurers/addons/kaykit_character_pack_adventures/Characters/gltf/Knight.glb \
        --out assets/sprites/hero \
        --size 96

Produces 4 synchronized artifacts inside `--out`:
    1. `<name>_diffuse.png`  — Quantized pixel-art albedo sheet
    2. `<name>_normal.png`   — Linear camera-space normal map for dynamic 2D lighting
    3. `<name>_emission.png` — Glow mask for Bevy HDR Bloom
    4. `<name>.ron`          — `SpriteSheetManifest` with animations, bone sockets & active hitboxes
"""

from __future__ import annotations

import argparse
import json
import math
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

import bpy
from bpy_extras.object_utils import world_to_camera_view
from mathutils import Vector

# Canonical gameplay animation clips mapped to candidate glTF action names.
# Works seamlessly with KayKit Adventurers, KayKit Animations, Quaternius UAL,
# and UniRig/Hunyuan3D rigs while falling back cleanly to whatever actions exist.
DEFAULT_ANIMATION_PRESETS: list[dict[str, Any]] = [
    {
        "key": "idle",
        "candidates": ["Idle", "Idle_A", "Unarmed_Idle", "2H_Melee_Idle"],
        "frames": 6,
        "fps": 10.0,
        "looping": True,
    },
    {
        "key": "run",
        "candidates": ["Running_A", "Run", "Walking_A"],
        "frames": 8,
        "fps": 16.0,
        "looping": True,
    },
    {
        "key": "sprint",
        "candidates": ["Running_B", "Sprint", "Running_A"],
        "frames": 8,
        "fps": 20.0,
        "looping": True,
    },
    {
        "key": "jump",
        "candidates": ["Jump_Start", "Jump_Full_Short", "Jump"],
        "frames": 4,
        "fps": 14.0,
        "looping": False,
    },
    {
        "key": "fall",
        "candidates": ["Jump_Idle", "Fall", "Jump_Land"],
        "frames": 4,
        "fps": 10.0,
        "looping": True,
    },
    {
        "key": "jab",
        "candidates": [
            "1H_Melee_Attack_Slice_Horizontal",
            "Melee_1H_Attack_Slice_Horizontal",
            "Unarmed_Melee_Attack_Punch_A",
            "Attack",
        ],
        "frames": 6,
        "fps": 18.0,
        "looping": False,
        "hitbox": {
            "active_local_frames": [2, 3],
            "bone": "hand.R",
            "forward_nudge": 8.0,
            "radius": 13.0,
            "damage": 7.0,
            "base_kb": 120.0,
            "kb_scale": 1.9,
            "angle_deg": 28.0,
        },
    },
    {
        "key": "dash_attack",
        "candidates": [
            "Dash_Slash",
            "1H_Melee_Attack_Stab",
            "Melee_1H_Attack_Stab",
            "Unarmed_Melee_Attack_Kick",
        ],
        "frames": 6,
        "fps": 18.0,
        "looping": False,
        "hitbox": {
            "active_local_frames": [2, 3],
            "bone": "hand.R",
            "forward_nudge": 11.0,
            "radius": 15.0,
            "damage": 12.0,
            "base_kb": 195.0,
            "kb_scale": 3.0,
            "angle_deg": 32.0,
        },
    },
    {
        "key": "f_air",
        "candidates": [
            "Air_Forward_Slash",
            "1H_Melee_Attack_Slice_Diagonal",
            "Melee_1H_Attack_Slice_Diagonal",
            "2H_Melee_Attack_Slice",
        ],
        "frames": 6,
        "fps": 18.0,
        "looping": False,
        "hitbox": {
            "active_local_frames": [2, 3],
            "bone": "hand.R",
            "forward_nudge": 10.0,
            "radius": 14.0,
            "damage": 11.0,
            "base_kb": 180.0,
            "kb_scale": 2.8,
            "angle_deg": 35.0,
        },
    },
    {
        "key": "d_air",
        "candidates": [
            "1H_Melee_Attack_Chop",
            "Melee_1H_Attack_Chop",
            "Melee_1H_Attack_Jump_Chop",
        ],
        "frames": 6,
        "fps": 18.0,
        "looping": False,
        "hitbox": {
            "active_local_frames": [2, 3],
            "bone": "foot.R",
            "forward_nudge": 4.0,
            "radius": 15.0,
            "damage": 13.0,
            "base_kb": 200.0,
            "kb_scale": 2.9,
            "angle_deg": -78.0,
        },
    },
    {
        "key": "up_special",
        "candidates": [
            "2H_Melee_Attack_Spinning",
            "Melee_2H_Attack_Spinning",
            "2H_Melee_Attack_Spin",
            "Jump_Full_Short",
        ],
        "frames": 6,
        "fps": 20.0,
        "looping": False,
        "hitbox": {
            "active_local_frames": [1, 2, 3, 4],
            "bone": "chest",
            "forward_nudge": 4.0,
            "radius": 18.0,
            "damage": 9.0,
            "base_kb": 170.0,
            "kb_scale": 2.4,
            "angle_deg": 75.0,
        },
    },
    {
        "key": "buster",
        "candidates": [
            "1H_Ranged_Shoot",
            "Ranged_1H_Shoot",
            "Spellcast_Shoot",
            "Throw",
        ],
        "frames": 4,
        "fps": 16.0,
        "looping": False,
    },
    {
        "key": "hitstun",
        "candidates": ["Hit_A", "Hit_B"],
        "frames": 4,
        "fps": 14.0,
        "looping": False,
    },
    {
        "key": "down_tilt",
        "candidates": ["Unarmed_Melee_Attack_Kick", "1H_Melee_Attack_Slice_Horizontal"],
        "frames": 5,
        "fps": 18.0,
        "looping": False,
        "hitbox": {
            "active_local_frames": [2, 3],
            "bone": "foot.R",
            "forward_nudge": 10.0,
            "radius": 14.0,
            "damage": 7.0,
            "base_kb": 165.0,
            "kb_scale": 1.6,
            "angle_deg": 72.0,
        },
    },
    {
        "key": "up_tilt",
        "candidates": ["Spellcast_Raise", "2H_Melee_Attack_Chop", "1H_Melee_Attack_Chop"],
        "frames": 5,
        "fps": 18.0,
        "looping": False,
        "hitbox": {
            "active_local_frames": [2, 3],
            "bone": "hand.R",
            "forward_nudge": 4.0,
            "radius": 16.0,
            "damage": 9.0,
            "base_kb": 230.0,
            "kb_scale": 2.5,
            "angle_deg": 84.0,
        },
    },
    {
        "key": "f_smash",
        "candidates": ["2H_Melee_Attack_Slice", "1H_Melee_Attack_Slice_Diagonal"],
        "frames": 7,
        "fps": 16.0,
        "looping": False,
        "hitbox": {
            "active_local_frames": [3, 4],
            "bone": "hand.R",
            "forward_nudge": 14.0,
            "radius": 19.0,
            "damage": 18.0,
            "base_kb": 285.0,
            "kb_scale": 3.6,
            "angle_deg": 32.0,
        },
    },
    {
        "key": "side_special",
        "candidates": ["2H_Melee_Attack_Stab", "Dualwield_Melee_Attack_Stab", "1H_Melee_Attack_Stab"],
        "frames": 6,
        "fps": 20.0,
        "looping": False,
        "hitbox": {
            "active_local_frames": [1, 2, 3],
            "bone": "hand.R",
            "forward_nudge": 12.0,
            "radius": 17.0,
            "damage": 13.0,
            "base_kb": 225.0,
            "kb_scale": 2.8,
            "angle_deg": 42.0,
        },
    },
    {
        "key": "down_special",
        "candidates": ["Block_Attack", "2H_Melee_Attack_Chop", "Spellcast_Long"],
        "frames": 6,
        "fps": 18.0,
        "looping": False,
        "hitbox": {
            "active_local_frames": [2, 3],
            "bone": "chest",
            "forward_nudge": 2.0,
            "radius": 24.0,
            "damage": 14.0,
            "base_kb": 260.0,
            "kb_scale": 3.2,
            "angle_deg": 65.0,
        },
    },
    {
        "key": "neutral_special",
        "candidates": ["Spellcast_Shoot", "1H_Ranged_Shoot", "Throw"],
        "frames": 5,
        "fps": 18.0,
        "looping": False,
    },
    {
        "key": "guard",
        "candidates": ["Blocking", "Block", "Idle"],
        "frames": 4,
        "fps": 10.0,
        "looping": True,
    },
    {
        "key": "jab_2",
        "candidates": [
            "1H_Melee_Attack_Slice_Diagonal",
            "Dualwield_Melee_Attack_Slice",
            "Unarmed_Melee_Attack_Punch_B",
        ],
        "frames": 5,
        "fps": 20.0,
        "looping": False,
        "hitbox": {
            "active_local_frames": [2, 3],
            "bone": "hand.R",
            "forward_nudge": 10.0,
            "radius": 15.0,
            "damage": 5.0,
            "base_kb": 98.0,
            "kb_scale": 0.55,
            "angle_deg": 26.0,
        },
    },
    {
        "key": "jab_3",
        "candidates": [
            "1H_Melee_Attack_Stab",
            "2H_Melee_Attack_Stab",
            "Dualwield_Melee_Attack_Stab",
        ],
        "frames": 6,
        "fps": 18.0,
        "looping": False,
        "hitbox": {
            "active_local_frames": [2, 3],
            "bone": "hand.R",
            "forward_nudge": 13.0,
            "radius": 18.0,
            "damage": 11.0,
            "base_kb": 245.0,
            "kb_scale": 3.1,
            "angle_deg": 36.0,
        },
    },
    {
        "key": "up_smash",
        "candidates": [
            "2H_Melee_Attack_Chop",
            "1H_Melee_Attack_Chop",
            "Spellcast_Raise",
        ],
        "frames": 7,
        "fps": 16.0,
        "looping": False,
        "hitbox": {
            "active_local_frames": [3, 4],
            "bone": "hand.R",
            "forward_nudge": 4.0,
            "radius": 22.0,
            "damage": 17.0,
            "base_kb": 290.0,
            "kb_scale": 3.5,
            "angle_deg": 84.0,
        },
    },
    {
        "key": "down_smash",
        "candidates": [
            "2H_Melee_Attack_Spin",
            "2H_Melee_Attack_Spinning",
            "1H_Melee_Attack_Slice_Horizontal",
        ],
        "frames": 7,
        "fps": 20.0,
        "looping": False,
        "hitbox": {
            "active_local_frames": [2, 3, 4],
            "bone": "chest",
            "forward_nudge": 0.0,
            "radius": 26.0,
            "damage": 16.0,
            "base_kb": 270.0,
            "kb_scale": 3.4,
            "angle_deg": 42.0,
        },
    },
    {
        "key": "sword_spin",
        "candidates": [
            "2H_Melee_Attack_Spinning",
            "2H_Melee_Attack_Spin",
            "Dualwield_Melee_Attack_Slice",
        ],
        "frames": 8,
        "fps": 24.0,
        "looping": True,
        "hitbox": {
            "active_local_frames": [1, 2, 3, 4, 5, 6],
            "bone": "chest",
            "forward_nudge": 0.0,
            "radius": 26.0,
            "damage": 12.0,
            "base_kb": 235.0,
            "kb_scale": 3.0,
            "angle_deg": 52.0,
        },
    },
    {
        "key": "dodge_fwd",
        "candidates": ["Dodge_Forward", "Running_B"],
        "frames": 5,
        "fps": 20.0,
        "looping": False,
    },
    {
        "key": "dodge_back",
        "candidates": ["Dodge_Backward", "Walking_Backwards"],
        "frames": 5,
        "fps": 20.0,
        "looping": False,
    },
    {
        "key": "spot_dodge",
        "candidates": ["Dodge_Left", "Dodge_Right", "Block_Hit"],
        "frames": 5,
        "fps": 20.0,
        "looping": False,
    },
    {
        "key": "taunt",
        "candidates": ["Cheer", "Idle"],
        "frames": 6,
        "fps": 12.0,
        "looping": False,
    },
    {
        "key": "telegraph",
        "candidates": ["Telegraph", "Blocking", "Block", "Idle"],
        "frames": 5,
        "fps": 12.0,
        "looping": False,
    },
]

# Logical bone slots required by SPEC.MD Section 3.3 mapped to common rig naming conventions.
BONE_SLOT_ALIASES: dict[str, list[str]] = {
    "hand.R": ["hand.R", "hand.r", "handslot.r", "RightHand", "mixamorig:RightHand", "Hand_R"],
    "hand.L": ["hand.L", "hand.l", "handslot.l", "LeftHand", "mixamorig:LeftHand", "Hand_L"],
    "foot.R": ["foot.R", "foot.r", "toes.r", "RightFoot", "mixamorig:RightFoot", "Foot_R"],
    "foot.L": ["foot.L", "foot.l", "toes.l", "LeftFoot", "mixamorig:LeftFoot", "Foot_L"],
    "buster_muzzle": [
        "buster_muzzle",
        "handslot.r",
        "hand.r",
        "hand.R",
        "RightHand",
        "mixamorig:RightHand",
    ],
    "chest": ["chest", "Chest", "spine_02", "Spine2", "mixamorig:Spine2", "spine"],
}


def parse_args() -> argparse.Namespace:
    argv = sys.argv
    if "--" in argv:
        argv = argv[argv.index("--") + 1 :]
    else:
        argv = []

    parser = argparse.ArgumentParser(description="Headless Blender 3-pass pixel-art sprite baker.")
    parser.add_argument("--input", required=True, type=Path, help="Input .glb or .gltf character rig.")
    parser.add_argument("--out", required=True, type=Path, help="Output directory (e.g. assets/sprites/hero).")
    parser.add_argument("--name", default=None, help="Output artifact prefix (defaults to --out folder name).")
    parser.add_argument("--size", type=int, default=96, help="Square pixel frame size (64, 96, 128; default: 96).")
    parser.add_argument("--columns", type=int, default=8, help="Atlas columns (default: 8).")
    parser.add_argument("--quantize", type=int, default=48, help="Diffuse color palette count (0 = off, default: 48).")
    parser.add_argument(
        "--yaw-deg",
        type=float,
        default=90.0,
        help="Z rotation in degrees so character faces +X right (default: 90.0 for KayKit -Y forward).",
    )
    parser.add_argument(
        "--equip",
        default="1H_Sword",
        help="Comma-separated attachment mesh names to keep on multi-item hand slots (use 'none' for unarmed, 'all' to keep everything).",
    )
    parser.add_argument(
        "--ortho-scale",
        type=float,
        default=None,
        help="Optional explicit orthographic camera scale override.",
    )
    parser.add_argument(
        "--no-auto-emission",
        action="store_true",
        help="Disable procedural visor/blade glow accents when the input glTF has no native emission.",
    )
    parser.add_argument(
        "--keep-frames",
        action="store_true",
        help="Keep intermediate raw per-frame PNGs inside `<out>/_frames` for inspection.",
    )
    parser.add_argument(
        "--python-bin",
        default="python3",
        help="External Python binary with Pillow for pack_atlas.py (default: python3).",
    )
    return parser.parse_args(argv)


def clear_scene() -> None:
    bpy.ops.wm.read_factory_settings(use_empty=True)


def import_gltf(input_path: Path) -> None:
    if not input_path.exists():
        raise FileNotFoundError(f"Input glTF/GLB does not exist: {input_path}")
    bpy.ops.import_scene.gltf(filepath=str(input_path.resolve()))


def find_armature() -> bpy.types.Object | None:
    armatures = [obj for obj in bpy.data.objects if obj.type == "ARMATURE"]
    return armatures[0] if armatures else None


def filter_character_meshes(armature: bpy.types.Object | None, equip_arg: str) -> list[bpy.types.Object]:
    """Hides bone-shape helper meshes (e.g. unparented material-less Icosphere) and
    resolves duplicate attachment meshes sharing the same handslot bone.
    """
    all_meshes = [obj for obj in bpy.data.objects if obj.type == "MESH"]
    has_parented = any(obj.parent is not None for obj in all_meshes)

    equip_mode = equip_arg.strip()
    equip_set = {item.strip().lower() for item in equip_mode.split(",") if item.strip()}

    # Group meshes that are bone-parented onto shared equipment slots (e.g. handslot.r/l)
    slot_groups: dict[str, list[bpy.types.Object]] = {}
    for obj in all_meshes:
        if obj.parent_bone and "slot" in obj.parent_bone.lower():
            slot_groups.setdefault(obj.parent_bone.lower(), []).append(obj)

    active_meshes: list[bpy.types.Object] = []
    for obj in all_meshes:
        hide = False

        # Drop stray glTF custom bone shapes (unparented meshes without any material)
        if armature is not None and has_parented and obj.parent is None and len(obj.data.materials) == 0:
            hide = True

        # Resolve multi-attachment equipment slots
        if not hide and obj.parent_bone and obj.parent_bone.lower() in slot_groups:
            group = slot_groups[obj.parent_bone.lower()]
            if len(group) > 1 and equip_mode.lower() != "all":
                if equip_mode.lower() == "none":
                    hide = True
                elif obj.name.lower() not in equip_set:
                    hide = True

        obj.hide_viewport = hide
        obj.hide_render = hide
        if not hide:
            active_meshes.append(obj)

    if not active_meshes:
        raise RuntimeError("No renderable meshes remained after filtering!")
    return active_meshes


def orient_character(armature: bpy.types.Object | None, meshes: list[bpy.types.Object], yaw_deg: float) -> None:
    """Rotates root objects around Z so the character faces +X (right) with right arm in foreground."""
    rad = math.radians(yaw_deg)
    roots: set[bpy.types.Object] = set()
    if armature is not None:
        root = armature
        while root.parent is not None:
            root = root.parent
        roots.add(root)
    for m in meshes:
        root = m
        while root.parent is not None:
            root = root.parent
        roots.add(root)

    for root in roots:
        root.rotation_mode = "XYZ"
        root.rotation_euler.z += rad

    bpy.context.view_layer.update()


def resolve_animations(armature: bpy.types.Object | None) -> list[dict[str, Any]]:
    """Matches canonical gameplay clips to actions present in the imported `.glb`."""
    if armature is None or len(bpy.data.actions) == 0:
        return [
            {
                "key": "idle",
                "action": None,
                "frames": 1,
                "fps": 1.0,
                "looping": True,
                "hitbox": None,
            }
        ]

    actions_by_lower = {a.name.lower(): a for a in bpy.data.actions}
    resolved: list[dict[str, Any]] = []

    for preset in DEFAULT_ANIMATION_PRESETS:
        matched_action = None
        for cand in preset["candidates"]:
            if cand.lower() in actions_by_lower:
                matched_action = actions_by_lower[cand.lower()]
                break
        if matched_action is not None:
            resolved.append(
                {
                    "key": preset["key"],
                    "action": matched_action,
                    "frames": int(preset["frames"]),
                    "fps": float(preset["fps"]),
                    "looping": bool(preset["looping"]),
                    "hitbox": preset.get("hitbox"),
                }
            )

    # Generic fallback for arbitrary UniRig / Hunyuan3D / custom `.glb` models
    if not resolved:
        for action in sorted(bpy.data.actions, key=lambda a: a.name)[:8]:
            clean_key = action.name.lower().replace(" ", "_").replace("-", "_")
            resolved.append(
                {
                    "key": clean_key,
                    "action": action,
                    "frames": 6,
                    "fps": 12.0,
                    "looping": True,
                    "hitbox": None,
                }
            )

    return resolved


def set_armature_action(armature: bpy.types.Object, action: bpy.types.Action) -> None:
    if armature.animation_data is None:
        armature.animation_data_create()
    armature.animation_data.action = action
    # Clear any NLA overrides so the assigned action always evaluates cleanly.
    armature.animation_data.use_nla = False
    # Blender 5.x slotted actions: always bind the action's primary slot explicitly.
    if hasattr(action, "slots") and len(action.slots) > 0:
        armature.animation_data.action_slot = action.slots[0]


def sample_action_frames(action: bpy.types.Action | None, count: int, looping: bool) -> list[float]:
    if action is None or count <= 1:
        return [0.0]
    start, end = action.frame_range
    if end <= start + 1e-4:
        return [start] * count
    if looping:
        # Exclude the duplicate loop-seam endpoint for seamless looping cycles.
        return [start + (end - start) * (i / count) for i in range(count)]
    return [start + (end - start) * (i / (count - 1)) for i in range(count)]


def compute_world_bounds(
    meshes: list[bpy.types.Object],
    armature: bpy.types.Object | None,
    clips: list[dict[str, Any]],
) -> tuple[Vector, Vector]:
    """Computes tight world-space bounding box across key poses so camera framing never clips."""
    depsgraph = bpy.context.evaluated_depsgraph_get()
    min_pt = Vector((float("inf"), float("inf"), float("inf")))
    max_pt = Vector((float("-inf"), float("-inf"), float("-inf")))

    # Sample start/mid/end poses of each clip (fast and covers melee swings & jumps).
    sample_poses: list[tuple[bpy.types.Action | None, float]] = []
    for clip in clips:
        frames = sample_action_frames(clip["action"], clip["frames"], clip["looping"])
        indices = sorted({0, len(frames) // 2, len(frames) - 1})
        for idx in indices:
            sample_poses.append((clip["action"], frames[idx]))

    for action, frame in sample_poses:
        if armature is not None and action is not None:
            set_armature_action(armature, action)
            bpy.context.scene.frame_set(int(math.floor(frame)), subframe=frame - math.floor(frame))
            bpy.context.view_layer.update()

        depsgraph.update()
        for obj in meshes:
            if "slashsmear" in obj.name.lower():
                continue
            eval_obj = obj.evaluated_get(depsgraph)
            for corner in eval_obj.bound_box:
                world_c = eval_obj.matrix_world @ Vector(corner)
                min_pt.x = min(min_pt.x, world_c.x)
                min_pt.y = min(min_pt.y, world_c.y)
                min_pt.z = min(min_pt.z, world_c.z)
                max_pt.x = max(max_pt.x, world_c.x)
                max_pt.y = max(max_pt.y, world_c.y)
                max_pt.z = max(max_pt.z, world_c.z)

    return min_pt, max_pt


def setup_camera_and_lighting(
    min_pt: Vector,
    max_pt: Vector,
    size: int,
    ortho_scale_override: float | None,
) -> bpy.types.Object:
    scene = bpy.context.scene
    scene.render.engine = "BLENDER_EEVEE"
    scene.render.resolution_x = size
    scene.render.resolution_y = size
    scene.render.resolution_percentage = 100
    scene.render.film_transparent = True
    scene.render.filter_size = 0.01
    scene.render.image_settings.file_format = "PNG"
    scene.render.image_settings.color_mode = "RGBA"
    scene.render.image_settings.color_depth = "8"
    scene.render.image_settings.compression = 15

    # Disable temporal anti-aliasing for razor-sharp pixel art
    scene.eevee.taa_render_samples = 1
    if hasattr(scene.eevee, "use_taa_reprojection"):
        scene.eevee.use_taa_reprojection = False

    # Neutral world background (transparent film keeps alpha=0)
    world = bpy.data.worlds.new("BakerWorld")
    scene.world = world
    world.use_nodes = True
    bg = world.node_tree.nodes.get("Background")
    if bg is not None:
        bg.inputs["Color"].default_value = (0.35, 0.38, 0.44, 1.0)
        bg.inputs["Strength"].default_value = 0.65

    width = max(max_pt.x - min_pt.x, 0.5)
    height = max(max_pt.z - min_pt.z, 0.5)
    ortho_scale = ortho_scale_override if ortho_scale_override is not None else max(width, height) * 1.18

    # Keep feet anchored cleanly near the lower portion of the frame (~8% bottom margin)
    center_x = 0.5 * (min_pt.x + max_pt.x) * 0.35  # bias near rig origin so walk/idle stays centered
    center_z = min_pt.z + ortho_scale * 0.43

    cam_data = bpy.data.cameras.new("OrthoSideCam")
    cam_data.type = "ORTHO"
    cam_data.ortho_scale = ortho_scale
    cam_data.clip_start = 0.1
    cam_data.clip_end = 100.0
    cam_obj = bpy.data.objects.new("OrthoSideCam", cam_data)
    scene.collection.objects.link(cam_obj)
    # Camera sits at -Y looking strictly along +Y:
    #   +X_cam = +X_world (character forward / right)
    #   +Y_cam = +Z_world (character up)
    #   +Z_cam = -Y_world (out of screen toward viewer)
    cam_obj.location = Vector((center_x, -10.0, center_z))
    cam_obj.rotation_euler = (math.radians(90.0), 0.0, 0.0)
    scene.camera = cam_obj

    # Clean, balanced 2-light diffuse rig (kept bright & even so Bevy's real-time normal shader
    # owns directional lighting without crushed black diffuse regions).
    key_data = bpy.data.lights.new("KeyLight", type="SUN")
    key_data.energy = 2.6
    key_data.angle = math.radians(1.0)
    key_obj = bpy.data.objects.new("KeyLight", key_data)
    key_obj.rotation_euler = (math.radians(48.0), math.radians(-18.0), math.radians(-22.0))
    scene.collection.objects.link(key_obj)

    fill_data = bpy.data.lights.new("FillLight", type="SUN")
    fill_data.energy = 1.25
    fill_data.color = (0.88, 0.94, 1.0)
    fill_obj = bpy.data.objects.new("FillLight", fill_data)
    fill_obj.rotation_euler = (math.radians(72.0), math.radians(24.0), math.radians(150.0))
    scene.collection.objects.link(fill_obj)

    return cam_obj


def configure_diffuse_materials(meshes: list[bpy.types.Object]) -> None:
    """Enforces crisp nearest-neighbor texture sampling and clean matte toon-ready albedo."""
    seen: set[bpy.types.Material] = set()
    for obj in meshes:
        for mat in obj.data.materials:
            if mat is None or mat in seen:
                continue
            seen.add(mat)
            if not mat.node_tree:
                continue
            for node in mat.node_tree.nodes:
                if node.type == "TEX_IMAGE":
                    node.interpolation = "Closest"
                elif node.type == "BSDF_PRINCIPLED":
                    # Reduce glossy blowout so diffuse captures clean cel-readable base colors.
                    if "Roughness" in node.inputs:
                        node.inputs["Roughness"].default_value = 0.85
                    if "Specular IOR Level" in node.inputs:
                        node.inputs["Specular IOR Level"].default_value = 0.15


def build_normal_override_material() -> bpy.types.Material:
    """Builds a camera-space normal map material (RGB = N_cam * 0.5 + 0.5)."""
    mat = bpy.data.materials.new("BakerPassNormal")
    mat.use_nodes = True
    nt = mat.node_tree
    nt.nodes.clear()

    geom = nt.nodes.new("ShaderNodeNewGeometry")
    vxf = nt.nodes.new("ShaderNodeVectorTransform")
    vxf.vector_type = "NORMAL"
    vxf.convert_from = "WORLD"
    vxf.convert_to = "CAMERA"

    norm = nt.nodes.new("ShaderNodeVectorMath")
    norm.operation = "NORMALIZE"

    # Encode [-1, 1] -> [0, 1] via MULTIPLY_ADD (v * 0.5 + 0.5)
    encode = nt.nodes.new("ShaderNodeVectorMath")
    encode.operation = "MULTIPLY_ADD"
    encode.inputs[1].default_value = (0.5, 0.5, 0.5)
    encode.inputs[2].default_value = (0.5, 0.5, 0.5)

    emis = nt.nodes.new("ShaderNodeEmission")
    emis.inputs["Strength"].default_value = 1.0

    out = nt.nodes.new("ShaderNodeOutputMaterial")

    nt.links.new(geom.outputs["Normal"], vxf.inputs["Vector"])
    nt.links.new(vxf.outputs["Vector"], norm.inputs[0])
    nt.links.new(norm.outputs["Vector"], encode.inputs[0])
    nt.links.new(encode.outputs["Vector"], emis.inputs["Color"])
    nt.links.new(emis.outputs["Emission"], out.inputs["Surface"])
    return mat


def material_has_native_emission(mat: bpy.types.Material | None) -> bool:
    if mat is None or not mat.node_tree:
        return False
    for node in mat.node_tree.nodes:
        if node.type == "BSDF_PRINCIPLED":
            ec = node.inputs.get("Emission Color")
            es = node.inputs.get("Emission Strength")
            if ec is not None and ec.is_linked:
                return True
            if ec is not None and es is not None and es.default_value > 0.01:
                c = ec.default_value
                if c[0] > 0.01 or c[1] > 0.01 or c[2] > 0.01:
                    return True
        elif node.type == "EMISSION":
            return True
    return False


def build_emission_material(
    obj: bpy.types.Object,
    orig_mat: bpy.types.Material | None,
    auto_emission: bool,
) -> bpy.types.Material:
    """Builds the emission pass material for a mesh.

    Uses native glTF emission if present; otherwise (when `auto_emission` is enabled)
    emits crisp cyber-visor & energy-blade highlights for Bevy HDR Bloom verification.
    """
    mat = bpy.data.materials.new(f"BakerPassEmission_{obj.name}")
    mat.use_nodes = True
    nt = mat.node_tree
    nt.nodes.clear()

    emis = nt.nodes.new("ShaderNodeEmission")
    emis.inputs["Color"].default_value = (0.0, 0.0, 0.0, 1.0)
    emis.inputs["Strength"].default_value = 1.0
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    nt.links.new(emis.outputs["Emission"], out.inputs["Surface"])

    # 1. Preserve native emission texture or color if present on the source glTF material.
    if material_has_native_emission(orig_mat) and orig_mat and orig_mat.node_tree:
        for node in orig_mat.node_tree.nodes:
            if node.type == "BSDF_PRINCIPLED":
                ec = node.inputs.get("Emission Color")
                es = node.inputs.get("Emission Strength")
                if ec is not None and ec.is_linked:
                    src = ec.links[0].from_node
                    if src.type == "TEX_IMAGE" and src.image is not None:
                        tex = nt.nodes.new("ShaderNodeTexImage")
                        tex.image = src.image
                        tex.interpolation = "Closest"
                        nt.links.new(tex.outputs["Color"], emis.inputs["Color"])
                        return mat
                if ec is not None and es is not None and es.default_value > 0.01:
                    c = ec.default_value
                    emis.inputs["Color"].default_value = (c[0], c[1], c[2], 1.0)
                    return mat

    if not auto_emission:
        return mat

    # 2. Procedural cyber-accent emission for Diffuse-only CC0 rigs (like KayKit Adventurers):
    #    - Weapon blade (`sword`, `blade`, `staff`, `gun`, `buster`) -> glowing electric cyan edge
    #    - Helmet/Head visor band -> crisp glowing amber/cyan visor slit on front-facing band
    name_l = obj.name.lower()
    if any(tok in name_l for tok in ("sword", "blade", "dagger", "knife", "axe", "staff", "wand", "buster")):
        # Highlight the upper blade half along local Z in vivid cyan
        tex_coord = nt.nodes.new("ShaderNodeTexCoord")
        sep = nt.nodes.new("ShaderNodeSeparateXYZ")
        ramp = nt.nodes.new("ShaderNodeValToRGB")
        ramp.color_ramp.interpolation = "CONSTANT"
        ramp.color_ramp.elements[0].position = 0.0
        ramp.color_ramp.elements[0].color = (0.0, 0.0, 0.0, 1.0)
        ramp.color_ramp.elements[1].position = 0.42
        ramp.color_ramp.elements[1].color = (0.12, 0.88, 1.0, 1.0)
        nt.links.new(tex_coord.outputs["Generated"], sep.inputs["Vector"])
        nt.links.new(sep.outputs["Z"], ramp.inputs["Fac"])
        nt.links.new(ramp.outputs["Color"], emis.inputs["Color"])
        return mat

    if "helmet" in name_l or "visor" in name_l:
        # Glowing cyber-visor stripe across the front eye-slit band of the helmet
        tex_coord = nt.nodes.new("ShaderNodeTexCoord")
        sep = nt.nodes.new("ShaderNodeSeparateXYZ")

        # Vertical band around eye level (Generated Z in ~[0.34, 0.50])
        z_gt = nt.nodes.new("ShaderNodeMath")
        z_gt.operation = "GREATER_THAN"
        z_gt.inputs[1].default_value = 0.34

        z_lt = nt.nodes.new("ShaderNodeMath")
        z_lt.operation = "LESS_THAN"
        z_lt.inputs[1].default_value = 0.49

        # Front face of helmet (KayKit local -Y is forward -> Generated Y < 0.32)
        y_lt = nt.nodes.new("ShaderNodeMath")
        y_lt.operation = "LESS_THAN"
        y_lt.inputs[1].default_value = 0.30

        mul1 = nt.nodes.new("ShaderNodeMath")
        mul1.operation = "MULTIPLY"
        mul2 = nt.nodes.new("ShaderNodeMath")
        mul2.operation = "MULTIPLY"

        mix = nt.nodes.new("ShaderNodeMix")
        mix.data_type = "RGBA"
        mix.inputs[6].default_value = (0.0, 0.0, 0.0, 1.0)
        mix.inputs[7].default_value = (1.0, 0.62, 0.12, 1.0)  # molten amber visor glow

        nt.links.new(tex_coord.outputs["Generated"], sep.inputs["Vector"])
        nt.links.new(sep.outputs["Z"], z_gt.inputs[0])
        nt.links.new(sep.outputs["Z"], z_lt.inputs[0])
        nt.links.new(sep.outputs["Y"], y_lt.inputs[0])
        nt.links.new(z_gt.outputs["Value"], mul1.inputs[0])
        nt.links.new(z_lt.outputs["Value"], mul1.inputs[1])
        nt.links.new(mul1.outputs["Value"], mul2.inputs[0])
        nt.links.new(y_lt.outputs["Value"], mul2.inputs[1])
        nt.links.new(mul2.outputs["Value"], mix.inputs[0])
        nt.links.new(mix.outputs[2], emis.inputs["Color"])
        return mat

    return mat


def resolve_bone_map(armature: bpy.types.Object | None) -> dict[str, str]:
    """Maps logical SPEC bone slot names to actual pose bone names in the rig."""
    if armature is None:
        return {}
    by_lower = {b.name.lower(): b.name for b in armature.pose.bones}
    mapping: dict[str, str] = {}
    for slot, aliases in BONE_SLOT_ALIASES.items():
        for alias in aliases:
            if alias.lower() in by_lower:
                mapping[slot] = by_lower[alias.lower()]
                break
    return mapping


def project_bone_sockets(
    armature: bpy.types.Object | None,
    bone_map: dict[str, str],
    camera: bpy.types.Object,
    size: int,
) -> dict[str, list[float] | None]:
    """Projects designated bones into centered 2D sprite pixel coords (+X right, +Y up)."""
    scene = bpy.context.scene
    sockets: dict[str, list[float] | None] = {slot: None for slot in BONE_SLOT_ALIASES}
    if armature is None:
        return sockets

    half = size * 0.5
    for slot, bone_name in bone_map.items():
        pb = armature.pose.bones.get(bone_name)
        if pb is None:
            continue
        # Use bone midpoint/tail for extremities so hitboxes sit cleanly at hand/foot strike tips
        local_pos = pb.tail if slot in ("hand.R", "hand.L", "foot.R", "foot.L", "buster_muzzle") else pb.head
        world_pos = armature.matrix_world @ local_pos
        ndc = world_to_camera_view(scene, camera, world_pos)
        px_x = (ndc.x * size) - half
        px_y = (ndc.y * size) - half
        sockets[slot] = [round(px_x, 2), round(px_y, 2)]

    return sockets


def set_view_transform_safe(scene: bpy.types.Scene, target: str) -> None:
    try:
        scene.view_settings.view_transform = target
    except TypeError:
        pass


def render_pass(
    scene: bpy.types.Scene,
    armature: bpy.types.Object | None,
    camera: bpy.types.Object,
    clips: list[dict[str, Any]],
    out_dir: Path,
    pass_name: str,
    size: int,
    bone_map: dict[str, str],
    record_bones: bool = False,
) -> tuple[dict[str, Any], dict[str, dict[str, list[float] | None]], int]:
    """Renders every clip frame for one pass (`diffuse`, `normal`, or `emission`)."""
    pass_dir = out_dir / pass_name
    pass_dir.mkdir(parents=True, exist_ok=True)

    animations_meta: dict[str, Any] = {}
    bone_sockets_meta: dict[str, dict[str, list[float] | None]] = {}
    global_frame = 0

    for clip in clips:
        action = clip["action"]
        sample_frames = sample_action_frames(action, clip["frames"], clip["looping"])
        start_idx = global_frame

        if armature is not None and action is not None:
            set_armature_action(armature, action)

        active_hitboxes: list[dict[str, Any]] = []
        hb_cfg = clip.get("hitbox")

        for local_i, anim_frame in enumerate(sample_frames):
            if armature is not None and action is not None:
                f_int = int(math.floor(anim_frame))
                scene.frame_set(f_int, subframe=anim_frame - f_int)
                bpy.context.view_layer.update()

            if record_bones:
                sockets = project_bone_sockets(armature, bone_map, camera, size)
                bone_sockets_meta[str(global_frame)] = sockets

                if hb_cfg is not None and local_i in hb_cfg["active_local_frames"]:
                    anchor = sockets.get(hb_cfg["bone"]) or sockets.get("chest") or [16.0, 0.0]
                    offset = [
                        round(anchor[0] + float(hb_cfg.get("forward_nudge", 0.0)), 2),
                        round(anchor[1], 2),
                    ]
                    active_hitboxes.append(
                        {
                            "frame": global_frame,
                            "offset": offset,
                            "radius": hb_cfg["radius"],
                            "damage": hb_cfg["damage"],
                            "base_kb": hb_cfg["base_kb"],
                            "kb_scale": hb_cfg["kb_scale"],
                            "angle_deg": hb_cfg["angle_deg"],
                        }
                    )

            scene.render.filepath = str(pass_dir / f"frame_{global_frame:04d}.png")
            bpy.ops.render.render(write_still=True)
            global_frame += 1

        if record_bones:
            animations_meta[clip["key"]] = {
                "start": start_idx,
                "end": global_frame - 1,
                "fps": clip["fps"],
                "looping": clip["looping"],
                "active_hitboxes": active_hitboxes,
            }

    return animations_meta, bone_sockets_meta, global_frame


def bake(args: argparse.Namespace) -> None:
    clear_scene()
    import_gltf(args.input)

    armature = find_armature()
    meshes = filter_character_meshes(armature, args.equip)
    orient_character(armature, meshes, args.yaw_deg)

    clips = resolve_animations(armature)
    bone_map = resolve_bone_map(armature)
    print(f"[bake_sprites] input={args.input}")
    print(f"[bake_sprites] active meshes ({len(meshes)}): {[m.name for m in meshes]}")
    print(f"[bake_sprites] clips ({len(clips)}): {[c['key'] for c in clips]}")
    print(f"[bake_sprites] bone map: {bone_map}")

    min_pt, max_pt = compute_world_bounds(meshes, armature, clips)
    camera = setup_camera_and_lighting(min_pt, max_pt, args.size, args.ortho_scale)
    configure_diffuse_materials(meshes)

    scene = bpy.context.scene
    out_dir = args.out.resolve()
    name = args.name or out_dir.name

    temp_root: Path
    cleanup_temp = not args.keep_frames
    if args.keep_frames:
        temp_root = out_dir / "_frames"
        if temp_root.exists():
            shutil.rmtree(temp_root)
        temp_root.mkdir(parents=True, exist_ok=True)
    else:
        temp_root = Path(tempfile.mkdtemp(prefix="vbreaker_bake_"))

    try:
        # Save original material slots per mesh before pass overrides
        original_mats: dict[bpy.types.Object, list[bpy.types.Material | None]] = {
            obj: [slot.material for slot in obj.material_slots] for obj in meshes
        }

        # --- Pass 1: Diffuse (Standard color management) ---
        set_view_transform_safe(scene, "Standard")
        scene.view_settings.look = "None"
        animations_meta, bone_sockets_meta, total_frames = render_pass(
            scene,
            armature,
            camera,
            clips,
            temp_root,
            "diffuse",
            args.size,
            bone_map,
            record_bones=True,
        )

        # --- Pass 2: Camera-Space Normal Map (Raw linear color management!) ---
        normal_mat = build_normal_override_material()
        for obj in meshes:
            if len(obj.data.materials) == 0:
                obj.data.materials.append(normal_mat)
            else:
                for i in range(len(obj.data.materials)):
                    obj.data.materials[i] = normal_mat

        set_view_transform_safe(scene, "Raw")
        render_pass(
            scene,
            armature,
            camera,
            clips,
            temp_root,
            "normal",
            args.size,
            bone_map,
            record_bones=False,
        )

        # --- Pass 3: Emission Mask (Standard color management, pure black non-emissive) ---
        auto_emission = not args.no_auto_emission
        for obj in meshes:
            orig_list = original_mats[obj]
            # If ANY material slot on this mesh carries native emission (e.g. a saber's plasma
            # cutting edge or a character body's Rift Diamond / visor glow map), preserve each
            # slot's exact native emission instead of overriding with procedural auto-emission.
            has_any_native = any(material_has_native_emission(m) for m in orig_list)
            slot_auto = auto_emission and not has_any_native
            for i in range(len(obj.data.materials)):
                orig_slot = orig_list[i] if i < len(orig_list) else None
                obj.data.materials[i] = build_emission_material(
                    obj,
                    orig_slot,
                    auto_emission=slot_auto,
                )

        set_view_transform_safe(scene, "Standard")
        render_pass(
            scene,
            armature,
            camera,
            clips,
            temp_root,
            "emission",
            args.size,
            bone_map,
            record_bones=False,
        )

        meta_path = temp_root / "frames_meta.json"
        meta_payload = {
            "source_model": str(args.input),
            "frame_size": [args.size, args.size],
            "frame_count": total_frames,
            "animations": animations_meta,
            "bone_sockets": bone_sockets_meta,
        }
        meta_path.write_text(json.dumps(meta_payload, indent=2), encoding="utf-8")

        # Invoke standalone pack_atlas.py to assemble synchronized sheets + .ron manifest
        pack_script = Path(__file__).resolve().parent / "pack_atlas.py"
        cmd = [
            args.python_bin,
            str(pack_script),
            "--frames-dir",
            str(temp_root),
            "--meta",
            str(meta_path),
            "--out",
            str(out_dir),
            "--name",
            name,
            "--columns",
            str(args.columns),
            "--quantize",
            str(args.quantize),
        ]
        print(f"[bake_sprites] running: {' '.join(cmd)}")
        subprocess.run(cmd, check=True)
        print(f"[bake_sprites] DONE: {total_frames} frames packed into {out_dir}")
    finally:
        if cleanup_temp and temp_root.exists():
            shutil.rmtree(temp_root, ignore_errors=True)


if __name__ == "__main__":
    bake(parse_args())
