//! Smash-inspired combat system & Mega Man Buster:
//! - `Hurtbox`, `Hitbox`, `DamagePercent`, `Hitstun`, `Hitstop`, `Invulnerable`
//! - Momentum-scaled Smash knockback: `base_kb + (target_percent * kb_scale) + (|v_attacker| * momentum_mult)`
//! - 3–8 frame impact hitstop pausing attacker & target simulation
//! - Directional moveset: 3-hit Jab, Dash Attack, F-Air, D-Air (meteor spike + recoil),
//!   and Up-Special (vertical recovery + instant Sonic horizontal air-brake)
//! - Mega Man Buster: max 3 active uncharged pellets; hold `>= 0.7s` for piercing Charged Shot
//!
//! Architectural invariant: completely decoupled from rendering for headless 60 Hz testing.

use std::time::Duration;

use bevy::prelude::*;

use crate::physics::{
    KinematicBody, MovementState, PhysicsSystems, PlayerIntent, SolidRect,
};
use crate::player::Player;
use crate::state::AppState;

/// Maximum uncharged Buster pellets allowed on screen simultaneously per shooter (`SPEC.MD`).
pub const MAX_UNCHARGED_BUSTER_PELLETS: usize = 3;

/// Hold duration (`seconds`) required to charge a piercing Charged Buster Shot (`SPEC.MD`).
pub const BUSTER_CHARGE_TIME_SECS: f32 = 0.70;

/// Simulation frame rate reference for converting hitstop frames (`3..=8`) to seconds.
const HITSTOP_REF_FPS: f32 = 60.0;

/// Combat faction preventing friendly fire / self-hits.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Team {
    Player,
    Enemy,
}

/// Circular vulnerable region attached to a fighter or target entity.
#[derive(Component, Debug, Clone, Copy)]
pub struct Hurtbox {
    /// Local offset from entity [`KinematicBody::position`] (`px`).
    pub offset: Vec2,
    /// Circle radius (`px`).
    pub radius: f32,
}

impl Default for Hurtbox {
    fn default() -> Self {
        Self {
            offset: Vec2::ZERO,
            radius: 16.0,
        }
    }
}

/// Active offensive hitbox (attached to an attacker or carried by a projectile).
#[derive(Component, Debug, Clone)]
pub struct Hitbox {
    /// Entity that spawned this hitbox (receives attacker hitstop & recoil).
    pub owner: Entity,
    /// Faction of the attacker.
    pub team: Team,
    /// Local offset from this hitbox entity's position (`+X` scaled by `facing`).
    pub offset: Vec2,
    /// Circle radius (`px`).
    pub radius: f32,
    /// Percentage damage added to target's [`DamagePercent`].
    pub damage: f32,
    /// Base Smash knockback velocity (`px/s`).
    pub base_kb: f32,
    /// Knockback scaling per `1%` of target [`DamagePercent`].
    pub kb_scale: f32,
    /// Momentum-to-Smash scaling multiplier applied to `|v_attacker|`.
    pub momentum_mult: f32,
    /// Launch angle in degrees (`0°` horizontal forward, `90°` up, negative = meteor spike).
    pub angle_deg: f32,
    /// Snapshot of attacker speed (`|v_attacker|`) when the attack launched.
    pub attacker_speed: f32,
    /// Facing sign (`+1.0` right, `-1.0` left).
    pub facing: f32,
    /// Remaining active duration before this hitbox despawns (if standalone) or deactivates.
    pub lifetime: Timer,
    /// If `true`, hitbox persists after hitting a target (e.g., Charged Buster Shot / melee sweep).
    pub piercing: bool,
    /// If `true`, despawn the hitbox entity on expiry (detached swing or projectile).
    pub despawn_on_expire: bool,
    /// If `true`, hitting a target grants the attacker an upward D-Air meteor recoil bounce.
    pub recoil_bounce_vy: Option<f32>,
    /// Entities already struck by this hitbox instance (prevents multi-frame repeat hits).
    pub hit_entities: Vec<Entity>,
    /// Marks whether this hitbox came from a Charged Buster Shot (for extra shake/FX).
    pub is_charged: bool,
}

/// Smash-style accumulated damage percentage (`0.0` .. unbounded).
#[derive(Component, Debug, Clone, Copy, Default, PartialEq)]
pub struct DamagePercent(pub f32);

