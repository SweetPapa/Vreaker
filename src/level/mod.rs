//! LDtk multi-stage level loading (`assets/levels/demo_gym.ldtk` & `assets/levels/cryo_conduit.ldtk`)
//! via `bevy_ecs_ldtk` with full entity hydration (solids, low-friction Mirror-Ice decks & slopes,
//! boost pads, spring launchers, drop-throughs, moving platforms, spike hazards, timed magma/cryo
//! vents, checkpoint beacons, collectibles, breakable bulkheads, wall turrets, progression enemies,
//! Lockdown Ambush Arenas, End Bosses Vulcanus & Borealis, and Blast Zones) plus clean stage
//! restart and Stage 1 <-> Stage 2 hot-swapping!

use bevy::prelude::*;
use bevy_ecs_ldtk::prelude::*;

use crate::combat::{
    BreakerWaveProjectile, BusterProjectile, DamagePercent, Hitbox, LastHitTelemetry,
};
use crate::enemies::{
    ArenaScore, BlastZone, EncounterPhase, EnemyArchetype, EnemyFighter, LockdownEncounter,
    LockdownGate, PendingArcanePillar, PendingEnemyRespawn, TurretProjectile, WallTurret,
    spawn_breakable_wall, spawn_stage_enemy, spawn_wall_turret,
};
use crate::physics::{
    BoostPad, BreakableWall, CheckpointBeacon, CollectibleItem, CollectibleKind, IceSurface,
    JumpPad, LoopSpline, MovingPlatform, OneWayPlatform, SlopeSegment, SolidRect, SpikeHazard,
    TimedHazardBeam,
};
use crate::player::PlayerSpawnPoint;
use crate::state::{CampaignState, StageId, StageRestartEvent, StageSwitchEvent, StageWorldEntity};

/// LDtk level pixel height & world-origin alignment matching both `demo_gym.ldtk` and `cryo_conduit.ldtk`.
pub const LDTK_LEVEL_HEIGHT: f32 = 1240.0;
pub const LDTK_ORIGIN_X: f32 = -360.0;
pub const LDTK_ORIGIN_Y: f32 = -260.0;

/// Marker identifying the active root [`LdtkWorldBundle`] entity so stage switching replaces it cleanly.
#[derive(Component, Debug, Default)]
pub struct ActiveLdtkWorldRoot;

/// Authoritative list of respawnable dynamic spawns parsed from the active LDtk level.
#[derive(Resource, Debug, Default, Clone)]
pub struct StageDynamicSpawns {
    pub enemies: Vec<(EnemyArchetype, Vec2)>,
    pub turrets: Vec<(Vec2, f32)>,
    pub encounters: Vec<LockdownEncounter>,
    pub collectibles: Vec<(CollectibleKind, Vec2)>,
    pub breakable_walls: Vec<(Vec2, Vec2, f32)>,
}

/// Tracks which [`StageId`] is currently spawned into the ECS world.
#[derive(Resource, Debug, Default)]
struct LoadedLdtkStage(Option<StageId>);

/// Plugin wiring `bevy_ecs_ldtk` multi-stage level loading, hot-reload entity hydration,
/// stage switching (`Stage 1 <-> Stage 2`), and clean stage restart.
pub struct LevelPlugin;

impl Plugin for LevelPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(LevelSelection::index(0))
            .insert_resource(LdtkSettings {
                level_background: LevelBackground::Nonexistent,
                set_clear_color: SetClearColor::No,
                ..default()
            })
            .init_resource::<StageDynamicSpawns>()
            .init_resource::<LoadedLdtkStage>()
            .add_systems(Startup, spawn_initial_ldtk_world)
            .add_systems(
                Update,
                (
                    handle_stage_switch,
                    hydrate_ldtk_entities,
                    handle_stage_restart,
                )
                    .chain(),
            );
    }
}

fn spawn_stage_ldtk_world(commands: &mut Commands, asset_server: &AssetServer, stage: StageId) {
    let ldtk_handle = asset_server.load(stage.ldtk_asset_path()).into();
    commands.spawn((
        Name::new(format!("LdtkWorld_{stage:?}")),
        ActiveLdtkWorldRoot,
        LdtkWorldBundle {
            ldtk_handle,
            ..default()
        },
    ));
}

