//! Custom 2D kinematic physics, slope alignment, Sonic 360° loop spline traversal,
//! Mega Man variable jump / coyote time / jump buffering / wall-jump, and Smash
//! one-way drop-through platforms.
//!
//! Architectural invariant: this module is completely decoupled from rendering,
//! windows, and textures so all simulation tests run deterministically in
//! `FixedUpdate` (60 Hz) with `MinimalPlugins`.

pub mod geometry;
pub mod loop_spline;

use bevy::prelude::*;
use leafwing_input_manager::prelude::ActionState;

use crate::combat::{Hitstop, Hitstun};
use crate::input::PlayerAction;
use crate::player::Player;
use crate::state::AppState;

pub use geometry::{
    BoostPad, GroundHit, MAX_WALKABLE_SLOPE_COS, MovingPlatform, OneWayPlatform, SlopeSegment,
    SolidRect, SpikeHazard,
};
pub use loop_spline::{LoopSpline, SplineSample};

/// Tunable kinematic physics parameters for Sonic flow + Mega Man precision + Smash feel.
#[derive(Resource, Debug, Clone)]
pub struct PhysicsConfig {
    /// Downward gravitational acceleration (`px/s^2`).
    pub gravity: f32,
    /// Terminal free-fall speed (`px/s`).
    pub max_fall_speed: f32,
    /// Standard horizontal run speed (`px/s`).
    pub run_max_speed: f32,
    /// High-speed Sonic sprint / boost top speed (`px/s`).
    pub sprint_max_speed: f32,
    /// Ground horizontal acceleration (`px/s^2`).
    pub ground_accel: f32,
    /// Extra acceleration when holding Spin Dash / Boost (`px/s^2`).
    pub boost_accel: f32,
    /// Ground deceleration when no horizontal input is pressed (`px/s^2`).
    pub ground_friction: f32,
    /// Ground turning/skid deceleration (`px/s^2`).
    pub ground_turn_decel: f32,
    /// Air horizontal acceleration (`px/s^2`).
    pub air_accel: f32,
    /// Air drag when no input is pressed (`px/s^2`).
    pub air_drag: f32,
    /// Slope gravity scale (`a = -gravity * sin(theta) * slope_gravity_mult`).
    pub slope_gravity_mult: f32,
    /// Initial upward jump velocity (`px/s`).
    pub jump_velocity: f32,
    /// Multiplier applied to upward velocity on early Jump release (50% per SPEC).
    pub variable_jump_cut: f32,
    /// Coyote time window after walking off a ledge (`0.09s` per SPEC).
    pub coyote_time: f32,
    /// Pre-landing jump buffer window (`0.10s` per SPEC).
    pub jump_buffer_time: f32,
    /// Duration one-way drop-through collision stays disabled (`0.15s` per SPEC).
    pub drop_through_duration: f32,
    /// Max downward speed while sliding down a vertical wall (`px/s`).
    pub wall_slide_max_fall: f32,
    /// Horizontal wall-jump launch speed away from wall (`px/s`).
    pub wall_jump_vx: f32,
    /// Vertical wall-jump launch speed (`px/s`).
    pub wall_jump_vy: f32,
    /// Grace window to trigger a wall-jump after nudging away from wall (`s`).
    pub wall_coyote_time: f32,
    /// Brief steering lock after wall-jump so the player arcs cleanly off vertical shafts (`s`).
    pub wall_jump_lock_time: f32,
    /// Tangential gravity multiplier while locked to a 360° loop spline.
    pub loop_gravity_factor: f32,
}

impl Default for PhysicsConfig {
    fn default() -> Self {
        Self {
            gravity: 1180.0,
            max_fall_speed: 620.0,
            run_max_speed: 250.0,
            sprint_max_speed: 540.0,
            ground_accel: 960.0,
            boost_accel: 1350.0,
            ground_friction: 820.0,
            ground_turn_decel: 1650.0,
            air_accel: 1400.0,
            air_drag: 160.0,
            slope_gravity_mult: 1.0,
            jump_velocity: 430.0,
            variable_jump_cut: 0.5,
            coyote_time: 0.09,
            jump_buffer_time: 0.10,
            drop_through_duration: 0.15,
            wall_slide_max_fall: 110.0,
            wall_jump_vx: 220.0,
            wall_jump_vy: 435.0,
            wall_coyote_time: 0.09,
            wall_jump_lock_time: 0.055,
            loop_gravity_factor: 0.22,
        }
    }
}

