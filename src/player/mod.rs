//! Player entity, combat component wiring, spawn/reset handling, and gameplay markers.
//!
//! Headless-safe (`MinimalPlugins`-compatible): visual sprite meshes and normal-map
//! materials are attached separately by `RenderingPlugin`.

use bevy::prelude::*;
use leafwing_input_manager::prelude::ActionState;

use crate::combat::{
    AirDodgeState, AttackState, BusterState, CombatPrePhysicsSystems, CounterStance, DamagePercent,
    GroundDodgeState, Hitstop, Hitstun, Hurtbox, Invulnerable, MeteorDiveState, ShieldState,
    SmashChargeState, Team,
};
use crate::enemies::{BlastEdge, BlastZone, BlastZoneKoEvent, EncounterStatus, PlayerCheckpoint};
use crate::input::{ControlConfig, PlayerAction, player_input_map_for_config};
use crate::physics::{KinematicBody, MovementState, PhysicsSystems, PlayerIntent};
use crate::state::{AppState, StageRestartEvent};

/// Default player spawn center in *"The Foundry Gym"* (Zone A warmup deck).
pub const DEFAULT_PLAYER_SPAWN: Vec2 = Vec2::new(-230.0, 20.0);

/// Duration (`seconds`) the player hovers on the Smash-style respawn angel platform.
pub const RESPAWN_ANGEL_PLATFORM_SECS: f32 = 1.5;

/// Blinking respawn invulnerability granted on angel platform respawn (`seconds`).
pub const RESPAWN_INVULN_SECS: f32 = 2.0;

/// Zone A safety kill-plane below the warmup gaps (`px`).
const ZONE_A_KILL_Y: f32 = -190.0;

/// Marker component for the primary player entity.
#[derive(Component, Debug, Default)]
pub struct Player;

/// Child pivot anchored at the player's feet (`(0, -half_extents.y)`) so slope and
/// rotation pivots stay locked around boot contact with zero floating/clipping.
#[derive(Component, Debug, Default)]
pub struct PlayerFeetPivot;

/// Grandchild holding the baked pixel-art sprite mesh + `SpriteSheetAnimator`.
#[derive(Component, Debug, Default)]
pub struct PlayerSpriteVisual;

/// Floating `P1` cyan triangle indicator anchored above the player's head.
#[derive(Component, Debug, Default)]
pub struct PlayerP1Marker;

/// Glowing Smash-style respawn angel platform sprite managed by `RenderingPlugin`.
#[derive(Component, Debug, Default)]
pub struct RespawnAngelPlatformVisual;

/// Active Smash-style respawn angel platform state for the player after a blast-zone KO.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct PlayerRespawnState {
    pub active: bool,
    pub timer: f32,
    pub lock_timer: f32,
    pub platform_pos: Vec2,
}

/// Authoritative respawn position for the player entity (updated when LDtk `PlayerSpawn` loads).
#[derive(Resource, Debug, Clone, Copy)]
pub struct PlayerSpawnPoint(pub Vec2);

impl Default for PlayerSpawnPoint {
    fn default() -> Self {
        Self(DEFAULT_PLAYER_SPAWN)
    }
}

/// Plugin managing canonical player spawning, Smash angel respawn platform, and stage resets.
pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerSpawnPoint>()
            .add_systems(Startup, spawn_player)
            .add_systems(OnEnter(AppState::Title), reset_player_to_spawn)
            .add_systems(Update, reset_player_on_stage_restart)
            .add_systems(
                FixedUpdate,
                tick_player_respawn_platform
                    .before(CombatPrePhysicsSystems)
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(
                FixedUpdate,
                respawn_if_fallen_out_of_bounds
                    .after(PhysicsSystems)
                    .run_if(in_state(AppState::Playing)),
            );
    }
}