fn spawn_initial_ldtk_world(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    campaign: Res<CampaignState>,
    mut loaded: ResMut<LoadedLdtkStage>,
) {
    spawn_stage_ldtk_world(&mut commands, &asset_server, campaign.active_stage);
    loaded.0 = Some(campaign.active_stage);
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn handle_stage_switch(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut switches: MessageReader<StageSwitchEvent>,
    mut loaded: ResMut<LoadedLdtkStage>,
    mut dynamic_spawns: ResMut<StageDynamicSpawns>,
    mut telemetry: ResMut<LastHitTelemetry>,
    mut score: ResMut<ArenaScore>,
    mut spawn_point: ResMut<PlayerSpawnPoint>,
    ldtk_roots: Query<Entity, With<ActiveLdtkWorldRoot>>,
    cleanup_q: Query<
        Entity,
        Or<(
            Or<(
                With<StageWorldEntity>,
                With<SolidRect>,
                With<SlopeSegment>,
                With<OneWayPlatform>,
                With<MovingPlatform>,
                With<BoostPad>,
                With<JumpPad>,
                With<CheckpointBeacon>,
                With<LoopSpline>,
                With<SpikeHazard>,
                With<TimedHazardBeam>,
                With<BlastZone>,
            )>,
            Or<(
                With<EnemyFighter>,
                With<PendingEnemyRespawn>,
                With<PendingArcanePillar>,
                With<WallTurret>,
                With<TurretProjectile>,
                With<BusterProjectile>,
                With<BreakerWaveProjectile>,
                With<Hitbox>,
                With<LockdownGate>,
                With<LockdownEncounter>,
                With<CollectibleItem>,
                With<BreakableWall>,
            )>,
        )>,
    >,
) {
    let Some(target_stage) = switches.read().last().map(|ev| ev.0) else {
        return;
    };
    if loaded.0 == Some(target_stage) {
        return;
    }

    for root in &ldtk_roots {
        commands.entity(root).despawn();
    }
    for e in &cleanup_q {
        commands.entity(e).despawn();
    }

    *dynamic_spawns = StageDynamicSpawns::default();
    *telemetry = LastHitTelemetry::default();
    *score = ArenaScore::default();
    *spawn_point = PlayerSpawnPoint::default();

    spawn_stage_ldtk_world(&mut commands, &asset_server, target_stage);
    loaded.0 = Some(target_stage);
}

/// Converts an LDtk top-left rectangle `(px, width, height)` to Bevy world `(min, max)`.
pub fn ldtk_rect_to_bevy_min_max(entity: &EntityInstance) -> (Vec2, Vec2) {
    let min_x = entity.px.x as f32 + LDTK_ORIGIN_X;
    let max_x = (entity.px.x + entity.width) as f32 + LDTK_ORIGIN_X;
    let max_y = (LDTK_LEVEL_HEIGHT - entity.px.y as f32) + LDTK_ORIGIN_Y;
    let min_y = (LDTK_LEVEL_HEIGHT - (entity.px.y + entity.height) as f32) + LDTK_ORIGIN_Y;
    (Vec2::new(min_x, min_y), Vec2::new(max_x, max_y))
}

/// Converts an LDtk entity instance center into Bevy world coordinates.
pub fn ldtk_entity_center(entity: &EntityInstance) -> Vec2 {
    let (min, max) = ldtk_rect_to_bevy_min_max(entity);
    (min + max) * 0.5
}

fn float_field(entity: &EntityInstance, name: &str, default: f32) -> f32 {
    entity
        .get_float_field(name)
        .copied()
        .unwrap_or(default)
}

fn int_field(entity: &EntityInstance, name: &str, default: i32) -> i32 {
    entity.get_int_field(name).copied().unwrap_or(default)
}

fn bool_field(entity: &EntityInstance, name: &str, default: bool) -> bool {
    entity.get_bool_field(name).copied().unwrap_or(default)
}

fn string_field<'a>(entity: &'a EntityInstance, name: &'a str, default: &'a str) -> &'a str {
    entity
        .get_string_field(name)
        .map(String::as_str)
        .unwrap_or(default)
}

