#!/usr/bin/env python3
"""Procedural 2D Side-View Combat Animation & 3D Emissive Slash-Smear Authoring
for *VELOCITY // BREAKER*.

Replaces top-down camera-foreshortened RPG melee actions with expressive, crisp
2D side-view Smash Bros. / Mega Man Zero / anime sword-fighting choreography
across all 6 Story Bible characters (`levy`, `vulcanus`, `strider`, `enforcer`,
`arc_weaver`, `phantom`), complete with:
  - Athletic combat-ready `Idle` silhouette (saber held visibly angled, breathing bob)
  - Dramatic raised-weapon `Telegraph` windup (used by enemies AND Levy's Smash Charge!)
  - Crisp 3-hit Saber Combo (`jab`, `jab_2`, `jab_3`), `Dash_Slash`, `Air_Forward_Slash`,
    `d_air`, `f_smash`, `up_tilt`, `up_smash`, `down_tilt`, `down_smash`, `sword_spin`,
    `side_special`, `down_special`, `neutral_special`, `buster` (left Rift Gauntlet!),
    `guard`, and expressive `hitstun`
  - Baked 3D emissive crescent `SlashSmear` mesh driven by `smear_bone` so peak swing
    frames render blazing pixel-art plasma trails directly into `_diffuse` + `_emission`
  - Root-motion centering on `Dodge_Forward/Backward/Left` & anime sword-carry polish
    on `Running_A` / `Running_B` so blades never poke awkwardly or clip out of frame!
"""

from __future__ import annotations

import math
from typing import Any

import bmesh
import bpy
from mathutils import Matrix, Vector

# Character-space conventions inside rig_character_blender.py (before +90 deg Z baker yaw):
#   Forward = -Y   |  Backward = +Y
#   Up      = +Z   |  Down     = -Z
#   Left    = +X   |  Right    = -X (CAMERA-FACING FOREGROUND ARM -> weapon hand!)

DEFORM_ORDER: list[tuple[str, str | None]] = [
    ("root", None),
    ("hips", "root"),
    ("spine", "hips"),
    ("chest", "spine"),
    ("head", "chest"),
    ("upperarm.l", "chest"),
    ("lowerarm.l", "upperarm.l"),
    ("wrist.l", "lowerarm.l"),
    ("hand.l", "wrist.l"),
    ("handslot.l", "hand.l"),
    ("upperarm.r", "chest"),
    ("lowerarm.r", "upperarm.r"),
    ("wrist.r", "lowerarm.r"),
    ("hand.r", "wrist.r"),
    ("handslot.r", "hand.r"),
    ("upperleg.l", "hips"),
    ("lowerleg.l", "upperleg.l"),
    ("foot.l", "lowerleg.l"),
    ("toes.l", "foot.l"),
    ("upperleg.r", "hips"),
    ("lowerleg.r", "upperleg.r"),
    ("foot.r", "lowerleg.r"),
    ("toes.r", "foot.r"),
]

# Signature aspect plasma colors for baked 3D SlashSmear trails
SMEAR_COLORS: dict[str, tuple[tuple[float, float, float], tuple[float, float, float]]] = {
    # preset -> (outer_trail_rgb, inner_hot_core_rgb)
    "levy": ((0.12, 0.88, 1.0), (0.85, 0.99, 1.0)),
    "vulcanus": ((1.0, 0.42, 0.06), (1.0, 0.90, 0.45)),
    "strider": ((1.0, 0.14, 0.24), (1.0, 0.78, 0.78)),
    "enforcer": ((1.0, 0.58, 0.10), (1.0, 0.92, 0.55)),
    "arc_weaver": ((0.72, 0.24, 1.0), (0.94, 0.80, 1.0)),
    "phantom": ((1.0, 0.18, 0.86), (1.0, 0.82, 0.96)),
}


def add_smear_bone_and_mesh(arm_obj: bpy.types.Object, preset: str) -> bpy.types.Object:
    """Adds a non-deform `smear_bone` (identity rest matrix) under `root` and attaches a
    curved tapered 3D emissive crescent `SlashSmear` mesh for pixel-baked swing trails."""
    bpy.ops.object.select_all(action="DESELECT")
    arm_obj.select_set(True)
    bpy.context.view_layer.objects.active = arm_obj
    bpy.ops.object.mode_set(mode="EDIT")

    ebones = arm_obj.data.edit_bones
    if "smear_bone" not in ebones:
        sb = ebones.new("smear_bone")
        sb.head = Vector((0.0, 0.0, 0.0))
        sb.tail = Vector((0.0, 1.0, 0.0))  # Exact identity rest matrix!
        sb.roll = 0.0
        sb.use_deform = False
        sb.parent = ebones["root"]
        sb.use_connect = False

    bpy.ops.object.mode_set(mode="OBJECT")

    trail_rgb, core_rgb = SMEAR_COLORS.get(preset, SMEAR_COLORS["levy"])

    def make_emis_mat(name: str, rgb: tuple[float, float, float], strength: float) -> bpy.types.Material:
        mat = bpy.data.materials.new(name)
        mat.use_nodes = True
        bsdf = mat.node_tree.nodes["Principled BSDF"]
        bsdf.inputs["Base Color"].default_value = (*rgb, 1.0)
        bsdf.inputs["Emission Color"].default_value = (*rgb, 1.0)
        bsdf.inputs["Emission Strength"].default_value = strength
        bsdf.inputs["Roughness"].default_value = 0.2
        return mat

    mat_trail = make_emis_mat(f"SmearTrail_{preset}", trail_rgb, 2.6)
    mat_core = make_emis_mat(f"SmearCore_{preset}", core_rgb, 3.4)

    # Build a razor-sharp 155-degree anime crescent arc lying in the YZ screen plane (x = -0.24 foreground!)
    # Angles run from trailing tail (-82 deg) to thick blazing leading edge (+72 deg).
    # Flipping `scale.z = -1` reverses the crescent cleanly for downward vs upward slashes!
    mesh = bpy.data.meshes.new("SlashSmear")
    bm = bmesh.new()

    segs = 14
    a_start = math.radians(-82.0)
    a_end = math.radians(72.0)
    r_outer = 1.06
    x_fg = -0.22
    half_thick = 0.032

    outer_f: list[Any] = []
    mid_f: list[Any] = []
    inner_f: list[Any] = []
    outer_b: list[Any] = []
    mid_b: list[Any] = []
    inner_b: list[Any] = []

    for i in range(segs + 1):
        t = i / segs  # 0 = trailing tail, 1 = leading blade edge
        ang = a_start + (a_end - a_start) * t
        # Tapered swoosh width: thin needle tail -> widest near t=0.72 -> crisp tip at t=1.0
        env = math.sin(t * math.pi) ** 0.72 * (0.45 + 0.85 * t)
        w_total = 0.36 * env
        w_core = 0.12 * env

        ro = r_outer
        rm = max(0.15, ro - w_core)
        ri = max(0.10, ro - w_total)

        ca, sa = math.cos(ang), math.sin(ang)
        # In character space: forward = -Y, up = +Z
        outer_f.append(bm.verts.new((x_fg - half_thick, -ro * ca, ro * sa)))
        mid_f.append(bm.verts.new((x_fg - half_thick, -rm * ca, rm * sa)))
        inner_f.append(bm.verts.new((x_fg - half_thick, -ri * ca, ri * sa)))

        outer_b.append(bm.verts.new((x_fg + half_thick, -ro * ca, ro * sa)))
        mid_b.append(bm.verts.new((x_fg + half_thick, -rm * ca, rm * sa)))
        inner_b.append(bm.verts.new((x_fg + half_thick, -ri * ca, ri * sa)))

    for i in range(segs):
        # Front faces (camera-facing)
        f1 = bm.faces.new((outer_f[i], outer_f[i + 1], mid_f[i + 1], mid_f[i]))
        f1.material_index = 1  # White-hot leading razor core!
        f2 = bm.faces.new((mid_f[i], mid_f[i + 1], inner_f[i + 1], inner_f[i]))
        f2.material_index = 0  # Signature plasma wake!
        # Back faces (visible if rotated 180 deg during spin)
        f3 = bm.faces.new((outer_b[i + 1], outer_b[i], mid_b[i], mid_b[i + 1]))
        f3.material_index = 1
        f4 = bm.faces.new((mid_b[i + 1], mid_b[i], inner_b[i], inner_b[i + 1]))
        f4.material_index = 0

    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bm.to_mesh(mesh)
    bm.free()

    mesh.materials.append(mat_trail)
    mesh.materials.append(mat_core)

    smear_obj = bpy.data.objects.new("SlashSmear", mesh)
    bpy.context.scene.collection.objects.link(smear_obj)
    smear_obj.parent = arm_obj
    smear_obj.parent_type = "BONE"
    smear_obj.parent_bone = "smear_bone"
    # Cancel Blender's bone-tail parenting offset (smear_bone length = 1.0 along +Y)
    smear_obj.location = Vector((0.0, -1.0, 0.0))

    # Hide smear_bone by default on all previously retargeted actions!
    pb_smear = arm_obj.pose.bones["smear_bone"]
    pb_smear.rotation_mode = "QUATERNION"
    for act in bpy.data.actions:
        if arm_obj.animation_data is None:
            arm_obj.animation_data_create()
        arm_obj.animation_data.action = act
        if hasattr(act, "slots") and len(act.slots) > 0:
            arm_obj.animation_data.action_slot = act.slots[0]
        f0, f1 = act.frame_range
        pb_smear.location = Vector((0.0, 0.0, 0.0))
        pb_smear.rotation_quaternion = Matrix.Identity(3).to_quaternion()
        pb_smear.scale = Vector((0.0001, 0.0001, 0.0001))
        for f in (int(math.floor(f0)), int(math.ceil(f1))):
            pb_smear.keyframe_insert(data_path="location", frame=f)
            pb_smear.keyframe_insert(data_path="rotation_quaternion", frame=f)
            pb_smear.keyframe_insert(data_path="scale", frame=f)

    return smear_obj


def _mat_rx(deg: float) -> Matrix:
    return Matrix.Rotation(math.radians(deg), 3, "X")


def _mat_ry(deg: float) -> Matrix:
    return Matrix.Rotation(math.radians(deg), 3, "Y")


def _mat_rz(deg: float) -> Matrix:
    return Matrix.Rotation(math.radians(deg), 3, "Z")


