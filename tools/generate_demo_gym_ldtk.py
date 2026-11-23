#!/usr/bin/env python3
"""Generates `assets/levels/demo_gym.ldtk` — *FULL STAGE 1: THE FOUNDRY CITADEL*.

A richly designed, full-length (~9,700px) 8-Sector Mega Man X x Super Smash Bros. flagship stage:
  - Sector 1: Outer Foundry Deck (-320 .. 470) — warm-up skirmishes, secret Breakable Bulkhead, uphill slope
  - Sector 2: Vertical Smelter Shaft (470 .. 1100) — Mega Man X wall-jump climb, moving lift, 2 turrets,
              Hidden Overlook with SECRET RELIC #1/3, Checkpoint #1
  - Sector 3: LOCKDOWN ARENA #1 — "THE SMELTER CAGE" (1100 .. 1740) — 3 Smash platforms, 2 ambush waves
  - Sector 4: The Slag-Slope Speedway (1740 .. 3660) — downhill speed slope, Boost chasm, BRANCHING HIGH
              (Spring Launcher -> Timed Magma Vents -> Breakable Bulkhead -> SECRET RELIC #2/3) vs LOW
              (Spike pit, Brawler + Arcanist combat lane, uphill rejoin slope), Checkpoints #2 & #3
  - Sector 5: Sky-Gondola Abyss & Timed Vent Bridge (3660 .. 5280) — twin synchronized gondolas over deep
              abyss, 3-phase Timed Magma-Vent Bridge, Spring-Vault Ascent onto elevated Sky Foundry Deck
  - Sector 6: The Foundry Conveyor Tower (5280 .. 6780) — vertical switchback tower with lift & turrets,
              Left Summit Breakable Vault containing SECRET RELIC #3/3 + Heal Pod, Stepped Descent Cascade,
              Checkpoint #4
  - Sector 7: LOCKDOWN ARENA #2 — "THE REACTOR CRUCIBLE" (6780 .. 7440) — 4 Smash platforms + twin corner
              Spring Launchers, 2 elite waves -> full 0% heal!
  - Sector 8A: The Citadel Approach (7440 .. 8520) — high-speed Boost Runway across Reactor Trench, final
              Vanguard Bridge, Checkpoint #5 + Full Heal Canister
  - Sector 8B: BOSS SANCTUM — "IRON TITAN: VULCANUS" (8520 .. 9220) — grand 700px arena with 3 Smash
              platforms + 2 corner Spring Launchers for 2-Phase Vulcanus (including Phase 2 Cyclone Spin!)
"""

from __future__ import annotations

import json
import uuid
from pathlib import Path
from typing import Any

LEVEL_W = 10000
LEVEL_H = 960
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
    return str(uuid.uuid5(uuid.NAMESPACE_DNS, f"velocity-breaker.foundry-citadel.v2.{seed}"))


def bevy_rect_to_ldtk(min_x: float, min_y: float, max_x: float, max_y: float) -> tuple[int, int, int, int]:
    """Converts Bevy world rect (Y-up) into LDtk top-left pixel coords `(x, y, w, h)`."""
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


