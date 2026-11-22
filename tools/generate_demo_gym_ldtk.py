#!/usr/bin/env python3
"""Generates `assets/levels/demo_gym.ldtk` — *"The Foundry Gym"* Milestone 1 LDtk 1.5.3 level.

Contains all 3 interconnected zones in one continuous LDtk level:
  - Zone A: Sonic Runway, Downhill Ramp, Boost Pads, 360° LoopSpline, Uphill Launch Ramp
  - Zone B: Mega Man Vertical Wall-Jump Shaft, 2 Moving Platforms, Spike Hazards, 2 Wall Turrets
  - Zone C: Smash Battlefield Chamber, 3 Drop-Through Platforms, 2 AI Fighters, Blast Zones
"""

from __future__ import annotations

import json
import uuid
from pathlib import Path
from typing import Any

LEVEL_W = 2240
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
    return str(uuid.uuid5(uuid.NAMESPACE_DNS, f"velocity-breaker.foundry-gym.{seed}"))


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
        ("LoopSpline", "#38BDF8"),
        ("OneWayPlatform", "#F59E0B"),
        ("MovingPlatform", "#14B8A6"),
        ("SpikeHazard", "#EF4444"),
        ("WallTurret", "#F43F5E"),
        ("EnemySpawn", "#FB7185"),
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

    # -------------------------------------------------------------------------
    # ZONE A — Sonic Runway, Downhill Ramp, Boost Pads & 360° Loop
    # -------------------------------------------------------------------------
    add_entity("PlayerSpawn", bevy_point_to_ldtk(-230, 20, 24), [], "spawn")
    add_entity("SolidBlock", bevy_rect_to_ldtk(-320, -40, -140, 0), [], "zoneA_spawn_deck")
    add_entity("SolidBlock", bevy_rect_to_ldtk(-348, -40, -320, 240), [], "zoneA_left_wall")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(-277, 53, -173, 59), [], "zoneA_warmup_oneway")

    # Downhill acceleration ramp: (-140, 0) -> (60, -100)
    add_entity(
        "Slope",
        bevy_point_to_ldtk(-140, 0, 16),
        [
            field_inst("start_x", "Float", -140.0, 501),
            field_inst("start_y", "Float", 0.0, 502),
            field_inst("end_x", "Float", 60.0, 503),
            field_inst("end_y", "Float", -100.0, 504),
        ],
        "zoneA_downhill",
    )

    # Sonic high-speed runway beneath 360° loop
    add_entity("SolidBlock", bevy_rect_to_ldtk(60, -132, 660, -100), [], "zoneA_runway")

    # Pre-loop Boost Pad -> 515 px/s
    add_entity(
        "BoostPad",
        bevy_rect_to_ldtk(157, -100, 213, -88),
        [
            field_inst("target_speed", "Float", 515.0, 505),
            field_inst("dir_x", "Float", 1.0, 506),
        ],
        "zoneA_boost_entry",
    )

    # 360° Sonic LoopSpline (center=(430, -24), radius=76, min_speed=420)
    add_entity(
        "LoopSpline",
        bevy_rect_to_ldtk(354, -100, 506, 52),
        [
            field_inst("loop_id", "Int", 1, 507),
            field_inst("center_x", "Float", 430.0, 508),
            field_inst("center_y", "Float", -24.0, 509),
            field_inst("radius", "Float", 76.0, 510),
            field_inst("approach_lead", "Float", 48.0, 511),
            field_inst("min_speed", "Float", 420.0, 512),
        ],
        "zoneA_loop_360",
    )

    # Post-loop launch booster
    add_entity(
        "BoostPad",
        bevy_rect_to_ldtk(561, -100, 609, -88),
        [
            field_inst("target_speed", "Float", 475.0, 505),
            field_inst("dir_x", "Float", 1.0, 506),
        ],
        "zoneA_boost_exit",
    )

    # Uphill launch ramp sending player across gap into Zone B: (640, -100) -> (765, -42)
    add_entity(
        "Slope",
        bevy_point_to_ldtk(640, -100, 16),
        [
            field_inst("start_x", "Float", 640.0, 501),
            field_inst("start_y", "Float", -100.0, 502),
            field_inst("end_x", "Float", 765.0, 503),
            field_inst("end_y", "Float", -42.0, 504),
        ],
        "zoneA_launch_ramp",
    )

    # -------------------------------------------------------------------------
    # ZONE B — Mega Man Vertical Shaft (Wall-Jump Walls, 2 Moving Platforms,
    #          Spike Hazards, 2 Wall-Mounted Turrets)
    # -------------------------------------------------------------------------
    add_entity("SolidBlock", bevy_rect_to_ldtk(775, -120, 1005, -80), [], "zoneB_floor")
    # Open 104px doorway at bottom (y=-80..24) so Zone A launch ramp enters the shaft cleanly!
    add_entity("SolidBlock", bevy_rect_to_ldtk(800, 24, 828, 320), [], "zoneB_wall_left")
    add_entity("SolidBlock", bevy_rect_to_ldtk(956, -80, 984, 220), [], "zoneB_wall_right")

    # Spike Hazards (brief i-frames + knockback, NOT instant death)
    add_entity("SpikeHazard", bevy_rect_to_ldtk(912, -80, 954, -68), [], "zoneB_spike_floor")
    add_entity("SolidBlock", bevy_rect_to_ldtk(828, 106, 864, 116), [], "zoneB_spike_shelf")
    add_entity("SpikeHazard", bevy_rect_to_ldtk(830, 116, 862, 128), [], "zoneB_spike_mid")

    # 2 Moving Platforms inside Zone B vertical shaft
    add_entity(
        "MovingPlatform",
        bevy_rect_to_ldtk(860, 10, 916, 16),
        [
            field_inst("travel_x", "Float", 26.0, 520),
            field_inst("travel_y", "Float", 0.0, 521),
            field_inst("speed", "Float", 2.0, 522),
        ],
        "zoneB_moving_horiz",
    )
    add_entity(
        "MovingPlatform",
        bevy_rect_to_ldtk(872, 122, 928, 128),
        [
            field_inst("travel_x", "Float", 0.0, 520),
            field_inst("travel_y", "Float", 74.0, 521),
            field_inst("speed", "Float", 1.7, 522),
        ],
        "zoneB_moving_elev",
    )

    # 2 Wall-Mounted Turrets rewarding Mega Man Buster shots
    add_entity(
        "WallTurret",
        bevy_point_to_ldtk(838, 66, 20),
        [field_inst("facing", "Float", 1.0, 530)],
        "zoneB_turret_left",
    )
    add_entity(
        "WallTurret",
        bevy_point_to_ldtk(946, 168, 20),
        [field_inst("facing", "Float", -1.0, 530)],
        "zoneB_turret_right",
    )

    # -------------------------------------------------------------------------
    # ZONE C — Smash Battlefield Chamber (3 Drop-Through Platforms,
    #          2 AI Fighters, Open Blast Zones)
    # -------------------------------------------------------------------------
    # Entry bridge from Zone B exit toward Battlefield
    add_entity("SolidBlock", bevy_rect_to_ldtk(956, 196, 1080, 220), [], "zoneC_entry_bridge")
    # Main Battlefield floating stage deck (open blast drops left, right, top, bottom!)
    add_entity("SolidBlock", bevy_rect_to_ldtk(1130, 196, 1650, 220), [], "zoneC_battlefield_deck")

    # 3 Classic Smash Drop-Through Platforms
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(1197, 273, 1293, 279), [], "zoneC_plat_left")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(1487, 273, 1583, 279), [], "zoneC_plat_right")
    add_entity("OneWayPlatform", bevy_rect_to_ldtk(1332, 333, 1448, 339), [], "zoneC_plat_top")

    # 2 AI Arena Fighters (1 Light Fast-Mover Rogue, 1 Heavy Punching-Bag Barbarian)
    add_entity(
        "EnemySpawn",
        bevy_point_to_ldtk(1310, 240, 24),
        [field_inst("archetype", "String", "light", 540)],
        "zoneC_enemy_light",
    )
    add_entity(
        "EnemySpawn",
        bevy_point_to_ldtk(1500, 240, 24),
        [field_inst("archetype", "String", "heavy", 540)],
        "zoneC_enemy_heavy",
    )

    # Smash Arena Blast Zone framing Zone C (left/right/top/bottom KOs!)
    add_entity(
        "BlastZone",
        bevy_rect_to_ldtk(1035, 40, 1765, 510),
        [
            field_inst("arena_center_x", "Float", 1390.0, 550),
            field_inst("arena_center_y", "Float", 290.0, 551),
        ],
        "zoneC_blast_zone",
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
        "iid": make_uuid("level.foundry_gym"),
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
    print(f"[generate_demo_gym_ldtk] wrote {out} ({len(ents)} entities across Zones A/B/C)")


if __name__ == "__main__":
    main()