def _weapon_target_matrix(
    blade_angle_deg: float,
    flip_edge: bool = False,
    splay_x: float = 0.0,
    root_yaw_deg: float = 0.0,
) -> Matrix:
    """Builds an exact orthonormal 3x3 target matrix for `handslot.r` so the weapon blade (+Y)
    points at `blade_angle_deg` strictly in the 2D screen plane (0=forward, +90=up, -90=down,
    +145=cocked high behind head) with its glowing cutting edge (-Z) facing the swing direction!"""
    a = math.radians(blade_angle_deg)
    # Forward = -Y, Up = +Z
    dir_y = Vector((splay_x, -math.cos(a), math.sin(a))).normalized()
    # Spine normal (+Z); cutting edge is -Z!
    dir_z = Vector((0.0, math.sin(a), math.cos(a))).normalized()
    if flip_edge:
        dir_z = -dir_z
    # Ensure strict orthogonality
    dir_x = dir_y.cross(dir_z).normalized()
    dir_z = dir_x.cross(dir_y).normalized()

    m = Matrix((
        (dir_x.x, dir_y.x, dir_z.x),
        (dir_x.y, dir_y.y, dir_z.y),
        (dir_x.z, dir_y.z, dir_z.z),
    ))
    if abs(root_yaw_deg) > 1e-4:
        m = _mat_rz(root_yaw_deg) @ m
    return m


def _make_pose_dict(**kwargs: Any) -> dict[str, Any]:
    """Creates a full-body 2D side-view pose specification with sensible combat-ready defaults."""
    base: dict[str, Any] = {
        "root_yaw": 0.0,
        "hips_y": 0.0,        # negative = lunge forward (-Y), positive = lean back (+Y)
        "hips_z": -0.06,      # slight athletic crouch drop (fraction of H)
        "torso_pitch": 10.0,  # + = lean forward, - = lean back
        "torso_yaw": 8.0,     # + = open chest slightly toward viewer
        "head_pitch": -4.0,   # counter-tilt so eyes stay locked straight ahead!
        # Right arm (CAMERA-FACING FOREGROUND WEAPON ARM)
        # Angles in 2D screen plane: 0 = forward, +90 = up, -90 = straight down, +145 = high back
        "r_ua": -58.0,        # upperarm.r angle
        "r_la": -8.0,         # lowerarm.r angle
        "r_flare": 6.0,       # slight outward splay toward camera (-X) so arm never clips torso
        "r_across": 0.0,      # yaw across body
        "weapon": 36.0,       # exact blade angle in 2D screen plane!
        "weapon_flip": False, # True for rising uppercut slashes (cutting edge faces UP!)
        "weapon_splay": -0.04,
        # Left arm (BACK ARM — Levy's glowing Rift Diamond Gauntlet!)
        "l_ua": -45.0,
        "l_la": 14.0,
        "l_flare": 10.0,
        "l_across": 0.0,
        # Legs (positive = leg swings FORWARD -Y, negative = leg swings BACKWARD +Y)
        "leg_l_hip": 16.0,    # front braced leg planted solidly on ground
        "leg_l_knee": 16.0,   # backward knee bend >= 0
        "leg_r_hip": -14.0,   # rear braced leg planted solidly on ground
        "leg_r_knee": 6.0,
        "foot_l_pitch": 0.0,
        "foot_r_pitch": 0.0,
        # Optional 3D emissive SlashSmear crescent: None or dict(y, z, pitch, scale, flip_z, horiz_spin)
        "smear": None,
    }
    base.update(kwargs)
    return base


def _apply_pose_and_keyframe(
    arm_obj: bpy.types.Object,
    m_rest: dict[str, Matrix],
    m_rest_rel: dict[str, Matrix],
    H: float,
    pose: dict[str, Any],
    frame: int,
) -> None:
    """Applies a world-space 2D fighter pose onto all 23 deform bones + `smear_bone`
    with zero gimbal lock and inserts crisp keyframes at `frame`."""
    root_yaw = float(pose["root_yaw"])
    r_root_yaw = _mat_rz(root_yaw)

    m_world: dict[str, Matrix] = {}

    def set_bone(name: str, parent: str | None, target_world: Matrix, loc: Vector | None = None) -> None:
        pb = arm_obj.pose.bones[name]
        pb.rotation_mode = "QUATERNION"
        if parent is None:
            m_base = m_rest[name]
        else:
            m_base = m_world[parent] @ m_rest_rel[name]
        m_local = m_base.inverted() @ target_world
        pb.rotation_quaternion = m_local.to_quaternion()
        if loc is not None:
            pb.location = loc
        else:
            pb.location = Vector((0.0, 0.0, 0.0))
        pb.keyframe_insert(data_path="rotation_quaternion", frame=frame)
        if name in ("root", "hips"):
            pb.keyframe_insert(data_path="location", frame=frame)
        m_world[name] = target_world

    # 1. Root & Hips
    set_bone("root", None, r_root_yaw @ m_rest["root"], Vector((0.0, 0.0, 0.0)))

    hips_world = r_root_yaw @ _mat_rz(pose["torso_yaw"] * 0.35) @ _mat_rx(pose["torso_pitch"] * 0.25) @ m_rest["hips"]
    # Convert desired character-space hips displacement (y, z) into hips local rest coordinates!
    hips_disp_world = Vector((0.0, pose["hips_y"] * H, pose["hips_z"] * H))
    hips_loc_local = (r_root_yaw @ m_rest["hips"]).inverted() @ (r_root_yaw @ hips_disp_world)
    set_bone("hips", "root", hips_world, hips_loc_local)

    # 2. Spine, Chest, Head
    tp = pose["torso_pitch"]
    ty = pose["torso_yaw"]
    spine_world = r_root_yaw @ _mat_rz(ty * 0.65) @ _mat_rx(tp * 0.65) @ m_rest["spine"]
    chest_world = r_root_yaw @ _mat_rz(ty) @ _mat_rx(tp) @ m_rest["chest"]
    head_world = r_root_yaw @ _mat_rz(ty * 0.20) @ _mat_rx(pose["head_pitch"]) @ m_rest["head"]
    set_bone("spine", "hips", spine_world)
    set_bone("chest", "spine", chest_world)
    set_bone("head", "chest", head_world)

    # 3. Right Arm (Foreground weapon arm, hanging basis = Ry(-82) @ M_rest)
    def arm_world(bname: str, is_right: bool, angle_deg: float, flare_deg: float, across_deg: float) -> Matrix:
        hang_y = -(82.0 - flare_deg) if is_right else (82.0 - flare_deg)
        m_hang = _mat_ry(hang_y) @ m_rest[bname]
        # angle_deg = 0 -> forward (-Y), +90 -> up (+Z), -90 -> down (-Z), +145 -> high back
        pitch_x = -(angle_deg + 90.0)
        z_yaw = -across_deg if is_right else across_deg
        return r_root_yaw @ _mat_rz(z_yaw) @ _mat_rx(pitch_x) @ m_hang

    r_ua = arm_world("upperarm.r", True, pose["r_ua"], pose["r_flare"], pose["r_across"])
    r_la = arm_world("lowerarm.r", True, pose["r_la"], pose["r_flare"] * 0.7, pose["r_across"] * 0.8)
    r_wr = arm_world("wrist.r", True, pose["r_la"], pose["r_flare"] * 0.5, pose["r_across"] * 0.6)
    r_hd = arm_world("hand.r", True, pose["r_la"], pose["r_flare"] * 0.4, pose["r_across"] * 0.5)
    r_ws = _weapon_target_matrix(
        pose["weapon"],
        flip_edge=bool(pose["weapon_flip"]),
        splay_x=float(pose["weapon_splay"]),
        root_yaw_deg=root_yaw,
    )
    set_bone("upperarm.r", "chest", r_ua)
    set_bone("lowerarm.r", "upperarm.r", r_la)
    set_bone("wrist.r", "lowerarm.r", r_wr)
    set_bone("hand.r", "wrist.r", r_hd)
    set_bone("handslot.r", "hand.r", r_ws)

    # 4. Left Arm (Off-hand — Levy's Rift Diamond Gauntlet!)
    l_ua = arm_world("upperarm.l", False, pose["l_ua"], pose["l_flare"], pose["l_across"])
    l_la = arm_world("lowerarm.l", False, pose["l_la"], pose["l_flare"] * 0.7, pose["l_across"] * 0.8)
    l_wr = arm_world("wrist.l", False, pose["l_la"], pose["l_flare"] * 0.5, pose["l_across"] * 0.6)
    l_hd = arm_world("hand.l", False, pose["l_la"], pose["l_flare"] * 0.4, pose["l_across"] * 0.5)
    l_ws = r_root_yaw @ m_rest["handslot.l"]
    set_bone("upperarm.l", "chest", l_ua)
    set_bone("lowerarm.l", "upperarm.l", l_la)
    set_bone("wrist.l", "lowerarm.l", l_wr)
    set_bone("hand.l", "wrist.l", l_hd)
    set_bone("handslot.l", "hand.l", l_ws)

    # 5. Legs (positive hip angle swings leg forward -Y; knee bends shin backward)
    for side, hip_a, knee_b, foot_p in (
        ("l", pose["leg_l_hip"], pose["leg_l_knee"], pose["foot_l_pitch"]),
        ("r", pose["leg_r_hip"], pose["leg_r_knee"], pose["foot_r_pitch"]),
    ):
        shin_a = hip_a - knee_b
        ul = r_root_yaw @ _mat_rx(-hip_a) @ m_rest[f"upperleg.{side}"]
        ll = r_root_yaw @ _mat_rx(-shin_a) @ m_rest[f"lowerleg.{side}"]
        ft = r_root_yaw @ _mat_rx(foot_p) @ m_rest[f"foot.{side}"]
        ts = r_root_yaw @ _mat_rx(foot_p) @ m_rest[f"toes.{side}"]
        set_bone(f"upperleg.{side}", "hips", ul)
        set_bone(f"lowerleg.{side}", f"upperleg.{side}", ll)
        set_bone(f"foot.{side}", f"lowerleg.{side}", ft)
        set_bone(f"toes.{side}", f"foot.{side}", ts)

    # 6. 3D Emissive SlashSmear Crescent Bone!
    pb_smear = arm_obj.pose.bones["smear_bone"]
    pb_smear.rotation_mode = "QUATERNION"
    sm = pose.get("smear")
    if sm is None:
        pb_smear.location = Vector((0.0, 0.0, 0.0))
        pb_smear.rotation_quaternion = Matrix.Identity(3).to_quaternion()
        pb_smear.scale = Vector((0.0001, 0.0001, 0.0001))
    else:
        sy = sm.get("y", -0.15) * H
        sz = sm.get("z", 0.58) * H
        sx = sm.get("x", 0.0) * H
        sc = float(sm.get("scale", 1.0)) * (H / 2.38)
        flip_z = -1.0 if sm.get("flip_z", False) else 1.0
        pitch = float(sm.get("pitch", 0.0))
        horiz = bool(sm.get("horiz_spin", False))
        spin_yaw = float(sm.get("spin_yaw", 0.0))

        pb_smear.location = Vector((sx, sy, sz))
        if horiz:
            # Tilted horizontal cyclone ring (62 deg) so the glowing 360-deg vortex reads
            # boldly in side orthographic pixel art without going edge-on!
            rot = _mat_rz(spin_yaw) @ _mat_ry(62.0) @ _mat_rx(pitch)
            pb_smear.rotation_quaternion = rot.to_quaternion()
            pb_smear.scale = Vector((sc * 1.10, sc * 1.08, sc * 1.08))
        else:
            # Pitch cleanly in 2D YZ screen plane!
            # Positive pitch rotates forward (-Y) UP toward (+Z): Rx(-pitch)!
            rot = _mat_rx(-pitch)
            pb_smear.rotation_quaternion = rot.to_quaternion()
            pb_smear.scale = Vector((sc, sc, sc * flip_z))

    pb_smear.keyframe_insert(data_path="location", frame=frame)
    pb_smear.keyframe_insert(data_path="rotation_quaternion", frame=frame)
    pb_smear.keyframe_insert(data_path="scale", frame=frame)


