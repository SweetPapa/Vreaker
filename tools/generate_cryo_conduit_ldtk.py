#!/usr/bin/env python3
"""Generates `assets/levels/cryo_conduit.ldtk` — *FULL STAGE 2: THE CRYO CONDUIT*.

Story Bible Stage 2 (`docs/story-bible.md`):
  - Theme: Sub-zero 2085 AI server farm & liquid-nitrogen cooling towers
  - Signature Gimmicks: Low-friction Mirror-Ice slopes & decks (`icy = true`),
    Timed Liquid-Nitrogen Cryo-Vent Blasts, Shatterable Cryo Ice Bulkheads
    (weak to Forge Diamond!), and high-speed ice-slide chasms!
  - Boss: **SUB-ZERO WARDEN: BOREALIS** (guarding the **Stasis Diamond** — 2/4)
  - 3 Hidden 1700s Keepsake Relics:
      1. *WEATHERED POWDER HORN* (Sector 2 Upper Frost Alcove)
      2. *SARAH'S HANDKERCHIEF* (Sector 4 High Cryo-Vent Skyway)
      3. *WOODEN CAVALRY TOY* (Sector 6 Coolant Spire Ice Vault)
"""

from __future__ import annotations

import json
import uuid
from pathlib import Path
from typing import Any

LEVEL_W = 10000
LEVEL_H = 1240
ORIGIN_X = -360.0
ORIGIN_Y = -260.0
GRID_SIZE = 16


def uid_gen() -> Any:
    n = 1
    while True:
        yield n
        n += 1


UID = uid_gen()


def make_uuid(seed: str) -> str:
    return str(uuid.uuid5(uuid.NAMESPACE_DNS, f"velocity-breaker.cryo-conduit.v1.{seed}"))


def bevy_rect_to_ldtk(min_x: float, min_y: float, max_x: float, max_y: float) -> tuple[int, int, int, int]:
    w = int(round(max_x - min_x))
    h = int(round(max_y - min_y))
    lx = int(round(min_x - ORIGIN_X))
    ly = int(round(LEVEL_H - (max_y - ORIGIN_Y)))
    return lx, ly, w, h


def bevy_point_to_ldtk(wx: float, wy: float, size: int = 16) -> tuple[int, int, int, int]:
    half = size * 0.5
    return bevy_rect_to_ldtk(wx - half, wy - half, wx + half, wy + half)


def field_inst(ident: str, ftype: str, value: Any, def_uid: int) -> dict[str, Any]:
    return {
        "__identifier": ident,
        "__type": ftype,
        "__value": value,
        "__tile": None,
        "defUid": def_uid,
        "realEditorValues": [],
    }


def entity_def(ident: str, uid: int, color: str, w: int = 16, h: int = 16) -> dict[str, Any]:
    return {
        "identifier": ident,
        "doc": None,
        "tags": [],
        "exportToToc": False,
        "allowOutOfBounds": True,
        "uid": uid,
        "width": w,
        "height": h,
        "resizableX": True,
        "resizableY": True,
        "minWidth": None,
        "maxWidth": None,
        "minHeight": None,
        "maxHeight": None,
        "keepAspectRatio": False,
        "tileOpacity": 1.0,
        "fillOpacity": 0.45,
        "lineOpacity": 1.0,
        "hollow": False,
        "color": color,
        "renderMode": "Rectangle",
        "showName": True,
        "tilesetId": None,
        "tileRenderMode": "FitInside",
        "tileRect": None,
        "uiTileRect": None,
        "nineSliceBorders": [],
        "maxCount": 0,
        "limitScope": "PerLevel",
        "limitBehavior": "MoveLastOne",
        "pivotX": 0.0,
        "pivotY": 0.0,
        "fieldDefs": [],
    }