/// Optional target knockback scaling multiplier (`< 1.0` for heavy fighters, `> 1.0` for light fighters).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct KnockbackWeight(pub f32);

impl Default for KnockbackWeight {
    fn default() -> Self {
        Self(1.0)
    }
}

/// Active hitstun timer preventing normal movement/attacks while launched.
#[derive(Component, Debug, Clone)]
pub struct Hitstun(pub Timer);

impl Hitstun {
    pub fn new(secs: f32) -> Self {
        Self(Timer::from_seconds(secs.max(0.01), TimerMode::Once))
    }

    pub fn is_active(&self) -> bool {
        !self.0.is_finished()
    }
}

/// Impact freeze-frame timer (`3`–`8` frames) pausing physics & animation on attacker and target.
#[derive(Component, Debug, Clone)]
pub struct Hitstop(pub Timer);

impl Hitstop {
    pub fn from_frames(frames: u32) -> Self {
        let secs = (frames.max(1) as f32) / HITSTOP_REF_FPS;
        Self(Timer::from_seconds(secs, TimerMode::Once))
    }

    pub fn from_seconds(secs: f32) -> Self {
        Self(Timer::from_seconds(secs.max(0.001), TimerMode::Once))
    }

    pub fn is_active(&self) -> bool {
        !self.0.is_finished()
    }

    pub fn remaining_secs(&self) -> f32 {
        self.0.remaining_secs()
    }
}

/// Invulnerability timer (e.g., hazard i-frames or respawn grace).
#[derive(Component, Debug, Clone)]
pub struct Invulnerable(pub Timer);

impl Invulnerable {
    pub fn new(secs: f32) -> Self {
        Self(Timer::from_seconds(secs.max(0.01), TimerMode::Once))
    }

    pub fn is_active(&self) -> bool {
        !self.0.is_finished()
    }
}

/// Minimum vertical-slice Smash directional moveset (`SPEC.MD` Section 4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttackKind {
    Jab1,
    Jab2,
    Jab3,
    DashAttack,
    ForwardAir,
    DownAir,
    UpSpecial,
}

impl AttackKind {
    /// Corresponding baked animation clip key in `assets/sprites/hero/hero.ron`.
    pub fn animation_name(self) -> &'static str {
        match self {
            AttackKind::Jab1 | AttackKind::Jab2 | AttackKind::Jab3 => "jab",
            AttackKind::DashAttack => "dash_attack",
            AttackKind::ForwardAir => "f_air",
            AttackKind::DownAir => "d_air",
            AttackKind::UpSpecial => "up_special",
        }
    }
}

/// Active melee/special attack state tracked on fighters.
#[derive(Component, Debug, Clone, Default)]
pub struct AttackState {
    pub active_move: Option<AttackKind>,
    pub active_timer: f32,
    pub cooldown: f32,
    pub jab_combo_step: u8,
    pub jab_combo_window: f32,
    pub up_special_used: bool,
    /// Brief timer set whenever a Buster shot fires so animation can flash `"buster"`.
    pub buster_anim_timer: f32,
}

/// Tracks Mega Man Buster tap/charge state on a shooter entity.
#[derive(Component, Debug, Clone, Default)]
pub struct BusterState {
    pub charge_secs: f32,
    pub was_held: bool,
    pub fire_cooldown: f32,
}

impl BusterState {
    pub fn is_fully_charged(&self) -> bool {
        self.charge_secs >= BUSTER_CHARGE_TIME_SECS
    }

    pub fn charge_ratio(&self) -> f32 {
        (self.charge_secs / BUSTER_CHARGE_TIME_SECS).clamp(0.0, 1.0)
    }
}

/// Energy projectile fired by the Mega Man Buster.
#[derive(Component, Debug, Clone, Copy)]
pub struct BusterProjectile {
    pub owner: Entity,
    pub velocity: Vec2,
    pub charged: bool,
}

/// Interactive Phase 3 sparring target dummy component (upgraded to AI enemies in Phase 4).
#[derive(Component, Debug, Clone, Copy)]
pub struct CombatDummy {
    pub home_pos: Vec2,
    pub heavy: bool,
}