def _write_custom_action(
    arm_obj: bpy.types.Object,
    m_rest: dict[str, Matrix],
    m_rest_rel: dict[str, Matrix],
    H: float,
    action_name: str,
    poses: list[dict[str, Any]],
    frame_step: int = 4,
) -> bpy.types.Action:
    """Replaces (or creates) `action_name` with crisp keyframed 2D side-view poses."""
    existing = bpy.data.actions.get(action_name)
    if existing is not None:
        bpy.data.actions.remove(existing, do_unlink=True)

    act = bpy.data.actions.new(name=action_name)
    act.use_fake_user = True
    if arm_obj.animation_data is None:
        arm_obj.animation_data_create()
    arm_obj.animation_data.use_nla = False
    arm_obj.animation_data.action = act
    if hasattr(arm_obj.animation_data, "action_slot"):
        arm_obj.animation_data.action_slot = None

    for idx, pose in enumerate(poses):
        frame = idx * frame_step
        _apply_pose_and_keyframe(arm_obj, m_rest, m_rest_rel, H, pose, frame)

    # Set linear interpolation on smear scale so smears pop ON/OFF cleanly with zero ghostly bleed!
    if hasattr(act, "layers"):
        for layer in act.layers:
            for strip in layer.strips:
                for cbag in getattr(strip, "channelbags", []):
                    for fc in cbag.fcurves:
                        if "smear_bone" in fc.data_path:
                            for kp in fc.keyframe_points:
                                kp.interpolation = "CONSTANT"
    elif hasattr(act, "fcurves"):
        for fc in act.fcurves:
            if "smear_bone" in fc.data_path:
                for kp in fc.keyframe_points:
                    kp.interpolation = "CONSTANT"

    return act


def _polish_retargeted_locomotion(arm_obj: bpy.types.Object, preset: str) -> None:
    """Fixes two issues inherited from KayKit locomotion clips:
    1. Clamps horizontal root/hips travel on Dodge clips so characters never walk out of the 96x96 frame.
    2. Adjusts `handslot.r` on `Running_A` and `Running_B` so blades trail heroically backward/angled
       at the fighter's side instead of poking horizontally straight forward like a broomstick!
    """
    is_heavy = preset in ("vulcanus", "enforcer")
    scene = bpy.context.scene

    # 1. Center Dodge clips cleanly in frame
    for act_name in ("Dodge_Forward", "Dodge_Backward", "Dodge_Left"):
        act = bpy.data.actions.get(act_name)
        if act is None:
            continue
        arm_obj.animation_data.action = act
        if hasattr(act, "slots") and len(act.slots) > 0:
            arm_obj.animation_data.action_slot = act.slots[0]
        f0, f1 = int(math.floor(act.frame_range[0])), int(math.ceil(act.frame_range[1]))
        for f in range(f0, f1 + 1):
            scene.frame_set(f)
            bpy.context.view_layer.update()
            for bname in ("root", "hips"):
                pb = arm_obj.pose.bones[bname]
                # Keep vertical bounce/crouch, clamp horizontal drift tightly within ±0.22 units!
                loc = pb.location.copy()
                loc.x = max(-0.18, min(0.18, loc.x))
                loc.y = max(-0.22, min(0.22, loc.y))
                pb.location = loc
                pb.keyframe_insert(data_path="location", frame=f)

    # 2. Polish sword carry angle during Running_A & Running_B (angled cleanly back-down / hero carry!)
    m_rest = {b.name: b.matrix_local.to_3x3().copy() for b in arm_obj.data.bones}
    m_rest_rel = {
        name: (m_rest[parent].inverted() @ m_rest[name]) if parent else m_rest[name]
        for name, parent in DEFORM_ORDER
    }
    carry_blade_deg = -132.0 if not is_heavy else 132.0  # Sleek back-trailing saber vs shoulder-carried cleaver!
    for act_name in ("Running_A", "Running_B"):
        act = bpy.data.actions.get(act_name)
        if act is None:
            continue
        arm_obj.animation_data.action = act
        if hasattr(act, "slots") and len(act.slots) > 0:
            arm_obj.animation_data.action_slot = act.slots[0]
        f0, f1 = int(math.floor(act.frame_range[0])), int(math.ceil(act.frame_range[1]))
        span = max(1, f1 - f0)
        for f in range(f0, f1 + 1):
            t = (f - f0) / span
            swing_bob = math.sin(t * math.tau) * 14.0
            scene.frame_set(f)
            bpy.context.view_layer.update()

            # Compute current world matrix of hand.r so we can override ONLY handslot.r cleanly
            hand_world = arm_obj.pose.bones["hand.r"].matrix.to_3x3()
            target_w = _weapon_target_matrix(carry_blade_deg + swing_bob, flip_edge=False, splay_x=-0.06)
            m_base = hand_world @ m_rest_rel["handslot.r"]
            pb = arm_obj.pose.bones["handslot.r"]
            pb.rotation_mode = "QUATERNION"
            pb.rotation_quaternion = (m_base.inverted() @ target_w).to_quaternion()
            pb.keyframe_insert(data_path="rotation_quaternion", frame=f)