/// Authoritative kinematic movement state for the player (and future fighters).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum MovementState {
    #[default]
    Airborne,
    Grounded,
    WallSliding {
        /// `-1` = wall on left, `+1` = wall on right.
        wall_dir: i8,
    },
    LoopLocked {
        spline_id: u32,
        progress: f32,
        speed: f32,
    },
}

/// Frame-sampled player control intent consumed deterministically inside `FixedUpdate`.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct PlayerIntent {
    pub move_axis: Vec2,
    pub jump_just_pressed: bool,
    pub jump_held: bool,
    pub jump_just_released: bool,
    pub boost_held: bool,
    pub down_just_pressed: bool,
    pub melee_just_pressed: bool,
    pub special_just_pressed: bool,
    pub buster_just_pressed: bool,
    pub buster_held: bool,
    prev_down_held: bool,
}

/// Marker opting an entity out of Leafwing action-state copying so headless simulation
/// tests can drive [`PlayerIntent`] directly with exact frame-level precision.
#[derive(Component, Debug, Default)]
pub struct ManualPlayerIntent;

/// Custom 2D kinematic physics body (no rigid-body solver).
#[derive(Component, Debug, Clone)]
pub struct KinematicBody {
    /// Center position of the AABB collider in virtual world pixels.
    pub position: Vec2,
    /// Current velocity (`px/s`).
    pub velocity: Vec2,
    /// Collider half-width and half-height (`px`).
    pub half_extents: Vec2,
    /// Current movement state (`Grounded`, `Airborne`, `WallSliding`, `LoopLocked`).
    pub state: MovementState,
    /// Signed speed along the ground surface tangent (`px/s`).
    pub ground_speed: f32,
    /// Unit surface normal under feet when grounded (or `Vec2::Y` airborne).
    pub ground_normal: Vec2,
    /// Visual tilt angle in radians matching slope inclination or 360° loop tangent.
    pub visual_angle: f32,
    /// Facing direction (`+1.0` right, `-1.0` left).
    pub facing: f32,
    /// Remaining coyote jump window (`s`).
    pub coyote_timer: f32,
    /// Remaining pre-landing jump buffer (`s`).
    pub jump_buffer_timer: f32,
    /// Remaining time during which one-way drop-through platforms are ignored (`s`).
    pub drop_through_timer: f32,
    /// Remaining grace window to wall-jump after touching a vertical wall (`s`).
    pub wall_coyote_timer: f32,
    /// Last contacted wall side (`-1` left, `+1` right).
    pub last_wall_dir: i8,
    /// Brief steering lock after wall-jumping (`s`).
    pub wall_jump_lock_timer: f32,
    /// Cooldown preventing immediate re-capture into a loop right after exiting (`s`).
    pub loop_exit_cooldown: f32,
    /// Ensures variable-jump 50% upward velocity cut applies at most once per jump ascent.
    pub jump_cut_applied: bool,
    /// Whether the player is currently standing on a one-way platform.
    pub on_one_way: bool,
}

impl Default for KinematicBody {
    fn default() -> Self {
        Self {
            position: Vec2::ZERO,
            velocity: Vec2::ZERO,
            half_extents: Vec2::new(10.0, 18.0),
            state: MovementState::Airborne,
            ground_speed: 0.0,
            ground_normal: Vec2::Y,
            visual_angle: 0.0,
            facing: 1.0,
            coyote_timer: 0.0,
            jump_buffer_timer: 0.0,
            drop_through_timer: 0.0,
            wall_coyote_timer: 0.0,
            last_wall_dir: 0,
            wall_jump_lock_timer: 0.0,
            loop_exit_cooldown: 0.0,
            jump_cut_applied: false,
            on_one_way: false,
        }
    }
}