/// Message emitted on every confirmed hitbox-hurtbox strike for sparks, screen shake, audio & HUD.
#[derive(Message, Debug, Clone, Copy)]
pub struct CombatHitEvent {
    pub attacker: Entity,
    pub target: Entity,
    pub hit_pos: Vec2,
    pub damage: f32,
    pub target_percent_after: f32,
    pub knockback_mag: f32,
    pub knockback_vec: Vec2,
    pub hitstop_frames: u32,
    pub is_charged: bool,
}

/// Message emitted whenever a Mega Man Buster pellet or Charged Shot is fired.
#[derive(Message, Debug, Clone, Copy)]
pub struct BusterShotEvent {
    pub shooter: Entity,
    pub origin: Vec2,
    pub direction: f32,
    pub charged: bool,
}

/// Stores last target struck for live Smash `%` telemetry on the HUD.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct LastHitTelemetry {
    pub target_percent: f32,
    pub last_knockback: f32,
    pub last_damage: f32,
    pub last_hitstop_frames: u32,
    pub has_hit: bool,
}

/// Computes the exact Momentum-Scaled Smash Knockback formula from `SPEC.MD` Section 4.2:
/// `KB = base_kb + (target_percent * kb_scale) + (|v_attacker| * momentum_mult)`.
pub fn compute_smash_knockback(
    base_kb: f32,
    target_percent: f32,
    kb_scale: f32,
    attacker_speed: f32,
    momentum_mult: f32,
) -> f32 {
    base_kb + (target_percent.max(0.0) * kb_scale) + (attacker_speed.max(0.0) * momentum_mult)
}

/// Computes impact freeze-frames (`3` to `8` frames at 60 Hz) scaled by knockback magnitude.
pub fn compute_hitstop_frames(knockback_mag: f32) -> u32 {
    let raw = 3.0 + ((knockback_mag - 130.0) / 85.0);
    raw.clamp(3.0, 8.0).round() as u32
}

/// Converts knockback magnitude, launch angle (`deg`), and attacker facing into a 2D velocity vector.
pub fn knockback_vector(knockback_mag: f32, angle_deg: f32, facing: f32) -> Vec2 {
    let rad = angle_deg.to_radians();
    let dir_x = if facing < 0.0 { -1.0 } else { 1.0 };
    Vec2::new(rad.cos() * dir_x, rad.sin()) * knockback_mag
}

/// System set for pre-physics combat actions (attacks, Up-B air-brake, Buster fire).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CombatPrePhysicsSystems;

/// System set for post-physics hitstop ticking, projectile travel, and hitbox overlap checks.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CombatPostPhysicsSystems;

/// Plugin registering deterministic 60 Hz Smash combat and Mega Man Buster systems.
pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LastHitTelemetry>()
            .add_message::<CombatHitEvent>()
            .add_message::<BusterShotEvent>()
            .configure_sets(
                FixedUpdate,
                (
                    crate::physics::InputSyncSystems,
                    CombatPrePhysicsSystems,
                    PhysicsSystems,
                    CombatPostPhysicsSystems,
                )
                    .chain()
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(
                FixedUpdate,
                (handle_player_melee_and_special, handle_buster_weapon)
                    .in_set(CombatPrePhysicsSystems),
            )
            .add_systems(
                FixedUpdate,
                (
                    tick_hitstop_and_status_timers,
                    advance_buster_projectiles,
                    sync_attached_hitboxes,
                    resolve_hitbox_hurtbox_collisions,
                    respawn_launched_dummies,
                )
                    .chain()
                    .in_set(CombatPostPhysicsSystems),
            );
    }
}

/// Spec parameters for a melee/special hitbox spawn.
struct MoveSpec {
    offset: Vec2,
    radius: f32,
    damage: f32,
    base_kb: f32,
    kb_scale: f32,
    momentum_mult: f32,
    angle_deg: f32,
    active_secs: f32,
    cooldown_secs: f32,
    recoil_bounce_vy: Option<f32>,
}