fn parse_archetype(s: &str) -> EnemyArchetype {
    if s.eq_ignore_ascii_case("heavy") || s.eq_ignore_ascii_case("brawler") {
        EnemyArchetype::HeavyBrawler
    } else if s.eq_ignore_ascii_case("mage")
        || s.eq_ignore_ascii_case("arcanist")
        || s.eq_ignore_ascii_case("arcane")
    {
        EnemyArchetype::ArcaneZoner
    } else if s.eq_ignore_ascii_case("assassin") || s.eq_ignore_ascii_case("shadow") {
        EnemyArchetype::ShadowAssassin
    } else if s.eq_ignore_ascii_case("borealis") || s.eq_ignore_ascii_case("cryo_boss") {
        EnemyArchetype::CryoBoss
    } else if s.eq_ignore_ascii_case("boss") || s.eq_ignore_ascii_case("vulcanus") {
        EnemyArchetype::FoundryBoss
    } else {
        EnemyArchetype::LightStriker
    }
}

fn parse_collectible_kind(s: &str) -> CollectibleKind {
    if s.eq_ignore_ascii_case("relic") || s.eq_ignore_ascii_case("secret_relic") {
        CollectibleKind::SecretRelic
    } else if s.eq_ignore_ascii_case("heal") || s.eq_ignore_ascii_case("canister") {
        CollectibleKind::HealCanister
    } else {
        CollectibleKind::EnergyCore
    }
}

/// Builds the scripted multi-wave enemy formations for each Lockdown Ambush / Boss Chamber.
fn build_encounter_waves(
    preset: &str,
    min_x: f32,
    max_x: f32,
    floor_y: f32,
) -> (bool, bool, Vec<Vec<(EnemyArchetype, Vec2)>>) {
    let cx = (min_x + max_x) * 0.5;
    match preset {
        "smelter_cage" => (
            false,
            true,
            vec![
                // Wave 1: Surround from left, right, and top Smash platforms!
                vec![
                    (EnemyArchetype::LightStriker, Vec2::new(min_x + 120.0, floor_y + 84.0)),
                    (EnemyArchetype::ShadowAssassin, Vec2::new(max_x - 120.0, floor_y + 84.0)),
                    (EnemyArchetype::ArcaneZoner, Vec2::new(cx, floor_y + 150.0)),
                ],
                // Wave 2: Heavy Brawler center drop + flanking Rogue & Arcanist!
                vec![
                    (EnemyArchetype::HeavyBrawler, Vec2::new(cx, floor_y + 24.0)),
                    (EnemyArchetype::LightStriker, Vec2::new(min_x + 120.0, floor_y + 84.0)),
                    (EnemyArchetype::ArcaneZoner, Vec2::new(max_x - 120.0, floor_y + 84.0)),
                ],
            ],
        ),
        "reactor_crucible" | "forge_gauntlet" => (
            false,
            true,
            vec![
                // Wave 1: High-intensity elite vanguard with corner spring-pad aerial play!
                vec![
                    (EnemyArchetype::HeavyBrawler, Vec2::new(cx, floor_y + 24.0)),
                    (EnemyArchetype::ShadowAssassin, Vec2::new(min_x + 125.0, floor_y + 92.0)),
                    (EnemyArchetype::ArcaneZoner, Vec2::new(max_x - 125.0, floor_y + 152.0)),
                ],
                // Wave 2: Twin Assassins + Iron Brawler + Arcanist climax before the Citadel Approach!
                vec![
                    (EnemyArchetype::HeavyBrawler, Vec2::new(cx - 40.0, floor_y + 24.0)),
                    (EnemyArchetype::ShadowAssassin, Vec2::new(min_x + 125.0, floor_y + 152.0)),
                    (EnemyArchetype::ShadowAssassin, Vec2::new(max_x - 125.0, floor_y + 92.0)),
                    (EnemyArchetype::ArcaneZoner, Vec2::new(cx + 60.0, floor_y + 152.0)),
                ],
            ],
        ),
        "vulcanus_boss" => (
            true,
            false, // Right side is the solid Foundry Citadel fortress wall
            vec![vec![(
                EnemyArchetype::FoundryBoss,
                Vec2::new(max_x - 150.0, floor_y + 34.0),
            )]],
        ),
        "cryo_server_farm" => (
            false,
            true,
            vec![
                // Wave 1: High-mobility Cryo Server Ambush across mirror-ice decks!
                vec![
                    (EnemyArchetype::ShadowAssassin, Vec2::new(min_x + 120.0, floor_y + 84.0)),
                    (EnemyArchetype::ArcaneZoner, Vec2::new(max_x - 120.0, floor_y + 84.0)),
                    (EnemyArchetype::LightStriker, Vec2::new(cx, floor_y + 150.0)),
                ],
                // Wave 2: Twin Arc Weavers + Heavy Slag Enforcer guarding the server rack exit!
                vec![
                    (EnemyArchetype::HeavyBrawler, Vec2::new(cx, floor_y + 24.0)),
                    (EnemyArchetype::ArcaneZoner, Vec2::new(min_x + 120.0, floor_y + 84.0)),
                    (EnemyArchetype::ShadowAssassin, Vec2::new(max_x - 120.0, floor_y + 150.0)),
                ],
            ],
        ),
        "cryo_vault" => (
            false,
            true,
            vec![
                // Wave 1: Elite sub-zero security perimeter!
                vec![
                    (EnemyArchetype::HeavyBrawler, Vec2::new(cx, floor_y + 24.0)),
                    (EnemyArchetype::ArcaneZoner, Vec2::new(min_x + 125.0, floor_y + 152.0)),
                    (EnemyArchetype::ShadowAssassin, Vec2::new(max_x - 125.0, floor_y + 92.0)),
                ],
                // Wave 2: Final Coolant Vault surge before Borealis's Absolute-Zero Sanctum!
                vec![
                    (EnemyArchetype::HeavyBrawler, Vec2::new(cx - 50.0, floor_y + 24.0)),
                    (EnemyArchetype::ShadowAssassin, Vec2::new(min_x + 125.0, floor_y + 92.0)),
                    (EnemyArchetype::ArcaneZoner, Vec2::new(max_x - 125.0, floor_y + 152.0)),
                    (EnemyArchetype::ShadowAssassin, Vec2::new(cx + 60.0, floor_y + 152.0)),
                ],
            ],
        ),
        "borealis_boss" => (
            true,
            false, // Right side is the solid Cryo Sanctum containment wall
            vec![vec![(
                EnemyArchetype::CryoBoss,
                Vec2::new(max_x - 150.0, floor_y + 34.0),
            )]],
        ),
        _ => (
            false,
            true,
            vec![vec![
                (EnemyArchetype::LightStriker, Vec2::new(min_x + 110.0, floor_y + 24.0)),
                (EnemyArchetype::ArcaneZoner, Vec2::new(max_x - 110.0, floor_y + 84.0)),
            ]],
        ),
    }
}