def author_all_2d_combat_animations(
    arm_obj: bpy.types.Object,
    lm: dict[str, Any],
    preset: str,
) -> list[str]:
    """Authors all expressive 2D side-view combat, stance, telegraph, and buster animations
    directly on `arm_obj` with baked 3D emissive SlashSmear crescent trails!"""
    H = float(lm["H"])
    is_heavy = preset in ("vulcanus", "enforcer")
    is_caster = preset == "arc_weaver"
    is_ninja = preset in ("strider", "phantom")

    m_rest = {b.name: b.matrix_local.to_3x3().copy() for b in arm_obj.data.bones}
    m_rest_rel = {
        name: (m_rest[parent].inverted() @ m_rest[name]) if parent else m_rest[name]
        for name, parent in DEFORM_ORDER
    }

    # First polish retargeted locomotion (dodge centering + anime sprint blade carry)
    _polish_retargeted_locomotion(arm_obj, preset)

    # Archetype-flavored combat idle stance (both boots planted cleanly on floor!)
    if is_heavy:
        # Vulcanus / Slag Enforcer: heavy grounded juggernaut with thermal cleaver/hammer resting high over shoulder!
        idle_base = dict(
            hips_z=-0.04, torso_pitch=8.0, torso_yaw=14.0,
            r_ua=-46.0, r_la=32.0, weapon=118.0,
            l_ua=-62.0, l_la=-22.0,
            leg_l_hip=14.0, leg_l_knee=16.0, leg_r_hip=-14.0, leg_r_knee=6.0,
        )
    elif is_caster:
        # Arc Weaver: poised mystic hover-ready stance, Tesla Staff held upright-forward!
        idle_base = dict(
            hips_z=-0.03, torso_pitch=4.0, torso_yaw=12.0,
            r_ua=-48.0, r_la=12.0, weapon=76.0,
            l_ua=-38.0, l_la=24.0,
            leg_l_hip=12.0, leg_l_knee=12.0, leg_r_hip=-12.0, leg_r_knee=4.0,
        )
    elif is_ninja:
        # Strider Drone / Phantom Interceptor: low agile predatory crouch with reverse-angled blade!
        idle_base = dict(
            hips_z=-0.08, torso_pitch=16.0, torso_yaw=16.0,
            r_ua=-112.0, r_la=-68.0, weapon=-142.0,
            l_ua=-30.0, l_la=26.0,
            leg_l_hip=22.0, leg_l_knee=26.0, leg_r_hip=-20.0, leg_r_knee=10.0,
        )
    else:
        # LEVY (HERO): Classic 1700s Duelist x Mega Man Zero battle stance!
        # Curved Rift Saber held gleaming up-forward at ~48 deg in crisp side profile,
        # left Rift Diamond Gauntlet raised in forward guard!
        idle_base = dict(
            hips_z=-0.04, torso_pitch=10.0, torso_yaw=14.0,
            r_ua=-54.0, r_la=6.0, weapon=48.0,
            l_ua=-36.0, l_la=24.0,
            leg_l_hip=16.0, leg_l_knee=18.0, leg_r_hip=-15.0, leg_r_knee=6.0,
        )

    def p(**overrides: Any) -> dict[str, Any]:
        d = dict(idle_base)
        d.update(overrides)
        return _make_pose_dict(**d)

    created: list[str] = []

    # -------------------------------------------------------------------------
    # 1. IDLE (6 frames, smooth looping breathing battle stance!)
    # -------------------------------------------------------------------------
    idle_poses = []
    for i in range(6):
        ph = (i / 6.0) * math.tau
        bob = math.sin(ph)
        idle_poses.append(
            p(
                hips_z=idle_base["hips_z"] + bob * 0.022,
                torso_pitch=idle_base["torso_pitch"] + bob * 3.2,
                r_ua=idle_base["r_ua"] + bob * 4.5,
                r_la=idle_base["r_la"] + bob * 5.0,
                weapon=idle_base["weapon"] + bob * 5.5,
                l_ua=idle_base["l_ua"] - bob * 3.5,
                l_la=idle_base["l_la"] + bob * 4.0,
                leg_l_knee=idle_base["leg_l_knee"] - bob * 4.0,
                leg_r_knee=idle_base["leg_r_knee"] - bob * 4.0,
            )
        )
    _write_custom_action(arm_obj, m_rest, m_rest_rel, H, "Idle", idle_poses)
    created.append("Idle")

    # -------------------------------------------------------------------------
    # 2. TELEGRAPH (5 frames — dramatic raised-weapon coiled anticipation windup!)
    #    Played by ALL enemies before attacking AND by Levy during Smash Charge!
    # -------------------------------------------------------------------------
    telegraph_poses = [
        p(hips_y=0.04, hips_z=-0.08, torso_pitch=-6.0, torso_yaw=-10.0, r_ua=45.0, r_la=95.0, weapon=118.0, l_ua=-25.0, l_la=35.0, leg_l_hip=18.0, leg_l_knee=28.0, leg_r_hip=-24.0, leg_r_knee=30.0),
        p(hips_y=0.08, hips_z=-0.11, torso_pitch=-16.0, torso_yaw=-22.0, r_ua=96.0, r_la=132.0, weapon=148.0, l_ua=10.0, l_la=52.0, leg_l_hip=26.0, leg_l_knee=34.0, leg_r_hip=-28.0, leg_r_knee=38.0),
        p(hips_y=0.10, hips_z=-0.12, torso_pitch=-20.0, torso_yaw=-26.0, r_ua=114.0, r_la=146.0, weapon=158.0, l_ua=16.0, l_la=58.0, leg_l_hip=28.0, leg_l_knee=36.0, leg_r_hip=-30.0, leg_r_knee=42.0),
        # Tension quiver at peak windup
        p(hips_y=0.11, hips_z=-0.13, torso_pitch=-22.0, torso_yaw=-28.0, r_ua=118.0, r_la=150.0, weapon=164.0, l_ua=18.0, l_la=62.0, leg_l_hip=28.0, leg_l_knee=38.0, leg_r_hip=-30.0, leg_r_knee=44.0),
        p(hips_y=0.10, hips_z=-0.12, torso_pitch=-19.0, torso_yaw=-25.0, r_ua=112.0, r_la=144.0, weapon=156.0, l_ua=14.0, l_la=56.0, leg_l_hip=27.0, leg_l_knee=36.0, leg_r_hip=-29.0, leg_r_knee=42.0),
    ]
    _write_custom_action(arm_obj, m_rest, m_rest_rel, H, "Telegraph", telegraph_poses)
    created.append("Telegraph")

    # -------------------------------------------------------------------------
    # 3. JAB 1 (`1H_Melee_Attack_Slice_Horizontal`) — SNAPPY OVERHEAD-TO-LOW RIFT CLEAVE (6 frames)
    # -------------------------------------------------------------------------
    jab1_poses = [
        # Frame 0: Quick snappy overhead cock
        p(hips_y=0.05, hips_z=-0.08, torso_pitch=-10.0, torso_yaw=-18.0, r_ua=88.0, r_la=124.0, weapon=140.0, l_ua=-18.0, l_la=35.0, leg_l_hip=20.0, leg_l_knee=26.0, leg_r_hip=-24.0, leg_r_knee=28.0),
        # Frame 1: Whipping forward-high cut!
        p(hips_y=-0.04, hips_z=-0.09, torso_pitch=16.0, torso_yaw=10.0, r_ua=38.0, r_la=32.0, weapon=34.0, l_ua=-75.0, l_la=-45.0, leg_l_hip=30.0, leg_l_knee=34.0, leg_r_hip=-28.0, leg_r_knee=20.0,
          smear=dict(y=-0.12, z=0.66, pitch=42.0, scale=0.92, flip_z=True)),
        # Frame 2 (PEAK IMPACT): HUGE 150-deg downward-forward sweeping cleave!
        p(hips_y=-0.11, hips_z=-0.12, torso_pitch=26.0, torso_yaw=26.0, r_ua=-24.0, r_la=-34.0, weapon=-38.0, l_ua=-120.0, l_la=-88.0, leg_l_hip=36.0, leg_l_knee=42.0, leg_r_hip=-34.0, leg_r_knee=16.0,
          smear=dict(y=-0.16, z=0.58, pitch=-4.0, scale=1.08, flip_z=True)),
        # Frame 3: Extended low-front follow-through + fading trail
        p(hips_y=-0.12, hips_z=-0.13, torso_pitch=28.0, torso_yaw=30.0, r_ua=-46.0, r_la=-54.0, weapon=-58.0, l_ua=-130.0, l_la=-96.0, leg_l_hip=36.0, leg_l_knee=44.0, leg_r_hip=-36.0, leg_r_knee=18.0,
          smear=dict(y=-0.18, z=0.52, pitch=-22.0, scale=0.90, flip_z=True)),
        # Frame 4: Follow-through hold (sets up Jab 2 rising slash!)
        p(hips_y=-0.08, hips_z=-0.11, torso_pitch=20.0, torso_yaw=22.0, r_ua=-56.0, r_la=-60.0, weapon=-64.0, l_ua=-105.0, l_la=-70.0, leg_l_hip=30.0, leg_l_knee=36.0, leg_r_hip=-28.0, leg_r_knee=22.0),
        # Frame 5: Snap back toward ready
        p(hips_y=-0.02, hips_z=-0.07, torso_pitch=12.0, torso_yaw=14.0, r_ua=-52.0, r_la=-4.0, weapon=36.0, l_ua=-46.0, l_la=14.0),
    ]
    _write_custom_action(arm_obj, m_rest, m_rest_rel, H, "1H_Melee_Attack_Slice_Horizontal", jab1_poses)
    created.append("1H_Melee_Attack_Slice_Horizontal")

    # -------------------------------------------------------------------------
    # 4. JAB 2 (`1H_Melee_Attack_Slice_Diagonal`) — RISING REVERSE UPPERCUT SLASH (5 frames)
    # -------------------------------------------------------------------------
    jab2_poses = [
        # Frame 0: Low crouched coil continuing from Jab 1!
        p(hips_y=-0.04, hips_z=-0.16, torso_pitch=24.0, torso_yaw=-16.0, r_ua=-88.0, r_la=-76.0, weapon=-82.0, weapon_flip=True, l_ua=-60.0, l_la=-20.0, leg_l_hip=34.0, leg_l_knee=48.0, leg_r_hip=-30.0, leg_r_knee=42.0),
        # Frame 1: Explosive upward rip through mid-front!
        p(hips_y=-0.08, hips_z=-0.08, torso_pitch=6.0, torso_yaw=12.0, r_ua=14.0, r_la=26.0, weapon=36.0, weapon_flip=True, l_ua=-110.0, l_la=-70.0, leg_l_hip=28.0, leg_l_knee=28.0, leg_r_hip=-28.0, leg_r_knee=16.0,
          smear=dict(y=-0.14, z=0.54, pitch=12.0, scale=1.02, flip_z=False)),
        # Frame 2 (PEAK IMPACT): HUGE SKYWARD RISING CRESCENT!
        p(hips_y=-0.10, hips_z=-0.02, torso_pitch=-14.0, torso_yaw=24.0, r_ua=76.0, r_la=96.0, weapon=110.0, weapon_flip=True, l_ua=-128.0, l_la=-92.0, leg_l_hip=22.0, leg_l_knee=16.0, leg_r_hip=-26.0, leg_r_knee=14.0,
          smear=dict(y=-0.14, z=0.68, pitch=56.0, scale=1.12, flip_z=False)),
        # Frame 3: High heroic follow-through!
        p(hips_y=-0.06, hips_z=-0.04, torso_pitch=-10.0, torso_yaw=20.0, r_ua=92.0, r_la=112.0, weapon=126.0, weapon_flip=True, l_ua=-95.0, l_la=-60.0, leg_l_hip=22.0, leg_l_knee=20.0, leg_r_hip=-22.0, leg_r_knee=18.0,
          smear=dict(y=-0.10, z=0.72, pitch=74.0, scale=0.86, flip_z=False)),
        # Frame 4: Recover
        p(hips_y=-0.02, hips_z=-0.06, torso_pitch=6.0, torso_yaw=14.0, r_ua=-18.0, r_la=22.0, weapon=54.0),
    ]
    _write_custom_action(arm_obj, m_rest, m_rest_rel, H, "1H_Melee_Attack_Slice_Diagonal", jab2_poses)
    created.append("1H_Melee_Attack_Slice_Diagonal")

    # -------------------------------------------------------------------------
    # 5. JAB 3 (`1H_Melee_Attack_Stab`) — BREAKER FULL-EXTENSION POWER LUNGE SLASH (6 frames)
    # -------------------------------------------------------------------------
    jab3_poses = [
        # Frame 0: Deep back-step coil, saber drawn high-back!
        p(hips_y=0.08, hips_z=-0.11, torso_pitch=-14.0, torso_yaw=-24.0, r_ua=110.0, r_la=140.0, weapon=152.0, l_ua=-10.0, l_la=40.0, leg_l_hip=22.0, leg_l_knee=34.0, leg_r_hip=-30.0, leg_r_knee=36.0),
        # Frame 1: Explosive forward drive!
        p(hips_y=-0.06, hips_z=-0.13, torso_pitch=24.0, torso_yaw=8.0, r_ua=24.0, r_la=20.0, weapon=22.0, l_ua=-110.0, l_la=-75.0, leg_l_hip=38.0, leg_l_knee=46.0, leg_r_hip=-40.0, leg_r_knee=18.0,
          smear=dict(y=-0.20, z=0.60, pitch=20.0, scale=1.08, flip_z=True)),
        # Frame 2 (PEAK IMPACT): FULL-REACH HORIZONTAL-DIAGONAL BREAKER CLEAVE!
        p(hips_y=-0.16, hips_z=-0.14, torso_pitch=34.0, torso_yaw=28.0, r_ua=-8.0, r_la=-12.0, weapon=-14.0, l_ua=-142.0, l_la=-120.0, leg_l_hip=44.0, leg_l_knee=52.0, leg_r_hip=-46.0, leg_r_knee=14.0,
          smear=dict(y=-0.28, z=0.56, pitch=-2.0, scale=1.24, flip_z=True)),
        # Frame 3: Massive follow-through streak!
        p(hips_y=-0.17, hips_z=-0.14, torso_pitch=34.0, torso_yaw=32.0, r_ua=-24.0, r_la=-28.0, weapon=-30.0, l_ua=-146.0, l_la=-125.0, leg_l_hip=44.0, leg_l_knee=52.0, leg_r_hip=-46.0, leg_r_knee=14.0,
          smear=dict(y=-0.28, z=0.52, pitch=-16.0, scale=1.08, flip_z=True)),
        # Frame 4: Planted heroic finish
        p(hips_y=-0.10, hips_z=-0.11, torso_pitch=22.0, torso_yaw=22.0, r_ua=-38.0, r_la=-36.0, weapon=-42.0, l_ua=-112.0, l_la=-80.0, leg_l_hip=34.0, leg_l_knee=40.0, leg_r_hip=-34.0, leg_r_knee=20.0),
        # Frame 5: Recover
        p(hips_y=-0.02, hips_z=-0.07, torso_pitch=12.0, torso_yaw=14.0),
    ]
    _write_custom_action(arm_obj, m_rest, m_rest_rel, H, "1H_Melee_Attack_Stab", jab3_poses)
    created.append("1H_Melee_Attack_Stab")

    # -------------------------------------------------------------------------
    # 6. DASH ATTACK (`Dash_Slash`) — HIGH-SPEED LEANING SPRINT CRESCENT (6 frames)
    # -------------------------------------------------------------------------
    dash_poses = [
        p(hips_y=-0.04, hips_z=-0.12, torso_pitch=26.0, torso_yaw=-20.0, r_ua=-132.0, r_la=-118.0, weapon=-138.0, l_ua=15.0, l_la=50.0, leg_l_hip=36.0, leg_l_knee=46.0, leg_r_hip=-42.0, leg_r_knee=24.0),
        p(hips_y=-0.12, hips_z=-0.14, torso_pitch=34.0, torso_yaw=4.0, r_ua=20.0, r_la=14.0, weapon=16.0, l_ua=-120.0, l_la=-90.0, leg_l_hip=42.0, leg_l_knee=48.0, leg_r_hip=-48.0, leg_r_knee=20.0,
          smear=dict(y=-0.24, z=0.56, pitch=12.0, scale=1.14, flip_z=True)),
        p(hips_y=-0.16, hips_z=-0.15, torso_pitch=38.0, torso_yaw=28.0, r_ua=-20.0, r_la=-26.0, weapon=-28.0, l_ua=-145.0, l_la=-122.0, leg_l_hip=44.0, leg_l_knee=54.0, leg_r_hip=-50.0, leg_r_knee=16.0,
          smear=dict(y=-0.28, z=0.52, pitch=-8.0, scale=1.22, flip_z=True)),
        p(hips_y=-0.14, hips_z=-0.13, torso_pitch=32.0, torso_yaw=30.0, r_ua=-44.0, r_la=-50.0, weapon=-56.0, l_ua=-135.0, l_la=-110.0, leg_l_hip=40.0, leg_l_knee=48.0, leg_r_hip=-42.0, leg_r_knee=18.0,
          smear=dict(y=-0.24, z=0.46, pitch=-26.0, scale=0.98, flip_z=True)),
        p(hips_y=-0.08, hips_z=-0.10, torso_pitch=22.0, torso_yaw=20.0, r_ua=-56.0, r_la=-50.0, weapon=-48.0, l_ua=-95.0, l_la=-60.0),
        p(hips_y=-0.02, hips_z=-0.07, torso_pitch=12.0, torso_yaw=14.0),
    ]
    _write_custom_action(arm_obj, m_rest, m_rest_rel, H, "Dash_Slash", dash_poses)
    created.append("Dash_Slash")

    # -------------------------------------------------------------------------
    # 7. FORWARD SMASH (`2H_Melee_Attack_Slice`) — TITAN 190° OVERHEAD KILL CLEAVE (7 frames)
    # -------------------------------------------------------------------------
    fsmash_poses = [
        # Frame 0: Coiled high back hoist (readable Smash Charge frame!)
        p(hips_y=0.08, hips_z=-0.10, torso_pitch=-16.0, torso_yaw=-22.0, r_ua=106.0, r_la=140.0, weapon=156.0, l_ua=88.0, l_la=124.0, leg_l_hip=24.0, leg_l_knee=34.0, leg_r_hip=-28.0, leg_r_knee=38.0),
        # Frame 1: Peak sky-high two-handed windup!
        p(hips_y=0.10, hips_z=-0.12, torso_pitch=-22.0, torso_yaw=-26.0, r_ua=124.0, r_la=154.0, weapon=168.0, l_ua=104.0, l_la=138.0, leg_l_hip=26.0, leg_l_knee=38.0, leg_r_hip=-32.0, leg_r_knee=44.0),
        # Frame 2: Thunderous overhead apex blur!
        p(hips_y=-0.04, hips_z=-0.08, torso_pitch=10.0, torso_yaw=4.0, r_ua=70.0, r_la=74.0, weapon=78.0, l_ua=54.0, l_la=62.0, leg_l_hip=34.0, leg_l_knee=38.0, leg_r_hip=-34.0, leg_r_knee=22.0,
          smear=dict(y=-0.10, z=0.72, pitch=52.0, scale=1.22, flip_z=True)),
        # Frame 3 (PEAK IMPACT): MASSIVE 190° HEAD-TO-BOOT TITAN CLEAVE!
        p(hips_y=-0.16, hips_z=-0.16, torso_pitch=36.0, torso_yaw=26.0, r_ua=-18.0, r_la=-26.0, weapon=-34.0, l_ua=-38.0, l_la=-44.0, leg_l_hip=44.0, leg_l_knee=54.0, leg_r_hip=-44.0, leg_r_knee=16.0,
          smear=dict(y=-0.22, z=0.58, pitch=-4.0, scale=1.36, flip_z=True)),
        # Frame 4: Ground-shattering low slam follow-through!
        p(hips_y=-0.17, hips_z=-0.18, torso_pitch=40.0, torso_yaw=30.0, r_ua=-46.0, r_la=-56.0, weapon=-62.0, l_ua=-64.0, l_la=-68.0, leg_l_hip=44.0, leg_l_knee=58.0, leg_r_hip=-44.0, leg_r_knee=18.0,
          smear=dict(y=-0.24, z=0.46, pitch=-28.0, scale=1.22, flip_z=True)),
        # Frame 5: Heavy recovery lift
        p(hips_y=-0.10, hips_z=-0.13, torso_pitch=24.0, torso_yaw=20.0, r_ua=-54.0, r_la=-46.0, weapon=-42.0, l_ua=-70.0, l_la=-42.0, leg_l_hip=34.0, leg_l_knee=42.0, leg_r_hip=-32.0, leg_r_knee=22.0),
        # Frame 6: Return to guard
        p(hips_y=-0.02, hips_z=-0.07, torso_pitch=12.0, torso_yaw=14.0),
    ]
    _write_custom_action(arm_obj, m_rest, m_rest_rel, H, "2H_Melee_Attack_Slice", fsmash_poses)
    created.append("2H_Melee_Attack_Slice")

    # -------------------------------------------------------------------------
    # 8. UP-TILT (`Spellcast_Raise`) — 180° OVERHEAD RAINBOW MOON SLASH (5 frames)
    # -------------------------------------------------------------------------
    uptilt_poses = [
        # Frame 0: Low-front crouch starting point
        p(hips_y=-0.04, hips_z=-0.14, torso_pitch=18.0, torso_yaw=10.0, r_ua=-38.0, r_la=-26.0, weapon=-20.0, weapon_flip=True, l_ua=-70.0, l_la=-30.0, leg_l_hip=30.0, leg_l_knee=42.0, leg_r_hip=-28.0, leg_r_knee=36.0),
        # Frame 1: Ripping upward-forward!
        p(hips_y=-0.04, hips_z=-0.05, torso_pitch=-4.0, torso_yaw=14.0, r_ua=46.0, r_la=56.0, weapon=64.0, weapon_flip=True, l_ua=-105.0, l_la=-65.0, leg_l_hip=22.0, leg_l_knee=22.0, leg_r_hip=-24.0, leg_r_knee=16.0,
          smear=dict(y=-0.08, z=0.72, pitch=58.0, scale=1.08, flip_z=False)),
        # Frame 2 (PEAK OVERHEAD MOON): Sweeping straight over crown!
        p(hips_y=0.0, hips_z=-0.02, torso_pitch=-16.0, torso_yaw=18.0, r_ua=96.0, r_la=108.0, weapon=118.0, weapon_flip=True, l_ua=-120.0, l_la=-85.0, leg_l_hip=18.0, leg_l_knee=16.0, leg_r_hip=-22.0, leg_r_knee=18.0,
          smear=dict(y=0.02, z=0.80, pitch=96.0, scale=1.18, flip_z=False)),
        # Frame 3: Completing overhead arc behind head!
        p(hips_y=0.04, hips_z=-0.06, torso_pitch=-14.0, torso_yaw=14.0, r_ua=136.0, r_la=148.0, weapon=162.0, weapon_flip=True, l_ua=-90.0, l_la=-50.0, leg_l_hip=20.0, leg_l_knee=24.0, leg_r_hip=-24.0, leg_r_knee=24.0,
          smear=dict(y=0.08, z=0.76, pitch=128.0, scale=0.96, flip_z=False)),
        # Frame 4: Settle
        p(hips_y=0.0, hips_z=-0.07, torso_pitch=8.0, torso_yaw=12.0),
    ]
    _write_custom_action(arm_obj, m_rest, m_rest_rel, H, "Spellcast_Raise", uptilt_poses)
    created.append("Spellcast_Raise")

    # -------------------------------------------------------------------------
    # 9. UP-SMASH (`2H_Melee_Attack_Chop`) — SKYWARD LEAPING RIFT UPPERCUT (7 frames)
    # -------------------------------------------------------------------------
    upsmash_poses = [
        # Frame 0–1: Deep coiled crouch with blade cocked low!
        p(hips_y=0.02, hips_z=-0.19, torso_pitch=26.0, torso_yaw=-14.0, r_ua=-78.0, r_la=-68.0, weapon=-74.0, weapon_flip=True, l_ua=-70.0, l_la=-40.0, leg_l_hip=38.0, leg_l_knee=58.0, leg_r_hip=-32.0, leg_r_knee=54.0),
        p(hips_y=0.02, hips_z=-0.21, torso_pitch=28.0, torso_yaw=-18.0, r_ua=-88.0, r_la=-78.0, weapon=-86.0, weapon_flip=True, l_ua=-80.0, l_la=-48.0, leg_l_hip=40.0, leg_l_knee=62.0, leg_r_hip=-34.0, leg_r_knee=58.0),
        # Frame 2: Explosive skyward launch!
        p(hips_y=-0.04, hips_z=0.04, torso_pitch=-8.0, torso_yaw=10.0, r_ua=54.0, r_la=64.0, weapon=72.0, weapon_flip=True, l_ua=40.0, l_la=55.0, leg_l_hip=12.0, leg_l_knee=14.0, leg_r_hip=-16.0, leg_r_knee=16.0,
          smear=dict(y=-0.10, z=0.78, pitch=66.0, scale=1.24, flip_z=False)),
        # Frame 3 (PEAK SKYWARD KILLER): Blade piercing straight up into the sky!
        p(hips_y=-0.04, hips_z=0.10, torso_pitch=-20.0, torso_yaw=18.0, r_ua=96.0, r_la=102.0, weapon=102.0, weapon_flip=True, l_ua=76.0, l_la=90.0, leg_l_hip=8.0, leg_l_knee=18.0, leg_r_hip=-14.0, leg_r_knee=22.0,
          smear=dict(y=-0.04, z=0.88, pitch=92.0, scale=1.34, flip_z=False)),
        # Frame 4: Peak arc follow-through
        p(hips_y=0.0, hips_z=0.04, torso_pitch=-16.0, torso_yaw=18.0, r_ua=124.0, r_la=132.0, weapon=140.0, weapon_flip=True, l_ua=96.0, l_la=112.0, leg_l_hip=14.0, leg_l_knee=22.0, leg_r_hip=-18.0, leg_r_knee=24.0,
          smear=dict(y=0.04, z=0.84, pitch=118.0, scale=1.12, flip_z=False)),
        # Frame 5: Athletic landing
        p(hips_y=-0.02, hips_z=-0.12, torso_pitch=14.0, torso_yaw=14.0, r_ua=-30.0, r_la=10.0, weapon=40.0, leg_l_hip=28.0, leg_l_knee=38.0, leg_r_hip=-26.0, leg_r_knee=34.0),
        # Frame 6: Ready
        p(),
    ]
    _write_custom_action(arm_obj, m_rest, m_rest_rel, H, "2H_Melee_Attack_Chop", upsmash_poses)
    created.append("2H_Melee_Attack_Chop")

    # -------------------------------------------------------------------------
    # 10. DOWN-TILT (`Unarmed_Melee_Attack_Kick`) — CROUCHING LOW ANKLE SABER SWEEP (5 frames)
    # -------------------------------------------------------------------------
    dtilt_poses = [
        # Frame 0: Drop low into kneeling crouch, saber pulled back low!
        p(hips_y=0.02, hips_z=-0.22, torso_pitch=28.0, torso_yaw=-20.0, r_ua=-126.0, r_la=-114.0, weapon=-158.0, l_ua=-40.0, l_la=-10.0, leg_l_hip=48.0, leg_l_knee=68.0, leg_r_hip=-36.0, leg_r_knee=64.0),
        # Frame 1: Razor-low floor sweep!
        p(hips_y=-0.10, hips_z=-0.24, torso_pitch=34.0, torso_yaw=14.0, r_ua=-38.0, r_la=-24.0, weapon=-16.0, l_ua=-120.0, l_la=-90.0, leg_l_hip=52.0, leg_l_knee=70.0, leg_r_hip=-44.0, leg_r_knee=58.0,
          smear=dict(y=-0.26, z=0.22, pitch=-8.0, scale=1.08, flip_z=False)),
        # Frame 2 (PEAK LOW SWEEP): Full low ankle extension with glowing floor streak!
        p(hips_y=-0.14, hips_z=-0.24, torso_pitch=36.0, torso_yaw=28.0, r_ua=-24.0, r_la=-14.0, weapon=-8.0, l_ua=-138.0, l_la=-112.0, leg_l_hip=54.0, leg_l_knee=70.0, leg_r_hip=-48.0, leg_r_knee=56.0,
          smear=dict(y=-0.30, z=0.20, pitch=-4.0, scale=1.18, flip_z=False)),
        # Frame 3: Crisp low follow-through
        p(hips_y=-0.08, hips_z=-0.18, torso_pitch=24.0, torso_yaw=20.0, r_ua=-40.0, r_la=-28.0, weapon=-20.0, l_ua=-90.0, l_la=-50.0, leg_l_hip=42.0, leg_l_knee=54.0, leg_r_hip=-36.0, leg_r_knee=44.0),
        # Frame 4: Spring up
        p(),
    ]
    _write_custom_action(arm_obj, m_rest, m_rest_rel, H, "Unarmed_Melee_Attack_Kick", dtilt_poses)
    created.append("Unarmed_Melee_Attack_Kick")

    # -------------------------------------------------------------------------
    # 11. DOWN-SMASH (`2H_Melee_Attack_Spin`) — 360° LOW DOUBLE-SIDED SPIN SWEEP (7 frames)
    # -------------------------------------------------------------------------
    dsmash_poses = [
        p(root_yaw=0.0, hips_z=-0.20, torso_pitch=24.0, torso_yaw=-24.0, r_ua=-115.0, r_la=-95.0, weapon=-150.0, leg_l_hip=44.0, leg_l_knee=62.0, leg_r_hip=-34.0, leg_r_knee=58.0),
        p(root_yaw=60.0, hips_z=-0.22, torso_pitch=28.0, torso_yaw=10.0, r_ua=-28.0, r_la=-16.0, weapon=-10.0, leg_l_hip=48.0, leg_l_knee=64.0, leg_r_hip=-40.0, leg_r_knee=56.0,
          smear=dict(y=0.0, z=0.26, pitch=0.0, scale=1.24, horiz_spin=True, spin_yaw=40.0)),
        p(root_yaw=140.0, hips_z=-0.22, torso_pitch=28.0, torso_yaw=10.0, r_ua=-24.0, r_la=-14.0, weapon=-8.0, leg_l_hip=48.0, leg_l_knee=64.0, leg_r_hip=-40.0, leg_r_knee=56.0,
          smear=dict(y=0.0, z=0.26, pitch=0.0, scale=1.30, horiz_spin=True, spin_yaw=140.0)),
        p(root_yaw=225.0, hips_z=-0.22, torso_pitch=28.0, torso_yaw=10.0, r_ua=-24.0, r_la=-14.0, weapon=-8.0, leg_l_hip=48.0, leg_l_knee=64.0, leg_r_hip=-40.0, leg_r_knee=56.0,
          smear=dict(y=0.0, z=0.26, pitch=0.0, scale=1.30, horiz_spin=True, spin_yaw=225.0)),
        p(root_yaw=310.0, hips_z=-0.20, torso_pitch=24.0, torso_yaw=14.0, r_ua=-28.0, r_la=-18.0, weapon=-12.0, leg_l_hip=42.0, leg_l_knee=56.0, leg_r_hip=-36.0, leg_r_knee=50.0,
          smear=dict(y=0.0, z=0.26, pitch=0.0, scale=1.18, horiz_spin=True, spin_yaw=310.0)),
        p(root_yaw=350.0, hips_z=-0.12, torso_pitch=16.0, torso_yaw=16.0, r_ua=-46.0, r_la=-18.0, weapon=18.0),
        p(root_yaw=360.0),
    ]
    _write_custom_action(arm_obj, m_rest, m_rest_rel, H, "2H_Melee_Attack_Spin", dsmash_poses)
    created.append("2H_Melee_Attack_Spin")

    # -------------------------------------------------------------------------
    # 12. FORWARD AIR (`Air_Forward_Slash`) — ICONIC MID-AIR 170° CRESCENT SLASH (6 frames)
    # -------------------------------------------------------------------------
    fair_poses = [
        # Frame 0: Athletic mid-air tuck, saber raised high overhead!
        p(hips_y=0.02, hips_z=0.06, torso_pitch=-14.0, torso_yaw=-18.0, r_ua=104.0, r_la=132.0, weapon=144.0, l_ua=-25.0, l_la=35.0, leg_l_hip=44.0, leg_l_knee=68.0, leg_r_hip=-16.0, leg_r_knee=56.0, foot_l_pitch=24.0, foot_r_pitch=24.0),
        # Frame 1: Top half of aerial crescent!
        p(hips_y=-0.04, hips_z=0.05, torso_pitch=12.0, torso_yaw=8.0, r_ua=44.0, r_la=42.0, weapon=44.0, l_ua=-88.0, l_la=-50.0, leg_l_hip=38.0, leg_l_knee=56.0, leg_r_hip=-24.0, leg_r_knee=46.0, foot_l_pitch=20.0, foot_r_pitch=24.0,
          smear=dict(y=-0.14, z=0.70, pitch=42.0, scale=1.08, flip_z=True)),
        # Frame 2 (PEAK AIR SLASH): Huge vertical front crescent!
        p(hips_y=-0.08, hips_z=0.04, torso_pitch=26.0, torso_yaw=24.0, r_ua=-22.0, r_la=-30.0, weapon=-34.0, l_ua=-126.0, l_la=-92.0, leg_l_hip=30.0, leg_l_knee=48.0, leg_r_hip=-32.0, leg_r_knee=40.0, foot_l_pitch=18.0, foot_r_pitch=26.0,
          smear=dict(y=-0.18, z=0.60, pitch=-8.0, scale=1.20, flip_z=True)),
        # Frame 3: Sweeping down past boots!
        p(hips_y=-0.08, hips_z=0.03, torso_pitch=30.0, torso_yaw=28.0, r_ua=-56.0, r_la=-66.0, weapon=-74.0, l_ua=-134.0, l_la=-104.0, leg_l_hip=26.0, leg_l_knee=46.0, leg_r_hip=-34.0, leg_r_knee=38.0, foot_l_pitch=22.0, foot_r_pitch=28.0,
          smear=dict(y=-0.16, z=0.48, pitch=-36.0, scale=1.06, flip_z=True)),
        # Frame 4: Follow-through air pose
        p(hips_y=-0.04, hips_z=0.03, torso_pitch=18.0, torso_yaw=16.0, r_ua=-74.0, r_la=-68.0, weapon=-70.0, leg_l_hip=32.0, leg_l_knee=52.0, leg_r_hip=-24.0, leg_r_knee=46.0),
        # Frame 5: Air ready
        p(hips_y=0.0, hips_z=0.04, torso_pitch=8.0, torso_yaw=12.0, leg_l_hip=36.0, leg_l_knee=56.0, leg_r_hip=-18.0, leg_r_knee=48.0),
    ]
    _write_custom_action(arm_obj, m_rest, m_rest_rel, H, "Air_Forward_Slash", fair_poses)
    created.append("Air_Forward_Slash")

    # -------------------------------------------------------------------------
    # 13. DOWN AIR (`1H_Melee_Attack_Chop`) — METEOR SPIKE DOWNWARD SABER PLUNGE (6 frames)
    # -------------------------------------------------------------------------
    dair_poses = [
        # Frame 0: Tuck high in air with saber raised
        p(hips_z=0.10, torso_pitch=-10.0, torso_yaw=-14.0, r_ua=82.0, r_la=112.0, weapon=118.0, l_ua=56.0, l_la=92.0, leg_l_hip=52.0, leg_l_knee=82.0, leg_r_hip=28.0, leg_r_knee=74.0),
        # Frame 1: Driving downward!
        p(hips_z=0.08, torso_pitch=26.0, torso_yaw=6.0, r_ua=-38.0, r_la=-56.0, weapon=-72.0, l_ua=-30.0, l_la=-46.0, leg_l_hip=36.0, leg_l_knee=68.0, leg_r_hip=-12.0, leg_r_knee=58.0,
          smear=dict(y=-0.10, z=0.36, pitch=-68.0, scale=1.12, flip_z=True)),
        # Frame 2 (PEAK METEOR SPIKE): Saber plunged straight down beneath boots!
        p(hips_z=0.06, torso_pitch=40.0, torso_yaw=14.0, r_ua=-78.0, r_la=-88.0, weapon=-92.0, l_ua=-66.0, l_la=-78.0, leg_l_hip=24.0, leg_l_knee=56.0, leg_r_hip=-24.0, leg_r_knee=48.0,
          smear=dict(y=-0.04, z=0.22, pitch=-92.0, scale=1.26, flip_z=True)),
        # Frame 3: Sustained downward spike!
        p(hips_z=0.05, torso_pitch=40.0, torso_yaw=14.0, r_ua=-82.0, r_la=-90.0, weapon=-94.0, l_ua=-70.0, l_la=-80.0, leg_l_hip=22.0, leg_l_knee=54.0, leg_r_hip=-26.0, leg_r_knee=46.0,
          smear=dict(y=-0.02, z=0.20, pitch=-96.0, scale=1.10, flip_z=True)),
        # Frame 4: Pull out
        p(hips_z=0.05, torso_pitch=24.0, torso_yaw=12.0, r_ua=-62.0, r_la=-54.0, weapon=-58.0, leg_l_hip=32.0, leg_l_knee=58.0, leg_r_hip=-18.0, leg_r_knee=48.0),
        # Frame 5: Air ready
        p(hips_z=0.04, torso_pitch=10.0, torso_yaw=12.0, leg_l_hip=34.0, leg_l_knee=56.0, leg_r_hip=-16.0, leg_r_knee=48.0),
    ]
    _write_custom_action(arm_obj, m_rest, m_rest_rel, H, "1H_Melee_Attack_Chop", dair_poses)
    created.append("1H_Melee_Attack_Chop")

    # -------------------------------------------------------------------------
    # 14. 360° HURRICANE SWORD SPIN (`2H_Melee_Attack_Spinning`) — 8-frame looping cyclone!
    # -------------------------------------------------------------------------
    spin_poses = []
    for i in range(8):
        yaw = i * 45.0
        spin_poses.append(
            p(
                root_yaw=yaw,
                hips_y=0.0,
                hips_z=-0.10,
                torso_pitch=14.0,
                torso_yaw=0.0,
                r_ua=0.0,
                r_la=2.0,
                r_flare=14.0,
                weapon=0.0,
                weapon_splay=-0.08,
                l_ua=-120.0,
                l_la=-95.0,
                leg_l_hip=26.0,
                leg_l_knee=36.0,
                leg_r_hip=-26.0,
                leg_r_knee=34.0,
                smear=dict(y=0.0, z=0.56, pitch=0.0, scale=1.28, horiz_spin=True, spin_yaw=yaw),
            )
        )
    _write_custom_action(arm_obj, m_rest, m_rest_rel, H, "2H_Melee_Attack_Spinning", spin_poses)
    created.append("2H_Melee_Attack_Spinning")

    # -------------------------------------------------------------------------
    # 15. SIDE SPECIAL (`2H_Melee_Attack_Stab`) — PLASMA BLITZ IAIDO DASH-CUT (6 frames)
    # -------------------------------------------------------------------------
    side_b_poses = [
        # Frame 0: Ultra-low coiled draw at hip!
        p(hips_y=0.06, hips_z=-0.18, torso_pitch=30.0, torso_yaw=-26.0, r_ua=-132.0, r_la=-110.0, weapon=-165.0, l_ua=-20.0, l_la=15.0, leg_l_hip=42.0, leg_l_knee=58.0, leg_r_hip=-36.0, leg_r_knee=50.0),
        # Frame 1: Blazing horizontal lightning blitz!
        p(hips_y=-0.16, hips_z=-0.16, torso_pitch=42.0, torso_yaw=16.0, r_ua=6.0, r_la=4.0, weapon=4.0, l_ua=-140.0, l_la=-120.0, leg_l_hip=46.0, leg_l_knee=54.0, leg_r_hip=-54.0, leg_r_knee=16.0,
          smear=dict(y=-0.30, z=0.52, pitch=4.0, scale=1.28, flip_z=True)),
        # Frame 2 (PEAK BLITZ): Full horizontal piercing cleave!
        p(hips_y=-0.18, hips_z=-0.16, torso_pitch=44.0, torso_yaw=28.0, r_ua=-8.0, r_la=-10.0, weapon=-10.0, l_ua=-150.0, l_la=-130.0, leg_l_hip=48.0, leg_l_knee=56.0, leg_r_hip=-56.0, leg_r_knee=14.0,
          smear=dict(y=-0.34, z=0.50, pitch=-4.0, scale=1.34, flip_z=True)),
        # Frame 3: Sustained electric cleave streak
        p(hips_y=-0.16, hips_z=-0.15, torso_pitch=40.0, torso_yaw=32.0, r_ua=-20.0, r_la=-24.0, weapon=-26.0, l_ua=-145.0, l_la=-124.0, leg_l_hip=44.0, leg_l_knee=52.0, leg_r_hip=-50.0, leg_r_knee=16.0,
          smear=dict(y=-0.30, z=0.46, pitch=-16.0, scale=1.18, flip_z=True)),
        # Frame 4: Sliding brake
        p(hips_y=-0.08, hips_z=-0.12, torso_pitch=24.0, torso_yaw=22.0, r_ua=-42.0, r_la=-40.0, weapon=-46.0, leg_l_hip=36.0, leg_l_knee=44.0, leg_r_hip=-34.0, leg_r_knee=22.0),
        # Frame 5: Ready
        p(),
    ]
    _write_custom_action(arm_obj, m_rest, m_rest_rel, H, "2H_Melee_Attack_Stab", side_b_poses)
    created.append("2H_Melee_Attack_Stab")

    # -------------------------------------------------------------------------
    # 16. DOWN SPECIAL (`Block_Attack`) — AEGIS RIFT COUNTER / SEISMIC SLAM (6 frames)
    # -------------------------------------------------------------------------
    down_b_poses = [
        # Frame 0–1: Heroic diagonal cross-blade Aegis parry stance!
        p(hips_y=0.04, hips_z=-0.12, torso_pitch=8.0, torso_yaw=-14.0, r_ua=-16.0, r_la=52.0, weapon=82.0, l_ua=-14.0, l_la=34.0, leg_l_hip=28.0, leg_l_knee=38.0, leg_r_hip=-28.0, leg_r_knee=36.0),
        p(hips_y=0.05, hips_z=-0.14, torso_pitch=6.0, torso_yaw=-18.0, r_ua=-10.0, r_la=58.0, weapon=92.0, l_ua=-8.0, l_la=40.0, leg_l_hip=30.0, leg_l_knee=42.0, leg_r_hip=-30.0, leg_r_knee=40.0),
        # Frame 2–3: Explosive counter burst / ground slam!
        p(hips_y=-0.10, hips_z=-0.18, torso_pitch=34.0, torso_yaw=26.0, r_ua=-24.0, r_la=-34.0, weapon=-44.0, l_ua=-122.0, l_la=-92.0, leg_l_hip=42.0, leg_l_knee=54.0, leg_r_hip=-42.0, leg_r_knee=22.0,
          smear=dict(y=-0.20, z=0.52, pitch=-14.0, scale=1.28, flip_z=True)),
        p(hips_y=-0.12, hips_z=-0.19, torso_pitch=36.0, torso_yaw=30.0, r_ua=-46.0, r_la=-58.0, weapon=-68.0, l_ua=-132.0, l_la=-104.0, leg_l_hip=44.0, leg_l_knee=56.0, leg_r_hip=-44.0, leg_r_knee=22.0,
          smear=dict(y=-0.22, z=0.44, pitch=-32.0, scale=1.14, flip_z=True)),
        p(hips_y=-0.06, hips_z=-0.12, torso_pitch=20.0, torso_yaw=18.0, r_ua=-52.0, r_la=-36.0, weapon=-30.0),
        p(),
    ]
    _write_custom_action(arm_obj, m_rest, m_rest_rel, H, "Block_Attack", down_b_poses)
    created.append("Block_Attack")

    # -------------------------------------------------------------------------
    # 17. NEUTRAL SPECIAL (`Spellcast_Shoot`) — BREAKER CRESCENT WAVE LAUNCH (5 frames)
    # -------------------------------------------------------------------------
    if is_caster:
        neutral_b_poses = [
            p(hips_y=0.05, hips_z=-0.06, torso_pitch=-10.0, torso_yaw=-16.0, r_ua=38.0, r_la=72.0, weapon=88.0, l_ua=20.0, l_la=55.0),
            p(hips_y=-0.08, hips_z=-0.08, torso_pitch=16.0, torso_yaw=16.0, r_ua=8.0, r_la=6.0, weapon=6.0, l_ua=-90.0, l_la=-50.0,
              smear=dict(y=-0.24, z=0.62, pitch=6.0, scale=0.96, flip_z=True)),
            p(hips_y=-0.10, hips_z=-0.08, torso_pitch=18.0, torso_yaw=20.0, r_ua=2.0, r_la=0.0, weapon=0.0, l_ua=-105.0, l_la=-65.0),
            p(hips_y=-0.04, hips_z=-0.06, torso_pitch=10.0, torso_yaw=14.0, r_ua=-18.0, r_la=10.0, weapon=38.0),
            p(),
        ]
    else:
        neutral_b_poses = [
            p(hips_y=0.06, hips_z=-0.10, torso_pitch=-12.0, torso_yaw=-22.0, r_ua=96.0, r_la=128.0, weapon=144.0, l_ua=-10.0, l_la=32.0),
            p(hips_y=-0.10, hips_z=-0.12, torso_pitch=24.0, torso_yaw=22.0, r_ua=-10.0, r_la=-14.0, weapon=-12.0, l_ua=-120.0, l_la=-88.0, leg_l_hip=36.0, leg_l_knee=42.0, leg_r_hip=-36.0, leg_r_knee=18.0,
              smear=dict(y=-0.24, z=0.58, pitch=4.0, scale=1.22, flip_z=True)),
            p(hips_y=-0.12, hips_z=-0.12, torso_pitch=26.0, torso_yaw=28.0, r_ua=-30.0, r_la=-36.0, weapon=-40.0, l_ua=-132.0, l_la=-100.0, leg_l_hip=38.0, leg_l_knee=44.0, leg_r_hip=-38.0, leg_r_knee=18.0,
              smear=dict(y=-0.24, z=0.52, pitch=-18.0, scale=1.06, flip_z=True)),
            p(hips_y=-0.06, hips_z=-0.09, torso_pitch=16.0, torso_yaw=18.0, r_ua=-46.0, r_la=-30.0, weapon=-20.0),
            p(),
        ]
    _write_custom_action(arm_obj, m_rest, m_rest_rel, H, "Spellcast_Shoot", neutral_b_poses)
    created.append("Spellcast_Shoot")

    # -------------------------------------------------------------------------
    # 18. RIFT GAUNTLET BUSTER (`1H_Ranged_Shoot`) — 4 frames
    #     Story-Bible accurate: Levy draws his saber arm back & thrusts his glowing
    #     LEFT Rift Diamond Gauntlet straight forward to fire Buster plasma shots!
    # -------------------------------------------------------------------------
    buster_poses = [
        p(hips_y=0.0, hips_z=-0.08, torso_pitch=10.0, torso_yaw=-16.0, r_ua=-116.0, r_la=-94.0, weapon=-138.0, l_ua=6.0, l_la=6.0, l_flare=4.0, leg_l_hip=26.0, leg_l_knee=32.0, leg_r_hip=-24.0, leg_r_knee=24.0),
        p(hips_y=0.04, hips_z=-0.09, torso_pitch=6.0, torso_yaw=-20.0, r_ua=-124.0, r_la=-102.0, weapon=-146.0, l_ua=14.0, l_la=14.0, l_flare=4.0, leg_l_hip=26.0, leg_l_knee=34.0, leg_r_hip=-26.0, leg_r_knee=26.0),
        p(hips_y=0.02, hips_z=-0.08, torso_pitch=10.0, torso_yaw=-16.0, r_ua=-118.0, r_la=-96.0, weapon=-140.0, l_ua=4.0, l_la=4.0, l_flare=4.0, leg_l_hip=24.0, leg_l_knee=30.0, leg_r_hip=-22.0, leg_r_knee=24.0),
        p(hips_y=0.0, hips_z=-0.07, torso_pitch=10.0, torso_yaw=-4.0, r_ua=-78.0, r_la=-36.0, weapon=14.0, l_ua=-18.0, l_la=14.0),
    ]
    _write_custom_action(arm_obj, m_rest, m_rest_rel, H, "1H_Ranged_Shoot", buster_poses)
    created.append("1H_Ranged_Shoot")

    # -------------------------------------------------------------------------
    # 19. GUARD (`Blocking`) — 4 frames looping braced defensive blade parry
    # -------------------------------------------------------------------------
    guard_poses = []
    for i in range(4):
        bob = math.sin((i / 4.0) * math.tau)
        guard_poses.append(
            p(
                hips_y=0.04,
                hips_z=-0.12 + bob * 0.015,
                torso_pitch=8.0,
                torso_yaw=-10.0,
                r_ua=-20.0,
                r_la=52.0 + bob * 3.0,
                weapon=86.0 + bob * 3.0,
                l_ua=-14.0,
                l_la=34.0,
                leg_l_hip=28.0,
                leg_l_knee=38.0,
                leg_r_hip=-28.0,
                leg_r_knee=36.0,
            )
        )
    _write_custom_action(arm_obj, m_rest, m_rest_rel, H, "Blocking", guard_poses)
    created.append("Blocking")

    # -------------------------------------------------------------------------
    # 20. HITSTUN (`Hit_A`) — 4 frames expressive knockback recoil!
    # -------------------------------------------------------------------------
    hitstun_poses = [
        p(hips_y=0.06, hips_z=-0.04, torso_pitch=-22.0, torso_yaw=-16.0, head_pitch=-16.0, r_ua=-115.0, r_la=-80.0, weapon=-120.0, l_ua=40.0, l_la=75.0, leg_l_hip=38.0, leg_l_knee=54.0, leg_r_hip=-16.0, leg_r_knee=30.0),
        p(hips_y=0.12, hips_z=0.0, torso_pitch=-34.0, torso_yaw=-22.0, head_pitch=-24.0, r_ua=-132.0, r_la=-104.0, weapon=-144.0, l_ua=58.0, l_la=96.0, leg_l_hip=48.0, leg_l_knee=66.0, leg_r_hip=-8.0, leg_r_knee=36.0),
        p(hips_y=0.09, hips_z=-0.02, torso_pitch=-28.0, torso_yaw=-18.0, head_pitch=-18.0, r_ua=-120.0, r_la=-90.0, weapon=-130.0, l_ua=46.0, l_la=82.0, leg_l_hip=40.0, leg_l_knee=56.0, leg_r_hip=-14.0, leg_r_knee=32.0),
        p(hips_y=0.03, hips_z=-0.06, torso_pitch=-12.0, torso_yaw=-8.0, head_pitch=-8.0, r_ua=-88.0, r_la=-46.0, weapon=-40.0, l_ua=-10.0, l_la=34.0),
    ]
    _write_custom_action(arm_obj, m_rest, m_rest_rel, H, "Hit_A", hitstun_poses)
    created.append("Hit_A")

    # -------------------------------------------------------------------------
    # 21. CENTERED DODGE CLIPS (`Dodge_Forward`, `Dodge_Backward`, `Dodge_Left`)
    #     Authored strictly in-place so characters NEVER drift off the 96x96 frame!
    # -------------------------------------------------------------------------
    dodge_fwd_poses = [
        p(hips_y=-0.04, hips_z=-0.14, torso_pitch=34.0, r_ua=-120.0, r_la=-98.0, weapon=-142.0, l_ua=-20.0, l_la=24.0, leg_l_hip=38.0, leg_l_knee=54.0, leg_r_hip=-36.0, leg_r_knee=40.0),
        p(hips_y=-0.06, hips_z=-0.22, torso_pitch=54.0, r_ua=-136.0, r_la=-118.0, weapon=-154.0, l_ua=10.0, l_la=48.0, leg_l_hip=48.0, leg_l_knee=74.0, leg_r_hip=-44.0, leg_r_knee=54.0),
        p(hips_y=-0.05, hips_z=-0.20, torso_pitch=46.0, r_ua=-132.0, r_la=-112.0, weapon=-150.0, l_ua=0.0, l_la=40.0, leg_l_hip=44.0, leg_l_knee=66.0, leg_r_hip=-40.0, leg_r_knee=46.0),
        p(hips_y=-0.02, hips_z=-0.12, torso_pitch=24.0, r_ua=-92.0, r_la=-56.0, weapon=-58.0, leg_l_hip=30.0, leg_l_knee=40.0, leg_r_hip=-26.0, leg_r_knee=24.0),
        p(),
    ]
    _write_custom_action(arm_obj, m_rest, m_rest_rel, H, "Dodge_Forward", dodge_fwd_poses)
    created.append("Dodge_Forward")

    dodge_back_poses = [
        p(hips_y=0.04, hips_z=-0.12, torso_pitch=-16.0, r_ua=-30.0, r_la=34.0, weapon=68.0, l_ua=14.0, l_la=52.0, leg_l_hip=32.0, leg_l_knee=42.0, leg_r_hip=-34.0, leg_r_knee=34.0),
        p(hips_y=0.08, hips_z=-0.04, torso_pitch=-26.0, r_ua=-46.0, r_la=14.0, weapon=48.0, l_ua=28.0, l_la=64.0, leg_l_hip=44.0, leg_l_knee=60.0, leg_r_hip=-24.0, leg_r_knee=52.0),
        p(hips_y=0.06, hips_z=-0.06, torso_pitch=-20.0, r_ua=-42.0, r_la=18.0, weapon=52.0, l_ua=22.0, l_la=58.0, leg_l_hip=38.0, leg_l_knee=52.0, leg_r_hip=-22.0, leg_r_knee=44.0),
        p(hips_y=0.02, hips_z=-0.10, torso_pitch=4.0, r_ua=-50.0, r_la=10.0, weapon=44.0, leg_l_hip=24.0, leg_l_knee=32.0, leg_r_hip=-20.0, leg_r_knee=20.0),
        p(),
    ]
    _write_custom_action(arm_obj, m_rest, m_rest_rel, H, "Dodge_Backward", dodge_back_poses)
    created.append("Dodge_Backward")

    spot_dodge_poses = [
        p(hips_z=-0.12, torso_pitch=6.0, torso_yaw=-36.0, r_ua=-44.0, r_la=22.0, weapon=72.0, l_ua=-30.0, l_la=34.0, leg_l_hip=24.0, leg_l_knee=34.0, leg_r_hip=-22.0, leg_r_knee=26.0),
        p(hips_z=-0.20, torso_pitch=16.0, torso_yaw=-62.0, r_ua=-52.0, r_la=18.0, weapon=82.0, l_ua=-24.0, l_la=42.0, leg_l_hip=34.0, leg_l_knee=50.0, leg_r_hip=-28.0, leg_r_knee=42.0),
        p(hips_z=-0.20, torso_pitch=16.0, torso_yaw=-62.0, r_ua=-52.0, r_la=18.0, weapon=82.0, l_ua=-24.0, l_la=42.0, leg_l_hip=34.0, leg_l_knee=50.0, leg_r_hip=-28.0, leg_r_knee=42.0),
        p(hips_z=-0.10, torso_pitch=10.0, torso_yaw=-24.0, r_ua=-50.0, r_la=12.0, weapon=62.0),
        p(),
    ]
    _write_custom_action(arm_obj, m_rest, m_rest_rel, H, "Dodge_Left", spot_dodge_poses)
    created.append("Dodge_Left")

    print(
        f"[combat_anim] authored {len(created)} expressive 2D side-view combat animations + SlashSmear for '{preset}'",
        flush=True,
    )
    return created
