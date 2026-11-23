#!/usr/bin/env python3
"""Generates `assets/levels/demo_gym.ldtk` — *STAGE 1: THE FOUNDRY CITADEL*.

A full, tightly paced 5-Act Mega Man X x Super Smash Bros. side-scrolling action stage (~5,600px):
  - Act 1: Entry Deck & Dash-Jump Catwalks (roaming Rogue Striker, Arcanist, Shadow Assassin)
  - Act 2: Vertical Mega Man X Wall-Jump Shaft (moving elevator, spike alcove, 2 wall turrets)
  - Act 3: LOCKDOWN AMBUSH #1 — "THE SMELTER CAGE" (3 Smash platforms, 2 surrounding waves)
  - Act 4A: ~25s Mega Man X Platforming Intermission (Boost chasm, Spike pit + turret, Moving
            sky-bridge over abyss, Mini wall-jump chimney + drop-through ledges, roaming foes)
  - Act 4B: LOCKDOWN AMBUSH #2 — "FORGE GAUNTLET" (1 tight, high-intensity 4-enemy elite wave)
  - Act 5A: Pre-Boss Gauntlet Hallway (vent gap, vanguard guard, turret, crimson boss bridge)
  - Act 5B: BOSS SANCTUM — "IRON TITAN: VULCANUS" (144px 2-Phase Boss with telegraphed lasers!)
"""

from __future__ import annotations

import json
import uuid
from pathlib import Path
from typing import Any

