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

use crate::combat::{
    AirDodgeState, AttackState, CounterStance, GroundDodgeState, Hitstop, Hitstun, Hurtbox,
    MeteorDiveState, ShieldState, SmashChargeState,
};
use crate::enemies::WallTurret;
use crate::input::{
    ControlConfig, DpadCalibration, DpadWizardState, LiveRawGamepadState, PlayerAction,
    ShieldButtonMode,
};
use crate::player::{Player, PlayerRespawnState};
use crate::state::AppState;

pub use geometry::{
    BoostPad, BreakableWall, CheckpointBeacon, CollectibleItem, CollectibleKind, GroundHit,
    HazardBeamPhase, JumpPad, MAX_WALKABLE_SLOPE_COS, MovingPlatform, OneWayPlatform, SlopeSegment,
    SolidRect, SpikeHazard, TimedHazardBeam,
};
pub use loop_spline::{LoopSpline, SplineSample};

/// Gameplay movement / defense feedback event emitted deterministically in `FixedUpdate`
/// for SFX (`GameAudioPlugin`) and dust/spark particles (`RenderingPlugin`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovementFxKind {
    Jump,
    WallJump,
    Land,
    BoostPad,
    JumpPad,
    Checkpoint,
    CollectCore,
    CollectRelic,
    CollectHeal,
    WallBreak,
    SpinDashRev,
    SpinDashRelease,
    ShieldBlock,
    AirDodge,
    RollDodge,
    SpotDodge,
    FastFall,
    Taunt,
}

/// Message emitted on crisp kinematic & defensive state transitions.
#[derive(Message, Debug, Clone, Copy)]
pub struct MovementEvent {
    pub entity: Entity,
    pub kind: MovementFxKind,
    pub pos: Vec2,
    pub dir: Vec2,
}

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
    /// Controlled Smash double-jump upward velocity (`px/s`).
    pub double_jump_velocity: f32,
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
            double_jump_velocity: 405.0,
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
    /// Crouched Sonic Spin Dash rev-up (`charge` in `0.0..=1.0`); releases into a high-speed burst.
    SpinCharging {
        charge: f32,
    },
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
    pub boost_just_pressed: bool,
    pub guard_held: bool,
    pub guard_just_pressed: bool,
    pub down_just_pressed: bool,
    pub left_just_pressed: bool,
    pub right_just_pressed: bool,
    pub up_just_pressed: bool,
    pub melee_just_pressed: bool,
    pub melee_held: bool,
    pub melee_just_released: bool,
    pub heavy_just_pressed: bool,
    pub heavy_held: bool,
    pub c_stick_axis: Vec2,
    pub c_stick_just_flicked: bool,
    pub special_just_pressed: bool,
    pub special_held: bool,
    pub buster_just_pressed: bool,
    pub buster_held: bool,
    pub taunt_just_pressed: bool,
    prev_down_held: bool,
    prev_left_held: bool,
    prev_right_held: bool,
    prev_up_held: bool,
    prev_jump_held: bool,
    prev_boost_held: bool,
    prev_guard_held: bool,
    prev_melee_held: bool,
    prev_heavy_held: bool,
    prev_c_stick_active: bool,
    prev_special_held: bool,
    prev_buster_held: bool,
    prev_taunt_held: bool,
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
    /// Periodic visual/audio rev pulse timer while Spin Charging.
    pub spin_rev_fx_timer: f32,
    /// Remaining mid-air Smash double jumps (refreshes to `1` on landing or wall-slide).
    pub air_jumps_remaining: u8,
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
            spin_rev_fx_timer: 0.0,
            air_jumps_remaining: 0,
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
            MovementState::SpinCharging { .. } => 0.0,
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

/// Tracks whether analog LT/RT triggers have been observed at rest (`< 0.15`) so macOS
/// uncalibrated `0.50` resting-midpoint triggers never lock Shield or Boost on,
/// plus automatic simultaneous D-Pad Right / Shield conflict lockout!
#[derive(Resource, Debug, Default)]
pub struct GamepadTriggerCalibration {
    pub lt2_calibrated: bool,
    pub rt2_calibrated: bool,
    /// Latched true when D-Pad Right and a gamepad Shield input rise on the exact same tick
    /// (protects against mis-mapped hardware/OS profiles where D-Pad Right also fires LB/LT!).
    pub dpad_right_shield_lockout: bool,
}