fn spawn_player(
    mut commands: Commands,
    spawn_point: Res<PlayerSpawnPoint>,
    control_cfg: Res<ControlConfig>,
) {
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
        (
            AttackState::default(),
            BusterState::default(),
            ShieldState::default(),
            AirDodgeState::default(),
            GroundDodgeState::default(),
            SmashChargeState::default(),
            CounterStance::default(),
            MeteorDiveState::default(),
            PlayerRespawnState::default(),
        ),
        (
            player_input_map_for_config(&control_cfg),
            ActionState::<PlayerAction>::default(),
            PlayerIntent::default(),
        ),
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
            &mut ShieldState,
            &mut AirDodgeState,
            &mut GroundDodgeState,
            &mut SmashChargeState,
            &mut CounterStance,
            &mut MeteorDiveState,
            &mut PlayerRespawnState,
        ),
        With<Player>,
    >,
) {
    for (
        entity,
        mut body,
        mut transform,
        mut percent,
        mut attack,
        mut buster,
        mut shield,
        mut dodge,
        mut ground_dodge,
        mut smash_charge,
        mut counter,
        mut dive,
        mut respawn,
    ) in players
    {
        *body = KinematicBody::at(pos);
        transform.translation.x = pos.x;
        transform.translation.y = pos.y;
        percent.0 = 0.0;
        *attack = AttackState::default();
        *buster = BusterState::default();
        *shield = ShieldState::default();
        *dodge = AirDodgeState::default();
        *ground_dodge = GroundDodgeState::default();
        *smash_charge = SmashChargeState::default();
        *counter = CounterStance::default();
        *dive = MeteorDiveState::default();
        *respawn = PlayerRespawnState::default();
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
            &mut ShieldState,
            &mut AirDodgeState,
            &mut GroundDodgeState,
            &mut SmashChargeState,
            &mut CounterStance,
            &mut MeteorDiveState,
            &mut PlayerRespawnState,
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
            &mut ShieldState,
            &mut AirDodgeState,
            &mut GroundDodgeState,
            &mut SmashChargeState,
            &mut CounterStance,
            &mut MeteorDiveState,
            &mut PlayerRespawnState,
        ),
        With<Player>,
    >,
) {
    if restarts.read().next().is_some() {
        apply_player_reset(&mut commands, spawn_point.0, &mut players);
    }
}

