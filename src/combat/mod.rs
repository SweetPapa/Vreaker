//! Smash-inspired combat system & Mega Man Buster:
//! - `Hurtbox`, `Hitbox`, `DamagePercent`, `Hitstun`, `Hitstop`, `Invulnerable`
//! - Momentum-scaled Smash knockback: `base_kb + (target_percent * kb_scale) + (|v_attacker| * momentum_mult)`
//! - 3–8 frame impact hitstop pausing attacker & target simulation
//! - Directional moveset: 3-hit Jab, Dash Attack, F-Air, D-Air (meteor spike + recoil),
//!   and Up-Special (vertical recovery + instant Sonic horizontal air-brake)
//! - Mega Man Buster: max 3 active uncharged pellets; hold `>= 0.7s` for piercing Charged Shot
//!
//! Architectural invariant: completely decoupled from rendering for headless 60 Hz testing.

use std::collections::HashSet;
use std::time::Duration;

use bevy::prelude::*;

use crate::input::ControlConfig;
use crate::physics::{
    KinematicBody, MovementEvent, MovementFxKind, MovementState, PhysicsSystems, PlayerIntent,
    SolidRect,
};
use crate::player::{Player, PlayerRespawnState};
use crate::sprite_manifest::SpriteSheetManifest;
use crate::state::{AppState, StageRestartEvent};

/// Maximum uncharged Buster pellets allowed on screen simultaneously per shooter (`SPEC.MD`).
pub const MAX_UNCHARGED_BUSTER_PELLETS: usize = 3;

/// Hold duration (`seconds`) required to charge a piercing Charged Buster Shot (`SPEC.MD`).
pub const BUSTER_CHARGE_TIME_SECS: f32 = 0.70;

/// Maximum hold duration (`seconds`) for Chargeable Smash Attacks (`1.0x -> 1.55x` damage & KB).
pub const MAX_SMASH_CHARGE_SECS: f32 = 0.85;

/// Hold threshold (`seconds`) promoting a held Up/Down Melee into an Up-Smash / Down-Smash.
const SMASH_HOLD_PROMOTE_SECS: f32 = 0.13;

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
    /// Entities already struck by this active attack/hitbox instance (guarantees <= 1 hit per swing).
    pub hit_entities: HashSet<Entity>,
    /// Marks whether this hitbox came from a Charged Buster Shot (for extra shake/FX).
    pub is_charged: bool,
    /// Optional explicit minimum hitstop frames for heavy finishers & charged shots (`5..=7`).
    pub min_hitstop_frames: u32,
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

/// Marker attached to 360° spinning / sweeping hitboxes so targets on the left launch left
/// and targets on the right launch right (true Smash Bros. Link Spin / Down-Smash knockback!).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct DualSidedHitbox;

/// Full Smash Bros. directional moveset (Grounded Normals, Chargeable Smashes, Aerials,
/// 4-Way Specials, 360° Hurricane Sword Spin, and Taunt).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttackKind {
    Jab1,
    Jab2,
    Jab3,
    DownTilt,
    UpTilt,
    ForwardSmash,
    UpSmash,
    DownSmash,
    DashAttack,
    NeutralAir,
    ForwardAir,
    BackAir,
    UpAir,
    DownAir,
    NeutralSpecial,
    SideSpecial,
    UpSpecial,
    CycloneSpin,
    DownSpecialCounter,
    DownSpecialDive,
    Taunt,
}

impl AttackKind {
    /// Corresponding baked animation clip key in `assets/sprites/hero/hero.ron`.
    pub fn animation_name(self) -> &'static str {
        match self {
            AttackKind::Jab1 => "jab",
            AttackKind::Jab2 => "jab_2",
            AttackKind::Jab3 => "jab_3",
            AttackKind::DownTilt => "down_tilt",
            AttackKind::UpTilt => "up_tilt",
            AttackKind::ForwardSmash => "f_smash",
            AttackKind::UpSmash => "up_smash",
            AttackKind::DownSmash => "down_smash",
            AttackKind::DashAttack => "dash_attack",
            AttackKind::NeutralAir => "sword_spin",
            AttackKind::ForwardAir | AttackKind::BackAir => "f_air",
            AttackKind::UpAir => "up_tilt",
            AttackKind::DownAir => "d_air",
            AttackKind::NeutralSpecial => "neutral_special",
            AttackKind::SideSpecial => "side_special",
            AttackKind::UpSpecial | AttackKind::CycloneSpin => "sword_spin",
            AttackKind::DownSpecialCounter | AttackKind::DownSpecialDive => "down_special",
            AttackKind::Taunt => "taunt",
        }
    }

    /// Arcade HUD callout name displayed when unleashing signature moves.
    pub fn display_name(self) -> &'static str {
        match self {
            AttackKind::Jab1 => "RIFT SLASH I",
            AttackKind::Jab2 => "RISING CROSS II",
            AttackKind::Jab3 => "BREAKER CLEAVE III!",
            AttackKind::DownTilt => "LOW SABER SWEEP",
            AttackKind::UpTilt => "MOON CRESCENT ARC",
            AttackKind::ForwardSmash => "TITAN F-SMASH!",
            AttackKind::UpSmash => "SKYWARD UP-SMASH!",
            AttackKind::DownSmash => "360° DOWN SMASH!",
            AttackKind::DashAttack => "DASH CRESCENT",
            AttackKind::NeutralAir => "AIR HURRICANE",
            AttackKind::ForwardAir => "FORWARD AIR SLASH",
            AttackKind::BackAir => "BACK AIR CLEAVE!",
            AttackKind::UpAir => "UP AIR CRESCENT",
            AttackKind::DownAir => "METEOR SPIKE!",
            AttackKind::NeutralSpecial => "BREAKER WAVE!",
            AttackKind::SideSpecial => "PLASMA BLITZ!",
            AttackKind::UpSpecial => "RISING CYCLONE!",
            AttackKind::CycloneSpin => "HURRICANE SWORD SPIN!",
            AttackKind::DownSpecialCounter => "AEGIS COUNTER!",
            AttackKind::DownSpecialDive => "METEOR DIVE!",
            AttackKind::Taunt => "BREAK ON!",
        }
    }

    pub fn is_heavy_callout(self) -> bool {
        matches!(
            self,
            AttackKind::Jab3
                | AttackKind::ForwardSmash
                | AttackKind::UpSmash
                | AttackKind::DownSmash
                | AttackKind::BackAir
                | AttackKind::DownAir
                | AttackKind::SideSpecial
                | AttackKind::UpSpecial
                | AttackKind::CycloneSpin
                | AttackKind::DownSpecialCounter
                | AttackKind::DownSpecialDive
        )
    }

    pub fn is_dual_sided(self) -> bool {
        matches!(
            self,
            AttackKind::DownSmash
                | AttackKind::CycloneSpin
                | AttackKind::UpSpecial
                | AttackKind::NeutralAir
        )
    }
}

/// Direction & variant of a grounded Smash defensive dodge (`Shield + Left/Right/Down`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroundDodgeKind {
    #[default]
    Spot,
    RollForward,
    RollBackward,
}

/// Grounded Smash Bros. Roll-Dodge & Spot-Dodge component (brief i-frames + ledge-clamped slide).
#[derive(Component, Debug, Clone, Default)]
pub struct GroundDodgeState {
    pub kind: GroundDodgeKind,
    pub active_timer: f32,
    pub recovery_timer: f32,
    pub roll_dir: f32,
    pub flip_on_finish: bool,
}

impl GroundDodgeState {
    pub fn is_active(&self) -> bool {
        self.active_timer > 0.0
    }

    pub fn is_busy(&self) -> bool {
        self.active_timer > 0.0 || self.recovery_timer > 0.0
    }

    pub fn animation_name(&self) -> &'static str {
        match self.kind {
            GroundDodgeKind::Spot => "spot_dodge",
            GroundDodgeKind::RollForward => "dodge_fwd",
            GroundDodgeKind::RollBackward => "dodge_back",
        }
    }
}

/// Tracks a Chargeable Smash Attack (`ForwardSmash`, `UpSmash`, `DownSmash`) while holding Melee or HeavySmash.
#[derive(Component, Debug, Clone, Default)]
pub struct SmashChargeState {
    /// Target Smash attack currently charging (`None` when not charging).
    pub kind: Option<AttackKind>,
    /// If `Some(tilt)`, releasing before `SMASH_HOLD_PROMOTE_SECS` fires the fast Tilt instead!
    pub quick_tap_tilt: Option<AttackKind>,
    pub charge_secs: f32,
    /// `true` when initiated by the dedicated `HeavySmash` (`U`/`V`) button instead of `MeleeAttack`.
    pub charge_source_heavy: bool,
}

impl SmashChargeState {
    pub fn is_charging(&self) -> bool {
        self.kind.is_some()
    }

    /// Returns `true` once committed to a visible Smash Charge glow (either direct Smash or past quick-tap threshold).
    pub fn is_visibly_charging(&self) -> bool {
        self.kind.is_some() && (self.quick_tap_tilt.is_none() || self.charge_secs >= SMASH_HOLD_PROMOTE_SECS)
    }

    pub fn charge_ratio(&self) -> f32 {
        (self.charge_secs / MAX_SMASH_CHARGE_SECS).clamp(0.0, 1.0)
    }
}

/// Live arcade move-name callout displayed briefly on the HUD whenever P1 executes a Smash move.
#[derive(Resource, Debug, Clone, Default)]
pub struct LastMoveCallout {
    pub label: String,
    pub is_heavy: bool,
    pub timer: f32,
}

impl LastMoveCallout {
    pub fn trigger(&mut self, label: impl Into<String>, is_heavy: bool) {
        self.label = label.into();
        self.is_heavy = is_heavy;
        self.timer = 1.15;
    }
}

/// Grounded `Down-B` Aegis Breaker Counter stance: negates an incoming hit and blasts back!
#[derive(Component, Debug, Clone, Default)]
pub struct CounterStance {
    pub active_timer: f32,
    pub cooldown: f32,
}

impl CounterStance {
    pub fn is_active(&self) -> bool {
        self.active_timer > 0.0
    }
}

/// Airborne `Down-B` Meteor Breaker Dive state: plummets rapidly and erupts in a ground shockwave!
#[derive(Component, Debug, Clone, Default)]
pub struct MeteorDiveState {
    pub active: bool,
}

/// Tall sweeping plasma crescent wave launched by Neutral-B (`Breaker Crescent Wave`).
#[derive(Component, Debug, Clone, Copy)]
pub struct BreakerWaveProjectile {
    pub owner: Entity,
    pub velocity: Vec2,
    pub facing: f32,
}