def build_demo_gym() -> dict[str, Any]:
    entity_specs = [
        ("PlayerSpawn", "#33E0FF"),
        ("SolidBlock", "#3B4A68"),
        ("Slope", "#22D3EE"),
        ("BoostPad", "#00F5FF"),
        ("JumpPad", "#FBBF24"),
        ("CheckpointBeacon", "#38BDF8"),
        ("Collectible", "#FDE047"),
        ("BreakableWall", "#EA580C"),
        ("TimedHazardBeam", "#F43F5E"),
        ("OneWayPlatform", "#F59E0B"),
        ("MovingPlatform", "#14B8A6"),
        ("SpikeHazard", "#EF4444"),
        ("WallTurret", "#E11D48"),
        ("EnemySpawn", "#FB7185"),
        ("EncounterZone", "#F97316"),
        ("BossSpawn", "#DC2626"),
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

    def add_slope(x0: float, y0: float, x1: float, y1: float, tag: str) -> None:
        min_x, max_x = min(x0, x1), max(x0, x1)
        min_y, max_y = min(y0, y1), max(y0, y1)
        add_entity(
            "Slope",
            bevy_rect_to_ldtk(min_x, min_y, max_x, max(max_y, min_y + 8)),
            [
                field_inst("start_x", "Float", float(x0), 501),
                field_inst("start_y", "Float", float(y0), 502),
                field_inst("end_x", "Float", float(x1), 503),
                field_inst("end_y", "Float", float(y1), 504),
            ],
            tag,
        )

    # =========================================================================
    # SECTOR 1 — OUTER FOUNDRY DECK (-320 .. 470)
    # =========================================================================
    add_entity("PlayerSpawn", bevy_point_to_ldtk(-230, 20, 24), [], "spawn")
    add_entity("SolidBlock", bevy_rect_to_ldtk(-348, -40, -320, 300), [], "s1_left_wall")
    add_entity("SolidBlock", bevy_rect_to_ldtk(-320, -40, -10, 0), [], "s1_start_deck")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(-244, 58, -138, 64), [], "s1_start_catwalk")
    add_core(-190, 82, "s1_core_1")
    add_entity(
        "EnemySpawn",
        bevy_point_to_ldtk(-78, 22, 24),
        [field_inst("archetype", "String", "light", 540)],
        "s1_opener_rogue",
    )

    # Jump Gap 1 (-10 .. 64) guided by Energy Core arc -> Mid Concourse (64 .. 300)
    add_core(26, 46, "s1_core_gap")
    add_entity("SolidBlock", bevy_rect_to_ldtk(64, -40, 300, 0), [], "s1_mid_concourse")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(108, 64, 248, 70), [], "s1_mid_upper")
    # Cracked Breakable Bulkhead right under the catwalk guarding a secret storage bay!
    add_entity(
        "BreakableWall",
        bevy_rect_to_ldtk(168, 0, 192, 62),
        [field_inst("hp", "Float", 20.0, 640)],
        "s1_breakable_bay",
    )
    add_core(216, 22, "s1_core_bay")
    add_heal(248, 22, "s1_heal_bay")
    add_entity(
        "EnemySpawn",
        bevy_point_to_ldtk(178, 88, 24),
        [field_inst("archetype", "String", "mage", 540)],
        "s1_concourse_mage",
    )

    # Smooth Uphill Foundry Ramp (300,0 -> 380,28) into Shaft Approach (380 .. 650, y=28)
    add_slope(300, 0, 380, 28, "s1_uphill_ramp")
    add_entity("SolidBlock", bevy_rect_to_ldtk(380, -12, 650, 28), [], "s1_shaft_approach")
    add_entity(
        "EnemySpawn",
        bevy_point_to_ldtk(438, 50, 24),
        [field_inst("archetype", "String", "assassin", 540)],
        "s1_approach_assassin",
    )

    # =========================================================================
    # SECTOR 2 — VERTICAL SMELTER SHAFT & HIDDEN OVERLOOK (470 .. 1100)
    # =========================================================================
    # Left wall stays strictly at x=470..496, min_y=116 (>= 110 doorway contract!)
    add_entity("SolidBlock", bevy_rect_to_ldtk(470, 116, 496, 344), [], "s2_shaft_wall_left")
    add_entity("SolidBlock", bevy_rect_to_ldtk(624, 28, 650, 220), [], "s2_shaft_wall_right")
    add_entity("SpikeHazard", bevy_rect_to_ldtk(580, 28, 622, 40), [], "s2_shaft_spike")
    add_entity(
        "MovingPlatform",
        bevy_rect_to_ldtk(532, 122, 588, 128),
        [
            field_inst("travel_x", "Float", 0.0, 520),
            field_inst("travel_y", "Float", 70.0, 521),
            field_inst("speed", "Float", 1.75, 522),
        ],
        "s2_moving_elev",
    )
    add_entity(
        "WallTurret",
        bevy_point_to_ldtk(614, 68, 20),
        [field_inst("facing", "Float", -1.0, 530)],
        "s2_turret_lower",
    )
    add_entity(
        "WallTurret",
        bevy_point_to_ldtk(506, 172, 20),
        [field_inst("facing", "Float", 1.0, 530)],
        "s2_turret_upper",
    )
    # Hidden Overlook above the Shaft Left Wall -> SECRET RELIC #1/3!
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(528, 278, 594, 284), [], "s2_overlook_step")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(392, 344, 500, 350), [], "s2_secret_overlook")
    add_relic(436, 372, "s2_secret_relic_1")
    add_core(474, 370, "s2_core_overlook")

    # Exit Bridge (624 .. 1100, y=220) with Checkpoint #1 & Iron Brawler Gatekeeper
    add_entity("SolidBlock", bevy_rect_to_ldtk(624, 196, 1100, 220), [], "s2_exit_bridge")
    add_checkpoint(1, 712, 220, "s2_checkpoint_1")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(805, 284, 925, 290), [], "s2_bridge_perch")
    add_heal(865, 312, "s2_bridge_heal")
    add_core(770, 246, "s2_core_bridge")
    add_entity(
        "EnemySpawn",
        bevy_point_to_ldtk(955, 244, 24),
        [field_inst("archetype", "String", "heavy", 540)],
        "s2_bridge_brawler",
    )

    # =========================================================================
    # SECTOR 3 — LOCKDOWN ARENA #1: "THE SMELTER CAGE" (1100 .. 1740)
    # =========================================================================
    add_entity("SolidBlock", bevy_rect_to_ldtk(1100, 196, 1740, 220), [], "s3_smelter_deck")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(1175, 286, 1295, 292), [], "s3_plat_left")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(1545, 286, 1665, 292), [], "s3_plat_right")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(1355, 354, 1485, 360), [], "s3_plat_top")
    add_entity(
        "EncounterZone",
        bevy_rect_to_ldtk(1100, 220, 1740, 520),
        [
            field_inst("encounter_id", "Int", 1, 600),
            field_inst("title", "String", "THE SMELTER CAGE", 601),
            field_inst("preset", "String", "smelter_cage", 602),
            field_inst("trigger_x", "Float", 1200.0, 603),
            field_inst("floor_y", "Float", 220.0, 604),
        ],
        "s3_encounter_smelter",
    )

    # =========================================================================
    # SECTOR 4 — THE SLAG-SLOPE SPEEDWAY & HIGH/LOW BRANCH (1740 .. 3660)
    # =========================================================================
    add_entity("SolidBlock", bevy_rect_to_ldtk(1740, 196, 1900, 220), [], "s4_exit_lip")
    add_checkpoint(2, 1810, 220, "s4_checkpoint_2")
    # Exhilarating 280px downhill Foundry Speed Slope (1900,220 -> 2180,140)!
    add_slope(1900, 220, 2180, 140, "s4_downhill_slope")
    add_core(1980, 214, "s4_core_slope1")
    add_core(2080, 186, "s4_core_slope2")
    # Boost Launch Deck (2180 .. 2360, y=140) -> Molten Chasm (2360 .. 2510)
    add_entity("SolidBlock", bevy_rect_to_ldtk(2180, 116, 2360, 140), [], "s4_boost_deck")
    add_entity(
        "BoostPad",
        bevy_rect_to_ldtk(2245, 140, 2335, 152),
        [
            field_inst("target_speed", "Float", 500.0, 510),
            field_inst("dir_x", "Float", 1.0, 511),
        ],
        "s4_boost_pad",
    )
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(2395, 182, 2475, 188), [], "s4_chasm_safety")
    add_core(2435, 226, "s4_core_chasm_arc")

    # Branching Fork Hub (2510 .. 2650, y=140) with Foundry Spring Launcher to High Route!
    add_entity("SolidBlock", bevy_rect_to_ldtk(2510, 116, 3230, 140), [], "s4_low_catwalk")
    add_entity(
        "JumpPad",
        bevy_rect_to_ldtk(2550, 140, 2592, 150),
        [field_inst("launch_vy", "Float", 600.0, 615)],
        "s4_spring_high_branch",
    )

    # --- HIGH SPEEDRUNNER SKY-ROUTE (y = 286 .. 365) ---
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(2620, 286, 2830, 292), [], "s4_high_beam_1")
    add_core(2675, 314, "s4_core_high1")
    add_entity(
        "TimedHazardBeam",
        bevy_rect_to_ldtk(2745, 292, 2765, 382),
        [field_inst("phase", "Float", 0.0, 650)],
        "s4_high_vent_1",
    )
    add_entity("SolidBlock", bevy_rect_to_ldtk(2865, 276, 3140, 296), [], "s4_high_vault_deck")
    add_entity(
        "BreakableWall",
        bevy_rect_to_ldtk(3005, 296, 3030, 366),
        [field_inst("hp", "Float", 24.0, 640)],
        "s4_high_breakable_wall",
    )
    add_relic(3082, 326, "s4_secret_relic_2")
    add_core(3115, 324, "s4_core_high2")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(3175, 286, 3365, 292), [], "s4_high_beam_3")
    add_entity(
        "TimedHazardBeam",
        bevy_rect_to_ldtk(3255, 292, 3275, 382),
        [field_inst("phase", "Float", 1.7, 650)],
        "s4_high_vent_2",
    )

    # --- LOW COMBAT CATWALK (2650 .. 3230, y=140) ---
    add_entity("SpikeHazard", bevy_rect_to_ldtk(2720, 140, 2824, 152), [], "s4_low_spikes")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(2735, 196, 2810, 202), [], "s4_low_spike_hop")
    add_core(2772, 222, "s4_core_low1")
    add_entity(
        "EnemySpawn",
        bevy_point_to_ldtk(2935, 164, 24),
        [field_inst("archetype", "String", "heavy", 540)],
        "s4_low_brawler",
    )
    add_entity(
        "EnemySpawn",
        bevy_point_to_ldtk(3120, 164, 24),
        [field_inst("archetype", "String", "mage", 540)],
        "s4_low_mage",
    )
    # Uphill Rejoin Slope (3230,140 -> 3410,220) onto Rejoin Hub (3410 .. 3660, y=220)
    add_slope(3230, 140, 3410, 220, "s4_rejoin_slope")
    add_entity("SolidBlock", bevy_rect_to_ldtk(3410, 196, 3660, 220), [], "s4_rejoin_deck")
    add_checkpoint(3, 3505, 220, "s4_checkpoint_3")
    add_heal(3570, 242, "s4_rejoin_heal")

    # =========================================================================
    # SECTOR 5 — SKY-GONDOLA ABYSS & TIMED MAGMA BRIDGE (3660 .. 5280)
    # =========================================================================
    # Twin Synchronized Moving Gondolas crossing the Deep Foundry Abyss (3660 .. 4250)!
    add_entity(
        "MovingPlatform",
        bevy_rect_to_ldtk(3730, 222, 3814, 228),
        [
            field_inst("travel_x", "Float", 104.0, 520),
            field_inst("travel_y", "Float", 0.0, 521),
            field_inst("speed", "Float", 1.45, 522),
        ],
        "s5_gondola_1",
    )
    add_core(3825, 260, "s5_core_gondola1")
    add_entity("SolidBlock", bevy_rect_to_ldtk(3910, 196, 4020, 220), [], "s5_mid_abyss_pillar")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(3915, 294, 4015, 300), [], "s5_mid_pillar_high")
    add_core(3965, 322, "s5_core_mid_pillar")
    add_entity(
        "EnemySpawn",
        bevy_point_to_ldtk(3965, 244, 24),
        [field_inst("archetype", "String", "mage", 540)],
        "s5_pillar_mage",
    )
    add_entity(
        "MovingPlatform",
        bevy_rect_to_ldtk(4085, 222, 4169, 228),
        [
            field_inst("travel_x", "Float", 102.0, 520),
            field_inst("travel_y", "Float", 0.0, 521),
            field_inst("speed", "Float", 1.55, 522),
        ],
        "s5_gondola_2",
    )
    add_core(4180, 260, "s5_core_gondola2")

    # Timed Magma-Vent Bridge (4250 .. 4870, y=220) — 3 staggered pulsing vents!
    add_entity("SolidBlock", bevy_rect_to_ldtk(4250, 196, 4870, 220), [], "s5_vent_bridge")
    add_entity(
        "TimedHazardBeam",
        bevy_rect_to_ldtk(4370, 220, 4392, 325),
        [field_inst("phase", "Float", 0.0, 650)],
        "s5_vent_1",
    )
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(4420, 286, 4490, 292), [], "s5_vent_perch_1")
    add_entity(
        "TimedHazardBeam",
        bevy_rect_to_ldtk(4525, 220, 4547, 325),
        [field_inst("phase", "Float", 1.1, 650)],
        "s5_vent_2",
    )
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(4575, 286, 4645, 292), [], "s5_vent_perch_2")
    add_core(4610, 314, "s5_core_vent_perch")
    add_entity(
        "TimedHazardBeam",
        bevy_rect_to_ldtk(4675, 220, 4697, 325),
        [field_inst("phase", "Float", 2.2, 650)],
        "s5_vent_3",
    )
    add_entity(
        "EnemySpawn",
        bevy_point_to_ldtk(4760, 244, 24),
        [field_inst("archetype", "String", "assassin", 540)],
        "s5_bridge_assassin",
    )
    # Foundry Spring Launcher -> Elevated Sky Foundry Deck (4900 .. 5506, y=340)!
    add_entity(
        "JumpPad",
        bevy_rect_to_ldtk(4815, 220, 4857, 230),
        [field_inst("launch_vy", "Float", 615.0, 615)],
        "s5_spring_ascent",
    )
    add_core(4865, 345, "s5_core_spring_arc")
    add_entity("SolidBlock", bevy_rect_to_ldtk(4900, 316, 5506, 340), [], "s5_sky_foundry_deck")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(5015, 404, 5135, 410), [], "s5_sky_perch")
    add_entity(
        "EnemySpawn",
        bevy_point_to_ldtk(5045, 364, 24),
        [field_inst("archetype", "String", "light", 540)],
        "s5_sky_rogue",
    )
    add_entity(
        "EnemySpawn",
        bevy_point_to_ldtk(5185, 364, 24),
        [field_inst("archetype", "String", "heavy", 540)],
        "s5_sky_brawler",
    )

    # =========================================================================
    # SECTOR 6 — FOUNDRY CONVEYOR TOWER & HIDDEN FOREMAN'S VAULT (5280 .. 6780)
    # =========================================================================
    add_entity("SolidBlock", bevy_rect_to_ldtk(5280, 422, 5306, 584), [], "s6_tower_wall_l")
    add_entity("SolidBlock", bevy_rect_to_ldtk(5480, 340, 5506, 500), [], "s6_tower_wall_r")
    add_entity("SpikeHazard", bevy_rect_to_ldtk(5436, 340, 5478, 352), [], "s6_tower_spike")
    add_entity(
        "MovingPlatform",
        bevy_rect_to_ldtk(5356, 396, 5424, 402),
        [
            field_inst("travel_x", "Float", 0.0, 520),
            field_inst("travel_y", "Float", 84.0, 521),
            field_inst("speed", "Float", 1.70, 522),
        ],
        "s6_tower_lift",
    )
    add_entity(
        "WallTurret",
        bevy_point_to_ldtk(5470, 392, 20),
        [field_inst("facing", "Float", -1.0, 530)],
        "s6_tower_turret_low",
    )
    add_entity(
        "WallTurret",
        bevy_point_to_ldtk(5316, 468, 20),
        [field_inst("facing", "Float", 1.0, 530)],
        "s6_tower_turret_high",
    )
    # Summit Bridge (5480 .. 5720, y=500) + Left Hidden Foreman's Vault (5060 .. 5280, y=500)!
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(5312, 496, 5476, 502), [], "s6_summit_bridge")
    add_entity("SolidBlock", bevy_rect_to_ldtk(5040, 476, 5280, 500), [], "s6_secret_vault_floor")
    add_entity(
        "BreakableWall",
        bevy_rect_to_ldtk(5254, 500, 5280, 576),
        [field_inst("hp", "Float", 26.0, 640)],
        "s6_vault_breakable_door",
    )
    add_relic(5115, 528, "s6_secret_relic_3")
    add_heal(5175, 524, "s6_vault_heal")
    add_core(5215, 524, "s6_core_vault")

    # Stepped Descent Cascade (5480 .. 6780, y = 500 -> 400 -> 310 -> 220)
    add_entity("SolidBlock", bevy_rect_to_ldtk(5480, 476, 5700, 500), [], "s6_summit_exit")
    add_core(5610, 528, "s6_core_summit")
    add_entity("SolidBlock", bevy_rect_to_ldtk(5720, 376, 5980, 400), [], "s6_cascade_tier1")
    add_entity(
        "EnemySpawn",
        bevy_point_to_ldtk(5860, 424, 24),
        [field_inst("archetype", "String", "light", 540)],
        "s6_cascade_rogue",
    )
    add_entity("SolidBlock", bevy_rect_to_ldtk(6010, 286, 6300, 310), [], "s6_cascade_tier2")
    add_core(6090, 342, "s6_core_cascade1")
    add_entity(
        "EnemySpawn",
        bevy_point_to_ldtk(6175, 334, 24),
        [field_inst("archetype", "String", "assassin", 540)],
        "s6_cascade_assassin",
    )
    add_entity("SolidBlock", bevy_rect_to_ldtk(6330, 196, 6780, 220), [], "s6_cascade_base")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(6415, 286, 6525, 292), [], "s6_base_perch")
    add_core(6470, 314, "s6_core_cascade2")
    add_entity(
        "EnemySpawn",
        bevy_point_to_ldtk(6470, 244, 24),
        [field_inst("archetype", "String", "mage", 540)],
        "s6_base_mage",
    )
    add_checkpoint(4, 6665, 220, "s6_checkpoint_4")
    add_heal(6725, 242, "s6_pre_crucible_heal")

    # =========================================================================
    # SECTOR 7 — LOCKDOWN ARENA #2: "THE REACTOR CRUCIBLE" (6780 .. 7440)
    # =========================================================================
    add_entity("SolidBlock", bevy_rect_to_ldtk(6780, 196, 7440, 220), [], "s7_crucible_deck")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(6860, 288, 6970, 294), [], "s7_plat_bl")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(7250, 288, 7360, 294), [], "s7_plat_br")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(6975, 356, 7090, 362), [], "s7_plat_tl")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(7130, 356, 7245, 362), [], "s7_plat_tr")
    # Twin Corner Foundry Spring Launchers for high-flying aerial Smash combos!
    add_entity(
        "JumpPad",
        bevy_rect_to_ldtk(6808, 220, 6846, 230),
        [field_inst("launch_vy", "Float", 560.0, 615)],
        "s7_spring_left",
    )
    add_entity(
        "JumpPad",
        bevy_rect_to_ldtk(7374, 220, 7412, 230),
        [field_inst("launch_vy", "Float", 560.0, 615)],
        "s7_spring_right",
    )
    add_entity(
        "EncounterZone",
        bevy_rect_to_ldtk(6780, 220, 7440, 540),
        [
            field_inst("encounter_id", "Int", 2, 600),
            field_inst("title", "String", "THE REACTOR CRUCIBLE", 601),
            field_inst("preset", "String", "reactor_crucible", 602),
            field_inst("trigger_x", "Float", 6880.0, 603),
            field_inst("floor_y", "Float", 220.0, 604),
        ],
        "s7_encounter_crucible",
    )

    # =========================================================================
    # SECTOR 8A — THE CITADEL APPROACH (7440 .. 8520)
    # =========================================================================
    add_entity("SolidBlock", bevy_rect_to_ldtk(7440, 196, 7900, 220), [], "s8a_boost_runway")
    add_entity(
        "BoostPad",
        bevy_rect_to_ldtk(7750, 220, 7845, 232),
        [
            field_inst("target_speed", "Float", 510.0, 510),
            field_inst("dir_x", "Float", 1.0, 511),
        ],
        "s8a_boost_pad",
    )
    # Reactor Trench (7900 .. 8050) with glowing Energy Core arc & mid safety beam
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(7938, 254, 8012, 260), [], "s8a_trench_safety")
    add_core(7935, 294, "s8a_core_trench1")
    add_core(7975, 306, "s8a_core_trench2")
    add_core(8015, 294, "s8a_core_trench3")

    # Citadel Vanguard Bridge (8050 .. 8520, y=220)
    add_entity("SolidBlock", bevy_rect_to_ldtk(8050, 196, 8520, 220), [], "s8a_vanguard_bridge")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(8115, 286, 8225, 292), [], "s8a_vanguard_plat")
    add_entity(
        "EnemySpawn",
        bevy_point_to_ldtk(8165, 244, 24),
        [field_inst("archetype", "String", "light", 540)],
        "s8a_vanguard_rogue",
    )
    add_entity(
        "WallTurret",
        bevy_point_to_ldtk(8265, 288, 20),
        [field_inst("facing", "Float", -1.0, 530)],
        "s8a_turret_gatekeeper",
    )
    add_checkpoint(5, 8365, 220, "s8a_checkpoint_5")
    add_heal(8430, 242, "s8a_pre_boss_heal")

    # =========================================================================
    # SECTOR 8B — BOSS SANCTUM: "IRON TITAN: VULCANUS" (8520 .. 9220)
    # =========================================================================
    add_entity("SolidBlock", bevy_rect_to_ldtk(8520, 196, 9220, 220), [], "s8b_boss_deck")
    add_entity("SolidBlock", bevy_rect_to_ldtk(9220, 196, 9268, 620), [], "s8b_citadel_backwall")
    # 3 Strategic Smash Platforms + 2 Corner Spring Launchers for Vulcanus's 2-Phase Showdown!
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(8615, 292, 8725, 298), [], "s8b_boss_plat_l")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(9015, 292, 9125, 298), [], "s8b_boss_plat_r")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(8805, 360, 8935, 366), [], "s8b_boss_plat_top")
    add_entity(
        "JumpPad",
        bevy_rect_to_ldtk(8552, 220, 8590, 230),
        [field_inst("launch_vy", "Float", 540.0, 615)],
        "s8b_boss_spring_l",
    )
    add_entity(
        "JumpPad",
        bevy_rect_to_ldtk(9150, 220, 9188, 230),
        [field_inst("launch_vy", "Float", 540.0, 615)],
        "s8b_boss_spring_r",
    )

    add_entity(
        "BossSpawn",
        bevy_point_to_ldtk(9060, 254, 32),
        [field_inst("boss_id", "String", "vulcanus", 610)],
        "s8b_boss_vulcanus",
    )
    add_entity(
        "EncounterZone",
        bevy_rect_to_ldtk(8520, 220, 9220, 560),
        [
            field_inst("encounter_id", "Int", 3, 600),
            field_inst("title", "String", "IRON TITAN: VULCANUS", 601),
            field_inst("preset", "String", "vulcanus_boss", 602),
            field_inst("trigger_x", "Float", 8605.0, 603),
            field_inst("floor_y", "Float", 220.0, 604),
        ],
        "s8b_encounter_boss",
    )

    # Full-stage BlastZone framing all 8 Sectors cleanly
    add_entity(
        "BlastZone",
        bevy_rect_to_ldtk(-360, -180, 9340, 690),
        [
            field_inst("arena_center_x", "Float", 1420.0, 550),
            field_inst("arena_center_y", "Float", 312.0, 551),
        ],
        "stage_blast_zone",
    )

    layer_def = {
        "__type": "Entities",
        "identifier": "Entities",
        "type": "Entities",
        "uid": layer_uid,
        "doc": None,
        "uiColor": None,
        "gridSize": GRID_SIZE,
        "guideGridWid": 0,
        "guideGridHei": 0,
        "displayOpacity": 1.0,
        "inactiveOpacity": 0.6,
        "hideInList": False,
        "hideFieldsWhenInactive": False,
        "canSelectWhenInactive": True,
        "renderInWorldView": True,
        "pxOffsetX": 0,
        "pxOffsetY": 0,
        "parallaxFactorX": 0.0,
        "parallaxFactorY": 0.0,
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
        "tilePivotX": 0.0,
        "tilePivotY": 0.0,
        "biomeFieldUid": None,
        "autoTilesetDefUid": None,
    }

    layer_instance = {
        "__identifier": "Entities",
        "__type": "Entities",
        "__cWid": LEVEL_W // GRID_SIZE,
        "__cHei": LEVEL_H // GRID_SIZE,
        "__gridSize": GRID_SIZE,
        "__opacity": 1.0,
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
        "seed": 42,
        "overrideTilesetUid": None,
        "gridTiles": [],
        "entityInstances": entities,
    }

    level = {
        "identifier": "TheFoundryGym",
        "iid": make_uuid("level.foundry_citadel"),
        "uid": level_uid,
        "worldX": 0,
        "worldY": 0,
        "worldDepth": 0,
        "pxWid": LEVEL_W,
        "pxHei": LEVEL_H,
        "__bgColor": "#080A14",
        "bgColor": None,
        "useAutoIdentifier": False,
        "bgRelPath": None,
        "bgPos": None,
        "bgPivotX": 0.5,
        "bgPivotY": 0.5,
        "__smartColor": "#22D3EE",
        "__bgPos": None,
        "externalRelPath": None,
        "fieldInstances": [],
        "layerInstances": [layer_instance],
        "__neighbours": [],
    }

    project = {
        "__header__": {
            "fileType": "LDtk Project JSON",
            "app": "LDtk",
            "doc": "https://ldtk.io/json",
            "schema": "https://ldtk.io/files/JSON_SCHEMA.json",
            "appAuthor": "Sebastien 'deepnight' Benard",
            "appVersion": "1.5.3",
            "url": "https://ldtk.io",
        },
        "iid": make_uuid("project.velocity_breaker"),
        "jsonVersion": "1.5.3",
        "appBuildId": 473703.0,
        "nextUid": next(UID) + 100,
        "identifierStyle": "Capitalize",
        "toc": [],
        "worldLayout": "Free",
        "worldGridWidth": 256,
        "worldGridHeight": 256,
        "defaultLevelWidth": LEVEL_W,
        "defaultLevelHeight": LEVEL_H,
        "defaultPivotX": 0.0,
        "defaultPivotY": 0.0,
        "defaultGridSize": GRID_SIZE,
        "defaultEntityWidth": 16,
        "defaultEntityHeight": 16,
        "bgColor": "#080A14",
        "defaultLevelBgColor": "#080A14",
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
            "layers": [layer_def],
            "entities": entity_defs,
            "tilesets": [],
            "enums": [],
            "externalEnums": [],
            "levelFields": [],
        },
        "levels": [level],
        "worlds": [],
        "dummyWorldIid": make_uuid("world.dummy"),
    }
    return project


def main() -> None:
    out = Path("assets/levels/demo_gym.ldtk")
    out.parent.mkdir(parents=True, exist_ok=True)
    data = build_demo_gym()
    out.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
    ents = data["levels"][0]["layerInstances"][0]["entityInstances"]
    print(f"[generate_demo_gym_ldtk] wrote {out} ({len(ents)} entities across full 8-Sector Stage 1!)")


if __name__ == "__main__":
    main()