impl KinematicBody {
    pub fn at(position: Vec2) -> Self {
        Self {
            position,
            ..default()
        }
    }

    pub fn feet_y(&self) -> f32 {
        self.position.y - self.half_extents.y
    }

    /// Current scalar speed (`px/s`) useful for Sonic afterimages & Smash momentum scaling.
    pub fn speed(&self) -> f32 {
        match self.state {
            MovementState::LoopLocked { speed, .. } => speed.abs(),
            MovementState::Grounded => self.ground_speed.abs().max(self.velocity.length()),
            MovementState::Airborne | MovementState::WallSliding { .. } => self.velocity.length(),
        }
    }
}

/// System set sampling input actions into [`PlayerIntent`] at the start of `FixedUpdate`.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct InputSyncSystems;

/// System set ordering deterministic 60 Hz gameplay physics inside `FixedUpdate`.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PhysicsSystems;

/// Plugin registering deterministic 60 Hz kinematic physics systems.
pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PhysicsConfig>()
            .add_systems(
                FixedUpdate,
                sync_player_intent_from_actions
                    .in_set(InputSyncSystems)
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(
                FixedUpdate,
                (advance_moving_platforms, step_kinematic_physics)
                    .chain()
                    .in_set(PhysicsSystems)
                    .run_if(in_state(AppState::Playing)),
            );
    }
}

#[allow(clippy::type_complexity)]
fn sync_player_intent_from_actions(
    mut query: Query<
        (&ActionState<PlayerAction>, &mut PlayerIntent),
        (With<Player>, Without<ManualPlayerIntent>),
    >,
) {
    for (actions, mut intent) in &mut query {
        let axis = actions.clamped_axis_pair(&PlayerAction::Move);
        let down_held = axis.y < -0.5;
        intent.move_axis = axis;
        intent.jump_just_pressed = actions.just_pressed(&PlayerAction::Jump);
        intent.jump_held = actions.pressed(&PlayerAction::Jump);
        intent.jump_just_released = actions.just_released(&PlayerAction::Jump);
        intent.boost_held = actions.pressed(&PlayerAction::SpinDash);
        intent.down_just_pressed = down_held && !intent.prev_down_held;
        intent.prev_down_held = down_held;
        intent.melee_just_pressed = actions.just_pressed(&PlayerAction::MeleeAttack);
        intent.special_just_pressed = actions.just_pressed(&PlayerAction::Special);
        intent.buster_just_pressed = actions.just_pressed(&PlayerAction::Buster);
        intent.buster_held = actions.pressed(&PlayerAction::Buster);
    }
}

/// Advances oscillating [`MovingPlatform`] entities deterministically and carries grounded riders.
fn advance_moving_platforms(
    fixed_time: Res<Time<Fixed>>,
    mut platforms: Query<(
        &mut MovingPlatform,
        &mut OneWayPlatform,
        Option<&mut Transform>,
    )>,
    mut riders: Query<(&mut KinematicBody, Option<&Hitstop>)>,
) {
    let dt = fixed_time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    let mut moved: Vec<(OneWayPlatform, Vec2)> = Vec::new();
    for (mut mp, mut plat, maybe_tf) in &mut platforms {
        let prev = plat.center;
        mp.phase += mp.speed * dt;
        let next = mp.current_center();
        let delta = next - prev;
        mp.last_delta = delta;
        plat.center = next;
        if let Some(mut tf) = maybe_tf {
            tf.translation.x = next.x;
            tf.translation.y = next.y;
        }
        moved.push((
            OneWayPlatform {
                center: prev,
                half_width: plat.half_width,
                thickness: plat.thickness,
            },
            delta,
        ));
    }

    if moved.is_empty() {
        return;
    }

    for (mut body, hitstop) in &mut riders {
        if hitstop.is_some_and(Hitstop::is_active) || body.state != MovementState::Grounded {
            continue;
        }
        let feet_y = body.feet_y();
        for &(prev_plat, delta) in &moved {
            let on_x = body.position.x + body.half_extents.x * 0.75 >= prev_plat.min_x()
                && body.position.x - body.half_extents.x * 0.75 <= prev_plat.max_x();
            let on_y = (feet_y - prev_plat.top_y()).abs() <= 4.5;
            if on_x && on_y {
                body.position += delta;
                break;
            }
        }
    }
}

