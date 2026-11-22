//! Player entity, combat component wiring, spawn/reset handling, and gameplay markers.
//!
//! Headless-safe (`MinimalPlugins`-compatible): visual sprite meshes and normal-map
//! materials are attached separately by `RenderingPlugin`.

use bevy::prelude::*;
use leafwing_input_manager::prelude::ActionState;

use crate::combat::{
    AttackState, BusterState, DamagePercent, Hitstop, Hitstun, Hurtbox, Invulnerable, Team,
};
use crate::input::{PlayerAction, default_player_input_map};
use crate::physics::{KinematicBody, MovementState, PhysicsSystems, PlayerIntent};
use crate::state::{AppState, StageRestartEvent};

/// Default player spawn center in *"The Foundry Gym"* (Zone A top of Sonic ramp).
pub const DEFAULT_PLAYER_SPAWN: Vec2 = Vec2::new(-230.0, 20.0);

/// Safety kill-plane bounds; falling below respawns cleanly at nearest zone safe point.
const FALL_RESPAWN_Y: f32 = -360.0;
const FAR_RIGHT_RESPAWN_X: f32 = 1840.0;

/// Marker component for the primary player entity.
#[derive(Component, Debug, Default)]
pub struct Player;

/// Child pivot anchored at the player's feet (`(0, -half_extents.y)`) so slope and
/// 360° loop rotations pivot strictly around boot contact with zero floating/clipping.
#[derive(Component, Debug, Default)]
pub struct PlayerFeetPivot;

/// Grandchild holding the baked pixel-art sprite mesh + `SpriteSheetAnimator`.
#[derive(Component, Debug, Default)]
pub struct PlayerSpriteVisual;

/// Authoritative respawn position for the player entity (updated when LDtk `PlayerSpawn` loads).
#[derive(Resource, Debug, Clone, Copy)]
pub struct PlayerSpawnPoint(pub Vec2);

impl Default for PlayerSpawnPoint {
    fn default() -> Self {
        Self(DEFAULT_PLAYER_SPAWN)
    }
}

/// Plugin managing canonical player spawning and respawn safety.
pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerSpawnPoint>()
            .add_systems(Startup, spawn_player)
            .add_systems(OnEnter(AppState::Title), reset_player_to_spawn)
            .add_systems(Update, reset_player_on_stage_restart)
            .add_systems(
                FixedUpdate,
                respawn_if_fallen_out_of_bounds
                    .after(PhysicsSystems)
                    .run_if(in_state(AppState::Playing)),
            );
    }
}

fn spawn_player(mut commands: Commands, spawn_point: Res<PlayerSpawnPoint>) {
    let pos = spawn_point.0;
    commands.spawn((
        Name::new("Player"),
        Player,
        Team::Player,
        Hurtbox {
            offset: Vec2::ZERO,
            radius: 16.0,
        },
        DamagePercent(0.0),
        AttackState::default(),
        BusterState::default(),
        default_player_input_map(),
        ActionState::<PlayerAction>::default(),
        PlayerIntent::default(),
        KinematicBody::at(pos),
        Transform::from_xyz(pos.x, pos.y, 0.0),
        Visibility::default(),
    ));
}

#[allow(clippy::type_complexity)]
fn apply_player_reset(
    commands: &mut Commands,
    pos: Vec2,
    players: &mut Query<
        (
            Entity,
            &mut KinematicBody,
            &mut Transform,
            &mut DamagePercent,
            &mut AttackState,
            &mut BusterState,
        ),
        With<Player>,
    >,
) {
    for (entity, mut body, mut transform, mut percent, mut attack, mut buster) in players {
        *body = KinematicBody::at(pos);
        transform.translation.x = pos.x;
        transform.translation.y = pos.y;
        percent.0 = 0.0;
        *attack = AttackState::default();
        *buster = BusterState::default();
        commands
            .entity(entity)
            .remove::<Hitstun>()
            .remove::<Hitstop>()
            .remove::<Invulnerable>();
    }
}

#[allow(clippy::type_complexity)]
fn reset_player_to_spawn(
    mut commands: Commands,
    spawn_point: Res<PlayerSpawnPoint>,
    mut players: Query<
        (
            Entity,
            &mut KinematicBody,
            &mut Transform,
            &mut DamagePercent,
            &mut AttackState,
            &mut BusterState,
        ),
        With<Player>,
    >,
) {
    apply_player_reset(&mut commands, spawn_point.0, &mut players);
}

#[allow(clippy::type_complexity)]
fn reset_player_on_stage_restart(
    mut commands: Commands,
    mut restarts: MessageReader<StageRestartEvent>,
    spawn_point: Res<PlayerSpawnPoint>,
    mut players: Query<
        (
            Entity,
            &mut KinematicBody,
            &mut Transform,
            &mut DamagePercent,
            &mut AttackState,
            &mut BusterState,
        ),
        With<Player>,
    >,
) {
    if restarts.read().next().is_some() {
        apply_player_reset(&mut commands, spawn_point.0, &mut players);
    }
}

#[allow(clippy::type_complexity)]
fn respawn_if_fallen_out_of_bounds(
    mut commands: Commands,
    spawn_point: Res<PlayerSpawnPoint>,
    mut players: Query<
        (
            Entity,
            &mut KinematicBody,
            &mut Transform,
            &mut DamagePercent,
            &mut AttackState,
        ),
        With<Player>,
    >,
) {
    for (entity, mut body, mut transform, mut percent, mut attack) in &mut players {
        let out_of_bounds =
            body.position.y < FALL_RESPAWN_Y || body.position.x > FAR_RIGHT_RESPAWN_X;
        if out_of_bounds {
            // If knocked out inside Zone C Smash Arena, respawn smoothly on the Battlefield center;
            // otherwise respawn at Zone A start.
            let respawn_pos = if body.position.x > 1000.0 {
                Vec2::new(1390.0, 260.0)
            } else {
                spawn_point.0
            };

            body.position = respawn_pos;
            body.velocity = Vec2::ZERO;
            body.ground_speed = 0.0;
            body.state = MovementState::Airborne;
            body.visual_angle = 0.0;
            body.ground_normal = Vec2::Y;
            body.drop_through_timer = 0.0;
            body.loop_exit_cooldown = 0.0;
            transform.translation.x = respawn_pos.x;
            transform.translation.y = respawn_pos.y;
            percent.0 = 0.0;
            attack.up_special_used = false;
            commands
                .entity(entity)
                .remove::<Hitstun>()
                .remove::<Hitstop>()
                .insert(Invulnerable::new(1.2));
        }
    }
}