LEVEL_W = 5600
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
    return str(uuid.uuid5(uuid.NAMESPACE_DNS, f"velocity-breaker.foundry-citadel.{seed}"))


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
        ("OneWayPlatform", "#F59E0B"),
        ("MovingPlatform", "#14B8A6"),
        ("SpikeHazard", "#EF4444"),
        ("WallTurret", "#F43F5E"),
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

    # =========================================================================
    # ACT 1 — FOUNDRY ENTRY DECK & DASH-JUMP CATWALKS (-320 .. 650)
    # =========================================================================
    add_entity("PlayerSpawn", bevy_point_to_ldtk(-230, 20, 24), [], "spawn")
    add_entity("SolidBlock", bevy_rect_to_ldtk(-348, -40, -320, 280), [], "act1_left_wall")
    add_entity("SolidBlock", bevy_rect_to_ldtk(-320, -40, -60, 0), [], "act1_start_deck")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(-244, 58, -138, 64), [], "act1_start_oneway")
    add_entity(
        "EnemySpawn",
        bevy_point_to_ldtk(-102, 22, 24),
        [field_inst("archetype", "String", "light", 540)],
        "act1_enemy_opener",
    )

    # Dash-Jump Gap 1 (-60..24) -> Mid Island (24..210)
    add_entity("SolidBlock", bevy_rect_to_ldtk(24, -40, 210, 0), [], "act1_mid_island")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(68, 64, 168, 70), [], "act1_mid_oneway")
    add_entity(
        "EnemySpawn",
        bevy_point_to_ldtk(126, 86, 24),
        [field_inst("archetype", "String", "mage", 540)],
        "act1_enemy_mage",
    )

    # Dash-Jump Gap 2 (210..294) -> Shaft Approach & Floor (294..650)
    add_entity("SolidBlock", bevy_rect_to_ldtk(294, -12, 650, 28), [], "act1_shaft_approach")
    add_entity(
        "EnemySpawn",
        bevy_point_to_ldtk(392, 50, 24),
        [field_inst("archetype", "String", "assassin", 540)],
        "act1_enemy_assassin",
    )

    # =========================================================================
    # ACT 2 — VERTICAL MEGA MAN X WALL-JUMP SHAFT (470 .. 650)
    # =========================================================================
    add_entity("SolidBlock", bevy_rect_to_ldtk(470, 116, 496, 330), [], "act2_shaft_wall_left")
    add_entity("SolidBlock", bevy_rect_to_ldtk(624, 28, 650, 220), [], "act2_shaft_wall_right")
    add_entity("SpikeHazard", bevy_rect_to_ldtk(580, 28, 622, 40), [], "act2_shaft_spike")
    add_entity(
        "MovingPlatform",
        bevy_rect_to_ldtk(532, 122, 588, 128),
        [
            field_inst("travel_x", "Float", 0.0, 520),
            field_inst("travel_y", "Float", 66.0, 521),
            field_inst("speed", "Float", 1.8, 522),
        ],
        "act2_moving_elev",
    )
    add_entity(
        "WallTurret",
        bevy_point_to_ldtk(614, 68, 20),
        [field_inst("facing", "Float", -1.0, 530)],
        "act2_turret_lower",
    )
    add_entity(
        "WallTurret",
        bevy_point_to_ldtk(506, 170, 20),
        [field_inst("facing", "Float", 1.0, 530)],
        "act2_turret_upper",
    )
    add_entity("SolidBlock", bevy_rect_to_ldtk(624, 196, 840, 220), [], "act2_exit_bridge")

    # =========================================================================
    # ACT 3 — LOCKDOWN AMBUSH #1: "THE SMELTER CAGE" (840 .. 1460)
    # =========================================================================
    add_entity("SolidBlock", bevy_rect_to_ldtk(840, 196, 1460, 220), [], "act3_smelter_deck")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(915, 284, 1025, 290), [], "act3_plat_left")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(1275, 284, 1385, 290), [], "act3_plat_right")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(1085, 350, 1215, 356), [], "act3_plat_top")
    add_entity(
        "EncounterZone",
        bevy_rect_to_ldtk(840, 220, 1460, 500),
        [
            field_inst("encounter_id", "Int", 1, 600),
            field_inst("title", "String", "THE SMELTER CAGE", 601),
            field_inst("preset", "String", "smelter_cage", 602),
            field_inst("trigger_x", "Float", 940.0, 603),
            field_inst("floor_y", "Float", 220.0, 604),
        ],
        "act3_encounter_smelter",
    )

    # =========================================================================
    # ACT 4A — SUBSTANTIAL MEGA MAN X PLATFORMING INTERMISSION (1460 .. 3320)
    #          (~25s of unlocked-camera dash-jumps, hazards, moving bridge,
    #           mini wall-jump chimney, turrets & isolated foes!)
    # =========================================================================
    # 1. Boost-Jump Launch Deck (1460..1710) over Molten Chasm (1710..1850)
    add_entity("SolidBlock", bevy_rect_to_ldtk(1460, 196, 1710, 220), [], "act4a_boost_deck")
    add_entity(
        "BoostPad",
        bevy_rect_to_ldtk(1600, 220, 1680, 232),
        [
            field_inst("target_speed", "Float", 470.0, 510),
            field_inst("dir_x", "Float", 1.0, 511),
        ],
        "act4a_boost_pad",
    )
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(1732, 282, 1826, 288), [], "act4a_chasm_plat")

    # 2. Spike Pit Hazard Catwalk (1850..2260) with staggered upper ledges, Arcanist & Turret
    add_entity("SolidBlock", bevy_rect_to_ldtk(1850, 196, 2260, 220), [], "act4a_spike_catwalk")
    add_entity("SpikeHazard", bevy_rect_to_ldtk(1970, 220, 2094, 232), [], "act4a_catwalk_spikes")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(1955, 278, 2045, 284), [], "act4a_spike_over_l")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(2085, 294, 2185, 300), [], "act4a_spike_over_r")
    add_entity(
        "EnemySpawn",
        bevy_point_to_ldtk(2135, 322, 24),
        [field_inst("archetype", "String", "mage", 540)],
        "act4a_catwalk_mage",
    )
    add_entity("SolidBlock", bevy_rect_to_ldtk(2236, 220, 2260, 300), [], "act4a_turret_pillar")
    add_entity(
        "WallTurret",
        bevy_point_to_ldtk(2226, 264, 20),
        [field_inst("facing", "Float", -1.0, 530)],
        "act4a_turret_catwalk",
    )

    # 3. Horizontal Moving Sky-Bridge across Deep Foundry Abyss (2260..2580)
    add_entity(
        "MovingPlatform",
        bevy_rect_to_ldtk(2385, 230, 2465, 236),
        [
            field_inst("travel_x", "Float", 102.0, 520),
            field_inst("travel_y", "Float", 0.0, 521),
            field_inst("speed", "Float", 1.55, 522),
        ],
        "act4a_moving_skybridge",
    )
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(2375, 312, 2475, 318), [], "act4a_abyss_high_plat")
    add_entity("SolidBlock", bevy_rect_to_ldtk(2580, 196, 2956, 220), [], "act4a_chimney_base")
    add_entity(
        "EnemySpawn",
        bevy_point_to_ldtk(2665, 242, 24),
        [field_inst("archetype", "String", "assassin", 540)],
        "act4a_landing_assassin",
    )

    # 4. Mini Wall-Jump Chimney & Drop-Through Switchback Climb (2800..3180)
    add_entity("SolidBlock", bevy_rect_to_ldtk(2800, 308, 2826, 440), [], "act4a_chimney_wall_l")
    add_entity("SolidBlock", bevy_rect_to_ldtk(2930, 220, 2956, 340), [], "act4a_chimney_wall_r")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(2836, 276, 2916, 282), [], "act4a_chimney_step1")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(2844, 336, 2924, 342), [], "act4a_chimney_step2")
    add_entity(
        "WallTurret",
        bevy_point_to_ldtk(2920, 296, 20),
        [field_inst("facing", "Float", -1.0, 530)],
        "act4a_turret_chimney",
    )
    # Upper Chimney Catwalk (2930..3180, y=340) guarded by isolated Iron Brawler!
    add_entity("SolidBlock", bevy_rect_to_ldtk(2930, 316, 3180, 340), [], "act4a_upper_catwalk")
    add_entity(
        "EnemySpawn",
        bevy_point_to_ldtk(3065, 362, 24),
        [field_inst("archetype", "String", "heavy", 540)],
        "act4a_upper_brawler",
    )
    # Descending Stepped Run-Up into Forge Gauntlet (3180..3320)
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(3195, 280, 3290, 286), [], "act4a_descent_step")
    add_entity("SolidBlock", bevy_rect_to_ldtk(3160, 196, 3320, 220), [], "act4a_gauntlet_entry")

    # =========================================================================
    # ACT 4B — TIGHTER LOCKDOWN AMBUSH #2: "FORGE GAUNTLET" (3320 .. 3920)
    #          (1 high-intensity elite ambush wave -> full 0% heal!)
    # =========================================================================
    add_entity("SolidBlock", bevy_rect_to_ldtk(3320, 196, 3920, 220), [], "act4b_gauntlet_deck")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(3380, 284, 3485, 290), [], "act4b_plat_bl")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(3755, 284, 3860, 290), [], "act4b_plat_br")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(3500, 348, 3605, 354), [], "act4b_plat_tl")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(3635, 348, 3740, 354), [], "act4b_plat_tr")
    add_entity(
        "EncounterZone",
        bevy_rect_to_ldtk(3320, 220, 3920, 500),
        [
            field_inst("encounter_id", "Int", 2, 600),
            field_inst("title", "String", "FORGE GAUNTLET", 601),
            field_inst("preset", "String", "forge_gauntlet", 602),
            field_inst("trigger_x", "Float", 3420.0, 603),
            field_inst("floor_y", "Float", 220.0, 604),
        ],
        "act4b_encounter_gauntlet",
    )

    # =========================================================================
    # ACT 5A — PRE-BOSS GAUNTLET HALLWAY (3920 .. 4460)
    # =========================================================================
    add_entity("SolidBlock", bevy_rect_to_ldtk(3920, 196, 4065, 220), [], "act5a_exit_deck")
    # Short Foundry Vent Gap (4065..4145) -> Vanguard Bridge (4145..4460)
    add_entity("SolidBlock", bevy_rect_to_ldtk(4145, 196, 4460, 220), [], "act5a_boss_corridor")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(4195, 284, 4295, 290), [], "act5a_vanguard_plat")
    add_entity(
        "EnemySpawn",
        bevy_point_to_ldtk(4225, 242, 24),
        [field_inst("archetype", "String", "light", 540)],
        "act5a_vanguard_rogue",
    )
    add_entity(
        "WallTurret",
        bevy_point_to_ldtk(4305, 288, 20),
        [field_inst("facing", "Float", -1.0, 530)],
        "act5a_turret_gatekeeper",
    )

    # =========================================================================
    # ACT 5B — BOSS SANCTUM: "IRON TITAN: VULCANUS" (4460 .. 5100)
    # =========================================================================
    add_entity("SolidBlock", bevy_rect_to_ldtk(4460, 196, 5100, 220), [], "act5b_boss_deck")
    add_entity("SolidBlock", bevy_rect_to_ldtk(5100, 196, 5140, 580), [], "act5b_citadel_backwall")
    # 3 Strategic Smash Platforms for dodging Vulcanus's Tremor Waves & Telegraphed Phase 2 Lasers!
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(4545, 288, 4655, 294), [], "act5b_boss_plat_l")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(4905, 288, 5015, 294), [], "act5b_boss_plat_r")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(4720, 354, 4840, 360), [], "act5b_boss_plat_top")

    add_entity(
        "BossSpawn",
        bevy_point_to_ldtk(4960, 254, 32),
        [field_inst("boss_id", "String", "vulcanus", 610)],
        "act5b_boss_vulcanus",
    )
    add_entity(
        "EncounterZone",
        bevy_rect_to_ldtk(4460, 220, 5100, 540),
        [
            field_inst("encounter_id", "Int", 3, 600),
            field_inst("title", "String", "IRON TITAN: VULCANUS", 601),
            field_inst("preset", "String", "vulcanus_boss", 602),
            field_inst("trigger_x", "Float", 4545.0, 603),
            field_inst("floor_y", "Float", 220.0, 604),
        ],
        "act5b_encounter_boss",
    )

    # Full-stage BlastZone framing chasms & upper ceiling safely
    add_entity(
        "BlastZone",
        bevy_rect_to_ldtk(-360, -180, 5200, 680),
        [
            field_inst("arena_center_x", "Float", 1150.0, 550),
            field_inst("arena_center_y", "Float", 310.0, 551),
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
    print(f"[generate_demo_gym_ldtk] wrote {out} ({len(ents)} entities across expanded 5-Act Stage 1)")


if __name__ == "__main__":
    main()