/// Hydrates every newly loaded (or hot-reloaded) LDtk [`EntityInstance`] into
/// deterministic physics, combat, enemy, encounter, collectible, and blast-zone ECS components.
fn hydrate_ldtk_entities(
    mut commands: Commands,
    mut spawn_point: ResMut<PlayerSpawnPoint>,
    mut dynamic_spawns: ResMut<StageDynamicSpawns>,
    mut score: ResMut<ArenaScore>,
    added: Query<(Entity, &EntityInstance), Added<EntityInstance>>,
) {
    for (ldtk_entity, inst) in &added {
        let (min, max) = ldtk_rect_to_bevy_min_max(inst);
        let center = (min + max) * 0.5;
        let size = max - min;

        match inst.identifier.as_str() {
            "PlayerSpawn" => {
                spawn_point.0 = center;
            }
            "SolidBlock" => {
                let mut ec = commands.spawn((
                    Name::new(format!("LdtkSolid_{}", inst.iid)),
                    StageWorldEntity,
                    SolidRect::from_min_max(min, max),
                ));
                if bool_field(inst, "icy", false) {
                    ec.insert(IceSurface);
                }
            }
            "Slope" => {
                let start = Vec2::new(
                    float_field(inst, "start_x", min.x),
                    float_field(inst, "start_y", min.y),
                );
                let end = Vec2::new(
                    float_field(inst, "end_x", max.x),
                    float_field(inst, "end_y", max.y),
                );
                let mut ec = commands.spawn((
                    Name::new(format!("LdtkSlope_{}", inst.iid)),
                    StageWorldEntity,
                    SlopeSegment::new(start, end),
                ));
                if bool_field(inst, "icy", false) {
                    ec.insert(IceSurface);
                }
            }
            "BoostPad" => {
                let target_speed = float_field(inst, "target_speed", 500.0);
                let dir_x = float_field(inst, "dir_x", 1.0);
                commands.spawn((
                    Name::new(format!("LdtkBoostPad_{}", inst.iid)),
                    BoostPad::new(center, size, Vec2::new(dir_x, 0.0), target_speed),
                ));
            }
            "JumpPad" => {
                let launch_vy = float_field(inst, "launch_vy", 580.0);
                commands.spawn((
                    Name::new(format!("LdtkJumpPad_{}", inst.iid)),
                    JumpPad::new(center, size, launch_vy),
                ));
            }
            "CheckpointBeacon" => {
                let id = int_field(inst, "checkpoint_id", 1).max(1) as u32;
                let base_pos = Vec2::new(center.x, min.y);
                commands.spawn((
                    Name::new(format!("CheckpointBeacon_{id}")),
                    CheckpointBeacon::new(id, base_pos),
                    Transform::from_xyz(base_pos.x, base_pos.y, -2.0),
                    Visibility::default(),
                ));
            }
            "Collectible" => {
                let kind = parse_collectible_kind(string_field(inst, "kind", "core"));
                match kind {
                    CollectibleKind::EnergyCore => score.cores_total += 1,
                    CollectibleKind::SecretRelic => score.relics_total += 1,
                    CollectibleKind::HealCanister => {}
                }
                dynamic_spawns.collectibles.push((kind, center));
                commands.spawn((
                    Name::new(format!("Collectible_{:?}", kind)),
                    CollectibleItem::new(kind, center),
                    Transform::from_xyz(center.x, center.y, 1.0),
                    Visibility::default(),
                ));
            }
            "BreakableWall" => {
                let hp = float_field(inst, "hp", 24.0);
                dynamic_spawns.breakable_walls.push((center, size, hp));
                spawn_breakable_wall(&mut commands, center, size, hp);
            }
            "TimedHazardBeam" => {
                let phase = float_field(inst, "phase", 0.0);
                commands.spawn((
                    Name::new(format!("LdtkTimedBeam_{}", inst.iid)),
                    TimedHazardBeam::new(center, size, phase),
                    Transform::from_xyz(center.x, center.y, 0.5),
                    Visibility::default(),
                ));
            }
            "LoopSpline" => {
                let id = int_field(inst, "loop_id", 1).max(1) as u32;
                let cx = float_field(inst, "center_x", center.x);
                let cy = float_field(inst, "center_y", center.y);
                let radius = float_field(inst, "radius", 76.0);
                let lead = float_field(inst, "approach_lead", 48.0);
                let min_speed = float_field(inst, "min_speed", 420.0);
                commands.spawn((
                    Name::new(format!("LdtkLoopSpline_{}", id)),
                    LoopSpline::circular_loop(id, Vec2::new(cx, cy), radius, lead, min_speed),
                ));
            }
            "OneWayPlatform" => {
                commands.spawn((
                    Name::new(format!("LdtkOneWay_{}", inst.iid)),
                    OneWayPlatform::new(center, size.x.max(16.0), size.y.max(6.0)),
                ));
            }
            "MovingPlatform" => {
                let travel = Vec2::new(
                    float_field(inst, "travel_x", 0.0),
                    float_field(inst, "travel_y", 48.0),
                );
                let speed = float_field(inst, "speed", 1.8);
                commands.spawn((
                    Name::new(format!("LdtkMovingPlatform_{}", inst.iid)),
                    OneWayPlatform::new(center, size.x.max(32.0), size.y.max(6.0)),
                    MovingPlatform::new(center, travel, speed),
                ));
            }
            "SpikeHazard" => {
                commands.spawn((
                    Name::new(format!("LdtkSpike_{}", inst.iid)),
                    SpikeHazard::new(center, size),
                ));
            }
            "WallTurret" => {
                let facing = float_field(inst, "facing", 1.0);
                dynamic_spawns.turrets.push((center, facing));
                spawn_wall_turret(&mut commands, center, facing);
            }
            "EnemySpawn" => {
                let kind_str = string_field(inst, "archetype", "light");
                let archetype = parse_archetype(kind_str);
                dynamic_spawns.enemies.push((archetype, center));
                spawn_stage_enemy(&mut commands, archetype, center, false, None);
            }
            "EncounterZone" => {
                let id = int_field(inst, "encounter_id", 1).max(1) as u32;
                let title = string_field(inst, "title", "AMBUSH LOCKDOWN").to_string();
                let preset = string_field(inst, "preset", "smelter_cage");
                let trigger_x = float_field(inst, "trigger_x", min.x + 85.0);
                let floor_y = float_field(inst, "floor_y", min.y);
                let (is_boss, spawn_right_gate, waves) =
                    build_encounter_waves(preset, min.x, max.x, floor_y);
                let encounter = LockdownEncounter {
                    id,
                    title,
                    is_boss,
                    trigger_x,
                    min_x: min.x,
                    max_x: max.x,
                    floor_y,
                    arena_center: Vec2::new((min.x + max.x) * 0.5, floor_y + 92.0),
                    spawn_right_gate,
                    waves,
                    current_wave: 0,
                    phase: EncounterPhase::Idle,
                    active_enemies: Vec::new(),
                    gate_entities: Vec::new(),
                };
                dynamic_spawns.encounters.push(encounter.clone());
                commands.spawn((Name::new(format!("EncounterZone_{id}")), encounter));
            }
            "BossSpawn" => {
                // Visual marker / schema confirmation; actual Boss spawns on Vulcanus Sanctum lockdown!
            }
            "BlastZone" => {
                let arena_center = Vec2::new(
                    float_field(inst, "arena_center_x", center.x),
                    float_field(inst, "arena_center_y", center.y),
                );
                commands.spawn((
                    Name::new("StageBlastZone"),
                    BlastZone {
                        min,
                        max,
                        arena_center,
                    },
                ));
            }
            other => {
                warn!("Unhandled LDtk entity identifier '{}'; skipping.", other);
            }
        }

        // Hide the raw LDtk entity marker node (our game entities render on WORLD_RENDER_LAYER)
        commands.entity(ldtk_entity).insert(Visibility::Hidden);
    }
}