fn move_spec(kind: AttackKind) -> MoveSpec {
    match kind {
        AttackKind::Jab1 => MoveSpec {
            offset: Vec2::new(20.0, 2.0),
            radius: 14.0,
            damage: 5.0,
            base_kb: 105.0,
            kb_scale: 1.3,
            momentum_mult: 0.25,
            angle_deg: 26.0,
            active_secs: 0.11,
            cooldown_secs: 0.15,
            recoil_bounce_vy: None,
        },
        AttackKind::Jab2 => MoveSpec {
            offset: Vec2::new(22.0, 3.0),
            radius: 15.0,
            damage: 6.0,
            base_kb: 120.0,
            kb_scale: 1.6,
            momentum_mult: 0.30,
            angle_deg: 30.0,
            active_secs: 0.11,
            cooldown_secs: 0.16,
            recoil_bounce_vy: None,
        },
        AttackKind::Jab3 => MoveSpec {
            offset: Vec2::new(25.0, 4.0),
            radius: 17.0,
            damage: 10.0,
            base_kb: 195.0,
            kb_scale: 2.8,
            momentum_mult: 0.45,
            angle_deg: 38.0,
            active_secs: 0.14,
            cooldown_secs: 0.28,
            recoil_bounce_vy: None,
        },
        AttackKind::DashAttack => MoveSpec {
            offset: Vec2::new(26.0, 2.0),
            radius: 17.0,
            damage: 12.0,
            base_kb: 190.0,
            kb_scale: 2.9,
            momentum_mult: 0.65,
            angle_deg: 32.0,
            active_secs: 0.17,
            cooldown_secs: 0.26,
            recoil_bounce_vy: None,
        },
        AttackKind::ForwardAir => MoveSpec {
            offset: Vec2::new(24.0, 4.0),
            radius: 16.0,
            damage: 11.0,
            base_kb: 180.0,
            kb_scale: 2.8,
            momentum_mult: 0.62,
            angle_deg: 35.0,
            active_secs: 0.15,
            cooldown_secs: 0.24,
            recoil_bounce_vy: None,
        },
        AttackKind::DownAir => MoveSpec {
            offset: Vec2::new(6.0, -22.0),
            radius: 17.0,
            damage: 13.0,
            base_kb: 205.0,
            kb_scale: 2.9,
            momentum_mult: 0.45,
            angle_deg: -78.0,
            active_secs: 0.16,
            cooldown_secs: 0.26,
            recoil_bounce_vy: Some(260.0),
        },
        AttackKind::UpSpecial => MoveSpec {
            offset: Vec2::new(4.0, 10.0),
            radius: 22.0,
            damage: 9.0,
            base_kb: 175.0,
            kb_scale: 2.4,
            momentum_mult: 0.35,
            angle_deg: 76.0,
            active_secs: 0.22,
            cooldown_secs: 0.30,
            recoil_bounce_vy: None,
        },
    }
}

/// Marker identifying a hitbox that follows its owner's position each tick.
#[derive(Component, Debug, Clone, Copy)]
pub struct AttachedHitbox;