/// Signature visual/audio combat events for directional slash arcs, shockwave rings,
/// counter triggers, ground slams, and enemy phase blinks.
#[derive(Message, Debug, Clone, Copy)]
pub enum CombatVisualEvent {
    SlashArc {
        pos: Vec2,
        facing: f32,
        angle_rad: f32,
        radius: f32,
        heavy: bool,
        color_rgb: [f32; 3],
    },
    ShockwaveRing {
        pos: Vec2,
        max_radius: f32,
        color_rgb: [f32; 3],
    },
    CounterTrigger {
        pos: Vec2,
    },
    GroundSlam {
        pos: Vec2,
    },
    SideBlitz {
        pos: Vec2,
        facing: f32,
    },
    EnemyBlink {
        from: Vec2,
        to: Vec2,
    },
    /// 0.48s amber axe charge-up + aiming sightline before Vulcanus fires Phase 2 lasers!
    BossLaserTelegraph {
        origin: Vec2,
        target_dir: Vec2,
        duration: f32,
    },
    /// Swirling 360° Hurricane Sword Spin / Greataxe Whirlwind vortex rings & sparks!
    CycloneSpin {
        pos: Vec2,
        radius: f32,
        color_rgb: [f32; 3],
    },
    /// Golden-cyan energy burst when unleashing a charged Smash attack!
    SmashChargeRelease {
        pos: Vec2,
        facing: f32,
        charge_ratio: f32,
    },
    /// Glowing violet ground rune telegraphing Plasma Arcanist's Arcane Pillar Eruption!
    ArcanePillarTelegraph {
        pos: Vec2,
        duration: f32,
    },
    /// Blazing violet vertical arcane energy pillar eruption!
    ArcanePillarErupt {
        pos: Vec2,
    },
}

/// Grounded Smash-style energy shield component (blocks hits, drains on hold/block, regenerates).
#[derive(Component, Debug, Clone)]
pub struct ShieldState {
    pub active: bool,
    pub health: f32,
    pub max_health: f32,
    pub broken_timer: f32,
}

impl Default for ShieldState {
    fn default() -> Self {
        Self {
            active: false,
            health: 100.0,
            max_health: 100.0,
            broken_timer: 0.0,
        }
    }
}

impl ShieldState {
    pub fn health_ratio(&self) -> f32 {
        (self.health / self.max_health.max(1.0)).clamp(0.0, 1.0)
    }
}

/// Smash-style directional Air-Dodge component (brief i-frames + directional burst, 1 per airtime).
#[derive(Component, Debug, Clone, Default)]
pub struct AirDodgeState {
    pub active_timer: f32,
    pub recovery_timer: f32,
    pub used_in_air: bool,
    pub direction: Vec2,
}

impl AirDodgeState {
    pub fn is_busy(&self) -> bool {
        self.active_timer > 0.0 || self.recovery_timer > 0.0
    }
}

/// Headless-safe fighter frame data loaded from `assets/sprites/hero/hero.ron`.
///
/// Supplies bone-projected `active_hitboxes` and `buster_muzzle` socket coordinates directly
/// to combat hitboxes, falling back gracefully to default specs if the `.ron` file is absent.
#[derive(Resource, Debug, Clone, Default)]
pub struct FighterFrameData {
    pub hero_manifest: Option<SpriteSheetManifest>,
}

impl FighterFrameData {
    pub fn load_default() -> Self {
        let hero_manifest = SpriteSheetManifest::load_from_file("assets/sprites/hero/hero.ron").ok();
        Self { hero_manifest }
    }

    /// Returns the bone-projected muzzle offset in `KinematicBody`-relative pixels.
    pub fn buster_muzzle_offset(&self) -> Vec2 {
        if let Some(ref m) = self.hero_manifest
            && let Some(anim) = m.animations.get("buster")
        {
            let sample_frame = (anim.start + 1).min(anim.end);
            if let Some(sock) = m.bone_sockets.get(&sample_frame)
                && let Some((mx, my)) = sock.buster_muzzle
            {
                return Vec2::new(mx.max(18.0), (my + 18.0).clamp(-4.0, 12.0));
            }
        }
        Vec2::new(20.0, 4.0)
    }
}

/// Live multi-hit combo counter tracking consecutive player hits on enemies.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct ComboTracker {
    pub hits: u32,
    pub damage: f32,
    pub decay_timer: f32,
    pub best_hits: u32,
    pub best_damage: f32,
}

impl ComboTracker {
    pub fn register_hit(&mut self, dmg: f32) {
        self.hits += 1;
        self.damage += dmg;
        self.decay_timer = 0.75;
        if self.hits > self.best_hits {
            self.best_hits = self.hits;
        }
        if self.damage > self.best_damage {
            self.best_damage = self.damage;
        }
    }
}

/// Deterministic `FixedUpdate` stage speedrun timer (`Playing` only, pauses in `Paused`).
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct StageRunTimer {
    pub elapsed_secs: f32,
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
    pub side_special_used: bool,
    /// Brief timer set whenever a Buster shot fires so animation can flash `"buster"`.
    pub buster_anim_timer: f32,
    /// 7-frame (`0.12s`) Smash-style input buffers so rapid sword combo taps never drop!
    pub buffered_melee_timer: f32,
    pub buffered_heavy_timer: f32,
    pub buffered_special_timer: f32,
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
        app.insert_resource(FighterFrameData::load_default())
            .init_resource::<LastHitTelemetry>()
            .init_resource::<LastMoveCallout>()
            .init_resource::<ComboTracker>()
            .init_resource::<StageRunTimer>()
            .add_message::<CombatHitEvent>()
            .add_message::<BusterShotEvent>()
            .add_message::<CombatVisualEvent>()
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
                (
                    handle_player_shield_and_air_dodge,
                    handle_player_melee_and_special,
                    handle_buster_weapon,
                )
                    .chain()
                    .in_set(CombatPrePhysicsSystems),
            )
            .add_systems(
                FixedUpdate,
                (
                    tick_hitstop_and_status_timers,
                    tick_combo_and_run_timer,
                    resolve_meteor_dive_impacts,
                    advance_buster_projectiles,
                    advance_breaker_waves,
                    sync_attached_hitboxes,
                    resolve_hitbox_hurtbox_collisions,
                    respawn_launched_dummies,
                )
                    .chain()
                    .in_set(CombatPostPhysicsSystems),
            )
            .add_systems(OnEnter(AppState::Title), reset_combo_and_run_timer)
            .add_systems(Update, reset_combo_and_timer_on_restart);
    }
}

fn reset_combo_and_run_timer(
    mut combo: ResMut<ComboTracker>,
    mut run_timer: ResMut<StageRunTimer>,
    mut callout: ResMut<LastMoveCallout>,
) {
    *combo = ComboTracker::default();
    *run_timer = StageRunTimer::default();
    *callout = LastMoveCallout::default();
}

fn reset_combo_and_timer_on_restart(
    mut restarts: MessageReader<StageRestartEvent>,
    mut combo: ResMut<ComboTracker>,
    mut run_timer: ResMut<StageRunTimer>,
    mut callout: ResMut<LastMoveCallout>,
) {
    if restarts.read().next().is_some() {
        *combo = ComboTracker::default();
        *run_timer = StageRunTimer::default();
        *callout = LastMoveCallout::default();
    }
}

fn tick_combo_and_run_timer(
    fixed_time: Res<Time<Fixed>>,
    enc_status: Option<Res<crate::enemies::EncounterStatus>>,
    enemies_in_stun: Query<&Team, With<Hitstun>>,
    mut combo: ResMut<ComboTracker>,
    mut run_timer: ResMut<StageRunTimer>,
    mut callout: ResMut<LastMoveCallout>,
) {
    let dt = fixed_time.delta_secs();
    let boss_already_ko = enc_status.is_some_and(|e| e.victory_delay_timer.is_some());
    if !boss_already_ko {
        run_timer.elapsed_secs += dt;
    }

    if callout.timer > 0.0 {
        callout.timer = (callout.timer - dt).max(0.0);
        if callout.timer <= 0.0 {
            callout.label.clear();
        }
    }

    if combo.hits > 0 {
        let any_enemy_stunned = enemies_in_stun.iter().any(|t| *t == Team::Enemy);
        if any_enemy_stunned {
            combo.decay_timer = combo.decay_timer.max(0.45);
        } else {
            combo.decay_timer = (combo.decay_timer - dt).max(0.0);
        }
        if combo.decay_timer <= 0.0 {
            combo.hits = 0;
            combo.damage = 0.0;
        }
    }
}

/// Spec parameters for a melee/special hitbox spawn.
#[derive(Debug, Clone, Copy)]
pub struct MoveSpec {
    pub offset: Vec2,
    pub radius: f32,
    pub damage: f32,
    pub base_kb: f32,
    pub kb_scale: f32,
    pub momentum_mult: f32,
    pub angle_deg: f32,
    pub active_secs: f32,
    pub cooldown_secs: f32,
    pub recoil_bounce_vy: Option<f32>,
    pub hitbox_facing_sign: f32,
    pub min_hitstop_frames: u32,
}

/// Resolves a move's hitbox spec by merging bone-projected `.ron` frame data from
/// [`FighterFrameData`] (when present) with crisp Smash launch arc tuning.
pub fn resolve_move_spec(kind: AttackKind, frame_data: &FighterFrameData) -> MoveSpec {
    let mut spec = default_move_spec(kind);

    if let Some(ref manifest) = frame_data.hero_manifest
        && let Some(anim) = manifest.animations.get(kind.animation_name())
        && let Some(hb) = anim.active_hitboxes.first()
    {
        // Convert sprite-center pixel coordinates (+Y up) to KinematicBody center (sprite sits +18px above body center)
        let body_y = hb.offset.1 + 18.0;
        match kind {
            AttackKind::Jab1 => {
                spec.offset = Vec2::new(hb.offset.0.clamp(20.0, 28.0), body_y.clamp(-2.0, 10.0));
                spec.radius = (hb.radius + 3.0).max(17.5);
            }
            AttackKind::Jab2 => {
                spec.offset = Vec2::new(hb.offset.0.clamp(22.0, 30.0), body_y.clamp(0.0, 14.0));
                spec.radius = (hb.radius + 3.5).max(18.5);
            }
            AttackKind::Jab3 => {
                spec.offset = Vec2::new(hb.offset.0.clamp(26.0, 34.0), body_y.clamp(-2.0, 12.0));
                spec.radius = (hb.radius + 4.0).max(21.0);
            }
            AttackKind::DashAttack => {
                spec.offset = Vec2::new(hb.offset.0.max(22.0), body_y.clamp(-4.0, 10.0));
                spec.radius = hb.radius.max(17.0);
                spec.damage = hb.damage;
            }
            AttackKind::ForwardAir => {
                spec.offset = Vec2::new(hb.offset.0.max(18.0), body_y.clamp(-4.0, 10.0));
                spec.radius = hb.radius.max(17.0);
                spec.damage = hb.damage;
            }
            AttackKind::DownAir => {
                // Anchor hitbox squarely at the plunging downward Rift Saber tip beneath boots
                spec.offset = Vec2::new(hb.offset.0.clamp(-6.0, 12.0), (body_y - 6.0).clamp(-24.0, -18.0));
                spec.radius = hb.radius.max(17.0);
                spec.damage = hb.damage;
                spec.angle_deg = hb.angle_deg;
            }
            AttackKind::UpSpecial => {
                spec.offset = Vec2::new(hb.offset.0, (body_y + 8.0).clamp(4.0, 16.0));
                spec.radius = hb.radius.max(20.0);
                spec.damage = hb.damage;
            }
            AttackKind::DownTilt => {
                spec.offset = Vec2::new(hb.offset.0.max(20.0), body_y.clamp(-14.0, -4.0));
                spec.radius = hb.radius.max(15.0);
                spec.damage = hb.damage;
            }
            AttackKind::ForwardSmash => {
                spec.offset = Vec2::new(hb.offset.0.max(26.0), body_y.clamp(-2.0, 10.0));
                spec.radius = hb.radius.max(20.0);
                spec.damage = hb.damage;
            }
            AttackKind::SideSpecial => {
                spec.offset = Vec2::new(hb.offset.0.max(22.0), body_y.clamp(-2.0, 10.0));
                spec.radius = hb.radius.max(18.0);
                spec.damage = hb.damage;
            }
            AttackKind::UpSmash => {
                spec.offset = Vec2::new(hb.offset.0.clamp(-4.0, 12.0), (body_y + 10.0).clamp(16.0, 28.0));
                spec.radius = hb.radius.max(22.0);
            }
            AttackKind::DownSmash | AttackKind::CycloneSpin => {
                spec.offset = Vec2::new(0.0, if kind == AttackKind::DownSmash { -4.0 } else { 4.0 });
                spec.radius = hb.radius.max(32.0);
            }
            AttackKind::UpTilt
            | AttackKind::NeutralAir
            | AttackKind::BackAir
            | AttackKind::UpAir
            | AttackKind::NeutralSpecial
            | AttackKind::DownSpecialCounter
            | AttackKind::DownSpecialDive
            | AttackKind::Taunt => {
                // Dedicated directional Smash variants retain their tuned arcs
            }
        }
    }

    spec
}