def build_cryo_conduit() -> dict[str, Any]:
    entity_specs = [
        ("PlayerSpawn", "#33E0FF"),
        ("SolidBlock", "#38BDF8"),
        ("Slope", "#67E8F9"),
        ("BoostPad", "#00F5FF"),
        ("JumpPad", "#7DD3FC"),
        ("CheckpointBeacon", "#38BDF8"),
        ("Collectible", "#FDE047"),
        ("BreakableWall", "#60A5FA"),
        ("TimedHazardBeam", "#22D3EE"),
        ("OneWayPlatform", "#93C5FD"),
        ("MovingPlatform", "#2DD4BF"),
        ("SpikeHazard", "#38BDF8"),
        ("WallTurret", "#6366F1"),
        ("EnemySpawn", "#FB7185"),
        ("EncounterZone", "#0EA5E9"),
        ("BossSpawn", "#38BDF8"),
        ("BlastZone", "#A855F7"),
    ]

    entity_defs = []
    def_uids: dict[str, int] = {}
    def_colors: dict[str, str] = {}
    for ident, color in entity_specs:
        u = next(UID)
        def_uids[ident] = u
        def_colors[ident] = color
        entity_defs.append(entity_def(ident, u, color))

    layer_uid = next(UID)
    level_uid = next(UID)
    entities: list[dict[str, Any]] = []

    def add_entity(ident: str, rect: tuple[int, int, int, int], fields: list[dict[str, Any]], tag: str) -> None:
        lx, ly, w, h = rect
        entities.append(
            {
                "__identifier": ident,
                "__grid": [lx // GRID_SIZE, ly // GRID_SIZE],
                "__pivot": [0.0, 0.0],
                "__tags": [],
                "__tile": None,
                "__smartColor": def_colors[ident],
                "__worldX": lx,
                "__worldY": ly,
                "iid": make_uuid(f"ent.{ident}.{tag}.{len(entities)}"),
                "width": w,
                "height": h,
                "defUid": def_uids[ident],
                "px": [lx, ly],
                "fieldInstances": fields,
            }
        )

    def add_solid(x0: float, y0: float, x1: float, y1: float, tag: str, icy: bool = False) -> None:
        fields = [field_inst("icy", "Bool", True, 650)] if icy else []
        add_entity("SolidBlock", bevy_rect_to_ldtk(x0, y0, x1, y1), fields, tag)

    def add_core(x: float, y: float, tag: str) -> None:
        add_entity("Collectible", bevy_point_to_ldtk(x, y, 16), [field_inst("kind", "String", "core", 630)], tag)

    def add_relic(x: float, y: float, tag: str) -> None:
        add_entity("Collectible", bevy_point_to_ldtk(x, y, 22), [field_inst("kind", "String", "relic", 630)], tag)

    def add_heal(x: float, y: float, tag: str) -> None:
        add_entity("Collectible", bevy_point_to_ldtk(x, y, 18), [field_inst("kind", "String", "heal", 630)], tag)

    def add_checkpoint(cp_id: int, x: float, floor_y: float, tag: str) -> None:
        add_entity(
            "CheckpointBeacon",
            bevy_rect_to_ldtk(x - 10, floor_y, x + 10, floor_y + 44),
            [field_inst("checkpoint_id", "Int", cp_id, 620)],
            tag,
        )

    def add_slope(x0: float, y0: float, x1: float, y1: float, tag: str, icy: bool = True) -> None:
        min_x, max_x = min(x0, x1), max(x0, x1)
        min_y, max_y = min(y0, y1), max(y0, y1)
        fields = [
            field_inst("start_x", "Float", float(x0), 501),
            field_inst("start_y", "Float", float(y0), 502),
            field_inst("end_x", "Float", float(x1), 503),
            field_inst("end_y", "Float", float(y1), 504),
        ]
        if icy:
            fields.append(field_inst("icy", "Bool", True, 650))
        add_entity(
            "Slope",
            bevy_rect_to_ldtk(min_x, min_y, max_x, max(max_y, min_y + 8)),
            fields,
            tag,
        )

    # =========================================================================
    # SECTOR 1 — CRYO INTAKE AIRLOCK (-320 .. 480)
    # =========================================================================
    add_entity("PlayerSpawn", bevy_point_to_ldtk(-230, 20, 24), [], "spawn")
    add_solid(-348, -40, -320, 320, "s1_left_wall")
    add_solid(-320, -40, 20, 0, "s1_airlock_deck", icy=True)  # Immediate mirror-ice feel!
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(-215, 62, -105, 68), [], "s1_catwalk")
    add_core(-160, 88, "s1_core_1")
    add_entity("EnemySpawn", bevy_point_to_ldtk(-60, 22, 24), [field_inst("archetype", "String", "light", 540)], "s1_strider")

    # Nitrogen Trench 1 (20..92) -> Mirror-Ice Server Hall (92..340)
    add_core(56, 52, "s1_core_gap")
    add_solid(92, -40, 340, 0, "s1_ice_hall", icy=True)
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(132, 70, 272, 76), [], "s1_hall_upper")
    # Shatterable Cryo Ice Bulkhead guarding Cryo Cache! (Forge Diamond shatters in 1 hit!)
    add_entity("BreakableWall", bevy_rect_to_ldtk(192, 0, 218, 68), [field_inst("hp", "Float", 26.0, 640)], "s1_ice_wall")
    add_core(246, 22, "s1_core_cache")
    add_heal(280, 22, "s1_heal_cache")
    add_entity("EnemySpawn", bevy_point_to_ldtk(202, 94, 24), [field_inst("archetype", "String", "mage", 540)], "s1_weaver")

    # Mirror-Ice Uphill Ramp (340,0 -> 420,34) -> Cooling Shaft Approach (420..660, y=34)
    add_slope(340, 0, 420, 34, "s1_ice_ramp", icy=True)
    add_solid(420, -8, 660, 34, "s1_shaft_approach")
    add_entity("EnemySpawn", bevy_point_to_ldtk(472, 56, 24), [field_inst("archetype", "String", "assassin", 540)], "s1_phantom")

    # =========================================================================
    # SECTOR 2 — LIQUID-NITROGEN COOLING SHAFT (480 .. 1120)
    # =========================================================================
    add_solid(480, 126, 506, 360, "s2_wall_left")
    add_solid(634, 34, 660, 226, "s2_wall_right")
    add_entity("SpikeHazard", bevy_rect_to_ldtk(588, 34, 632, 46), [], "s2_icicles")
    add_entity(
        "MovingPlatform",
        bevy_rect_to_ldtk(540, 130, 598, 136),
        [
            field_inst("travel_x", "Float", 0.0, 520),
            field_inst("travel_y", "Float", 74.0, 521),
            field_inst("speed", "Float", 1.85, 522),
        ],
        "s2_cryo_lift",
    )
    add_entity("WallTurret", bevy_point_to_ldtk(624, 80, 20), [field_inst("facing", "Float", -1.0, 530)], "s2_turret_low")
    add_entity("WallTurret", bevy_point_to_ldtk(516, 184, 20), [field_inst("facing", "Float", 1.0, 530)], "s2_turret_high")
    # Upper Frost Alcove -> STAGE 2 SECRET RELIC #1/3 (WEATHERED POWDER HORN)!
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(536, 290, 602, 296), [], "s2_step_up")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(394, 360, 508, 366), [], "s2_secret_alcove")
    add_relic(438, 388, "s2_relic_1")
    add_core(478, 386, "s2_core_alcove")

    # Server Overpass (634..1120, y=226) + Checkpoint #1 + Cryo Vent + Enforcer
    add_solid(634, 202, 1120, 226, "s2_overpass", icy=True)
    add_checkpoint(1, 720, 226, "s2_cp1")
    add_entity("TimedHazardBeam", bevy_rect_to_ldtk(832, 226, 854, 336), [field_inst("phase", "Float", 0.3, 645)], "s2_cryo_vent")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(878, 292, 988, 298), [], "s2_perch")
    add_heal(932, 320, "s2_heal")
    add_core(782, 254, "s2_core_bridge")
    add_entity("EnemySpawn", bevy_point_to_ldtk(1010, 250, 24), [field_inst("archetype", "String", "heavy", 540)], "s2_enforcer")

    # =========================================================================
    # SECTOR 3 — LOCKDOWN ARENA #1: "SERVER FARM ALPHA" (1120 .. 1760)
    # =========================================================================
    add_solid(1120, 202, 1760, 226, "s3_arena_deck", icy=True)
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(1195, 294, 1315, 300), [], "s3_plat_l")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(1565, 294, 1685, 300), [], "s3_plat_r")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(1375, 366, 1505, 372), [], "s3_plat_top")
    add_entity(
        "EncounterZone",
        bevy_rect_to_ldtk(1120, 226, 1760, 540),
        [
            field_inst("encounter_id", "Int", 1, 600),
            field_inst("title", "String", "SERVER FARM ALPHA", 601),
            field_inst("preset", "String", "cryo_server_farm", 602),
            field_inst("trigger_x", "Float", 1220.0, 603),
            field_inst("floor_y", "Float", 226.0, 604),
        ],
        "s3_encounter_servers",
    )

    # =========================================================================
    # SECTOR 4 — THE GLACIAL SPEEDWAY & HIGH/LOW BRANCH (1760 .. 3680)
    # =========================================================================
    add_solid(1760, 202, 1920, 226, "s4_exit_lip")
    add_checkpoint(2, 1830, 226, "s4_cp2")
    # Massive 320px Mirror-Ice Downhill Speedway Slope (1920,226 -> 2240,126)!
    add_slope(1920, 226, 2240, 126, "s4_glacial_slope", icy=True)
    add_core(2005, 218, "s4_core_s1")
    add_core(2100, 188, "s4_core_s2")
    add_core(2185, 162, "s4_core_s3")
    # Superconductor Boost Runway -> Liquid-Nitrogen Chasm!
    add_solid(2240, 102, 2410, 126, "s4_boost_deck", icy=True)
    add_entity(
        "BoostPad",
        bevy_rect_to_ldtk(2285, 126, 2380, 138),
        [field_inst("target_speed", "Float", 540.0, 510), field_inst("dir_x", "Float", 1.0, 511)],
        "s4_cryo_boost",
    )
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(2455, 176, 2535, 182), [], "s4_chasm_step")
    add_core(2495, 222, "s4_core_chasm")

    # Branching Hub (2580..3280, y=126) -> HIGH CRYO SKYWAY vs LOW FROZEN TRENCH
    add_solid(2580, 102, 3280, 126, "s4_low_trench", icy=True)
    add_entity("JumpPad", bevy_rect_to_ldtk(2615, 126, 2658, 136), [field_inst("launch_vy", "Float", 610.0, 615)], "s4_spring_high")

    # HIGH ROUTE: Timed Cryo-Vents -> Shatterable Ice Bulkhead -> SECRET RELIC #2/3 (SARAH'S HANDKERCHIEF)!
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(2690, 286, 2890, 292), [], "s4_high_1")
    add_core(2745, 314, "s4_core_h1")
    add_entity("TimedHazardBeam", bevy_rect_to_ldtk(2810, 292, 2832, 396), [field_inst("phase", "Float", 0.0, 645)], "s4_high_vent1")
    add_solid(2920, 272, 3220, 296, "s4_high_vault", icy=True)
    add_entity("BreakableWall", bevy_rect_to_ldtk(3040, 296, 3066, 372), [field_inst("hp", "Float", 30.0, 640)], "s4_high_icewall")
    add_relic(3128, 322, "s4_relic_2")
    add_core(3170, 320, "s4_core_h2")

    # LOW ROUTE: Icicle Spikes + Elite Ambush + Uphill Mirror-Ice Rejoin Slope
    add_entity("SpikeHazard", bevy_rect_to_ldtk(2760, 126, 2816, 138), [], "s4_low_spikes")
    add_entity("EnemySpawn", bevy_point_to_ldtk(2910, 150, 24), [field_inst("archetype", "String", "heavy", 540)], "s4_low_enforcer")
    add_entity("EnemySpawn", bevy_point_to_ldtk(3080, 150, 24), [field_inst("archetype", "String", "mage", 540)], "s4_low_weaver")
    add_core(2995, 154, "s4_core_low")
    add_slope(3280, 126, 3440, 220, "s4_rejoin_slope", icy=True)
    add_solid(3440, 196, 3680, 220, "s4_rejoin_deck")
    add_checkpoint(3, 3530, 220, "s4_cp3")
    add_heal(3600, 242, "s4_cp3_heal")

    # =========================================================================
    # SECTOR 5 — CRYO-GONDOLA DATA ABYSS (3680 .. 5280) — GENEROUS HEADROOM!
    # =========================================================================
    add_entity(
        "MovingPlatform",
        bevy_rect_to_ldtk(3750, 224, 3838, 230),
        [field_inst("travel_x", "Float", 104.0, 520), field_inst("travel_y", "Float", 0.0, 521), field_inst("speed", "Float", 1.50, 522)],
        "s5_gondola_1",
    )
    add_core(3845, 260, "s5_core_g1")
    add_solid(3930, 196, 4050, 220, "s5_island", icy=True)
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(3938, 296, 4042, 302), [], "s5_island_top")
    add_core(3990, 324, "s5_core_island")
    add_entity("EnemySpawn", bevy_point_to_ldtk(3990, 244, 24), [field_inst("archetype", "String", "mage", 540)], "s5_island_mage")

    add_entity(
        "MovingPlatform",
        bevy_rect_to_ldtk(4115, 224, 4203, 230),
        [field_inst("travel_x", "Float", 100.0, 520), field_inst("travel_y", "Float", 18.0, 521), field_inst("speed", "Float", 1.55, 522)],
        "s5_gondola_2",
    )
    add_core(4210, 268, "s5_core_g2")

    # 3-Phase Liquid-Nitrogen Cryo-Vent Bridge (4280..4880)
    add_solid(4280, 196, 4880, 220, "s5_vent_bridge", icy=True)
    add_entity("TimedHazardBeam", bevy_rect_to_ldtk(4390, 220, 4412, 330), [field_inst("phase", "Float", 0.0, 645)], "s5_vent_1")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(4440, 292, 4512, 298), [], "s5_step_1")
    add_entity("TimedHazardBeam", bevy_rect_to_ldtk(4545, 220, 4567, 330), [field_inst("phase", "Float", 1.0, 645)], "s5_vent_2")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(4595, 292, 4667, 298), [], "s5_step_2")
    add_core(4630, 320, "s5_core_bridge")
    add_entity("TimedHazardBeam", bevy_rect_to_ldtk(4695, 220, 4717, 330), [field_inst("phase", "Float", 2.0, 645)], "s5_vent_3")
    add_entity("EnemySpawn", bevy_point_to_ldtk(4768, 244, 24), [field_inst("archetype", "String", "assassin", 540)], "s5_phantom")
    add_entity("JumpPad", bevy_rect_to_ldtk(4822, 220, 4866, 230), [field_inst("launch_vy", "Float", 615.0, 615)], "s5_spring_spire")
    add_core(4868, 344, "s5_core_launch")

    # Upper Cooling Deck (4910..5510, y=340) — wide-open 192px clearance beneath upper vault!
    add_solid(4910, 316, 5510, 340, "s5_upper_deck", icy=True)
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(5020, 408, 5145, 414), [], "s5_upper_catwalk")
    add_entity("EnemySpawn", bevy_point_to_ldtk(5050, 364, 24), [field_inst("archetype", "String", "light", 540)], "s5_strider")
    add_entity("EnemySpawn", bevy_point_to_ldtk(5190, 364, 24), [field_inst("archetype", "String", "heavy", 540)], "s5_enforcer")

    # =========================================================================
    # SECTOR 6 — THE COOLANT SPIRE (5280 .. 6780)
    # =========================================================================
    add_solid(5284, 446, 5310, 640, "s6_spire_left")
    add_solid(5484, 340, 5510, 556, "s6_spire_right")
    add_entity("SpikeHazard", bevy_rect_to_ldtk(5440, 340, 5482, 352), [], "s6_spire_spikes")
    add_entity(
        "MovingPlatform",
        bevy_rect_to_ldtk(5360, 422, 5430, 428),
        [field_inst("travel_x", "Float", 0.0, 520), field_inst("travel_y", "Float", 112.0, 521), field_inst("speed", "Float", 1.75, 522)],
        "s6_spire_elev",
    )
    add_entity("WallTurret", bevy_point_to_ldtk(5464, 392, 20), [field_inst("facing", "Float", -1.0, 530)], "s6_turret_1")
    add_entity("WallTurret", bevy_point_to_ldtk(5320, 524, 20), [field_inst("facing", "Float", 1.0, 530)], "s6_turret_2")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(5316, 552, 5480, 558), [], "s6_summit_cap")

    # Left Summit Ice Vault -> SECRET RELIC #3/3 (WOODEN CAVALRY TOY) + Heal!
    add_solid(5044, 532, 5284, 556, "s6_ice_vault_floor", icy=True)
    add_entity("BreakableWall", bevy_rect_to_ldtk(5258, 556, 5284, 632), [field_inst("hp", "Float", 28.0, 640)], "s6_vault_icewall")
    add_relic(5118, 584, "s6_relic_3")
    add_heal(5178, 582, "s6_vault_heal")
    add_core(5220, 582, "s6_vault_core")

    # Stepped Mirror-Ice Descent Cascade (5484..6780)
    add_solid(5484, 532, 5710, 556, "s6_descent_1", icy=True)
    add_core(5610, 584, "s6_core_d1")
    add_solid(5730, 396, 5990, 420, "s6_descent_2", icy=True)
    add_entity("EnemySpawn", bevy_point_to_ldtk(5860, 444, 24), [field_inst("archetype", "String", "assassin", 540)], "s6_phantom")
    add_slope(5990, 420, 6260, 290, "s6_descent_slope", icy=True)
    add_core(6120, 376, "s6_core_dslope")
    add_solid(6260, 266, 6380, 290, "s6_descent_3", icy=True)
    add_solid(6380, 196, 6780, 220, "s6_pre_vault_deck")
    add_entity("EnemySpawn", bevy_point_to_ldtk(6500, 244, 24), [field_inst("archetype", "String", "light", 540)], "s6_strider")
    add_checkpoint(4, 6665, 220, "s6_cp4")

    # =========================================================================
    # SECTOR 7 — LOCKDOWN ARENA #2: "ABSOLUTE ZERO VAULT" (6780 .. 7440)
    # =========================================================================
    add_solid(6780, 196, 7440, 220, "s7_vault_deck", icy=True)
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(6855, 292, 6975, 298), [], "s7_p1")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(7245, 292, 7365, 298), [], "s7_p2")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(6975, 366, 7095, 372), [], "s7_p3")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(7125, 366, 7245, 372), [], "s7_p4")
    add_entity("JumpPad", bevy_rect_to_ldtk(6804, 220, 6846, 230), [field_inst("launch_vy", "Float", 560.0, 615)], "s7_spring_l")
    add_entity("JumpPad", bevy_rect_to_ldtk(7374, 220, 7416, 230), [field_inst("launch_vy", "Float", 560.0, 615)], "s7_spring_r")
    add_entity(
        "EncounterZone",
        bevy_rect_to_ldtk(6780, 220, 7440, 550),
        [
            field_inst("encounter_id", "Int", 2, 600),
            field_inst("title", "String", "ABSOLUTE ZERO VAULT", 601),
            field_inst("preset", "String", "cryo_vault", 602),
            field_inst("trigger_x", "Float", 6880.0, 603),
            field_inst("floor_y", "Float", 220.0, 604),
        ],
        "s7_encounter_vault",
    )

    # =========================================================================
    # SECTOR 8A — STASIS CORRIDOR APPROACH (7440 .. 8520)
    # =========================================================================
    add_solid(7440, 196, 7900, 220, "s8_approach_1", icy=True)
    add_entity(
        "BoostPad",
        bevy_rect_to_ldtk(7640, 220, 7760, 232),
        [field_inst("target_speed", "Float", 560.0, 510), field_inst("dir_x", "Float", 1.0, 511)],
        "s8_ice_boost",
    )
    add_core(7975, 290, "s8_core_trench")
    add_solid(8050, 196, 8520, 220, "s8_approach_2")
    add_entity("EnemySpawn", bevy_point_to_ldtk(8180, 244, 24), [field_inst("archetype", "String", "heavy", 540)], "s8_enforcer")
    add_entity("EnemySpawn", bevy_point_to_ldtk(8270, 244, 24), [field_inst("archetype", "String", "mage", 540)], "s8_weaver")
    add_checkpoint(5, 8365, 220, "s8_cp5")
    add_heal(8445, 244, "s8_boss_heal")

    # =========================================================================
    # SECTOR 8B — BOSS SANCTUM: "SUB-ZERO WARDEN: BOREALIS" (8520 .. 9220)
    # =========================================================================
    add_solid(8520, 196, 9220, 220, "s8_boss_deck", icy=True)
    add_solid(9220, 196, 9268, 660, "s8_sanctum_wall")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(8600, 296, 8720, 302), [], "s8_boss_pl")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(9020, 296, 9140, 302), [], "s8_boss_pr")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(8800, 370, 8940, 376), [], "s8_boss_pt")
    add_entity("JumpPad", bevy_rect_to_ldtk(8548, 220, 8590, 230), [field_inst("launch_vy", "Float", 560.0, 615)], "s8_boss_sl")
    add_entity("JumpPad", bevy_rect_to_ldtk(9152, 220, 9194, 230), [field_inst("launch_vy", "Float", 560.0, 615)], "s8_boss_sr")
    add_entity("BossSpawn", bevy_point_to_ldtk(9060, 254, 32), [], "s8_boss_spawn")
    add_entity(
        "EncounterZone",
        bevy_rect_to_ldtk(8520, 220, 9220, 580),
        [
            field_inst("encounter_id", "Int", 3, 600),
            field_inst("title", "String", "SUB-ZERO WARDEN: BOREALIS", 601),
            field_inst("preset", "String", "borealis_boss", 602),
            field_inst("trigger_x", "Float", 8615.0, 603),
            field_inst("floor_y", "Float", 220.0, 604),
        ],
        "s8_encounter_borealis",
    )

    # Full-stage BlastZone with generous +960 upper headroom
    add_entity(
        "BlastZone",
        bevy_rect_to_ldtk(-360, -180, 9340, 960),
        [
            field_inst("arena_center_x", "Float", 1440.0, 550),
            field_inst("arena_center_y", "Float", 316.0, 551),
        ],
        "stage2_blast_zone",
    )

    return {
        "__header__": {
            "fileType": "LDtk Project JSON",
            "app": "LDtk",
            "doc": "https://ldtk.io/json",
            "schema": "https://ldtk.io/files/JSON_SCHEMA.json",
            "appAuthor": "Sebastien 'deepnight' Benard",
            "appVersion": "1.5.3",
            "url": "https://ldtk.io",
        },
        "iid": make_uuid("project"),
        "jsonVersion": "1.5.3",
        "appBuildId": 473703,
        "nextUid": next(UID),
        "identifierStyle": "Capitalize",
        "toc": [],
        "worldLayout": "Free",
        "worldGridWidth": 256,
        "worldGridHeight": 256,
        "defaultLevelWidth": LEVEL_W,
        "defaultLevelHeight": LEVEL_H,
        "defaultPivotX": 0,
        "defaultPivotY": 0,
        "defaultGridSize": GRID_SIZE,
        "defaultEntityWidth": 16,
        "defaultEntityHeight": 16,
        "bgColor": "#081020",
        "defaultLevelBgColor": "#0B162C",
        "minifyJson": False,
        "externalLevels": False,
        "exportTiled": False,
        "simplifiedExport": False,
        "imageExportMode": "None",
        "exportLevelBg": True,
        "pngFilePattern": None,
        "backupOnSave": False,
        "backupLimit": 10,
        "backupRelPath": None,
        "levelNamePattern": "Level_%idx",
        "tutorialDesc": None,
        "customCommands": [],
        "flags": [],
        "defs": {
            "layers": [
                {
                    "__type": "Entities",
                    "identifier": "Entities",
                    "type": "Entities",
                    "uid": layer_uid,
                    "doc": None,
                    "uiColor": None,
                    "gridSize": GRID_SIZE,
                    "guideGridWid": 0,
                    "guideGridHei": 0,
                    "displayOpacity": 1,
                    "inactiveOpacity": 0.6,
                    "hideInList": False,
                    "hideFieldsWhenInactive": True,
                    "canSelectWhenInactive": True,
                    "renderInWorldView": True,
                    "pxOffsetX": 0,
                    "pxOffsetY": 0,
                    "parallaxFactorX": 0,
                    "parallaxFactorY": 0,
                    "parallaxScaling": True,
                    "requiredTags": [],
                    "excludedTags": [],
                    "autoTilesKilledByOtherLayerUid": None,
                    "uiFilterTags": [],
                    "useAsyncRender": False,
                    "intGridValues": [],
                    "intGridValuesGroups": [],
                    "autoRuleGroups": [],
                    "autoSourceLayerDefUid": None,
                    "tilesetDefUid": None,
                    "tilePivotX": 0,
                    "tilePivotY": 0,
                    "biomeFieldUid": None,
                }
            ],
            "entities": entity_defs,
            "tilesets": [],
            "enums": [],
            "externalEnums": [],
            "levelFields": [],
        },
        "levels": [
            {
                "identifier": "TheCryoConduit",
                "iid": make_uuid("level.cryo_conduit"),
                "uid": level_uid,
                "worldX": 0,
                "worldY": 0,
                "worldDepth": 0,
                "pxWid": LEVEL_W,
                "pxHei": LEVEL_H,
                "__bgColor": "#0B162C",
                "bgColor": None,
                "useAutoIdentifier": False,
                "bgRelPath": None,
                "bgPos": None,
                "bgPivotX": 0.5,
                "bgPivotY": 0.5,
                "__smartColor": "#38BDF8",
                "__bgPos": None,
                "externalRelPath": None,
                "fieldInstances": [],
                "layerInstances": [
                    {
                        "__identifier": "Entities",
                        "__type": "Entities",
                        "__cWid": LEVEL_W // GRID_SIZE,
                        "__cHei": LEVEL_H // GRID_SIZE,
                        "__gridSize": GRID_SIZE,
                        "__opacity": 1,
                        "__pxTotalOffsetX": 0,
                        "__pxTotalOffsetY": 0,
                        "__tilesetDefUid": None,
                        "__tilesetRelPath": None,
                        "iid": make_uuid("layer.entities"),
                        "levelId": level_uid,
                        "layerDefUid": layer_uid,
                        "pxOffsetX": 0,
                        "pxOffsetY": 0,
                        "visible": True,
                        "optionalRules": [],
                        "intGridCsv": [],
                        "autoLayerTiles": [],
                        "seed": 208502,
                        "overrideTilesetUid": None,
                        "gridTiles": [],
                        "entityInstances": entities,
                    }
                ],
                "__neighbours": [],
            }
        ],
        "worlds": [],
        "dummyWorldIid": make_uuid("world"),
    }


def main() -> None:
    out = Path(__file__).resolve().parent.parent / "assets" / "levels" / "cryo_conduit.ldtk"
    data = build_cryo_conduit()
    out.write_text(json.dumps(data, indent=2), encoding="utf-8")
    ents = data["levels"][0]["layerInstances"][0]["entityInstances"]
    print(f"Generated {out} ({len(ents)} entities across 8 Sectors, 10000x1240)")


if __name__ == "__main__":
    main()