#[allow(clippy::type_complexity)]
fn handle_player_melee_and_special(
    mut commands: Commands,
    fixed_time: Res<Time<Fixed>>,
    mut query: Query<
        (
            Entity,
            &Team,
            &PlayerIntent,
            &mut KinematicBody,
            &mut AttackState,
            Option<&Hitstop>,
            Option<&Hitstun>,
        ),
        With<Player>,
    >,
) {
    let dt = fixed_time.delta_secs();

    for (entity, team, intent, mut body, mut attack, hitstop, hitstun) in &mut query {
        // Freeze combat state transitions during Hitstop
        if hitstop.is_some_and(Hitstop::is_active) {
            continue;
        }

        // Refresh Up-Special recovery whenever touching ground or wall-sliding
        if matches!(
            body.state,
            MovementState::Grounded | MovementState::WallSliding { .. }
        ) {
            attack.up_special_used = false;
        }

        attack.cooldown = (attack.cooldown - dt).max(0.0);
        attack.jab_combo_window = (attack.jab_combo_window - dt).max(0.0);
        attack.buster_anim_timer = (attack.buster_anim_timer - dt).max(0.0);
        if attack.jab_combo_window <= 0.0 {
            attack.jab_combo_step = 0;
        }
        if attack.active_timer > 0.0 {
            attack.active_timer = (attack.active_timer - dt).max(0.0);
            if attack.active_timer <= 0.0 {
                attack.active_move = None;
            }
        }

        if hitstun.is_some_and(Hitstun::is_active) {
            continue;
        }
        if matches!(body.state, MovementState::LoopLocked { .. }) {
            continue;
        }
        if attack.cooldown > 0.0 {
            continue;
        }

        let attacker_speed = body.speed();

        // 1. Up-Special (Up-B): High vertical recovery + instant horizontal Sonic air-brake!
        if intent.special_just_pressed && !attack.up_special_used {
            let spec = move_spec(AttackKind::UpSpecial);
            // Instant air-brake cancels horizontal Sonic momentum cleanly into Mega Man precision!
            body.velocity.x *= 0.18;
            body.ground_speed = body.velocity.x;
            body.velocity.y = 485.0;
            body.state = MovementState::Airborne;
            body.on_one_way = false;
            body.coyote_timer = 0.0;
            body.ground_normal = Vec2::Y;
            body.visual_angle = 0.0;
            body.jump_cut_applied = true;

            attack.active_move = Some(AttackKind::UpSpecial);
            attack.active_timer = spec.active_secs;
            attack.cooldown = spec.cooldown_secs;
            attack.up_special_used = true;

            spawn_melee_hitbox(&mut commands, entity, *team, &body, attacker_speed, &spec);
            continue;
        }

        // 2. Directional Melee Attacks (Jab 3-hit combo, Dash Attack, F-Air, D-Air)
        if intent.melee_just_pressed {
            let kind = match body.state {
                MovementState::Airborne | MovementState::WallSliding { .. } => {
                    if intent.move_axis.y < -0.35 {
                        AttackKind::DownAir
                    } else {
                        // F-Air acts as a light air-brake if traveling faster than normal run speed
                        if body.velocity.x.abs() > 260.0 {
                            body.velocity.x *= 0.80;
                        }
                        AttackKind::ForwardAir
                    }
                }
                MovementState::Grounded => {
                    if attacker_speed >= 155.0 {
                        // Dash Attack preserves Sonic momentum with a crisp lunge burst
                        let boost = (attacker_speed * 1.06).clamp(240.0, 560.0);
                        body.ground_speed = body.facing * boost;
                        body.velocity.x = body.ground_speed;
                        AttackKind::DashAttack
                    } else {
                        match attack.jab_combo_step {
                            0 => {
                                attack.jab_combo_step = 1;
                                attack.jab_combo_window = 0.42;
                                AttackKind::Jab1
                            }
                            1 => {
                                attack.jab_combo_step = 2;
                                attack.jab_combo_window = 0.42;
                                AttackKind::Jab2
                            }
                            _ => {
                                attack.jab_combo_step = 0;
                                attack.jab_combo_window = 0.0;
                                AttackKind::Jab3
                            }
                        }
                    }
                }
                MovementState::LoopLocked { .. } => continue,
            };

            let spec = move_spec(kind);
            attack.active_move = Some(kind);
            attack.active_timer = spec.active_secs;
            attack.cooldown = spec.cooldown_secs;

            spawn_melee_hitbox(&mut commands, entity, *team, &body, attacker_speed, &spec);
        }
    }
}

fn spawn_melee_hitbox(
    commands: &mut Commands,
    owner: Entity,
    team: Team,
    body: &KinematicBody,
    attacker_speed: f32,
    spec: &MoveSpec,
) {
    let world_offset = Vec2::new(spec.offset.x * body.facing, spec.offset.y);
    let pos = body.position + world_offset;
    commands.spawn((
        Name::new("MeleeHitbox"),
        AttachedHitbox,
        Hitbox {
            owner,
            team,
            offset: spec.offset,
            radius: spec.radius,
            damage: spec.damage,
            base_kb: spec.base_kb,
            kb_scale: spec.kb_scale,
            momentum_mult: spec.momentum_mult,
            angle_deg: spec.angle_deg,
            attacker_speed,
            facing: body.facing,
            lifetime: Timer::from_seconds(spec.active_secs, TimerMode::Once),
            piercing: true, // Melee arc can cleave multiple targets once each
            despawn_on_expire: true,
            recoil_bounce_vy: spec.recoil_bounce_vy,
            hit_entities: Vec::new(),
            is_charged: false,
        },
        Transform::from_xyz(pos.x, pos.y, 1.0),
    ));
}