/// Manages the Smash-style 1.5s angel respawn platform; holds player safely at top-center
/// until they act (after brief grace) or the 1.5s timer expires.
#[allow(clippy::type_complexity)]
fn tick_player_respawn_platform(
    fixed_time: Res<Time<Fixed>>,
    mut players: Query<
        (
            &PlayerIntent,
            &mut PlayerRespawnState,
            &mut KinematicBody,
            &mut Transform,
        ),
        With<Player>,
    >,
) {
    let dt = fixed_time.delta_secs();
    for (intent, mut respawn, mut body, mut tf) in &mut players {
        if !respawn.active {
            continue;
        }
        respawn.timer = (respawn.timer - dt).max(0.0);
        respawn.lock_timer = (respawn.lock_timer - dt).max(0.0);

        let player_acted = respawn.lock_timer <= 0.0
            && (intent.move_axis.length_squared() > 0.16
                || intent.jump_just_pressed
                || intent.melee_just_pressed
                || intent.special_just_pressed
                || intent.buster_just_pressed);

        if respawn.timer <= 0.0 || player_acted {
            respawn.active = false;
            body.state = MovementState::Airborne;
            body.coyote_timer = 0.09;
            body.air_jumps_remaining = 1;
        } else {
            let hover_pos = respawn.platform_pos + Vec2::new(0.0, body.half_extents.y + 4.0);
            body.position = hover_pos;
            body.velocity = Vec2::ZERO;
            body.ground_speed = 0.0;
            body.state = MovementState::Grounded;
            body.air_jumps_remaining = 1;
            tf.translation.x = hover_pos.x;
            tf.translation.y = hover_pos.y;
        }
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn respawn_if_fallen_out_of_bounds(
    mut commands: Commands,
    spawn_point: Res<PlayerSpawnPoint>,
    checkpoint: Res<PlayerCheckpoint>,
    enc_status: Res<EncounterStatus>,
    blast_zones: Query<&BlastZone>,
    mut ko_events: MessageWriter<BlastZoneKoEvent>,
    mut players: Query<
        (
            Entity,
            &mut KinematicBody,
            &mut Transform,
            &mut DamagePercent,
            &mut AttackState,
            &mut AirDodgeState,
            &mut GroundDodgeState,
            &mut SmashChargeState,
            &mut CounterStance,
            &mut MeteorDiveState,
            &mut PlayerRespawnState,
        ),
        With<Player>,
    >,
) {
    let zone = blast_zones.iter().next().copied();

    for (
        entity,
        mut body,
        mut transform,
        mut percent,
        mut attack,
        mut dodge,
        mut ground_dodge,
        mut smash_charge,
        mut counter,
        mut dive,
        mut respawn,
    ) in &mut players
    {
        if respawn.active {
            continue;
        }

        let in_arena_region = zone.is_some_and(|z| body.position.x >= z.min.x - 8.0);
        let ko_edge = if let Some(z) = zone
            && in_arena_region
            && !z.contains(body.position)
        {
            // Ignore walking cleanly across a bridge near the left edge of a BlastZone
            let on_entry_bridge = body.position.x < z.min.x + 12.0 && body.position.y >= 175.0;
            if on_entry_bridge {
                None
            } else {
                Some(z.crossed_edge(body.position))
            }
        } else if body.position.y < ZONE_A_KILL_Y {
            Some(BlastEdge::Bottom)
        } else {
            None
        };

        let Some(edge) = ko_edge else {
            continue;
        };

        // Emit full Smash blast-zone KO flash & audio event!
        ko_events.write(BlastZoneKoEvent {
            is_player: true,
            archetype: None,
            ko_pos: body.position,
            edge,
            final_percent: percent.0,
        });

        let angel_pos = if let Some(lock_center) = enc_status.camera_lock_center {
            // Inside an active Lockdown Ambush or Boss Chamber -> top-center of that room!
            Vec2::new(lock_center.x, lock_center.y + 84.0)
        } else if let Some(z) = zone
            && in_arena_region
            && (z.max.x - z.min.x) < 1200.0
        {
            // Standalone Smash Battlefield blast zone (preserves headless arena test contract)
            Vec2::new(z.arena_center.x, z.arena_center.y + 92.0)
        } else if checkpoint.0 != DEFAULT_PLAYER_SPAWN {
            checkpoint.0 + Vec2::new(0.0, 52.0)
        } else {
            spawn_point.0 + Vec2::new(0.0, 68.0)
        };

        let hover_pos = angel_pos + Vec2::new(0.0, body.half_extents.y + 4.0);
        body.position = hover_pos;
        body.velocity = Vec2::ZERO;
        body.ground_speed = 0.0;
        body.state = MovementState::Grounded;
        body.visual_angle = 0.0;
        body.ground_normal = Vec2::Y;
        body.drop_through_timer = 0.0;
        body.loop_exit_cooldown = 0.0;
        body.air_jumps_remaining = 1;
        transform.translation.x = hover_pos.x;
        transform.translation.y = hover_pos.y;

        percent.0 = 0.0;
        attack.up_special_used = false;
        attack.side_special_used = false;
        attack.active_move = None;
        attack.active_timer = 0.0;
        *dodge = AirDodgeState::default();
        *ground_dodge = GroundDodgeState::default();
        *smash_charge = SmashChargeState::default();
        *counter = CounterStance::default();
        *dive = MeteorDiveState::default();

        respawn.active = true;
        respawn.timer = RESPAWN_ANGEL_PLATFORM_SECS;
        respawn.lock_timer = 0.35;
        respawn.platform_pos = angel_pos;

        commands
            .entity(entity)
            .remove::<Hitstun>()
            .remove::<Hitstop>()
            .insert(Invulnerable::new(RESPAWN_INVULN_SECS));
    }
}