/// Executes one deterministic `FixedUpdate` kinematic simulation step for all bodies.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn step_kinematic_physics(
    fixed_time: Res<Time<Fixed>>,
    config: Res<PhysicsConfig>,
    solids_q: Query<&SolidRect>,
    slopes_q: Query<&SlopeSegment>,
    one_ways_q: Query<&OneWayPlatform>,
    boost_pads_q: Query<&BoostPad>,
    loops_q: Query<&LoopSpline>,
    mut bodies: Query<(
        &mut KinematicBody,
        &mut Transform,
        Option<&PlayerIntent>,
        Option<&Hitstop>,
        Option<&Hitstun>,
    )>,
) {
    let dt = fixed_time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    let solids: Vec<SolidRect> = solids_q.iter().copied().collect();
    let slopes: Vec<SlopeSegment> = slopes_q.iter().copied().collect();
    let one_ways: Vec<OneWayPlatform> = one_ways_q.iter().copied().collect();
    let boost_pads: Vec<BoostPad> = boost_pads_q.iter().copied().collect();
    let loops: Vec<&LoopSpline> = loops_q.iter().collect();

    for (mut body, mut transform, maybe_intent, hitstop, hitstun) in &mut bodies {
        // Active Hitstop pauses ALL physics integration until the freeze-frame timer expires.
        if hitstop.is_some_and(Hitstop::is_active) {
            continue;
        }

        if hitstun.is_some_and(Hitstun::is_active) {
            step_hitstun_body(&mut body, dt, &config, &solids, &slopes, &one_ways);
        } else {
            let intent = maybe_intent.copied().unwrap_or_default();
            step_single_body(
                &mut body, &intent, dt, &config, &solids, &slopes, &one_ways, &boost_pads, &loops,
            );
        }

        // Sync Transform (rendering layers snap visual sprites to integer virtual pixels).
        transform.translation.x = body.position.x;
        transform.translation.y = body.position.y;
    }
}

/// Deterministic Smash knockback/tumble integration while an entity is in active [`Hitstun`].
fn step_hitstun_body(
    body: &mut KinematicBody,
    dt: f32,
    cfg: &PhysicsConfig,
    solids: &[SolidRect],
    slopes: &[SlopeSegment],
    one_ways: &[OneWayPlatform],
) {
    let prev_feet_y = body.feet_y();
    let was_grounded = body.state == MovementState::Grounded;

    body.velocity.y = (body.velocity.y - cfg.gravity * dt).max(-cfg.max_fall_speed * 1.4);
    body.velocity.x = move_toward(body.velocity.x, 0.0, cfg.air_drag * 0.55 * dt);
    body.position += body.velocity * dt;

    let (resolved_x, wall_hit) =
        geometry::resolve_horizontal_solids(body.position, body.half_extents, solids, 2.0);
    body.position.x = resolved_x;
    if wall_hit.is_some() {
        // Crisp Smash wall-bounce on high-speed impacts
        body.velocity.x = -body.velocity.x * 0.55;
    }

    let (resolved_y, hit_ceiling) =
        geometry::resolve_ceiling_solids(body.position, body.half_extents, body.velocity.y, solids);
    body.position.y = resolved_y;
    if hit_ceiling {
        body.velocity.y = -body.velocity.y * 0.45;
    }

    if let Some(hit) = geometry::cast_dual_foot_sensors(
        body.position,
        body.half_extents,
        prev_feet_y,
        body.velocity.y,
        was_grounded,
        false,
        solids,
        slopes,
        one_ways,
    ) {
        body.position.y = hit.surface_y + body.half_extents.y;
        body.ground_normal = hit.normal;
        body.on_one_way = hit.is_one_way;
        body.velocity.y = 0.0;
        body.velocity.x = move_toward(body.velocity.x, 0.0, cfg.ground_friction * 0.6 * dt);
        body.ground_speed = body.velocity.x;
        body.state = MovementState::Grounded;
    } else {
        body.state = MovementState::Airborne;
        body.on_one_way = false;
    }
}