fn default_move_spec(kind: AttackKind) -> MoveSpec {
    match kind {
        // Snappy 3-hit Rift Saber Combo: Overhead Cleave -> Rising Uppercut Cross -> Breaker Cleave Finisher!
        AttackKind::Jab1 => MoveSpec {
            offset: Vec2::new(22.0, 2.0),
            radius: 17.5,
            damage: 5.0,
            base_kb: 92.0,
            kb_scale: 0.45,
            momentum_mult: 0.18,
            angle_deg: 24.0,
            active_secs: 0.11,
            cooldown_secs: 0.14,
            recoil_bounce_vy: None,
            hitbox_facing_sign: 1.0,
            min_hitstop_frames: 3,
        },
        AttackKind::Jab2 => MoveSpec {
            offset: Vec2::new(24.0, 5.0),
            radius: 18.5,
            damage: 6.0,
            base_kb: 105.0,
            kb_scale: 0.58,
            momentum_mult: 0.22,
            angle_deg: 40.0,
            active_secs: 0.11,
            cooldown_secs: 0.15,
            recoil_bounce_vy: None,
            hitbox_facing_sign: 1.0,
            min_hitstop_frames: 3,
        },
        AttackKind::Jab3 => MoveSpec {
            offset: Vec2::new(28.0, 4.0),
            radius: 21.0,
            damage: 12.0,
            base_kb: 255.0,
            kb_scale: 3.15,
            momentum_mult: 0.48,
            angle_deg: 36.0,
            active_secs: 0.14,
            cooldown_secs: 0.30,
            recoil_bounce_vy: None,
            hitbox_facing_sign: 1.0,
            min_hitstop_frames: 6,
        },
        // Down-Tilt: fast low sweeping kick popping opponents upward at 72° for aerial combos!
        AttackKind::DownTilt => MoveSpec {
            offset: Vec2::new(22.0, -10.0),
            radius: 16.0,
            damage: 7.0,
            base_kb: 170.0,
            kb_scale: 1.65,
            momentum_mult: 0.25,
            angle_deg: 72.0,
            active_secs: 0.12,
            cooldown_secs: 0.20,
            recoil_bounce_vy: None,
            hitbox_facing_sign: 1.0,
            min_hitstop_frames: 4,
        },
        // Up-Tilt: grounded upward juggler setting up aerial combos!
        AttackKind::UpTilt => MoveSpec {
            offset: Vec2::new(10.0, 22.0),
            radius: 18.0,
            damage: 9.0,
            base_kb: 230.0,
            kb_scale: 2.5,
            momentum_mult: 0.25,
            angle_deg: 84.0,
            active_secs: 0.13,
            cooldown_secs: 0.26,
            recoil_bounce_vy: None,
            hitbox_facing_sign: 1.0,
            min_hitstop_frames: 5,
        },
        // Forward-Smash: blazing heavy horizontal kill-slash (chargeable, ledge-clamped, 7f hitstop!)
        AttackKind::ForwardSmash => MoveSpec {
            offset: Vec2::new(28.0, 4.0),
            radius: 21.0,
            damage: 18.0,
            base_kb: 285.0,
            kb_scale: 3.6,
            momentum_mult: 0.45,
            angle_deg: 32.0,
            active_secs: 0.16,
            cooldown_secs: 0.42,
            recoil_bounce_vy: None,
            hitbox_facing_sign: 1.0,
            min_hitstop_frames: 7,
        },
        // Up-Smash: devastating 2H overhead vertical kill-arc (chargeable!)
        AttackKind::UpSmash => MoveSpec {
            offset: Vec2::new(6.0, 26.0),
            radius: 23.0,
            damage: 17.0,
            base_kb: 295.0,
            kb_scale: 3.6,
            momentum_mult: 0.30,
            angle_deg: 84.0,
            active_secs: 0.16,
            cooldown_secs: 0.42,
            recoil_bounce_vy: None,
            hitbox_facing_sign: 1.0,
            min_hitstop_frames: 7,
        },
        // Down-Smash: 360° low double-sided spin sweep hitting BOTH left & right outward!
        AttackKind::DownSmash => MoveSpec {
            offset: Vec2::new(0.0, -4.0),
            radius: 32.0,
            damage: 16.0,
            base_kb: 270.0,
            kb_scale: 3.4,
            momentum_mult: 0.25,
            angle_deg: 38.0,
            active_secs: 0.20,
            cooldown_secs: 0.42,
            recoil_bounce_vy: None,
            hitbox_facing_sign: 1.0,
            min_hitstop_frames: 7,
        },
        // Dash Attack: controlled forward burst (ledge-clamped!) with clean 34° Smash launch
        AttackKind::DashAttack => MoveSpec {
            offset: Vec2::new(25.0, 2.0),
            radius: 17.0,
            damage: 12.0,
            base_kb: 215.0,
            kb_scale: 2.9,
            momentum_mult: 0.55,
            angle_deg: 34.0,
            active_secs: 0.15,
            cooldown_secs: 0.30,
            recoil_bounce_vy: None,
            hitbox_facing_sign: 1.0,
            min_hitstop_frames: 5,
        },
        // Neutral-Air: fast 360° spinning cyan blade ring (combo extender + momentum-scaled!)
        AttackKind::NeutralAir => MoveSpec {
            offset: Vec2::new(8.0, 2.0),
            radius: 22.0,
            damage: 9.0,
            base_kb: 180.0,
            kb_scale: 2.4,
            momentum_mult: 0.62,
            angle_deg: 46.0,
            active_secs: 0.15,
            cooldown_secs: 0.21,
            recoil_bounce_vy: None,
            hitbox_facing_sign: 1.0,
            min_hitstop_frames: 4,
        },
        // Forward-Air: iconic ~30° horizontal Smash launcher scaling strongly with air momentum
        AttackKind::ForwardAir => MoveSpec {
            offset: Vec2::new(24.0, 4.0),
            radius: 17.0,
            damage: 11.0,
            base_kb: 215.0,
            kb_scale: 3.0,
            momentum_mult: 0.62,
            angle_deg: 30.0,
            active_secs: 0.14,
            cooldown_secs: 0.26,
            recoil_bounce_vy: None,
            hitbox_facing_sign: 1.0,
            min_hitstop_frames: 5,
        },
        // Back-Air: high-knockback reverse horizontal kill move
        AttackKind::BackAir => MoveSpec {
            offset: Vec2::new(24.0, 4.0),
            radius: 17.0,
            damage: 13.0,
            base_kb: 235.0,
            kb_scale: 3.3,
            momentum_mult: 0.55,
            angle_deg: 28.0,
            active_secs: 0.14,
            cooldown_secs: 0.27,
            recoil_bounce_vy: None,
            hitbox_facing_sign: -1.0,
            min_hitstop_frames: 6,
        },
        // Up-Air: sweeping overhead juggler launching vertically
        AttackKind::UpAir => MoveSpec {
            offset: Vec2::new(4.0, 24.0),
            radius: 18.0,
            damage: 10.0,
            base_kb: 215.0,
            kb_scale: 2.7,
            momentum_mult: 0.42,
            angle_deg: 84.0,
            active_secs: 0.14,
            cooldown_secs: 0.25,
            recoil_bounce_vy: None,
            hitbox_facing_sign: 1.0,
            min_hitstop_frames: 5,
        },
        // Down-Air: brutal meteor spike (-78°) with upward recoil hop!
        AttackKind::DownAir => MoveSpec {
            offset: Vec2::new(6.0, -22.0),
            radius: 17.0,
            damage: 13.0,
            base_kb: 225.0,
            kb_scale: 3.0,
            momentum_mult: 0.45,
            angle_deg: -78.0,
            active_secs: 0.15,
            cooldown_secs: 0.28,
            recoil_bounce_vy: Some(265.0),
            hitbox_facing_sign: 1.0,
            min_hitstop_frames: 6,
        },
        // Neutral-Special (Neutral-B grounded): Breaker Crescent Energy Wave
        AttackKind::NeutralSpecial => MoveSpec {
            offset: Vec2::new(22.0, 4.0),
            radius: 18.0,
            damage: 11.0,
            base_kb: 195.0,
            kb_scale: 2.4,
            momentum_mult: 0.20,
            angle_deg: 36.0,
            active_secs: 0.16,
            cooldown_secs: 0.38,
            recoil_bounce_vy: None,
            hitbox_facing_sign: 1.0,
            min_hitstop_frames: 4,
        },
        // Side-Special (Side-B): Plasma Blitz electric horizontal dash-slash (ledge-clamped grounded!)
        AttackKind::SideSpecial => MoveSpec {
            offset: Vec2::new(24.0, 4.0),
            radius: 19.0,
            damage: 13.0,
            base_kb: 225.0,
            kb_scale: 2.8,
            momentum_mult: 0.45,
            angle_deg: 42.0,
            active_secs: 0.18,
            cooldown_secs: 0.36,
            recoil_bounce_vy: None,
            hitbox_facing_sign: 1.0,
            min_hitstop_frames: 5,
        },
        // Up-Special (Airborne Up-B): Rising 360° Cyclone Recovery + instant horizontal air-brake
        AttackKind::UpSpecial => MoveSpec {
            offset: Vec2::new(0.0, 10.0),
            radius: 25.0,
            damage: 10.0,
            base_kb: 205.0,
            kb_scale: 2.5,
            momentum_mult: 0.30,
            angle_deg: 78.0,
            active_secs: 0.26,
            cooldown_secs: 0.34,
            recoil_bounce_vy: None,
            hitbox_facing_sign: 1.0,
            min_hitstop_frames: 5,
        },
        // Grounded Up-B: 360° Hurricane Sword Spin Attack (sweeps BOTH sides, steerable, ledge-clamped!)
        AttackKind::CycloneSpin => MoveSpec {
            offset: Vec2::new(0.0, 4.0),
            radius: 34.0,
            damage: 14.0,
            base_kb: 255.0,
            kb_scale: 3.2,
            momentum_mult: 0.35,
            angle_deg: 52.0,
            active_secs: 0.38,
            cooldown_secs: 0.46,
            recoil_bounce_vy: None,
            hitbox_facing_sign: 1.0,
            min_hitstop_frames: 6,
        },
        // Down-Special Grounded (Down-B): Aegis Breaker Counter Stance
        AttackKind::DownSpecialCounter => MoveSpec {
            offset: Vec2::ZERO,
            radius: 56.0,
            damage: 16.0,
            base_kb: 290.0,
            kb_scale: 3.4,
            momentum_mult: 0.0,
            angle_deg: 40.0,
            active_secs: 0.38,
            cooldown_secs: 0.50,
            recoil_bounce_vy: None,
            hitbox_facing_sign: 1.0,
            min_hitstop_frames: 8,
        },
        // Down-Special Airborne (Down-B): Meteor Breaker Dive & Ground Shockwave Slam
        AttackKind::DownSpecialDive => MoveSpec {
            offset: Vec2::new(0.0, -20.0),
            radius: 20.0,
            damage: 14.0,
            base_kb: 245.0,
            kb_scale: 3.1,
            momentum_mult: 0.35,
            angle_deg: -80.0,
            active_secs: 0.55,
            cooldown_secs: 0.36,
            recoil_bounce_vy: None,
            hitbox_facing_sign: 1.0,
            min_hitstop_frames: 6,
        },
        // Stylish Victory Taunt pose (cancellable anytime)
        AttackKind::Taunt => MoveSpec {
            offset: Vec2::ZERO,
            radius: 0.0,
            damage: 0.0,
            base_kb: 0.0,
            kb_scale: 0.0,
            momentum_mult: 0.0,
            angle_deg: 0.0,
            active_secs: 0.58,
            cooldown_secs: 0.15,
            recoil_bounce_vy: None,
            hitbox_facing_sign: 1.0,
            min_hitstop_frames: 3,
        },
    }
}