/// Plugin registering deterministic 60 Hz kinematic physics systems.
pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PhysicsConfig>()
            .init_resource::<GamepadTriggerCalibration>()
            .add_message::<MovementEvent>()
            .add_systems(
                FixedUpdate,
                sync_player_intent_from_actions
                    .in_set(InputSyncSystems)
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(
                FixedUpdate,
                (
                    advance_moving_platforms,
                    step_kinematic_physics,
                    apply_fighter_pushbox_separation,
                )
                    .chain()
                    .in_set(PhysicsSystems)
                    .run_if(in_state(AppState::Playing)),
            );
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn sync_player_intent_from_actions(
    mut cal: ResMut<GamepadTriggerCalibration>,
    control_cfg: Res<ControlConfig>,
    dpad_cal: Res<DpadCalibration>,
    wizard: Res<DpadWizardState>,
    raw_gp: Res<LiveRawGamepadState>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    gamepads: Query<&Gamepad>,
    mut query: Query<
        (&ActionState<PlayerAction>, &mut PlayerIntent),
        (With<Player>, Without<ManualPlayerIntent>),
    >,
) {
    // Freeze player movement/actions while the 4-step D-Pad calibration wizard is open
    if wizard.active {
        for (_, mut intent) in &mut query {
            intent.move_axis = Vec2::ZERO;
            intent.jump_just_pressed = false;
            intent.jump_held = false;
            intent.jump_just_released = false;
            intent.boost_held = false;
            intent.boost_just_pressed = false;
            intent.guard_held = false;
            intent.guard_just_pressed = false;
            intent.down_just_pressed = false;
            intent.left_just_pressed = false;
            intent.right_just_pressed = false;
            intent.up_just_pressed = false;
            intent.melee_just_pressed = false;
            intent.melee_held = false;
            intent.melee_just_released = false;
            intent.heavy_just_pressed = false;
            intent.heavy_held = false;
            intent.c_stick_axis = Vec2::ZERO;
            intent.c_stick_just_flicked = false;
            intent.special_just_pressed = false;
            intent.special_held = false;
            intent.buster_just_pressed = false;
            intent.buster_held = false;
            intent.taunt_just_pressed = false;
        }
        return;
    }

    // Priority 1: User-calibrated raw D-Pad profile (fixes ANY hardware/OS HID remapping!)
    let mut direct_dpad = if dpad_cal.is_active() {
        dpad_cal.sample_vector(&raw_gp)
    } else {
        Vec2::ZERO
    };
    let mut direct_lt2_active = false;
    let mut direct_rt2_active = false;
    let allow_lt2_shield = matches!(
        control_cfg.shield_mode,
        ShieldButtonMode::LbAndLt | ShieldButtonMode::LtOnly
    );

    for gp in &gamepads {
        if direct_dpad == Vec2::ZERO {
            let mut dx = 0.0_f32;
            let mut dy = 0.0_f32;
            if gp.pressed(GamepadButton::DPadLeft) {
                dx -= 1.0;
            }
            if gp.pressed(GamepadButton::DPadRight) {
                dx += 1.0;
            }
            if gp.pressed(GamepadButton::DPadUp) {
                dy += 1.0;
            }
            if gp.pressed(GamepadButton::DPadDown) {
                dy -= 1.0;
            }
            if dx != 0.0 || dy != 0.0 {
                direct_dpad = Vec2::new(dx, dy);
            } else {
                let dpad_v = gp.dpad();
                if dpad_v.length_squared() > 0.04 {
                    direct_dpad = dpad_v;
                }
            }
        }

        if allow_lt2_shield && !dpad_cal.claims_button(GamepadButton::LeftTrigger2) {
            let lt2 = gp.get(GamepadButton::LeftTrigger2).unwrap_or(0.0);
            if lt2 < 0.15 {
                cal.lt2_calibrated = true;
            }
            if cal.lt2_calibrated && lt2 >= 0.78 {
                direct_lt2_active = true;
            }
        }

        if !dpad_cal.claims_button(GamepadButton::RightTrigger2) {
            let rt2 = gp.get(GamepadButton::RightTrigger2).unwrap_or(0.0);
            if rt2 < 0.15 {
                cal.rt2_calibrated = true;
            }
            if cal.rt2_calibrated && rt2 >= 0.78 {
                direct_rt2_active = true;
            }
        }
    }

    // If calibrated D-Pad claims LB/LeftTrigger, completely suppress LB from activating Shield!
    let lb_claimed_by_dpad = dpad_cal.claims_button(GamepadButton::LeftTrigger);
    let keyboard_guard_held = keys.as_ref().is_some_and(|k| {
        k.pressed(KeyCode::KeyQ) || k.pressed(KeyCode::KeyF) || k.pressed(KeyCode::ControlRight)
    });

    for (actions, mut intent) in &mut query {
        let raw_axis = if direct_dpad != Vec2::ZERO {
            direct_dpad
        } else {
            actions.clamped_axis_pair(&PlayerAction::Move)
        };
        // Apply crisp analog stick deadzone so controller drift never walks or turns on its own
        let axis = Vec2::new(
            if raw_axis.x.abs() >= 0.22 { raw_axis.x } else { 0.0 },
            if raw_axis.y.abs() >= 0.22 { raw_axis.y } else { 0.0 },
        );
        let down_held = axis.y < -0.55;
        let up_held = axis.y > 0.55;
        let left_held = axis.x < -0.55;
        let right_held = axis.x > 0.55;

        let up_just_pressed = up_held && !intent.prev_up_held;
        let jump_btn_held = actions.pressed(&PlayerAction::Jump);
        let jump_held = jump_btn_held || (control_cfg.tap_jump && up_held);
        let jump_just_pressed = (jump_btn_held && !intent.prev_jump_held)
            || (control_cfg.tap_jump && up_just_pressed);

        // Digital keys/bumpers report 1.0; analog triggers must pass rest-calibration >= 0.78
        let leafwing_boost = actions.pressed(&PlayerAction::SpinDash)
            && actions.clamped_button_value(&PlayerAction::SpinDash) >= 0.85;
        let boost_held = leafwing_boost || direct_rt2_active;

        let leafwing_guard = if lb_claimed_by_dpad {
            keyboard_guard_held
        } else {
            actions.pressed(&PlayerAction::Guard)
        };
        let raw_guard_held = leafwing_guard || direct_lt2_active;

        // Hardware/OS conflict guard: if D-Pad Right AND gamepad Shield both rise on the exact
        // same tick (and Shield wasn't already held to Roll-Dodge), suppress Shield until D-Pad Right releases!
        let dpad_right_pressed = direct_dpad.x > 0.5;
        let dpad_right_rising = dpad_right_pressed && !intent.prev_right_held;
        let guard_rising = raw_guard_held && !intent.prev_guard_held;
        if dpad_right_rising && guard_rising && !keyboard_guard_held {
            cal.dpad_right_shield_lockout = true;
        }
        if !dpad_right_pressed {
            cal.dpad_right_shield_lockout = false;
        }
        let guard_held = if cal.dpad_right_shield_lockout && !keyboard_guard_held {
            false
        } else {
            raw_guard_held
        };

        let melee_held = actions.pressed(&PlayerAction::MeleeAttack);
        let heavy_held = actions.pressed(&PlayerAction::HeavySmash);
        let raw_c_stick = actions.clamped_axis_pair(&PlayerAction::SmashStick);
        let c_stick_active = raw_c_stick.length_squared() >= 0.52 * 0.52;
        let c_stick_axis = if c_stick_active {
            raw_c_stick
        } else {
            Vec2::ZERO
        };
        let special_held = actions.pressed(&PlayerAction::Special);
        let buster_held = actions.pressed(&PlayerAction::Buster);
        let taunt_held = actions.pressed(&PlayerAction::Taunt);

        // Edge-detect strictly inside FixedUpdate so multi-tick render frames can NEVER double-fire an action!
        intent.move_axis = axis;
        intent.jump_just_pressed = jump_just_pressed;
        intent.jump_held = jump_held;
        intent.jump_just_released = !jump_btn_held && intent.prev_jump_held;
        intent.boost_held = boost_held;
        intent.boost_just_pressed = boost_held && !intent.prev_boost_held;
        intent.guard_held = guard_held;
        intent.guard_just_pressed = guard_held && !intent.prev_guard_held;
        intent.down_just_pressed = down_held && !intent.prev_down_held;
        intent.left_just_pressed = left_held && !intent.prev_left_held;
        intent.right_just_pressed = right_held && !intent.prev_right_held;
        intent.up_just_pressed = up_just_pressed;
        intent.melee_just_pressed = melee_held && !intent.prev_melee_held;
        intent.melee_held = melee_held;
        intent.melee_just_released = !melee_held && intent.prev_melee_held;
        intent.heavy_just_pressed = heavy_held && !intent.prev_heavy_held;
        intent.heavy_held = heavy_held;
        intent.c_stick_axis = c_stick_axis;
        intent.c_stick_just_flicked = c_stick_active && !intent.prev_c_stick_active;
        intent.special_just_pressed = special_held && !intent.prev_special_held;
        intent.special_held = special_held;
        intent.buster_just_pressed = buster_held && !intent.prev_buster_held;
        intent.buster_held = buster_held;
        intent.taunt_just_pressed = taunt_held && !intent.prev_taunt_held;

        intent.prev_down_held = down_held;
        intent.prev_left_held = left_held;
        intent.prev_right_held = right_held;
        intent.prev_up_held = up_held;
        intent.prev_jump_held = jump_btn_held;
        intent.prev_boost_held = boost_held;
        intent.prev_guard_held = guard_held;
        intent.prev_melee_held = melee_held;
        intent.prev_heavy_held = heavy_held;
        intent.prev_c_stick_active = c_stick_active;
        intent.prev_special_held = special_held;
        intent.prev_buster_held = buster_held;
        intent.prev_taunt_held = taunt_held;
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
    jump_pads_q: Query<&JumpPad>,
    loops_q: Query<&LoopSpline>,
    mut bodies: Query<
        (
            Entity,
            &mut KinematicBody,
            &mut Transform,
            Option<&PlayerIntent>,
            Option<&AttackState>,
            Option<&PlayerRespawnState>,
            Option<&Hitstop>,
            Option<&Hitstun>,
            Option<&ShieldState>,
            Option<&AirDodgeState>,
            Option<&GroundDodgeState>,
            Option<&SmashChargeState>,
            Option<&CounterStance>,
            Option<&MeteorDiveState>,
        ),
        Without<BreakableWall>,
    >,
    mut movement_events: MessageWriter<MovementEvent>,
) {
    let dt = fixed_time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    let solids: Vec<SolidRect> = solids_q.iter().copied().collect();
    let slopes: Vec<SlopeSegment> = slopes_q.iter().copied().collect();
    let one_ways: Vec<OneWayPlatform> = one_ways_q.iter().copied().collect();
    let boost_pads: Vec<BoostPad> = boost_pads_q.iter().copied().collect();
    let jump_pads: Vec<JumpPad> = jump_pads_q.iter().copied().collect();
    let loops: Vec<&LoopSpline> = loops_q.iter().collect();

    for (
        entity,
        mut body,
        mut transform,
        maybe_intent,
        attack,
        respawn,
        hitstop,
        hitstun,
        shield,
        air_dodge,
        ground_dodge,
        smash_charge,
        counter,
        meteor_dive,
    ) in &mut bodies
    {
        // Active Hitstop or Smash Angel Respawn Platform pauses kinematic integration.
        if hitstop.is_some_and(Hitstop::is_active) || respawn.is_some_and(|r| r.active) {
            continue;
        }

        if hitstun.is_some_and(Hitstun::is_active) {
            step_hitstun_body(&mut body, dt, &config, &solids, &slopes, &one_ways);
        } else {
            let intent = maybe_intent.copied().unwrap_or_default();
            let is_rolling = ground_dodge.is_some_and(|g| g.active_timer > 0.0);
            let is_charging_smash = smash_charge.is_some_and(SmashChargeState::is_charging);
            let shielding = shield.is_some_and(|s| s.active)
                || counter.is_some_and(|c| c.active_timer > 0.0)
                || is_charging_smash;
            let air_dodging = air_dodge.is_some_and(|a| a.active_timer > 0.0)
                || meteor_dive.is_some_and(|m| m.active)
                || is_rolling;
            let attack_active = attack.is_some_and(|a| a.active_move.is_some()) || is_rolling;
            step_single_body(
                entity,
                &mut body,
                &intent,
                shielding,
                air_dodging,
                attack_active,
                dt,
                &config,
                &solids,
                &slopes,
                &one_ways,
                &boost_pads,
                &jump_pads,
                &loops,
                &mut movement_events,
            );
        }

        // Sync Transform (rendering layers snap visual sprites to integer virtual pixels).
        transform.translation.x = body.position.x;
        transform.translation.y = body.position.y;
    }
}

/// Deterministic Smash knockback/tumble integration while an entity is in active [`Hitstun`].
/// Uses strict separated-axis (X then Y) collision resolution so falling onto wide decks
/// never horizontally teleports a fighter off the stage.
fn step_hitstun_body(
    body: &mut KinematicBody,
    dt: f32,
    cfg: &PhysicsConfig,
    solids: &[SolidRect],
    slopes: &[SlopeSegment],
    one_ways: &[OneWayPlatform],
) {
    let prev_x = body.position.x;
    let prev_feet_y = body.feet_y();
    let prev_vy = body.velocity.y;

    body.velocity.y = (body.velocity.y - cfg.gravity * 0.92 * dt).max(-cfg.max_fall_speed * 1.4);
    body.velocity.x = move_toward(body.velocity.x, 0.0, cfg.air_drag * 0.35 * dt);

    // Axis 1: Move & resolve X ONLY at previous Y!
    body.position.x += body.velocity.x * dt;
    let (resolved_x, wall_hit) =
        geometry::resolve_horizontal_solids(prev_x, body.position, body.half_extents, solids, 2.0);
    body.position.x = resolved_x;
    if wall_hit.is_some() {
        // Crisp Smash wall-bounce on high-speed impacts
        body.velocity.x = -body.velocity.x * 0.60;
    }

    // Axis 2: Move & resolve Y ONLY (ceiling + floor/bounce)!
    body.position.y += body.velocity.y * dt;
    let (resolved_y, hit_ceiling) =
        geometry::resolve_ceiling_solids(body.position, body.half_extents, body.velocity.y, solids);
    body.position.y = resolved_y;
    if hit_ceiling {
        body.velocity.y = -body.velocity.y * 0.50;
    }

    // Never snap a rising Smash launch arc back to the floor; only catch/bounce while descending!
    if body.velocity.y <= 0.0
        && let Some(hit) = geometry::cast_dual_foot_sensors(
            body.position,
            body.half_extents,
            prev_feet_y,
            body.velocity.y,
            false,
            false,
            solids,
            slopes,
            one_ways,
        )
    {
        body.position.y = hit.surface_y + body.half_extents.y;
        body.ground_normal = hit.normal;
        body.on_one_way = hit.is_one_way;
        if prev_vy < -220.0 {
            // Crisp Smash floor bounce when spiked or slammed hard into the stage
            body.velocity.y = -prev_vy * 0.42;
            body.velocity.x *= 0.85;
            body.state = MovementState::Airborne;
        } else {
            body.velocity.y = 0.0;
            body.velocity.x = move_toward(body.velocity.x, 0.0, cfg.ground_friction * 0.45 * dt);
            body.ground_speed = body.velocity.x;
            body.state = MovementState::Grounded;
        }
    } else {
        body.state = MovementState::Airborne;
        body.on_one_way = false;
    }
}

/// Gentle soft pushbox separation between grounded fighters so sprites never stack 100% on top
/// of each other in melee pileups (clamped so neither fighter is shoved off a ledge).
#[allow(clippy::type_complexity)]
fn apply_fighter_pushbox_separation(
    fixed_time: Res<Time<Fixed>>,
    solids_q: Query<&SolidRect>,
    slopes_q: Query<&SlopeSegment>,
    one_ways_q: Query<&OneWayPlatform>,
    mut fighters: Query<
        (Entity, &mut KinematicBody, &mut Transform),
        (
            With<Hurtbox>,
            Without<WallTurret>,
            Without<BreakableWall>,
            Without<Hitstop>,
            Without<Hitstun>,
        ),
    >,
) {
    let dt = fixed_time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    let solids: Vec<SolidRect> = solids_q.iter().copied().collect();
    let slopes: Vec<SlopeSegment> = slopes_q.iter().copied().collect();
    let one_ways: Vec<OneWayPlatform> = one_ways_q.iter().copied().collect();

    let snapshot: Vec<(Entity, Vec2, Vec2, bool)> = fighters
        .iter()
        .map(|(e, b, _)| (e, b.position, b.half_extents, b.state == MovementState::Grounded))
        .collect();

    let min_sep_x = 22.0_f32;
    let max_step = 135.0 * dt;

    for (entity, mut body, mut tf) in &mut fighters {
        if body.state != MovementState::Grounded {
            continue;
        }
        let mut push_x = 0.0_f32;
        for &(other_e, other_pos, _, other_grounded) in &snapshot {
            if other_e == entity || !other_grounded {
                continue;
            }
            let dy = (body.position.y - other_pos.y).abs();
            if dy > 24.0 {
                continue;
            }
            let dx = body.position.x - other_pos.x;
            if dx.abs() < min_sep_x {
                let dir = if dx.abs() > 0.01 {
                    dx.signum()
                } else if entity.index_u32() > other_e.index_u32() {
                    1.0
                } else {
                    -1.0
                };
                let overlap = min_sep_x - dx.abs();
                push_x += dir * (overlap * 0.5).min(max_step);
            }
        }
        if push_x.abs() > 0.01 {
            let candidate = Vec2::new(body.position.x + push_x, body.position.y);
            let (resolved_x, wall_hit) = geometry::resolve_horizontal_solids(
                body.position.x,
                candidate,
                body.half_extents,
                &solids,
                6.0,
            );
            if wall_hit.is_none()
                && geometry::cast_dual_foot_sensors(
                    Vec2::new(resolved_x, body.position.y),
                    body.half_extents,
                    body.feet_y(),
                    0.0,
                    true,
                    false,
                    &solids,
                    &slopes,
                    &one_ways,
                )
                .is_some()
            {
                body.position.x = resolved_x;
                tf.translation.x = resolved_x;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn step_single_body(
    entity: Entity,
    body: &mut KinematicBody,
    intent: &PlayerIntent,
    shielding: bool,
    air_dodging: bool,
    attack_active: bool,
    dt: f32,
    cfg: &PhysicsConfig,
    solids: &[SolidRect],
    slopes: &[SlopeSegment],
    one_ways: &[OneWayPlatform],
    boost_pads: &[BoostPad],
    jump_pads: &[JumpPad],
    loops: &[&LoopSpline],
    events: &mut MessageWriter<MovementEvent>,
) {
    // 1. Advance state timers deterministically
    body.coyote_timer = (body.coyote_timer - dt).max(0.0);
    body.jump_buffer_timer = (body.jump_buffer_timer - dt).max(0.0);
    body.drop_through_timer = (body.drop_through_timer - dt).max(0.0);
    body.wall_coyote_timer = (body.wall_coyote_timer - dt).max(0.0);
    body.wall_jump_lock_timer = (body.wall_jump_lock_timer - dt).max(0.0);
    body.loop_exit_cooldown = (body.loop_exit_cooldown - dt).max(0.0);
    body.spin_rev_fx_timer = (body.spin_rev_fx_timer - dt).max(0.0);

    if intent.jump_just_pressed && !shielding {
        body.jump_buffer_timer = cfg.jump_buffer_time;
    }

    // 2. Smash drop-through platform trigger & Airborne Fast-Fall!
    let down_held = intent.move_axis.y < -0.5;
    if !shielding && !air_dodging {
        if body.state == MovementState::Airborne
            && body.velocity.y <= 20.0
            && body.velocity.y > -450.0
            && intent.down_just_pressed
        {
            body.velocity.y = body.velocity.y.min(-465.0);
            body.drop_through_timer = cfg.drop_through_duration;
            events.write(MovementEvent {
                entity,
                kind: MovementFxKind::FastFall,
                pos: body.position,
                dir: -Vec2::Y,
            });
        }

        if (body.state == MovementState::Grounded
            && body.on_one_way
            && (intent.down_just_pressed || down_held))
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
    }

    // 2b. Chargeable Spin Dash: requires explicit fresh SpinDash press while crouched on solid ground
    // (never triggers accidentally just because Dash/Boost was held while tapping Down!)
    if !shielding
        && body.state == MovementState::Grounded
        && !body.on_one_way
        && down_held
        && intent.boost_just_pressed
    {
        body.state = MovementState::SpinCharging { charge: 0.28 };
        body.spin_rev_fx_timer = 0.11;
        events.write(MovementEvent {
            entity,
            kind: MovementFxKind::SpinDashRev,
            pos: Vec2::new(body.position.x, body.feet_y()),
            dir: Vec2::new(-body.facing, 0.4),
        });
    }

    if let MovementState::SpinCharging { mut charge } = body.state {
        if intent.move_axis.x.abs() > 0.15 {
            body.facing = intent.move_axis.x.signum();
        }
        // Brake cleanly in place while revving the Spin Dash
        body.ground_speed = move_toward(body.ground_speed, 0.0, cfg.ground_turn_decel * 1.4 * dt);
        let theta = (-body.ground_normal.x).atan2(body.ground_normal.y);
        let tangent = Vec2::new(theta.cos(), theta.sin());
        body.velocity = tangent * body.ground_speed;

        if shielding {
            body.state = MovementState::Grounded;
        } else if body.jump_buffer_timer > 0.0 {
            // Allow crisp jump-cancel out of Spin Dash charge
            body.velocity.y = cfg.jump_velocity;
            body.state = MovementState::Airborne;
            body.coyote_timer = 0.0;
            body.jump_buffer_timer = 0.0;
            body.jump_cut_applied = false;
            body.ground_normal = Vec2::Y;
            body.visual_angle = 0.0;
            events.write(MovementEvent {
                entity,
                kind: MovementFxKind::Jump,
                pos: Vec2::new(body.position.x, body.feet_y()),
                dir: Vec2::Y,
            });
            return;
        } else if down_held && intent.boost_held {
            let tap_bonus = if intent.boost_just_pressed { 0.24 } else { 0.0 };
            charge = (charge + dt * 1.45 + tap_bonus).min(1.0);
            body.state = MovementState::SpinCharging { charge };
            if body.spin_rev_fx_timer <= 0.0 || intent.boost_just_pressed {
                body.spin_rev_fx_timer = 0.12;
                events.write(MovementEvent {
                    entity,
                    kind: MovementFxKind::SpinDashRev,
                    pos: Vec2::new(body.position.x, body.feet_y()),
                    dir: Vec2::new(-body.facing, 0.45),
                });
            }
            return;
        } else if charge >= 0.35 {
            // Release deliberate Spin Dash -> explosive burst along facing direction
            let launch_speed = 440.0 + charge * 180.0;
            body.ground_speed = body.facing * launch_speed;
            body.velocity = tangent * body.ground_speed;
            body.state = MovementState::Grounded;
            events.write(MovementEvent {
                entity,
                kind: MovementFxKind::SpinDashRelease,
                pos: Vec2::new(body.position.x, body.feet_y() + 6.0),
                dir: Vec2::new(body.facing, 0.0),
            });
        } else {
            // Micro-tap cancel: stay cleanly stopped instead of shooting forward unexpectedly
            body.ground_speed = 0.0;
            body.velocity = Vec2::ZERO;
            body.state = MovementState::Grounded;
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
                let fresh_trigger = current_along < pad.target_speed - 24.0;
                body.velocity = pad.direction * pad.target_speed;
                body.ground_speed = if pad.direction.x >= 0.0 {
                    pad.target_speed
                } else {
                    -pad.target_speed
                };
                if pad.direction.x.abs() > 0.1 {
                    body.facing = pad.direction.x.signum();
                }
                if fresh_trigger {
                    events.write(MovementEvent {
                        entity,
                        kind: MovementFxKind::BoostPad,
                        pos: Vec2::new(body.position.x, body.feet_y() + 6.0),
                        dir: pad.direction,
                    });
                }
            }
        }
    }

    // 4b. Pneumatic Foundry Spring Launchers (JumpPads)
    for jpad in jump_pads {
        if jpad.overlaps(body.position, body.half_extents) && body.velocity.y < jpad.launch_vy * 0.72 {
            body.velocity.y = jpad.launch_vy;
            body.position.y = (jpad.center.y + jpad.half_extents.y + body.half_extents.y + 2.0)
                .max(body.position.y);
            body.state = MovementState::Airborne;
            body.on_one_way = false;
            body.coyote_timer = 0.0;
            body.jump_cut_applied = true;
            body.ground_normal = Vec2::Y;
            body.visual_angle = 0.0;
            body.air_jumps_remaining = 1; // Refreshes mid-air double jump!
            events.write(MovementEvent {
                entity,
                kind: MovementFxKind::JumpPad,
                pos: jpad.center + Vec2::new(0.0, jpad.half_extents.y),
                dir: Vec2::Y,
            });
            break;
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

    // 6. Jump execution: Ground / Coyote Jump, Wall Jump, or Controlled Smash Double Jump
    let can_ground_jump = body.state == MovementState::Grounded || body.coyote_timer > 0.0;
    let can_wall_jump =
        !can_ground_jump && body.wall_coyote_timer > 0.0 && body.last_wall_dir != 0;
    let can_double_jump = !can_ground_jump
        && !can_wall_jump
        && !air_dodging
        && body.state == MovementState::Airborne
        && body.air_jumps_remaining > 0;

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
        events.write(MovementEvent {
            entity,
            kind: MovementFxKind::Jump,
            pos: Vec2::new(body.position.x, body.feet_y()),
            dir: Vec2::Y,
        });
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
        events.write(MovementEvent {
            entity,
            kind: MovementFxKind::WallJump,
            pos: body.position + Vec2::new(body.last_wall_dir as f32 * body.half_extents.x, 0.0),
            dir: Vec2::new(away, 0.7).normalize(),
        });
    } else if body.jump_buffer_timer > 0.0 && can_double_jump {
        body.air_jumps_remaining = body.air_jumps_remaining.saturating_sub(1);
        body.velocity.y = cfg.double_jump_velocity;
        if intent.move_axis.x.abs() > 0.25 {
            body.facing = intent.move_axis.x.signum();
            body.velocity.x = intent.move_axis.x * cfg.run_max_speed * 0.85;
        }
        body.jump_buffer_timer = 0.0;
        body.jump_cut_applied = false;
        body.ground_normal = Vec2::Y;
        body.visual_angle = 0.0;
        events.write(MovementEvent {
            entity,
            kind: MovementFxKind::Jump,
            pos: Vec2::new(body.position.x, body.feet_y()),
            dir: Vec2::Y,
        });
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
    let move_x = if body.wall_jump_lock_timer > 0.0 || shielding || air_dodging {
        0.0
    } else if intent.move_axis.x.abs() > 0.22 {
        intent.move_axis.x
    } else {
        0.0
    };

    if move_x.abs() > 0.0 {
        body.facing = move_x.signum();
    }

    let was_grounded = body.state == MovementState::Grounded;
    let prev_feet_y = body.feet_y();
    let pre_step_vy = body.velocity.y;

    if was_grounded {
        // Slope inclination theta (> 0 uphill right, < 0 downhill right)
        let theta = (-body.ground_normal.x).atan2(body.ground_normal.y);

        if shielding {
            body.ground_speed = move_toward(body.ground_speed, 0.0, cfg.ground_turn_decel * 1.4 * dt);
            let tangent = Vec2::new(theta.cos(), theta.sin());
            body.velocity = tangent * body.ground_speed;
        } else {
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
            } else if theta.abs() < 0.05 {
                // Flat ground friction: NEVER auto-run forward when move_x == 0 (even if boost/RT is held!)
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
        }
    } else if air_dodging {
        // Directional Air-Dodge maintains crisp burst with light deceleration & minimal gravity
        body.velocity.x = move_toward(body.velocity.x, 0.0, cfg.air_drag * 1.2 * dt);
        body.velocity.y = (body.velocity.y - cfg.gravity * 0.18 * dt).max(-cfg.max_fall_speed);
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

    // 8. Separated-axis position integration (X first, then Y) so falling onto wide floors
    //    can NEVER falsely trigger horizontal wall ejection/teleporting!
    let prev_x = body.position.x;
    let ledge_stop_active = was_grounded
        && (attack_active
            || (intent.boost_held
                && body.ground_speed.abs() > cfg.run_max_speed * 1.05
                && !intent.jump_held));

    // Axis 1: Move & resolve X ONLY at previous Y
    body.position.x += body.velocity.x * dt;

    if ledge_stop_active && body.velocity.x.abs() > 1.0 {
        let lead_dir = body.velocity.x.signum();
        let probe_x = body.position.x + lead_dir * (body.half_extents.x * 0.75);
        let probe_half = Vec2::new(2.0, body.half_extents.y);
        let has_floor_ahead = geometry::cast_dual_foot_sensors(
            Vec2::new(probe_x, body.position.y),
            probe_half,
            prev_feet_y,
            0.0,
            true,
            false,
            solids,
            slopes,
            one_ways,
        )
        .is_some();
        if !has_floor_ahead {
            body.position.x = prev_x;
            body.velocity.x = 0.0;
            body.ground_speed = 0.0;
        }
    }

    let step_clearance = if was_grounded { 8.0 } else { 2.0 };
    let (resolved_x, wall_hit) = geometry::resolve_horizontal_solids(
        prev_x,
        body.position,
        body.half_extents,
        solids,
        step_clearance,
    );
    body.position.x = resolved_x;
    if wall_hit.is_some() {
        body.velocity.x = 0.0;
        if was_grounded {
            body.ground_speed = 0.0;
        }
    }

    // Axis 2: Move & resolve Y ONLY (ceilings + dual downward foot sensors)
    body.position.y += body.velocity.y * dt;

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
            if pre_step_vy < -150.0 {
                events.write(MovementEvent {
                    entity,
                    kind: MovementFxKind::Land,
                    pos: Vec2::new(body.position.x, hit.surface_y),
                    dir: hit.normal,
                });
            }
        }

        let tangent = Vec2::new(theta.cos(), theta.sin());
        body.velocity = tangent * body.ground_speed;
        body.state = MovementState::Grounded;
        body.coyote_timer = cfg.coyote_time;
        body.wall_coyote_timer = 0.0;
        body.air_jumps_remaining = 1;

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
            events.write(MovementEvent {
                entity,
                kind: MovementFxKind::Jump,
                pos: Vec2::new(body.position.x, hit.surface_y),
                dir: Vec2::Y,
            });
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
                body.air_jumps_remaining = 1;
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