#[allow(clippy::too_many_arguments)]
fn step_single_body(
    body: &mut KinematicBody,
    intent: &PlayerIntent,
    dt: f32,
    cfg: &PhysicsConfig,
    solids: &[SolidRect],
    slopes: &[SlopeSegment],
    one_ways: &[OneWayPlatform],
    boost_pads: &[BoostPad],
    loops: &[&LoopSpline],
) {
    // 1. Advance state timers deterministically
    body.coyote_timer = (body.coyote_timer - dt).max(0.0);
    body.jump_buffer_timer = (body.jump_buffer_timer - dt).max(0.0);
    body.drop_through_timer = (body.drop_through_timer - dt).max(0.0);
    body.wall_coyote_timer = (body.wall_coyote_timer - dt).max(0.0);
    body.wall_jump_lock_timer = (body.wall_jump_lock_timer - dt).max(0.0);
    body.loop_exit_cooldown = (body.loop_exit_cooldown - dt).max(0.0);

    if intent.jump_just_pressed {
        body.jump_buffer_timer = cfg.jump_buffer_time;
    }

    // 2. Smash drop-through platform trigger (tap Down grounded, or hold Down while falling)
    let down_held = intent.move_axis.y < -0.5;
    if (body.state == MovementState::Grounded && body.on_one_way && (intent.down_just_pressed || down_held))
        || (body.state == MovementState::Airborne && body.velocity.y < 0.0 && down_held)
    {
        if body.drop_through_timer <= 0.0 {
            body.drop_through_timer = cfg.drop_through_duration;
        }
        if body.state == MovementState::Grounded && body.on_one_way {
            body.state = MovementState::Airborne;
            body.on_one_way = false;
            body.coyote_timer = 0.0;
            body.position.y -= 2.0;
            body.velocity.y = body.velocity.y.min(-80.0);
        }
    }

    // 3. Handle active 360° LoopSpline traversal
    if let MovementState::LoopLocked {
        spline_id,
        mut progress,
        mut speed,
    } = body.state
    {
        if let Some(spline) = loops.iter().find(|s| s.id == spline_id) {
            let sample = spline.sample(progress);

            // Apply gentle tangential gravity (plus minor drive when holding boost/forward)
            let tangent_grav = -cfg.gravity * sample.tangent.y * cfg.loop_gravity_factor;
            let boost_drive = if intent.boost_held { 180.0 } else { 0.0 };
            speed = (speed + (tangent_grav + boost_drive) * dt).max(0.0);

            // Check apex stall condition: upside-down / upper arc AND speed below stall threshold
            if sample.normal.y < -0.25 && speed < spline.stall_speed {
                body.state = MovementState::Airborne;
                body.velocity = sample.tangent * speed;
                body.ground_normal = Vec2::Y;
                body.visual_angle = 0.0;
                body.loop_exit_cooldown = 0.35;
                return;
            }

            // Advance uniformly along arc-length-parameterized spline
            progress += (speed * dt) / spline.total_length();
            if progress >= 1.0 {
                let exit = spline.sample(1.0);
                body.position = exit.position + exit.normal * body.half_extents.y;
                body.velocity = exit.tangent * speed;
                body.ground_speed = if exit.tangent.x >= 0.0 { speed } else { -speed };
                body.ground_normal = if exit.normal.y > 0.0 {
                    exit.normal
                } else {
                    Vec2::Y
                };
                body.visual_angle = exit.angle_rad;
                body.facing = if exit.tangent.x < -0.05 { -1.0 } else { 1.0 };
                body.loop_exit_cooldown = 0.40;
                body.state = if exit.tangent.y > 0.35 {
                    MovementState::Airborne
                } else {
                    MovementState::Grounded
                };
                return;
            }

            let next_sample = spline.sample(progress);
            body.position = next_sample.position + next_sample.normal * body.half_extents.y;
            body.velocity = next_sample.tangent * speed;
            body.ground_speed = speed;
            body.ground_normal = next_sample.normal;
            body.visual_angle = next_sample.angle_rad;
            body.state = MovementState::LoopLocked {
                spline_id,
                progress,
                speed,
            };
            return;
        } else {
            body.state = MovementState::Airborne;
            body.visual_angle = 0.0;
        }
    }

    // 4. Boost pads (Sonic speed boosters)
    for pad in boost_pads {
        if pad.overlaps(body.position, body.half_extents) {
            let current_along = body.velocity.dot(pad.direction);
            if current_along < pad.target_speed {
                body.velocity = pad.direction * pad.target_speed;
                body.ground_speed = if pad.direction.x >= 0.0 {
                    pad.target_speed
                } else {
                    -pad.target_speed
                };
                if pad.direction.x.abs() > 0.1 {
                    body.facing = pad.direction.x.signum();
                }
            }
        }
    }

    // 5. Check 360° LoopSpline entry triggers (requires entry speed >= spline.min_speed)
    if body.loop_exit_cooldown <= 0.0 {
        let feet_pos = Vec2::new(body.position.x, body.feet_y());
        for spline in loops {
            let entry_pt = spline.entry_point();
            if feet_pos.distance(entry_pt) <= spline.entry_radius {
                let entry_tan = spline.entry_tangent();
                let entry_speed = body.velocity.dot(entry_tan).max(
                    if body.state == MovementState::Grounded {
                        body.ground_speed * entry_tan.x.signum()
                    } else {
                        0.0
                    },
                );
                if entry_speed >= spline.min_speed {
                    let initial_progress = ((entry_speed * dt) / spline.total_length()).min(0.1);
                    let s = spline.sample(initial_progress);
                    body.position = s.position + s.normal * body.half_extents.y;
                    body.velocity = s.tangent * entry_speed;
                    body.ground_speed = entry_speed;
                    body.ground_normal = s.normal;
                    body.visual_angle = s.angle_rad;
                    body.state = MovementState::LoopLocked {
                        spline_id: spline.id,
                        progress: initial_progress,
                        speed: entry_speed,
                    };
                    return;
                }
            }
        }
    }

    // 6. Jump execution: Ground / Coyote Jump OR Wall Jump
    let can_ground_jump = body.state == MovementState::Grounded || body.coyote_timer > 0.0;
    let can_wall_jump =
        !can_ground_jump && body.wall_coyote_timer > 0.0 && body.last_wall_dir != 0;

    if body.jump_buffer_timer > 0.0 && can_ground_jump {
        // Launch along normal-biased upward vector (clean crisp vertical Mega Man jump)
        body.velocity.y = cfg.jump_velocity;
        if body.state == MovementState::Grounded {
            body.velocity.x = body.ground_speed * body.ground_normal.y.max(0.75);
        }
        body.state = MovementState::Airborne;
        body.on_one_way = false;
        body.coyote_timer = 0.0;
        body.jump_buffer_timer = 0.0;
        body.jump_cut_applied = false;
        body.ground_normal = Vec2::Y;
        body.visual_angle = 0.0;
    } else if body.jump_buffer_timer > 0.0 && can_wall_jump {
        let away = -(body.last_wall_dir as f32);
        body.velocity.x = away * cfg.wall_jump_vx;
        body.velocity.y = cfg.wall_jump_vy;
        body.facing = away;
        body.state = MovementState::Airborne;
        body.wall_coyote_timer = 0.0;
        body.wall_jump_lock_timer = cfg.wall_jump_lock_time;
        body.jump_buffer_timer = 0.0;
        body.jump_cut_applied = false;
        body.ground_normal = Vec2::Y;
        body.visual_angle = 0.0;
    }

    // Variable jump height: releasing Jump early cuts upward velocity by 50%
    if intent.jump_just_released
        && matches!(
            body.state,
            MovementState::Airborne | MovementState::WallSliding { .. }
        )
        && body.velocity.y > 0.0
        && !body.jump_cut_applied
    {
        body.velocity.y *= cfg.variable_jump_cut;
        body.jump_cut_applied = true;
    }

    // 7. Horizontal / Slope acceleration
    let move_x = if body.wall_jump_lock_timer > 0.0 {
        0.0
    } else if intent.move_axis.x.abs() > 0.12 {
        intent.move_axis.x
    } else {
        0.0
    };

    if move_x.abs() > 0.0 {
        body.facing = move_x.signum();
    }

    let was_grounded = body.state == MovementState::Grounded;
    let prev_feet_y = body.feet_y();

    if was_grounded {
        // Slope inclination theta (> 0 uphill right, < 0 downhill right)
        let theta = (-body.ground_normal.x).atan2(body.ground_normal.y);

        // Apply Sonic slope gravity: a = -g * sin(theta)
        let slope_accel = -cfg.gravity * theta.sin() * cfg.slope_gravity_mult;
        body.ground_speed += slope_accel * dt;

        let target_max = if intent.boost_held {
            cfg.sprint_max_speed
        } else {
            cfg.run_max_speed
        };
        let accel = if intent.boost_held {
            cfg.boost_accel
        } else {
            cfg.ground_accel
        };

        if move_x != 0.0 {
            let input_dir = move_x.signum();
            if body.ground_speed != 0.0 && body.ground_speed.signum() != input_dir {
                // Crisp turn/skid deceleration
                body.ground_speed = move_toward(body.ground_speed, 0.0, cfg.ground_turn_decel * dt);
            } else {
                // Accelerate up to target_max without artificially clamping downhill/boost overspeed
                let desired = move_x * target_max;
                if (desired > 0.0 && body.ground_speed < desired)
                    || (desired < 0.0 && body.ground_speed > desired)
                {
                    body.ground_speed = move_toward(body.ground_speed, desired, accel * dt);
                }
            }
        } else if intent.boost_held {
            // Holding Spin Dash / Boost accelerates forward along facing direction
            let desired = body.facing * cfg.sprint_max_speed;
            body.ground_speed = move_toward(body.ground_speed, desired, cfg.boost_accel * dt);
        } else if theta.abs() < 0.05 {
            // Flat ground friction
            body.ground_speed = move_toward(body.ground_speed, 0.0, cfg.ground_friction * dt);
        } else {
            // Minimal rolling drag on slopes so Sonic downhill gravity builds momentum briskly
            body.ground_speed = move_toward(body.ground_speed, 0.0, cfg.ground_friction * 0.08 * dt);
        }

        body.ground_speed = body
            .ground_speed
            .clamp(-cfg.sprint_max_speed * 1.25, cfg.sprint_max_speed * 1.25);

        let tangent = Vec2::new(theta.cos(), theta.sin());
        body.velocity = tangent * body.ground_speed;
    } else {
        // Airborne / Wall-Sliding horizontal air control (snappy Mega Man X air reversal)
        let air_max = cfg.run_max_speed.max(body.velocity.x.abs());
        if move_x != 0.0 {
            let desired = move_x * cfg.run_max_speed;
            if body.velocity.x != 0.0 && body.velocity.x.signum() != move_x.signum() {
                body.velocity.x = move_toward(body.velocity.x, desired, cfg.air_accel * 1.55 * dt);
            } else if (desired > 0.0 && body.velocity.x < desired)
                || (desired < 0.0 && body.velocity.x > desired)
            {
                body.velocity.x = move_toward(body.velocity.x, desired, cfg.air_accel * dt);
            }
        } else {
            body.velocity.x = move_toward(body.velocity.x, 0.0, cfg.air_drag * dt);
        }
        body.velocity.x = body.velocity.x.clamp(-air_max, air_max);

        // Gravity
        body.velocity.y = (body.velocity.y - cfg.gravity * dt).max(-cfg.max_fall_speed);
    }

    // 8. Integrate position + resolve solid walls & ceilings
    body.position += body.velocity * dt;

    let step_clearance = if was_grounded { 8.0 } else { 2.0 };
    let (resolved_x, wall_hit) =
        geometry::resolve_horizontal_solids(body.position, body.half_extents, solids, step_clearance);
    body.position.x = resolved_x;
    if wall_hit.is_some() {
        body.velocity.x = 0.0;
        if was_grounded {
            body.ground_speed = 0.0;
        }
    }

    let (resolved_y, hit_ceiling) =
        geometry::resolve_ceiling_solids(body.position, body.half_extents, body.velocity.y, solids);
    body.position.y = resolved_y;
    if hit_ceiling {
        body.velocity.y = 0.0;
    }

    // 9. Cast dual downward foot sensors for ground / slope / one-way platform snap
    let drop_active = body.drop_through_timer > 0.0;
    if let Some(hit) = geometry::cast_dual_foot_sensors(
        body.position,
        body.half_extents,
        prev_feet_y,
        body.velocity.y,
        was_grounded,
        drop_active,
        solids,
        slopes,
        one_ways,
    ) {
        body.position.y = hit.surface_y + body.half_extents.y;
        body.ground_normal = hit.normal;
        body.on_one_way = hit.is_one_way;

        let theta = (-hit.normal.x).atan2(hit.normal.y);
        body.visual_angle = theta;

        if !was_grounded {
            // Project landing velocity cleanly onto the slope tangent (Sonic momentum preservation!)
            let tangent = Vec2::new(theta.cos(), theta.sin());
            body.ground_speed = body.velocity.dot(tangent);
            body.jump_cut_applied = false;
        }

        let tangent = Vec2::new(theta.cos(), theta.sin());
        body.velocity = tangent * body.ground_speed;
        body.state = MovementState::Grounded;
        body.coyote_timer = cfg.coyote_time;
        body.wall_coyote_timer = 0.0;

        // Immediately execute buffered jump if player pressed Jump within 0.10s before landing
        if body.jump_buffer_timer > 0.0 {
            body.velocity.y = cfg.jump_velocity;
            body.velocity.x = body.ground_speed * hit.normal.y.max(0.75);
            body.state = MovementState::Airborne;
            body.on_one_way = false;
            body.coyote_timer = 0.0;
            body.jump_buffer_timer = 0.0;
            body.jump_cut_applied = false;
            body.ground_normal = Vec2::Y;
            body.visual_angle = 0.0;
        }
    } else {
        // Left ground -> transition to Airborne (keep coyote_timer active when walking off a ledge!)
        if was_grounded {
            body.coyote_timer = cfg.coyote_time;
            body.on_one_way = false;
        }
        body.ground_normal = Vec2::Y;
        body.visual_angle = move_toward(body.visual_angle, 0.0, 10.0 * dt);

        // Probe walls for Mega Man wall-slide / wall-jump
        if let Some(wall_dir) =
            geometry::probe_wall_contact(body.position, body.half_extents, move_x, solids)
        {
            body.wall_coyote_timer = cfg.wall_coyote_time;
            body.last_wall_dir = wall_dir;
            let pushing_wall = move_x * (wall_dir as f32) > 0.1;
            if body.velocity.y <= 0.0 && (pushing_wall || wall_hit == Some(wall_dir)) {
                body.velocity.y = body.velocity.y.max(-cfg.wall_slide_max_fall);
                body.state = MovementState::WallSliding { wall_dir };
            } else {
                body.state = MovementState::Airborne;
            }
        } else {
            body.state = MovementState::Airborne;
        }
    }
}

fn move_toward(current: f32, target: f32, max_delta: f32) -> f32 {
    if (target - current).abs() <= max_delta {
        target
    } else {
        current + (target - current).signum() * max_delta
    }
}