#[allow(clippy::type_complexity)]
fn handle_buster_weapon(
    mut commands: Commands,
    fixed_time: Res<Time<Fixed>>,
    projectiles_q: Query<&BusterProjectile>,
    mut shooters: Query<
        (
            Entity,
            &Team,
            &PlayerIntent,
            &KinematicBody,
            &mut BusterState,
            Option<&mut AttackState>,
            Option<&Hitstop>,
            Option<&Hitstun>,
        ),
        With<Player>,
    >,
    mut shot_events: MessageWriter<BusterShotEvent>,
) {
    let dt = fixed_time.delta_secs();

    for (entity, team, intent, body, mut buster, mut maybe_attack, hitstop, hitstun) in
        &mut shooters
    {
        if hitstop.is_some_and(Hitstop::is_active) {
            continue;
        }
        buster.fire_cooldown = (buster.fire_cooldown - dt).max(0.0);

        if hitstun.is_some_and(Hitstun::is_active) {
            buster.charge_secs = 0.0;
            buster.was_held = false;
            continue;
        }

        // Count currently active uncharged pellets belonging to this shooter
        let active_pellets = projectiles_q
            .iter()
            .filter(|p| p.owner == entity && !p.charged)
            .count();

        let muzzle_pos = body.position + Vec2::new(body.facing * 20.0, 4.0);

        // Tap Buster -> fire rapid uncharged pellet (capped at MAX_UNCHARGED_BUSTER_PELLETS = 3)
        if intent.buster_just_pressed
            && buster.fire_cooldown <= 0.0
            && active_pellets < MAX_UNCHARGED_BUSTER_PELLETS
        {
            spawn_buster_projectile(&mut commands, entity, *team, muzzle_pos, body.facing, false);
            shot_events.write(BusterShotEvent {
                shooter: entity,
                origin: muzzle_pos,
                direction: body.facing,
                charged: false,
            });
            buster.fire_cooldown = 0.09;
            if let Some(ref mut atk) = maybe_attack {
                atk.buster_anim_timer = 0.16;
            }
        }

        // Hold Buster -> accumulate charge; releasing at >= 0.70s fires piercing Charged Shot!
        if intent.buster_held {
            buster.charge_secs += dt;
            buster.was_held = true;
        } else if buster.was_held {
            if buster.charge_secs >= BUSTER_CHARGE_TIME_SECS {
                spawn_buster_projectile(&mut commands, entity, *team, muzzle_pos, body.facing, true);
                shot_events.write(BusterShotEvent {
                    shooter: entity,
                    origin: muzzle_pos,
                    direction: body.facing,
                    charged: true,
                });
                buster.fire_cooldown = 0.18;
                if let Some(ref mut atk) = maybe_attack {
                    atk.buster_anim_timer = 0.24;
                }
            }
            buster.charge_secs = 0.0;
            buster.was_held = false;
        }
    }
}

fn spawn_buster_projectile(
    commands: &mut Commands,
    owner: Entity,
    team: Team,
    origin: Vec2,
    facing: f32,
    charged: bool,
) {
    let speed = if charged { 540.0 } else { 460.0 };
    let velocity = Vec2::new(facing * speed, 0.0);
    let (radius, damage, base_kb, kb_scale, angle_deg, lifetime_secs) = if charged {
        (16.0, 16.0, 240.0, 3.2, 30.0, 1.35)
    } else {
        (7.0, 3.5, 85.0, 0.7, 20.0, 0.90)
    };

    commands.spawn((
        Name::new(if charged {
            "BusterChargedShot"
        } else {
            "BusterPellet"
        }),
        BusterProjectile {
            owner,
            velocity,
            charged,
        },
        Hitbox {
            owner,
            team,
            offset: Vec2::ZERO,
            radius,
            damage,
            base_kb,
            kb_scale,
            momentum_mult: 0.0,
            angle_deg,
            attacker_speed: 0.0,
            facing,
            lifetime: Timer::from_seconds(lifetime_secs, TimerMode::Once),
            piercing: charged,
            despawn_on_expire: true,
            recoil_bounce_vy: None,
            hit_entities: Vec::new(),
            is_charged: charged,
        },
        Transform::from_xyz(origin.x, origin.y, 3.0),
        Visibility::default(),
    ));
}