/// Resets dynamic fighters, turrets, collectibles, breakable walls, checkpoints, encounters,
/// gates, projectiles, moving platforms, and telemetry cleanly on Stage Restart.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn handle_stage_restart(
    mut commands: Commands,
    mut restarts: MessageReader<StageRestartEvent>,
    dynamic_spawns: Res<StageDynamicSpawns>,
    mut telemetry: ResMut<LastHitTelemetry>,
    cleanup_q: Query<
        Entity,
        Or<(
            With<EnemyFighter>,
            With<PendingEnemyRespawn>,
            With<PendingArcanePillar>,
            With<WallTurret>,
            With<TurretProjectile>,
            With<BusterProjectile>,
            With<BreakerWaveProjectile>,
            With<Hitbox>,
            With<LockdownGate>,
            With<LockdownEncounter>,
            With<CollectibleItem>,
            With<BreakableWall>,
        )>,
    >,
    mut beacons_q: Query<&mut CheckpointBeacon>,
    mut moving_q: Query<(&mut MovingPlatform, &mut OneWayPlatform, Option<&mut Transform>)>,
) {
    if restarts.read().next().is_none() {
        return;
    }

    *telemetry = LastHitTelemetry::default();

    for e in &cleanup_q {
        commands.entity(e).despawn();
    }

    for mut beacon in &mut beacons_q {
        beacon.activated = false;
    }

    for (mut mp, mut plat, maybe_tf) in &mut moving_q {
        mp.phase = 0.0;
        mp.last_delta = Vec2::ZERO;
        plat.center = mp.origin;
        if let Some(mut tf) = maybe_tf {
            tf.translation.x = mp.origin.x;
            tf.translation.y = mp.origin.y;
        }
    }

    for &(archetype, pos) in &dynamic_spawns.enemies {
        spawn_stage_enemy(&mut commands, archetype, pos, false, None);
    }
    for &(pos, facing) in &dynamic_spawns.turrets {
        let e = spawn_wall_turret(&mut commands, pos, facing);
        commands.entity(e).insert(DamagePercent(0.0));
    }
    for &(kind, pos) in &dynamic_spawns.collectibles {
        commands.spawn((
            Name::new(format!("Collectible_{:?}", kind)),
            CollectibleItem::new(kind, pos),
            Transform::from_xyz(pos.x, pos.y, 1.0),
            Visibility::default(),
        ));
    }
    for &(center, size, hp) in &dynamic_spawns.breakable_walls {
        spawn_breakable_wall(&mut commands, center, size, hp);
    }
    for enc in &dynamic_spawns.encounters {
        commands.spawn((Name::new(format!("EncounterZone_{}", enc.id)), enc.clone()));
    }
}