/// Marker identifying a hitbox that follows its owner's position each tick.
#[derive(Component, Debug, Clone, Copy)]
pub struct AttachedHitbox;

/// Handles grounded Smash energy Shield, Shield Roll-Dodge (`Left/Right`), Shield Spot-Dodge (`Down`),
/// and airborne directional Air-Dodge (1 per airtime).
#[allow(clippy::type_complexity)]
fn handle_player_shield_and_air_dodge(
    mut commands: Commands,
    fixed_time: Res<Time<Fixed>>,
    mut callout: ResMut<LastMoveCallout>,
    mut query: Query<
        (
            Entity,
            &PlayerIntent,
            &mut KinematicBody,
            &mut ShieldState,
            &mut AirDodgeState,
            Option<&mut GroundDodgeState>,
            Option<&SmashChargeState>,
            Option<&Hitstop>,
            Option<&Hitstun>,
        ),
        With<Player>,
    >,
    mut move_events: MessageWriter<MovementEvent>,
) {
    let dt = fixed_time.delta_secs();

    for (
        entity,
        intent,
        mut body,
        mut shield,
        mut dodge,
        mut maybe_gdodge,
        smash_charge,
        hitstop,
        hitstun,
    ) in &mut query
    {
        if hitstop.is_some_and(Hitstop::is_active) {
            continue;
        }

        shield.broken_timer = (shield.broken_timer - dt).max(0.0);
        if dodge.active_timer > 0.0 {
            dodge.active_timer = (dodge.active_timer - dt).max(0.0);
        } else if dodge.recovery_timer > 0.0 {
            dodge.recovery_timer = (dodge.recovery_timer - dt).max(0.0);
        }

        // Tick grounded Smash Roll-Dodge / Spot-Dodge
        let mut gdodge_busy = false;
        if let Some(ref mut gd) = maybe_gdodge {
            if gd.active_timer > 0.0 {
                gd.active_timer = (gd.active_timer - dt).max(0.0);
                if gd.kind != GroundDodgeKind::Spot && body.state == MovementState::Grounded {
                    let frac = (gd.active_timer / 0.24).clamp(0.32, 1.0);
                    body.ground_speed = gd.roll_dir * (275.0 * frac);
                    body.velocity.x = body.ground_speed;
                }
                if gd.active_timer <= 0.0 && gd.flip_on_finish {
                    // Classic Smash Bros. roll finish: automatically turn toward the opponent behind you!
                    body.facing = -gd.roll_dir;
                    body.ground_speed *= 0.20;
                    body.velocity.x = body.ground_speed;
                    gd.flip_on_finish = false;
                }
            } else if gd.recovery_timer > 0.0 {
                gd.recovery_timer = (gd.recovery_timer - dt).max(0.0);
            }
            if body.state != MovementState::Grounded {
                gd.active_timer = 0.0;
                gd.recovery_timer = 0.0;
                gd.flip_on_finish = false;
            }
            gdodge_busy = gd.is_busy();
        }

        let grounded_or_wall = matches!(
            body.state,
            MovementState::Grounded | MovementState::WallSliding { .. }
        );
        if grounded_or_wall {
            dodge.used_in_air = false;
            dodge.active_timer = 0.0;
        }

        let impaired = hitstun.is_some_and(Hitstun::is_active)
            || smash_charge.is_some_and(SmashChargeState::is_charging)
            || matches!(
                body.state,
                MovementState::LoopLocked { .. } | MovementState::SpinCharging { .. }
            );

        if impaired || gdodge_busy {
            shield.active = false;
            if shield.broken_timer <= 0.0 {
                shield.health = (shield.health + 12.0 * dt).min(shield.max_health);
            }
            continue;
        }

        // Grounded Shield -> Roll-Dodge (Shield + Left/Right) or Spot-Dodge (Shield + Down)!
        if body.state == MovementState::Grounded && intent.guard_held && shield.broken_timer <= 0.0 {
            if let Some(ref mut gd) = maybe_gdodge
                && shield.active
            {
                if intent.left_just_pressed || intent.right_just_pressed {
                    let roll_dir = if intent.left_just_pressed { -1.0 } else { 1.0 };
                    gd.kind = if roll_dir * body.facing >= 0.0 {
                        GroundDodgeKind::RollForward
                    } else {
                        GroundDodgeKind::RollBackward
                    };
                    gd.active_timer = 0.24;
                    gd.recovery_timer = 0.08;
                    gd.roll_dir = roll_dir;
                    gd.flip_on_finish = true;
                    body.ground_speed = roll_dir * 275.0;
                    body.velocity.x = body.ground_speed;
                    shield.active = false;
                    commands.entity(entity).insert(Invulnerable::new(0.22));
                    callout.trigger("ROLL DODGE!", false);
                    move_events.write(MovementEvent {
                        entity,
                        kind: MovementFxKind::RollDodge,
                        pos: body.position,
                        dir: Vec2::new(roll_dir, 0.0),
                    });
                    continue;
                } else if intent.down_just_pressed {
                    gd.kind = GroundDodgeKind::Spot;
                    gd.active_timer = 0.22;
                    gd.recovery_timer = 0.08;
                    gd.roll_dir = 0.0;
                    gd.flip_on_finish = false;
                    body.ground_speed = 0.0;
                    body.velocity.x = 0.0;
                    shield.active = false;
                    commands.entity(entity).insert(Invulnerable::new(0.20));
                    callout.trigger("SPOT DODGE!", false);
                    move_events.write(MovementEvent {
                        entity,
                        kind: MovementFxKind::SpotDodge,
                        pos: body.position,
                        dir: Vec2::Y,
                    });
                    continue;
                }
            }

            shield.active = true;
            shield.health = (shield.health - 18.0 * dt).max(0.0);
            if shield.health <= 0.0 {
                shield.active = false;
                shield.broken_timer = 1.6;
                shield.health = 25.0;
                commands.entity(entity).insert(Hitstun::new(0.85));
            }
        } else {
            shield.active = false;
            if shield.broken_timer <= 0.0 {
                shield.health = (shield.health + 26.0 * dt).min(shield.max_health);
            }
        }

        // Airborne Directional Air-Dodge (brief i-frames + crisp directional burst)
        if body.state == MovementState::Airborne
            && intent.guard_just_pressed
            && !dodge.used_in_air
            && !dodge.is_busy()
        {
            dodge.used_in_air = true;
            dodge.active_timer = 0.24;
            dodge.recovery_timer = 0.16;

            let dir = if intent.move_axis.length() > 0.22 {
                intent.move_axis.normalize()
            } else {
                Vec2::ZERO
            };
            dodge.direction = dir;
            if dir != Vec2::ZERO {
                body.velocity = dir * 290.0;
            } else {
                body.velocity *= 0.35;
            }

            commands.entity(entity).insert(Invulnerable::new(0.24));
            callout.trigger("AIR DODGE!", false);
            move_events.write(MovementEvent {
                entity,
                kind: MovementFxKind::AirDodge,
                pos: body.position,
                dir: if dir != Vec2::ZERO { dir } else { Vec2::Y },
            });
        }
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn handle_player_melee_and_special(
    mut commands: Commands,
    fixed_time: Res<Time<Fixed>>,
    frame_data: Res<FighterFrameData>,
    control_cfg: Res<ControlConfig>,
    mut callout: ResMut<LastMoveCallout>,
    active_attached: Query<(Entity, &Hitbox), With<AttachedHitbox>>,
    mut vis_events: MessageWriter<CombatVisualEvent>,
    mut move_events: MessageWriter<MovementEvent>,
    mut query: Query<
        (
            Entity,
            &Team,
            &PlayerIntent,
            &mut KinematicBody,
            &mut AttackState,
            Option<&mut SmashChargeState>,
            Option<&mut CounterStance>,
            Option<&mut MeteorDiveState>,
            Option<&ShieldState>,
            Option<&AirDodgeState>,
            Option<&GroundDodgeState>,
            Option<&PlayerRespawnState>,
            Option<&Hitstop>,
            Option<&Hitstun>,
        ),
        With<Player>,
    >,
) {
    let dt = fixed_time.delta_secs();

    for (
        entity,
        team,
        intent,
        mut body,
        mut attack,
        mut maybe_smash_charge,
        mut maybe_counter,
        mut maybe_dive,
        shield,
        dodge,
        ground_dodge,
        respawn,
        hitstop,
        hitstun,
    ) in &mut query
    {
        // Freeze combat state transitions during Hitstop or Angel Respawn hover
        if hitstop.is_some_and(Hitstop::is_active) || respawn.is_some_and(|r| r.active) {
            continue;
        }

        if let Some(ref mut c) = maybe_counter {
            c.active_timer = (c.active_timer - dt).max(0.0);
            c.cooldown = (c.cooldown - dt).max(0.0);
        }

        // Refresh Up-Special & Side-Special recoveries whenever touching ground or wall-sliding
        if matches!(
            body.state,
            MovementState::Grounded | MovementState::WallSliding { .. }
        ) {
            attack.up_special_used = false;
            attack.side_special_used = false;
        }

        attack.cooldown = (attack.cooldown - dt).max(0.0);
        attack.jab_combo_window = (attack.jab_combo_window - dt).max(0.0);
        attack.buster_anim_timer = (attack.buster_anim_timer - dt).max(0.0);
        attack.buffered_melee_timer = (attack.buffered_melee_timer - dt).max(0.0);
        attack.buffered_heavy_timer = (attack.buffered_heavy_timer - dt).max(0.0);
        attack.buffered_special_timer = (attack.buffered_special_timer - dt).max(0.0);
        if intent.melee_just_pressed {
            attack.buffered_melee_timer = 0.12;
        }
        if intent.heavy_just_pressed {
            attack.buffered_heavy_timer = 0.12;
        }
        if intent.special_just_pressed {
            attack.buffered_special_timer = 0.12;
        }
        if attack.jab_combo_window <= 0.0 {
            attack.jab_combo_step = 0;
        }
        let prev_active_timer = attack.active_timer;
        if attack.active_timer > 0.0 {
            attack.active_timer = (attack.active_timer - dt).max(0.0);
            // Allow gentle horizontal steering + second whirlwind ring during Grounded Cyclone Sword Spin!
            if attack.active_move == Some(AttackKind::CycloneSpin)
                && body.state == MovementState::Grounded
            {
                let steer = if intent.move_axis.x.abs() > 0.25 {
                    intent.move_axis.x.signum() * 145.0
                } else {
                    body.ground_speed * 0.88
                };
                body.ground_speed = steer;
                body.velocity.x = steer;
                if prev_active_timer > 0.20 && attack.active_timer <= 0.20 {
                    vis_events.write(CombatVisualEvent::CycloneSpin {
                        pos: body.position + Vec2::new(0.0, 4.0),
                        radius: 36.0,
                        color_rgb: [0.35, 4.2, 5.4],
                    });
                }
            }
            // Allow immediate cancel out of Taunt on any movement or action press!
            if attack.active_move == Some(AttackKind::Taunt)
                && (intent.move_axis.length_squared() > 0.2
                    || intent.jump_just_pressed
                    || intent.melee_just_pressed
                    || intent.special_just_pressed
                    || intent.guard_held)
            {
                attack.active_move = None;
                attack.active_timer = 0.0;
                attack.cooldown = 0.0;
            } else if attack.active_timer <= 0.0 {
                attack.active_move = None;
            }
        }

        let counter_active = maybe_counter.as_ref().is_some_and(|c| c.is_active());
        let dive_active = maybe_dive.as_ref().is_some_and(|d| d.active);
        let stunned_or_busy = hitstun.is_some_and(Hitstun::is_active)
            || shield.is_some_and(|s| s.active)
            || dodge.is_some_and(AirDodgeState::is_busy)
            || ground_dodge.is_some_and(GroundDodgeState::is_busy)
            || counter_active
            || dive_active
            || matches!(
                body.state,
                MovementState::LoopLocked { .. } | MovementState::SpinCharging { .. }
            );

        if stunned_or_busy {
            if let Some(ref mut sc) = maybe_smash_charge {
                **sc = SmashChargeState::default();
            }
            continue;
        }

        // 0. Active Chargeable Smash Attack (hold Melee or HeavySmash to power up 1.0x -> 1.55x, release to unleash!)
        if let Some(ref mut sc) = maybe_smash_charge
            && let Some(target_smash) = sc.kind
        {
            if body.state != MovementState::Grounded {
                **sc = SmashChargeState::default();
            } else {
                sc.charge_secs += dt;
                body.ground_speed *= (1.0 - dt * 14.0).max(0.0);
                body.velocity.x = body.ground_speed;

                let hold_active = if sc.charge_source_heavy {
                    intent.heavy_held
                } else {
                    intent.melee_held
                };
                let should_release = !hold_active || sc.charge_secs >= MAX_SMASH_CHARGE_SECS;
                if should_release {
                    let (final_kind, charge_ratio) = if let Some(tilt) = sc.quick_tap_tilt
                        && sc.charge_secs < SMASH_HOLD_PROMOTE_SECS
                    {
                        (tilt, 0.0)
                    } else {
                        (target_smash, sc.charge_ratio())
                    };
                    **sc = SmashChargeState::default();

                    if final_kind == AttackKind::ForwardSmash {
                        body.ground_speed = body.facing * (95.0 + charge_ratio * 60.0);
                        body.velocity.x = body.ground_speed;
                    }
                    attack.buster_anim_timer = 0.0;
                    attack.buffered_melee_timer = 0.0;
                    attack.buffered_heavy_timer = 0.0;
                    let attacker_speed = body.speed();
                    execute_melee_attack(
                        &mut commands,
                        entity,
                        *team,
                        &body,
                        &mut attack,
                        attacker_speed,
                        final_kind,
                        charge_ratio,
                        &frame_data,
                        &active_attached,
                        &mut callout,
                        &mut vis_events,
                    );
                }
                continue;
            }
        }

        if attack.cooldown > 0.0 {
            continue;
        }

        let attacker_speed = body.speed();
        let special_triggered = intent.special_just_pressed || attack.buffered_special_timer > 0.0;
        let heavy_triggered = intent.heavy_just_pressed || attack.buffered_heavy_timer > 0.0;
        let melee_triggered = intent.melee_just_pressed || attack.buffered_melee_timer > 0.0;

        // 0b. Stylish Smash Victory Taunt (Select / T while grounded)
        if intent.taunt_just_pressed && body.state == MovementState::Grounded {
            let spec = resolve_move_spec(AttackKind::Taunt, &frame_data);
            body.ground_speed = 0.0;
            body.velocity.x = 0.0;
            attack.active_move = Some(AttackKind::Taunt);
            attack.active_timer = spec.active_secs;
            attack.cooldown = spec.cooldown_secs;
            callout.trigger(AttackKind::Taunt.display_name(), false);
            move_events.write(MovementEvent {
                entity,
                kind: MovementFxKind::Taunt,
                pos: body.position,
                dir: Vec2::Y,
            });
            vis_events.write(CombatVisualEvent::ShockwaveRing {
                pos: body.position + Vec2::new(0.0, 6.0),
                max_radius: 32.0,
                color_rgb: [0.4, 4.0, 5.2],
            });
            continue;
        }

        // 1. Full 4-Direction Smash Specials (Grounded Up-B Hurricane Sword Spin, Airborne Up-B Rising Cyclone,
        //    Down-B Counter/Dive, Side-B Blitz, Neutral-B Breaker Wave)
        if special_triggered {
            attack.buffered_special_timer = 0.0;
            attack.buster_anim_timer = 0.0;
            let grounded = body.state == MovementState::Grounded;
            if intent.move_axis.y < -0.45 {
                if grounded {
                    // Grounded Down-B: Aegis Breaker Counter Stance!
                    let can_counter = maybe_counter
                        .as_ref()
                        .is_none_or(|c| c.cooldown <= 0.0);
                    if can_counter {
                        let spec = resolve_move_spec(AttackKind::DownSpecialCounter, &frame_data);
                        if let Some(ref mut c) = maybe_counter {
                            c.active_timer = spec.active_secs;
                            c.cooldown = spec.active_secs + 0.35;
                        }
                        body.ground_speed = 0.0;
                        body.velocity.x = 0.0;
                        attack.active_move = Some(AttackKind::DownSpecialCounter);
                        attack.active_timer = spec.active_secs;
                        attack.cooldown = spec.cooldown_secs;
                        callout.trigger(AttackKind::DownSpecialCounter.display_name(), true);
                        vis_events.write(CombatVisualEvent::ShockwaveRing {
                            pos: body.position,
                            max_radius: 34.0,
                            color_rgb: [4.8, 3.6, 0.6],
                        });
                        continue;
                    }
                } else {
                    // Airborne Down-B: Meteor Breaker Dive Slam!
                    let spec = resolve_move_spec(AttackKind::DownSpecialDive, &frame_data);
                    if let Some(ref mut d) = maybe_dive {
                        d.active = true;
                    }
                    body.velocity = Vec2::new(body.velocity.x * 0.20, -560.0);
                    body.ground_speed = 0.0;
                    body.drop_through_timer = 0.0; // Land solidly on platforms for shockwave slam!
                    attack.active_move = Some(AttackKind::DownSpecialDive);
                    attack.active_timer = spec.active_secs;
                    attack.cooldown = spec.cooldown_secs;
                    callout.trigger(AttackKind::DownSpecialDive.display_name(), true);
                    despawn_owner_attached_hitboxes(&mut commands, entity, &active_attached);
                    spawn_melee_hitbox(
                        &mut commands,
                        entity,
                        *team,
                        &body,
                        attacker_speed,
                        &spec,
                        false,
                        false,
                    );
                    vis_events.write(CombatVisualEvent::SlashArc {
                        pos: body.position + Vec2::new(0.0, -14.0),
                        facing: body.facing,
                        angle_rad: -std::f32::consts::FRAC_PI_2,
                        radius: 26.0,
                        heavy: true,
                        color_rgb: [4.8, 2.2, 0.4],
                    });
                    continue;
                }
            } else if grounded && intent.move_axis.y > 0.40 {
                // Grounded Up-B: Iconic 360° HURRICANE SWORD SPIN ATTACK (dual-sided, animated, ledge-clamped!)
                let spec = resolve_move_spec(AttackKind::CycloneSpin, &frame_data);
                if intent.move_axis.x.abs() > 0.25 {
                    body.facing = intent.move_axis.x.signum();
                }
                body.ground_speed = body.facing * 125.0;
                body.velocity.x = body.ground_speed;
                attack.active_move = Some(AttackKind::CycloneSpin);
                attack.active_timer = spec.active_secs;
                attack.cooldown = spec.cooldown_secs;
                callout.trigger(AttackKind::CycloneSpin.display_name(), true);
                despawn_owner_attached_hitboxes(&mut commands, entity, &active_attached);
                spawn_melee_hitbox(
                    &mut commands,
                    entity,
                    *team,
                    &body,
                    attacker_speed.max(220.0),
                    &spec,
                    true,
                    true,
                );
                vis_events.write(CombatVisualEvent::CycloneSpin {
                    pos: body.position + Vec2::new(0.0, 4.0),
                    radius: 38.0,
                    color_rgb: [0.35, 4.2, 5.4],
                });
                continue;
            } else if intent.move_axis.y <= 0.40
                && intent.move_axis.x.abs() > 0.45
                && !attack.side_special_used
            {
                // Side-B: Plasma Blitz electric horizontal dash-slash!
                let spec = resolve_move_spec(AttackKind::SideSpecial, &frame_data);
                let dir = intent.move_axis.x.signum();
                body.facing = dir;
                body.velocity.x = dir * 365.0;
                body.ground_speed = dir * 365.0;
                if !grounded {
                    body.velocity.y = body.velocity.y.max(45.0);
                    attack.side_special_used = true;
                }
                attack.active_move = Some(AttackKind::SideSpecial);
                attack.active_timer = spec.active_secs;
                attack.cooldown = spec.cooldown_secs;
                callout.trigger(AttackKind::SideSpecial.display_name(), true);
                despawn_owner_attached_hitboxes(&mut commands, entity, &active_attached);
                spawn_melee_hitbox(
                    &mut commands,
                    entity,
                    *team,
                    &body,
                    attacker_speed.max(340.0),
                    &spec,
                    false,
                    false,
                );
                vis_events.write(CombatVisualEvent::SideBlitz {
                    pos: body.position,
                    facing: dir,
                });
                vis_events.write(CombatVisualEvent::SlashArc {
                    pos: body.position + Vec2::new(dir * 18.0, 4.0),
                    facing: dir,
                    angle_rad: 0.0,
                    radius: 30.0,
                    heavy: true,
                    color_rgb: [0.3, 4.0, 5.4],
                });
                continue;
            } else if grounded && intent.move_axis.y <= 0.40 && intent.move_axis.x.abs() <= 0.45 {
                // Grounded Neutral-B: Breaker Crescent Energy Wave!
                let spec = resolve_move_spec(AttackKind::NeutralSpecial, &frame_data);
                attack.active_move = Some(AttackKind::NeutralSpecial);
                attack.active_timer = spec.active_secs;
                attack.cooldown = spec.cooldown_secs;
                callout.trigger(AttackKind::NeutralSpecial.display_name(), false);
                let origin = body.position + Vec2::new(body.facing * 24.0, 4.0);
                commands.spawn((
                    Name::new("BreakerCrescentWave"),
                    BreakerWaveProjectile {
                        owner: entity,
                        velocity: Vec2::new(body.facing * 335.0, 0.0),
                        facing: body.facing,
                    },
                    Hitbox {
                        owner: entity,
                        team: *team,
                        offset: Vec2::ZERO,
                        radius: 20.0,
                        damage: spec.damage,
                        base_kb: spec.base_kb,
                        kb_scale: spec.kb_scale,
                        momentum_mult: 0.0,
                        angle_deg: spec.angle_deg,
                        attacker_speed: 0.0,
                        facing: body.facing,
                        lifetime: Timer::from_seconds(0.85, TimerMode::Once),
                        piercing: true,
                        despawn_on_expire: true,
                        recoil_bounce_vy: None,
                        hit_entities: HashSet::new(),
                        is_charged: true,
                        min_hitstop_frames: 4,
                    },
                    Transform::from_xyz(origin.x, origin.y, 4.0),
                    Visibility::default(),
                ));
                vis_events.write(CombatVisualEvent::SlashArc {
                    pos: origin,
                    facing: body.facing,
                    angle_rad: 0.0,
                    radius: 28.0,
                    heavy: true,
                    color_rgb: [0.25, 3.8, 5.2],
                });
                continue;
            } else if !attack.up_special_used {
                // Airborne Up-B (or Airborne Neutral-B): Rising 360° Cyclone Recovery + horizontal air-brake!
                let spec = resolve_move_spec(AttackKind::UpSpecial, &frame_data);
                let steer_vx = if intent.move_axis.x.abs() > 0.2 {
                    let dir = intent.move_axis.x.signum();
                    body.facing = dir;
                    dir * 135.0
                } else {
                    (body.velocity.x * 0.18).clamp(-110.0, 110.0)
                };
                body.velocity.x = steer_vx;
                body.ground_speed = steer_vx;
                body.velocity.y = 435.0;
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
                callout.trigger(AttackKind::UpSpecial.display_name(), true);

                despawn_owner_attached_hitboxes(&mut commands, entity, &active_attached);
                spawn_melee_hitbox(
                    &mut commands,
                    entity,
                    *team,
                    &body,
                    attacker_speed,
                    &spec,
                    true,
                    false,
                );
                vis_events.write(CombatVisualEvent::CycloneSpin {
                    pos: body.position + Vec2::new(0.0, 10.0),
                    radius: 30.0,
                    color_rgb: [0.35, 3.8, 5.4],
                });
                continue;
            }
        }

        // 1b. Classic Smash Bros. Right-Stick (C-Stick) Instant Smashes & Aerials!
        if intent.c_stick_just_flicked {
            let cs = intent.c_stick_axis;
            attack.buster_anim_timer = 0.0;
            match body.state {
                MovementState::Airborne | MovementState::WallSliding { .. } => {
                    let kind = if cs.y < -0.45 && cs.y.abs() >= cs.x.abs() {
                        AttackKind::DownAir
                    } else if cs.y > 0.45 && cs.y.abs() >= cs.x.abs() {
                        AttackKind::UpAir
                    } else if cs.x * body.facing < -0.35 {
                        AttackKind::BackAir
                    } else {
                        AttackKind::ForwardAir
                    };
                    execute_melee_attack(
                        &mut commands,
                        entity,
                        *team,
                        &body,
                        &mut attack,
                        attacker_speed,
                        kind,
                        0.0,
                        &frame_data,
                        &active_attached,
                        &mut callout,
                        &mut vis_events,
                    );
                    continue;
                }
                MovementState::Grounded => {
                    let kind = if cs.y < -0.45 && cs.y.abs() >= cs.x.abs() {
                        body.ground_speed *= 0.20;
                        body.velocity.x = body.ground_speed;
                        AttackKind::DownSmash
                    } else if cs.y > 0.45 && cs.y.abs() >= cs.x.abs() {
                        body.ground_speed *= 0.25;
                        body.velocity.x = body.ground_speed;
                        AttackKind::UpSmash
                    } else {
                        if cs.x.abs() > 0.25 {
                            body.facing = cs.x.signum();
                        }
                        body.ground_speed = body.facing * 95.0;
                        body.velocity.x = body.ground_speed;
                        AttackKind::ForwardSmash
                    };
                    execute_melee_attack(
                        &mut commands,
                        entity,
                        *team,
                        &body,
                        &mut attack,
                        attacker_speed,
                        kind,
                        0.0,
                        &frame_data,
                        &active_attached,
                        &mut callout,
                        &mut vis_events,
                    );
                    continue;
                }
                MovementState::LoopLocked { .. } | MovementState::SpinCharging { .. } => {}
            }
        }

        // 1c. Dedicated Heavy / Chargeable Smash Button (`U` / `V` / `R3`):
        //     Fires or holds-to-charge F-Smash, Up-Smash, or Down-Smash with ZERO conflict with instant sword combos!
        if heavy_triggered {
            attack.buffered_heavy_timer = 0.0;
            attack.buster_anim_timer = 0.0;
            match body.state {
                MovementState::Airborne | MovementState::WallSliding { .. } => {
                    let kind = if intent.move_axis.y < -0.35 {
                        AttackKind::DownAir
                    } else if intent.move_axis.y > 0.40 {
                        AttackKind::UpAir
                    } else if intent.move_axis.x * body.facing < -0.45 {
                        AttackKind::BackAir
                    } else {
                        AttackKind::ForwardAir
                    };
                    execute_melee_attack(
                        &mut commands,
                        entity,
                        *team,
                        &body,
                        &mut attack,
                        attacker_speed,
                        kind,
                        0.0,
                        &frame_data,
                        &active_attached,
                        &mut callout,
                        &mut vis_events,
                    );
                    continue;
                }
                MovementState::Grounded => {
                    let target_smash = if intent.move_axis.y < -0.45 {
                        AttackKind::DownSmash
                    } else if intent.move_axis.y > 0.45 {
                        AttackKind::UpSmash
                    } else {
                        if intent.move_axis.x.abs() > 0.25 {
                            body.facing = intent.move_axis.x.signum();
                        }
                        AttackKind::ForwardSmash
                    };
                    let can_charge_heavy = control_cfg.smash_charge
                        && intent.heavy_held
                        && maybe_smash_charge.is_some();
                    if can_charge_heavy
                        && let Some(ref mut sc) = maybe_smash_charge
                    {
                        body.ground_speed = 0.0;
                        body.velocity.x = 0.0;
                        **sc = SmashChargeState {
                            kind: Some(target_smash),
                            quick_tap_tilt: None,
                            charge_secs: 0.0,
                            charge_source_heavy: true,
                        };
                    } else {
                        if target_smash == AttackKind::ForwardSmash {
                            body.ground_speed = body.facing * 95.0;
                            body.velocity.x = body.ground_speed;
                        } else {
                            body.ground_speed *= 0.20;
                            body.velocity.x = body.ground_speed;
                        }
                        execute_melee_attack(
                            &mut commands,
                            entity,
                            *team,
                            &body,
                            &mut attack,
                            attacker_speed,
                            target_smash,
                            0.0,
                            &frame_data,
                            &active_attached,
                            &mut callout,
                            &mut vis_events,
                        );
                    }
                    continue;
                }
                MovementState::LoopLocked { .. } | MovementState::SpinCharging { .. } => continue,
            }
        }

        // 2. Instant Sword-First Melee Normals (3-Hit Advancing Saber Combo, Zero-Lag Tilts,
        //    Dash Crescent, Standstill Chargeable Smashes, & 5-Way Aerials!)
        if melee_triggered {
            attack.buffered_melee_timer = 0.0;
            attack.buster_anim_timer = 0.0;
            match body.state {
                MovementState::Airborne | MovementState::WallSliding { .. } => {
                    let kind = if intent.move_axis.y < -0.35 {
                        AttackKind::DownAir
                    } else if intent.move_axis.y > 0.40 {
                        AttackKind::UpAir
                    } else if intent.move_axis.x * body.facing < -0.45 {
                        AttackKind::BackAir
                    } else if intent.move_axis.x.abs() > 0.35 {
                        if body.velocity.x.abs() > 260.0 {
                            body.velocity.x *= 0.80;
                        }
                        AttackKind::ForwardAir
                    } else {
                        AttackKind::NeutralAir
                    };
                    execute_melee_attack(
                        &mut commands,
                        entity,
                        *team,
                        &body,
                        &mut attack,
                        attacker_speed,
                        kind,
                        0.0,
                        &frame_data,
                        &active_attached,
                        &mut callout,
                        &mut vis_events,
                    );
                }
                MovementState::Grounded => {
                    let can_charge = control_cfg.smash_charge
                        && intent.melee_held
                        && maybe_smash_charge.is_some();

                    if intent.move_axis.y < -0.45 {
                        body.ground_speed *= 0.20;
                        body.velocity.x = body.ground_speed;
                        if intent.boost_held {
                            if can_charge
                                && let Some(ref mut sc) = maybe_smash_charge
                            {
                                **sc = SmashChargeState {
                                    kind: Some(AttackKind::DownSmash),
                                    quick_tap_tilt: None,
                                    charge_secs: 0.0,
                                    charge_source_heavy: false,
                                };
                            } else {
                                execute_melee_attack(
                                    &mut commands,
                                    entity,
                                    *team,
                                    &body,
                                    &mut attack,
                                    attacker_speed,
                                    AttackKind::DownSmash,
                                    0.0,
                                    &frame_data,
                                    &active_attached,
                                    &mut callout,
                                    &mut vis_events,
                                );
                            }
                        } else {
                            // Zero-lag instant Low Saber Sweep (Down-Tilt)!
                            // (Use `U`/`V`, Right Stick, or `Boost + Down + Melee` for 360° Down-Smash!)
                            execute_melee_attack(
                                &mut commands,
                                entity,
                                *team,
                                &body,
                                &mut attack,
                                attacker_speed,
                                AttackKind::DownTilt,
                                0.0,
                                &frame_data,
                                &active_attached,
                                &mut callout,
                                &mut vis_events,
                            );
                        }
                    } else if intent.move_axis.y > 0.45 {
                        body.ground_speed *= 0.25;
                        body.velocity.x = body.ground_speed;
                        if intent.boost_held {
                            if can_charge
                                && let Some(ref mut sc) = maybe_smash_charge
                            {
                                **sc = SmashChargeState {
                                    kind: Some(AttackKind::UpSmash),
                                    quick_tap_tilt: None,
                                    charge_secs: 0.0,
                                    charge_source_heavy: false,
                                };
                            } else {
                                execute_melee_attack(
                                    &mut commands,
                                    entity,
                                    *team,
                                    &body,
                                    &mut attack,
                                    attacker_speed,
                                    AttackKind::UpSmash,
                                    0.0,
                                    &frame_data,
                                    &active_attached,
                                    &mut callout,
                                    &mut vis_events,
                                );
                            }
                        } else {
                            // Zero-lag instant 180° Overhead Moon Crescent (Up-Tilt)!
                            // (Use `U`/`V`, Right Stick, or `Boost + Up + Melee` for Skyward Up-Smash!)
                            execute_melee_attack(
                                &mut commands,
                                entity,
                                *team,
                                &body,
                                &mut attack,
                                attacker_speed,
                                AttackKind::UpTilt,
                                0.0,
                                &frame_data,
                                &active_attached,
                                &mut callout,
                                &mut vis_events,
                            );
                        }
                    } else if attacker_speed >= 230.0
                        && intent.move_axis.x * body.facing > 0.35
                        && attack.jab_combo_step == 0
                    {
                        // High-Speed Sprint Dash Crescent Slash (controlled lunge, ledge-clamped!)
                        let boost = (attacker_speed * 0.95).clamp(225.0, 315.0);
                        body.ground_speed = body.facing * boost;
                        body.velocity.x = body.ground_speed;
                        execute_melee_attack(
                            &mut commands,
                            entity,
                            *team,
                            &body,
                            &mut attack,
                            attacker_speed,
                            AttackKind::DashAttack,
                            0.0,
                            &frame_data,
                            &active_attached,
                            &mut callout,
                            &mut vis_events,
                        );
                    } else if intent.move_axis.x.abs() > 0.72
                        && attacker_speed <= 25.0
                        && attack.jab_combo_step == 0
                    {
                        // Standstill Hard-Side Forward-Smash: tap for heavy Titan F-Smash, hold to charge up to 1.55x!
                        body.facing = intent.move_axis.x.signum();
                        if can_charge
                            && let Some(ref mut sc) = maybe_smash_charge
                        {
                            body.ground_speed = 0.0;
                            body.velocity.x = 0.0;
                            **sc = SmashChargeState {
                                kind: Some(AttackKind::ForwardSmash),
                                quick_tap_tilt: None,
                                charge_secs: 0.0,
                                charge_source_heavy: false,
                            };
                        } else {
                            body.ground_speed = body.facing * 95.0;
                            body.velocity.x = body.ground_speed;
                            execute_melee_attack(
                                &mut commands,
                                entity,
                                *team,
                                &body,
                                &mut attack,
                                attacker_speed,
                                AttackKind::ForwardSmash,
                                0.0,
                                &frame_data,
                                &active_attached,
                                &mut callout,
                                &mut vis_events,
                            );
                        }
                    } else {
                        // Instant Advancing 3-Hit Rift Saber Combo (works both standing AND while walking/chaining!)
                        if intent.move_axis.x.abs() > 0.25 {
                            body.facing = intent.move_axis.x.signum();
                        }
                        let (jab_kind, lunge_spd) = match attack.jab_combo_step {
                            0 => {
                                attack.jab_combo_step = 1;
                                attack.jab_combo_window = 0.58;
                                (AttackKind::Jab1, 90.0_f32)
                            }
                            1 => {
                                attack.jab_combo_step = 2;
                                attack.jab_combo_window = 0.58;
                                (AttackKind::Jab2, 108.0_f32)
                            }
                            _ => {
                                attack.jab_combo_step = 0;
                                attack.jab_combo_window = 0.0;
                                (AttackKind::Jab3, 145.0_f32)
                            }
                        };
                        let step_mag = if intent.move_axis.x.abs() > 0.25 {
                            lunge_spd
                        } else {
                            lunge_spd * 0.60
                        };
                        body.ground_speed = body.facing * step_mag;
                        body.velocity.x = body.ground_speed;
                        execute_melee_attack(
                            &mut commands,
                            entity,
                            *team,
                            &body,
                            &mut attack,
                            attacker_speed,
                            jab_kind,
                            0.0,
                            &frame_data,
                            &active_attached,
                            &mut callout,
                            &mut vis_events,
                        );
                    }
                }
                MovementState::LoopLocked { .. } | MovementState::SpinCharging { .. } => continue,
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_melee_attack(
    commands: &mut Commands,
    entity: Entity,
    team: Team,
    body: &KinematicBody,
    attack: &mut AttackState,
    attacker_speed: f32,
    kind: AttackKind,
    charge_ratio: f32,
    frame_data: &FighterFrameData,
    active_attached: &Query<(Entity, &Hitbox), With<AttachedHitbox>>,
    callout: &mut LastMoveCallout,
    vis_events: &mut MessageWriter<CombatVisualEvent>,
) {
    let mut spec = resolve_move_spec(kind, frame_data);
    let is_charged_smash = charge_ratio >= 0.15;
    if charge_ratio > 0.0 {
        spec.damage *= 1.0 + 0.55 * charge_ratio;
        spec.base_kb *= 1.0 + 0.45 * charge_ratio;
        spec.kb_scale *= 1.0 + 0.35 * charge_ratio;
        if charge_ratio >= 0.55 {
            spec.min_hitstop_frames = (spec.min_hitstop_frames + 1).min(8);
        }
    }

    attack.active_move = Some(kind);
    attack.active_timer = spec.active_secs;
    attack.cooldown = spec.cooldown_secs;

    let label = if is_charged_smash {
        format!("CHARGED {} ({:.0}%)", kind.display_name(), 100.0 + charge_ratio * 55.0)
    } else {
        kind.display_name().to_string()
    };
    callout.trigger(label, kind.is_heavy_callout() || is_charged_smash);

    despawn_owner_attached_hitboxes(commands, entity, active_attached);
    spawn_melee_hitbox(
        commands,
        entity,
        team,
        body,
        attacker_speed,
        &spec,
        kind.is_dual_sided(),
        is_charged_smash,
    );

    if is_charged_smash {
        vis_events.write(CombatVisualEvent::SmashChargeRelease {
            pos: body.position,
            facing: body.facing,
            charge_ratio,
        });
    }

    if kind == AttackKind::DownSmash {
        vis_events.write(CombatVisualEvent::CycloneSpin {
            pos: body.position + Vec2::new(0.0, -4.0),
            radius: 34.0 + charge_ratio * 8.0,
            color_rgb: [5.0, 2.8, 0.42],
        });
    } else if kind == AttackKind::NeutralAir {
        vis_events.write(CombatVisualEvent::CycloneSpin {
            pos: body.position,
            radius: 26.0,
            color_rgb: [0.4, 4.0, 5.4],
        });
    } else {
        let (heavy, arc_angle, color_rgb) = match kind {
            AttackKind::ForwardSmash => (true, -0.06, [5.0, 2.8, 0.35]),
            AttackKind::Jab3 => (true, 0.04, [0.45, 4.4, 5.6]),
            AttackKind::UpSmash => (true, std::f32::consts::FRAC_PI_2, [5.2, 3.2, 0.42]),
            AttackKind::DownAir => (true, -std::f32::consts::FRAC_PI_2, [4.8, 2.0, 0.40]),
            AttackKind::UpTilt | AttackKind::UpAir => {
                (false, std::f32::consts::FRAC_PI_2, [0.32, 3.9, 5.4])
            }
            AttackKind::DownTilt => (false, -0.36, [0.35, 4.0, 5.2]),
            AttackKind::BackAir => (true, 0.0, [4.8, 2.8, 0.55]),
            AttackKind::Jab2 => (false, 0.48, [0.40, 4.2, 5.5]),
            AttackKind::Jab1 => (false, -0.18, [0.32, 3.8, 5.4]),
            AttackKind::DashAttack => (true, -0.12, [0.38, 4.2, 5.5]),
            AttackKind::ForwardAir => (false, -0.10, [0.35, 4.0, 5.4]),
            _ => (false, 0.0, [0.35, 3.6, 5.0]),
        };
        let hb_facing = body.facing * spec.hitbox_facing_sign;
        vis_events.write(CombatVisualEvent::SlashArc {
            pos: body.position + Vec2::new(spec.offset.x * hb_facing, spec.offset.y),
            facing: hb_facing,
            angle_rad: arc_angle,
            radius: spec.radius + 10.0 + charge_ratio * 7.0,
            heavy: heavy || is_charged_smash,
            color_rgb,
        });
    }
}

/// Detects when an airborne `Down-B` Meteor Breaker Dive slams onto the ground/platform,
/// detonating a wide dual-sided ground shockwave that launches nearby enemies upward!
#[allow(clippy::type_complexity)]
fn resolve_meteor_dive_impacts(
    mut commands: Commands,
    mut divers: Query<
        (Entity, &Team, &KinematicBody, &mut MeteorDiveState, &mut AttackState),
        With<Player>,
    >,
    mut vis_events: MessageWriter<CombatVisualEvent>,
) {
    for (entity, team, body, mut dive, mut attack) in &mut divers {
        if !dive.active {
            continue;
        }
        if body.state == MovementState::Grounded {
            dive.active = false;
            attack.active_move = None;
            attack.active_timer = 0.0;
            let slam_pos = Vec2::new(body.position.x, body.feet_y() + 8.0);
            commands.spawn((
                Name::new("MeteorSlamShockwave"),
                Hitbox {
                    owner: entity,
                    team: *team,
                    offset: Vec2::ZERO,
                    radius: 56.0,
                    damage: 14.0,
                    base_kb: 245.0,
                    kb_scale: 3.0,
                    momentum_mult: 0.0,
                    angle_deg: 70.0,
                    attacker_speed: 0.0,
                    facing: body.facing,
                    lifetime: Timer::from_seconds(0.12, TimerMode::Once),
                    piercing: true,
                    despawn_on_expire: true,
                    recoil_bounce_vy: None,
                    hit_entities: HashSet::new(),
                    is_charged: true,
                    min_hitstop_frames: 6,
                },
                Transform::from_xyz(slam_pos.x, slam_pos.y, 2.0),
            ));
            vis_events.write(CombatVisualEvent::GroundSlam { pos: slam_pos });
            vis_events.write(CombatVisualEvent::ShockwaveRing {
                pos: slam_pos,
                max_radius: 62.0,
                color_rgb: [4.8, 2.4, 0.45],
            });
        }
    }
}

fn advance_breaker_waves(
    mut commands: Commands,
    fixed_time: Res<Time<Fixed>>,
    solids: Query<&SolidRect>,
    mut waves: Query<(Entity, &BreakerWaveProjectile, &Hitbox, &mut Transform)>,
) {
    let dt = fixed_time.delta_secs();
    let solid_list: Vec<SolidRect> = solids.iter().copied().collect();

    for (entity, wave, hb, mut tf) in &mut waves {
        tf.translation.x += wave.velocity.x * dt;
        tf.translation.y += wave.velocity.y * dt;

        let p = tf.translation.truncate();
        let hit_wall = solid_list.iter().any(|rect| {
            let min = rect.min();
            let max = rect.max();
            p.x + hb.radius * 0.6 >= min.x
                && p.x - hb.radius * 0.6 <= max.x
                && p.y + hb.radius * 0.6 >= min.y
                && p.y - hb.radius * 0.6 <= max.y
        });
        if hit_wall {
            commands.entity(entity).despawn();
        }
    }
}

fn despawn_owner_attached_hitboxes(
    commands: &mut Commands,
    owner: Entity,
    attached: &Query<(Entity, &Hitbox), With<AttachedHitbox>>,
) {
    for (hb_entity, hb) in attached {
        if hb.owner == owner {
            commands.entity(hb_entity).despawn();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_melee_hitbox(
    commands: &mut Commands,
    owner: Entity,
    team: Team,
    body: &KinematicBody,
    attacker_speed: f32,
    spec: &MoveSpec,
    dual_sided: bool,
    is_charged: bool,
) {
    let hb_facing = body.facing * spec.hitbox_facing_sign;
    let world_offset = Vec2::new(spec.offset.x * hb_facing, spec.offset.y);
    let pos = body.position + world_offset;
    let mut ec = commands.spawn((
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
            facing: hb_facing,
            lifetime: Timer::from_seconds(spec.active_secs, TimerMode::Once),
            piercing: true, // Cleaves multiple distinct targets once each via HashSet dedup
            despawn_on_expire: true,
            recoil_bounce_vy: spec.recoil_bounce_vy,
            hit_entities: HashSet::new(),
            is_charged,
            min_hitstop_frames: spec.min_hitstop_frames,
        },
        Transform::from_xyz(pos.x, pos.y, 1.0),
    ));
    if dual_sided {
        ec.insert(DualSidedHitbox);
    }
}

#[allow(clippy::type_complexity)]
fn handle_buster_weapon(
    mut commands: Commands,
    fixed_time: Res<Time<Fixed>>,
    frame_data: Res<FighterFrameData>,
    projectiles_q: Query<&BusterProjectile>,
    mut shooters: Query<
        (
            Entity,
            &Team,
            &PlayerIntent,
            &KinematicBody,
            &mut BusterState,
            Option<&mut AttackState>,
            Option<&ShieldState>,
            Option<&AirDodgeState>,
            Option<&GroundDodgeState>,
            Option<&SmashChargeState>,
            Option<&CounterStance>,
            Option<&MeteorDiveState>,
            Option<&PlayerRespawnState>,
            Option<&Hitstop>,
            Option<&Hitstun>,
        ),
        With<Player>,
    >,
    mut shot_events: MessageWriter<BusterShotEvent>,
) {
    let dt = fixed_time.delta_secs();
    let muzzle_local = frame_data.buster_muzzle_offset();

    for (
        entity,
        team,
        intent,
        body,
        mut buster,
        mut maybe_attack,
        shield,
        dodge,
        ground_dodge,
        smash_charge,
        counter,
        dive,
        respawn,
        hitstop,
        hitstun,
    ) in &mut shooters
    {
        if hitstop.is_some_and(Hitstop::is_active) || respawn.is_some_and(|r| r.active) {
            continue;
        }
        buster.fire_cooldown = (buster.fire_cooldown - dt).max(0.0);

        if hitstun.is_some_and(Hitstun::is_active)
            || shield.is_some_and(|s| s.active)
            || dodge.is_some_and(AirDodgeState::is_busy)
            || ground_dodge.is_some_and(GroundDodgeState::is_busy)
            || smash_charge.is_some_and(SmashChargeState::is_charging)
            || counter.is_some_and(CounterStance::is_active)
            || dive.is_some_and(|d| d.active)
            || matches!(body.state, MovementState::SpinCharging { .. })
        {
            buster.charge_secs = 0.0;
            buster.was_held = false;
            continue;
        }

        // Count currently active uncharged pellets belonging to this shooter
        let active_pellets = projectiles_q
            .iter()
            .filter(|p| p.owner == entity && !p.charged)
            .count();

        let muzzle_pos = body.position + Vec2::new(body.facing * muzzle_local.x, muzzle_local.y);

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
            hit_entities: HashSet::new(),
            is_charged: charged,
            min_hitstop_frames: if charged { 6 } else { 3 },
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
    solids: Query<&SolidRect, Without<crate::physics::BreakableWall>>,
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

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn resolve_hitbox_hurtbox_collisions(
    mut commands: Commands,
    mut hitboxes: Query<(Entity, &mut Hitbox, &Transform, Has<DualSidedHitbox>)>,
    mut targets: Query<(
        Entity,
        &Team,
        &Hurtbox,
        &mut KinematicBody,
        &mut DamagePercent,
        Option<&Invulnerable>,
        Option<&KnockbackWeight>,
        Option<&mut ShieldState>,
        Option<&mut CounterStance>,
    )>,
    mut telemetry: ResMut<LastHitTelemetry>,
    mut combo: ResMut<ComboTracker>,
    mut hit_events: MessageWriter<CombatHitEvent>,
    mut move_events: MessageWriter<MovementEvent>,
    mut vis_events: MessageWriter<CombatVisualEvent>,
) {
    let mut attacker_recoil_updates: Vec<(Entity, f32)> = Vec::new();
    let mut attacker_hitstops: Vec<(Entity, u32)> = Vec::new();

    for (hb_entity, mut hb, hb_tf, is_dual_sided) in &mut hitboxes {
        if hb.lifetime.is_finished() {
            continue;
        }
        let hb_pos = hb_tf.translation.truncate();
        let mut consumed_non_piercing = false;

        for (
            target_entity,
            target_team,
            hurtbox,
            mut target_body,
            mut percent,
            invuln,
            kb_weight,
            mut maybe_shield,
            mut maybe_counter,
        ) in &mut targets
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

            // Dual-sided 360° attacks (CycloneSpin, DownSmash, UpSpecial, Boss Whirlwind) blast foes outward!
            let hit_facing = if is_dual_sided {
                if hurt_pos.x >= hb_pos.x { 1.0 } else { -1.0 }
            } else {
                hb.facing
            };

            // Grounded Down-B Aegis Breaker Counter: negates 100% damage and blasts back!
            if let Some(ref mut counter) = maybe_counter
                && counter.is_active()
            {
                counter.active_timer = 0.0;
                hb.hit_entities.insert(target_entity);
                let counter_dir = -hit_facing;
                target_body.facing = counter_dir;
                let burst_pos = target_body.position;
                commands
                    .entity(target_entity)
                    .insert((Invulnerable::new(0.45), Hitstop::from_frames(8)));
                commands.spawn((
                    Name::new("AegisCounterBurst"),
                    DualSidedHitbox,
                    Hitbox {
                        owner: target_entity,
                        team: *target_team,
                        offset: Vec2::ZERO,
                        radius: 62.0,
                        damage: (hb.damage * 1.5).clamp(14.0, 26.0),
                        base_kb: 290.0,
                        kb_scale: 3.4,
                        momentum_mult: 0.0,
                        angle_deg: 38.0,
                        attacker_speed: 0.0,
                        facing: counter_dir,
                        lifetime: Timer::from_seconds(0.14, TimerMode::Once),
                        piercing: true,
                        despawn_on_expire: true,
                        recoil_bounce_vy: None,
                        hit_entities: HashSet::new(),
                        is_charged: true,
                        min_hitstop_frames: 8,
                    },
                    Transform::from_xyz(burst_pos.x, burst_pos.y, 2.0),
                ));
                vis_events.write(CombatVisualEvent::CounterTrigger { pos: burst_pos });
                vis_events.write(CombatVisualEvent::ShockwaveRing {
                    pos: burst_pos,
                    max_radius: 68.0,
                    color_rgb: [5.0, 4.0, 0.8],
                });
                if !hb.piercing {
                    consumed_non_piercing = true;
                    break;
                }
                continue;
            }

            // Grounded Smash Shield cleanly blocks incoming attacks with zero % damage!
            if let Some(ref mut shield) = maybe_shield
                && shield.active
            {
                shield.health = (shield.health - hb.damage * 2.4).max(0.0);
                target_body.ground_speed = hit_facing * 95.0;
                target_body.velocity.x = hit_facing * 95.0;
                hb.hit_entities.insert(target_entity);
                move_events.write(MovementEvent {
                    entity: target_entity,
                    kind: MovementFxKind::ShieldBlock,
                    pos: hb_pos.lerp(hurt_pos, 0.5),
                    dir: Vec2::new(-hit_facing, 0.25).normalize(),
                });
                if shield.health <= 0.0 {
                    shield.active = false;
                    shield.broken_timer = 1.6;
                    shield.health = 25.0;
                    commands.entity(target_entity).insert(Hitstun::new(1.0));
                }
                if !hb.piercing {
                    consumed_non_piercing = true;
                    break;
                }
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
            let kb_vec = knockback_vector(kb_mag, hb.angle_deg, hit_facing);
            let hitstop_frames = compute_hitstop_frames(kb_mag)
                .max(hb.min_hitstop_frames)
                .clamp(3, 8);
            let hitstun_secs = (0.14 + kb_mag / 1180.0).clamp(0.14, 0.80);

            target_body.velocity = kb_vec;
            target_body.ground_speed = kb_vec.x;
            if kb_vec.y > 35.0 || target_body.state == MovementState::Grounded {
                // Crisp upward liftoff clearance so Smash launch arcs leave the floor cleanly!
                if kb_vec.y > 45.0 {
                    target_body.position.y += 3.0;
                }
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

            hb.hit_entities.insert(target_entity);

            if hb.team == Team::Player && *target_team == Team::Enemy {
                combo.register_hit(hb.damage);
            }

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
        if let Ok((_, _, _, mut body, _, _, _, _, _)) = targets.get_mut(attacker) {
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