fn tick_hitstop_and_status_timers(
    mut commands: Commands,
    fixed_time: Res<Time<Fixed>>,
    mut hitstops: Query<(Entity, &mut Hitstop)>,
    mut hitstuns: Query<(Entity, &mut Hitstun)>,
    mut invulns: Query<(Entity, &mut Invulnerable)>,
    mut hitboxes: Query<(Entity, &mut Hitbox)>,
) {
    let dt = Duration::from_secs_f32(fixed_time.delta_secs());

    for (entity, mut hs) in &mut hitstops {
        hs.0.tick(dt);
        if hs.0.is_finished() {
            commands.entity(entity).remove::<Hitstop>();
        }
    }

    for (entity, mut stun) in &mut hitstuns {
        stun.0.tick(dt);
        if stun.0.is_finished() {
            commands.entity(entity).remove::<Hitstun>();
        }
    }

    for (entity, mut inv) in &mut invulns {
        inv.0.tick(dt);
        if inv.0.is_finished() {
            commands.entity(entity).remove::<Invulnerable>();
        }
    }

    for (entity, mut hb) in &mut hitboxes {
        hb.lifetime.tick(dt);
        if hb.lifetime.is_finished() && hb.despawn_on_expire {
            commands.entity(entity).despawn();
        }
    }
}

fn advance_buster_projectiles(
    mut commands: Commands,
    fixed_time: Res<Time<Fixed>>,
    solids: Query<&SolidRect>,
    mut projectiles: Query<(Entity, &BusterProjectile, &Hitbox, &mut Transform)>,
) {
    let dt = fixed_time.delta_secs();
    let solid_list: Vec<SolidRect> = solids.iter().copied().collect();

    for (entity, proj, hb, mut tf) in &mut projectiles {
        tf.translation.x += proj.velocity.x * dt;
        tf.translation.y += proj.velocity.y * dt;

        // Uncharged pellets shatter cleanly on solid walls; Charged Shots pierce thin obstacles
        if !proj.charged {
            let p = tf.translation.truncate();
            let hit_wall = solid_list.iter().any(|rect| {
                let min = rect.min();
                let max = rect.max();
                p.x + hb.radius >= min.x
                    && p.x - hb.radius <= max.x
                    && p.y + hb.radius >= min.y
                    && p.y - hb.radius <= max.y
            });
            if hit_wall {
                commands.entity(entity).despawn();
            }
        }
    }
}

fn sync_attached_hitboxes(
    bodies: Query<&KinematicBody>,
    mut hitboxes: Query<(&Hitbox, &mut Transform), With<AttachedHitbox>>,
) {
    for (hb, mut tf) in &mut hitboxes {
        if let Ok(owner_body) = bodies.get(hb.owner) {
            let pos = owner_body.position + Vec2::new(hb.offset.x * hb.facing, hb.offset.y);
            tf.translation.x = pos.x;
            tf.translation.y = pos.y;
        }
    }
}

