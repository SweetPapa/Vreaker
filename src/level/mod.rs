//! LDtk level loading (`assets/levels/demo_gym.ldtk`) via `bevy_ecs_ldtk` with
//! full entity hydration (solids, slopes, 360° loops, boost pads, drop-throughs,
//! moving platforms, spike hazards, wall turrets, AI fighters, and Smash blast zones)
//! plus clean stage restart handling.

use bevy::prelude::*;
use bevy_ecs_ldtk::prelude::*;

use crate::combat::{BusterProjectile, DamagePercent, Hitbox, LastHitTelemetry};
use crate::enemies::{
    BlastZone, EnemyArchetype, EnemyFighter, PendingEnemyRespawn, TurretProjectile, WallTurret,
    spawn_enemy_fighter, spawn_wall_turret,
};
use crate::physics::{
    BoostPad, LoopSpline, MovingPlatform, OneWayPlatform, SlopeSegment, SolidRect, SpikeHazard,
};
use crate::player::PlayerSpawnPoint;
use crate::state::StageRestartEvent;

/// LDtk level pixel height & world-origin alignment matching `assets/levels/demo_gym.ldtk`.
pub const LDTK_LEVEL_HEIGHT: f32 = 960.0;
pub const LDTK_ORIGIN_X: f32 = -360.0;
pub const LDTK_ORIGIN_Y: f32 = -260.0;

/// Authoritative list of respawnable dynamic spawns parsed from the active LDtk level.
#[derive(Resource, Debug, Default, Clone)]
pub struct StageDynamicSpawns {
    pub enemies: Vec<(EnemyArchetype, Vec2)>,
    pub turrets: Vec<(Vec2, f32)>,
}

/// Plugin wiring `bevy_ecs_ldtk` level loading (`assets/levels/demo_gym.ldtk`),
/// hot-reload entity hydration, and clean stage restart.
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
            .add_systems(Startup, spawn_demo_gym_ldtk_world)
            .add_systems(Update, (hydrate_ldtk_entities, handle_stage_restart));
    }
}

fn spawn_demo_gym_ldtk_world(mut commands: Commands, asset_server: Res<AssetServer>) {
    let ldtk_handle = asset_server.load("levels/demo_gym.ldtk").into();
    commands.spawn((
        Name::new("FoundryGymLdtkWorld"),
        LdtkWorldBundle {
            ldtk_handle,
            ..default()
        },
    ));
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

/// Hydrates every newly loaded (or hot-reloaded) LDtk [`EntityInstance`] into
/// deterministic physics, combat, enemy, and blast-zone ECS components.
fn hydrate_ldtk_entities(
    mut commands: Commands,
    mut spawn_point: ResMut<PlayerSpawnPoint>,
    mut dynamic_spawns: ResMut<StageDynamicSpawns>,
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
                commands.spawn((
                    Name::new(format!("LdtkSolid_{}", inst.iid)),
                    SolidRect::from_min_max(min, max),
                ));
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
                commands.spawn((
                    Name::new(format!("LdtkSlope_{}", inst.iid)),
                    SlopeSegment::new(start, end),
                ));
            }
            "BoostPad" => {
                let target_speed = float_field(inst, "target_speed", 500.0);
                let dir_x = float_field(inst, "dir_x", 1.0);
                commands.spawn((
                    Name::new(format!("LdtkBoostPad_{}", inst.iid)),
                    BoostPad::new(center, size, Vec2::new(dir_x, 0.0), target_speed),
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
                let kind_str = inst
                    .get_string_field("archetype")
                    .map(String::as_str)
                    .unwrap_or("light");
                let archetype = if kind_str.eq_ignore_ascii_case("heavy") {
                    EnemyArchetype::HeavyBrawler
                } else {
                    EnemyArchetype::LightStriker
                };
                dynamic_spawns.enemies.push((archetype, center));
                spawn_enemy_fighter(&mut commands, archetype, center, false);
            }
            "BlastZone" => {
                let arena_center = Vec2::new(
                    float_field(inst, "arena_center_x", center.x),
                    float_field(inst, "arena_center_y", center.y),
                );
                commands.spawn((
                    Name::new("ZoneC_BlastZone"),
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

/// Resets dynamic fighters, turrets, projectiles, moving platforms, and hit telemetry on restart.
#[allow(clippy::too_many_arguments)]
fn handle_stage_restart(
    mut commands: Commands,
    mut restarts: MessageReader<StageRestartEvent>,
    dynamic_spawns: Res<StageDynamicSpawns>,
    mut telemetry: ResMut<LastHitTelemetry>,
    enemies_q: Query<Entity, With<EnemyFighter>>,
    pending_q: Query<Entity, With<PendingEnemyRespawn>>,
    turrets_q: Query<Entity, With<WallTurret>>,
    turret_orbs_q: Query<Entity, With<TurretProjectile>>,
    buster_q: Query<Entity, With<BusterProjectile>>,
    hitboxes_q: Query<Entity, With<Hitbox>>,
    mut moving_q: Query<(&mut MovingPlatform, &mut OneWayPlatform, Option<&mut Transform>)>,
) {
    if restarts.read().next().is_none() {
        return;
    }

    *telemetry = LastHitTelemetry::default();

    for e in enemies_q
        .iter()
        .chain(pending_q.iter())
        .chain(turrets_q.iter())
        .chain(turret_orbs_q.iter())
        .chain(buster_q.iter())
        .chain(hitboxes_q.iter())
    {
        commands.entity(e).despawn();
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
        spawn_enemy_fighter(&mut commands, archetype, pos, false);
    }
    for &(pos, facing) in &dynamic_spawns.turrets {
        let e = spawn_wall_turret(&mut commands, pos, facing);
        commands.entity(e).insert(DamagePercent(0.0));
    }
}