#[allow(clippy::type_complexity)]
fn resolve_hitbox_hurtbox_collisions(
    mut commands: Commands,
    mut hitboxes: Query<(Entity, &mut Hitbox, &Transform)>,
    mut targets: Query<(
        Entity,
        &Team,
        &Hurtbox,
        &mut KinematicBody,
        &mut DamagePercent,
        Option<&Invulnerable>,
        Option<&KnockbackWeight>,
    )>,
    mut telemetry: ResMut<LastHitTelemetry>,
    mut hit_events: MessageWriter<CombatHitEvent>,
) {
    let mut attacker_recoil_updates: Vec<(Entity, f32)> = Vec::new();
    let mut attacker_hitstops: Vec<(Entity, u32)> = Vec::new();

    for (hb_entity, mut hb, hb_tf) in &mut hitboxes {
        if hb.lifetime.is_finished() {
            continue;
        }
        let hb_pos = hb_tf.translation.truncate();
        let mut consumed_non_piercing = false;

        for (target_entity, target_team, hurtbox, mut target_body, mut percent, invuln, kb_weight) in
            &mut targets
        {
            if target_entity == hb.owner || *target_team == hb.team {
                continue;
            }
            if invuln.is_some_and(Invulnerable::is_active) {
                continue;
            }
            if hb.hit_entities.contains(&target_entity) {
                continue;
            }

            let hurt_pos = target_body.position + hurtbox.offset;
            let combined_r = hb.radius + hurtbox.radius;
            if hb_pos.distance_squared(hurt_pos) > combined_r * combined_r {
                continue;
            }

            // Confirmed hit! Apply Smash DamagePercent & Momentum-Scaled Knockback formula
            percent.0 += hb.damage;
            let weight_mult = kb_weight.map_or(1.0, |w| w.0);
            let kb_mag = compute_smash_knockback(
                hb.base_kb,
                percent.0,
                hb.kb_scale,
                hb.attacker_speed,
                hb.momentum_mult,
            ) * weight_mult;
            let kb_vec = knockback_vector(kb_mag, hb.angle_deg, hb.facing);
            let hitstop_frames = compute_hitstop_frames(kb_mag);
            let hitstun_secs = (0.12 + kb_mag / 1350.0).clamp(0.12, 0.72);

            target_body.velocity = kb_vec;
            target_body.ground_speed = kb_vec.x;
            if kb_vec.y > 0.0 || target_body.state == MovementState::Grounded {
                target_body.state = MovementState::Airborne;
                target_body.on_one_way = false;
                target_body.coyote_timer = 0.0;
                target_body.ground_normal = Vec2::Y;
                target_body.visual_angle = 0.0;
            }

            commands
                .entity(target_entity)
                .insert((Hitstun::new(hitstun_secs), Hitstop::from_frames(hitstop_frames)));

            // Apply matching impact freeze-frames to melee attacker (projectiles don't freeze shooter)
            if hb.momentum_mult > 0.0 {
                attacker_hitstops.push((hb.owner, hitstop_frames));
            }
            if let Some(recoil_vy) = hb.recoil_bounce_vy {
                attacker_recoil_updates.push((hb.owner, recoil_vy));
            }

            hb.hit_entities.push(target_entity);

            let hit_pos = hb_pos.lerp(hurt_pos, 0.5);
            telemetry.target_percent = percent.0;
            telemetry.last_knockback = kb_mag;
            telemetry.last_damage = hb.damage;
            telemetry.last_hitstop_frames = hitstop_frames;
            telemetry.has_hit = true;

            hit_events.write(CombatHitEvent {
                attacker: hb.owner,
                target: target_entity,
                hit_pos,
                damage: hb.damage,
                target_percent_after: percent.0,
                knockback_mag: kb_mag,
                knockback_vec: kb_vec,
                hitstop_frames,
                is_charged: hb.is_charged,
            });

            if !hb.piercing {
                consumed_non_piercing = true;
                break;
            }
        }

        if consumed_non_piercing {
            commands.entity(hb_entity).despawn();
        }
    }

    for (attacker, frames) in attacker_hitstops {
        if let Ok(mut ec) = commands.get_entity(attacker) {
            ec.insert(Hitstop::from_frames(frames));
        }
    }

    for (attacker, recoil_vy) in attacker_recoil_updates {
        if let Ok((_, _, _, mut body, _, _, _)) = targets.get_mut(attacker) {
            body.velocity.y = body.velocity.y.max(recoil_vy);
            body.state = MovementState::Airborne;
        }
    }
}

/// Respawns Phase 3 combat training dummies cleanly if launched out of the arena bounds.
fn respawn_launched_dummies(
    mut commands: Commands,
    mut dummies: Query<(
        Entity,
        &CombatDummy,
        &mut KinematicBody,
        &mut DamagePercent,
        &mut Transform,
    )>,
) {
    for (entity, dummy, mut body, mut percent, mut tf) in &mut dummies {
        let out_of_bounds =
            body.position.y < -360.0 || (body.position - dummy.home_pos).length() > 540.0;
        if out_of_bounds {
            body.position = dummy.home_pos;
            body.velocity = Vec2::ZERO;
            body.ground_speed = 0.0;
            body.state = MovementState::Airborne;
            percent.0 = 0.0;
            tf.translation.x = dummy.home_pos.x;
            tf.translation.y = dummy.home_pos.y;
            commands
                .entity(entity)
                .remove::<Hitstun>()
                .remove::<Hitstop>()
                .insert(Invulnerable::new(0.6));
        }
    }
}
