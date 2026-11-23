//! Enemy AI roster (Rogue Striker, Iron Brawler, Plasma Arcanist, Shadow Assassin,
//! and End-of-Level Boss *IRON TITAN: VULCANUS*), Lockdown Ambush Arenas with laser
//! energy gates, Wall Turrets, spike hazards, Smash Blast-Zone KOs, and Enemy HP KOs.
//!
//! Completely decoupled from rendering (`MinimalPlugins`-safe).

use std::collections::HashSet;
use std::time::Duration;

use bevy::prelude::*;

use crate::combat::{
    AttachedHitbox, CombatHitEvent, CombatPostPhysicsSystems, CombatPrePhysicsSystems,
    CombatVisualEvent, DamagePercent, DualSidedHitbox, Hitbox, Hitstop, Hitstun, Hurtbox,
    Invulnerable, KnockbackWeight, Team,
};
use crate::physics::{
    BreakableWall, CheckpointBeacon, CollectibleItem, CollectibleKind, HazardBeamPhase,
    KinematicBody, MovementEvent, MovementFxKind, MovementState, OneWayPlatform, SlopeSegment,
    SolidRect, SpikeHazard, TimedHazardBeam,
};
use crate::player::{DEFAULT_PLAYER_SPAWN, Player};
use crate::state::{
    AppState, AspectDiamond, CampaignState, GameStateCommand, StageId, StageRestartEvent,
    StageSwitchEvent,
};
use crate::story::{
    StoryBeat, StoryTriggerEvent, relic_beat_for_stage, stage_relic_names, stage1_relic_index_for_x,
};

/// Respawn delay in seconds after knocking an endless arena fighter into a Smash blast zone.
pub const ENEMY_KO_RESPAWN_SECS: f32 = 3.0;

/// Enemy fighter archetype (4 regular enemy kits + towering End-of-Level Titan Boss).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnemyArchetype {
    /// Agile fast-mover rendered with baked Rogue sprite sheets.
    LightStriker,
    /// Heavy bruiser rendered with baked Barbarian sprite sheets (cleave + seismic ground stomp).
    HeavyBrawler,
    /// Ranged arcane zoner rendered with baked Mage sprite sheets (aimed plasma orbs + phase blink).
    ArcaneZoner,
    /// High-speed ambush hunter rendered with baked Hooded Rogue sprite sheets (shadow dash-through + dive).
    ShadowAssassin,
    /// Towering 144px Stage 1 Boss (*IRON TITAN: VULCANUS*) with 2-phase melee & shockwave kit.
    FoundryBoss,
    /// Towering 144px Stage 2 Boss (*SUB-ZERO WARDEN: BOREALIS*) guarding the Stasis Diamond.
    CryoBoss,
}

impl EnemyArchetype {
    pub fn is_boss(self) -> bool {
        matches!(self, EnemyArchetype::FoundryBoss | EnemyArchetype::CryoBoss)
    }

    pub fn display_name(self) -> &'static str {
        match self {
            EnemyArchetype::LightStriker => "STRIDER DRONE (MK-II)",
            EnemyArchetype::HeavyBrawler => "SLAG ENFORCER",
            EnemyArchetype::ArcaneZoner => "ARC WEAVER",
            EnemyArchetype::ShadowAssassin => "PHANTOM INTERCEPTOR",
            EnemyArchetype::FoundryBoss => "IRON TITAN: VULCANUS",
            EnemyArchetype::CryoBoss => "SUB-ZERO WARDEN: BOREALIS",
        }
    }

    pub fn move_speed(self) -> f32 {
        match self {
            EnemyArchetype::LightStriker => 185.0,
            EnemyArchetype::HeavyBrawler => 105.0,
            EnemyArchetype::ArcaneZoner => 118.0,
            EnemyArchetype::ShadowAssassin => 215.0,
            EnemyArchetype::FoundryBoss => 126.0,
            EnemyArchetype::CryoBoss => 148.0,
        }
    }

    pub fn attack_range(self) -> f32 {
        match self {
            EnemyArchetype::LightStriker => 42.0,
            EnemyArchetype::HeavyBrawler => 54.0,
            EnemyArchetype::ArcaneZoner => 260.0,
            EnemyArchetype::ShadowAssassin => 165.0,
            EnemyArchetype::FoundryBoss => 78.0,
            EnemyArchetype::CryoBoss => 86.0,
        }
    }

    pub fn telegraph_secs(self) -> f32 {
        match self {
            EnemyArchetype::LightStriker => 0.34,
            EnemyArchetype::HeavyBrawler => 0.52,
            EnemyArchetype::ArcaneZoner => 0.48,
            EnemyArchetype::ShadowAssassin => 0.30,
            EnemyArchetype::FoundryBoss => 0.48,
            EnemyArchetype::CryoBoss => 0.44,
        }
    }

    pub fn half_extents(self) -> Vec2 {
        match self {
            EnemyArchetype::LightStriker => Vec2::new(10.0, 18.0),
            EnemyArchetype::HeavyBrawler => Vec2::new(12.0, 19.0),
            EnemyArchetype::ArcaneZoner => Vec2::new(10.0, 18.0),
            EnemyArchetype::ShadowAssassin => Vec2::new(10.0, 18.0),
            EnemyArchetype::FoundryBoss | EnemyArchetype::CryoBoss => Vec2::new(20.0, 30.0),
        }
    }

    pub fn max_hp(self) -> f32 {
        match self {
            EnemyArchetype::LightStriker => 42.0,
            EnemyArchetype::HeavyBrawler => 78.0,
            EnemyArchetype::ArcaneZoner => 38.0,
            EnemyArchetype::ShadowAssassin => 44.0,
            EnemyArchetype::FoundryBoss => 240.0,
            EnemyArchetype::CryoBoss => 280.0,
        }
    }
}

/// Finite state machine for stage, ambush, and boss AI fighters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EnemyAiState {
    Patrol,
    Chase,
    /// Clear visual telegraph before striking so the player can punish/counter.
    Telegraph {
        remaining: f32,
    },
    Attacking {
        remaining: f32,
    },
    Recovery {
        remaining: f32,
    },
}

/// Real combat health bar for enemies & the End Boss (runs alongside Smash `%` knockback scaling!).
#[derive(Component, Debug, Clone, Copy)]
pub struct EnemyHealth {
    pub current: f32,
    pub max: f32,
}

impl EnemyHealth {
    pub fn new(max: f32) -> Self {
        Self { current: max, max }
    }

    pub fn ratio(&self) -> f32 {
        (self.current / self.max.max(1.0)).clamp(0.0, 1.0)
    }
}

/// End-of-Level Boss state tracking 2-phase progression, locked laser aim & arena containment.
#[derive(Component, Debug, Clone, Copy)]
pub struct BossState {
    pub phase: u8,
    pub enraged_triggered: bool,
    pub pattern_step: u8,
    pub arena_min_x: f32,
    pub arena_max_x: f32,
    pub slam_pending: bool,
    /// Aiming vector locked when Phase 2 laser/volley telegraph begins so dodging warning lines is 100% fair!
    pub locked_aim_dir: Vec2,
}

impl BossState {
    pub fn new(arena_min_x: f32, arena_max_x: f32) -> Self {
        Self {
            phase: 1,
            enraged_triggered: false,
            pattern_step: 0,
            arena_min_x,
            arena_max_x,
            slam_pending: false,
            locked_aim_dir: -Vec2::X,
        }
    }
}

/// Delayed ground-rune Arcane / Stasis Cryo Pillar Eruption.
#[derive(Component, Debug, Clone)]
pub struct PendingArcanePillar {
    pub owner: Entity,
    pub pos: Vec2,
    pub timer: f32,
    pub is_cryo: bool,
}

/// Arena / stage AI fighter component.
#[derive(Component, Debug, Clone)]
pub struct EnemyFighter {
    pub archetype: EnemyArchetype,
    pub home_pos: Vec2,
    pub state: EnemyAiState,
    pub jump_cooldown: f32,
    pub special_cooldown: f32,
    /// Distinguishes active attack variants:
    /// - `0` = primary melee slash / cleave
    /// - `1` = seismic stomp / boss tremor slam
    /// - `2` = arcane orb / boss forge volley
    /// - `3` = shadow dash-through / boss bull-rush
    /// - `4` = Rogue Striker 2nd combo cross-slash (`jab_2`)
    /// - `5` = Rogue Striker backstep evade roll (`dodge_back`)
    /// - `6` = Iron Brawler & Vulcanus 360° Whirlwind Greataxe Spin (`sword_spin`)
    /// - `7` = Plasma Arcanist Arcane Pillar Eruption (`up_tilt`)
    /// - `8` = Shadow Assassin Smoke Backflip + Kunai throw (`dodge_back`)
    pub attack_variant: u8,
    /// Alternates signature enemy moves so encounters stay varied & rhythmic.
    pub pattern_counter: u8,
    /// `true` for endless sparring fighters (3s respawn), `false` for stage/ambush/boss progression.
    pub respawns: bool,
    /// Optional explicit horizontal patrol bounds (`(min_x, max_x)`) for lockdown rooms & islands.
    pub leash_bounds: Option<(f32, f32)>,
}

impl EnemyFighter {
    pub fn new(archetype: EnemyArchetype, home_pos: Vec2) -> Self {
        Self {
            archetype,
            home_pos,
            state: EnemyAiState::Patrol,
            jump_cooldown: 0.0,
            special_cooldown: 0.75,
            attack_variant: 0,
            pattern_counter: 0,
            respawns: true,
            leash_bounds: None,
        }
    }

    pub fn is_telegraphing(&self) -> bool {
        matches!(self.state, EnemyAiState::Telegraph { .. })
    }
}

/// Lifecycle phase of a [`LockdownEncounter`] ambush or boss chamber.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncounterPhase {
    Idle,
    Active,
    Cleared,
}

/// Multi-wave Lockdown Ambush Arena or Boss Chamber that traps the player inside glowing
/// laser [`LockdownGate`] barriers until all waves (or the Boss) are defeated!
#[derive(Component, Debug, Clone)]
pub struct LockdownEncounter {
    pub id: u32,
    pub title: String,
    pub is_boss: bool,
    pub trigger_x: f32,
    pub min_x: f32,
    pub max_x: f32,
    pub floor_y: f32,
    pub arena_center: Vec2,
    pub spawn_right_gate: bool,
    pub waves: Vec<Vec<(EnemyArchetype, Vec2)>>,
    pub current_wave: usize,
    pub phase: EncounterPhase,
    pub active_enemies: Vec<Entity>,
    pub gate_entities: Vec<Entity>,
}

impl LockdownEncounter {
    pub fn total_waves(&self) -> usize {
        self.waves.len().max(1)
    }
}

/// Glowing physical laser energy barrier spawned by an active [`LockdownEncounter`].
#[derive(Component, Debug, Clone, Copy)]
pub struct LockdownGate {
    pub encounter_id: u32,
    pub is_boss: bool,
}

/// Live HUD & camera coordination resource for active Lockdown Ambush rooms & Boss Fights.
#[derive(Resource, Debug, Clone, Default)]
pub struct EncounterStatus {
    pub active_title: Option<String>,
    pub is_boss: bool,
    pub current_wave: usize,
    pub total_waves: usize,
    pub enemies_remaining: usize,
    pub camera_lock_center: Option<Vec2>,
    pub banner_text: String,
    pub banner_timer: f32,
    /// Brief dramatic delay (`1.15s`) after defeating the Boss before transitioning to `AppState::Victory`.
    pub victory_delay_timer: Option<f32>,
}

/// Active player progression checkpoint updated automatically when advancing through the stage.
#[derive(Resource, Debug, Clone, Copy)]
pub struct PlayerCheckpoint(pub Vec2);

impl Default for PlayerCheckpoint {
    fn default() -> Self {
        Self(DEFAULT_PLAYER_SPAWN)
    }
}

/// Wall-mounted Mega Man shaft turret that telegraphs and fires plasma bolts,
/// rewarding Buster shots (`SPEC.MD` Section 6 Zone B).
#[derive(Component, Debug, Clone)]
pub struct WallTurret {
    pub home_pos: Vec2,
    /// Firing direction (`+1.0` right, `-1.0` left).
    pub facing: f32,
    pub fire_timer: f32,
    pub fire_interval: f32,
    pub telegraph_secs: f32,
    pub hp_threshold: f32,
    pub disabled_timer: f32,
}

impl WallTurret {
    pub fn new(home_pos: Vec2, facing: f32) -> Self {
        Self {
            home_pos,
            facing: if facing < 0.0 { -1.0 } else { 1.0 },
            fire_timer: 1.6,
            fire_interval: 2.3,
            telegraph_secs: 0.45,
            hp_threshold: 7.0,
            disabled_timer: 0.0,
        }
    }

    pub fn is_active(&self) -> bool {
        self.disabled_timer <= 0.0
    }

    pub fn is_telegraphing(&self) -> bool {
        self.is_active() && self.fire_timer <= self.telegraph_secs
    }
}

/// Enemy energy bolt fired by a [`WallTurret`], [`EnemyArchetype::ArcaneZoner`],
/// [`EnemyArchetype::FoundryBoss`] ground shockwave, or [`EnemyArchetype::CryoBoss`] frost wave.
#[derive(Component, Debug, Clone, Copy)]
pub struct TurretProjectile {
    pub velocity: Vec2,
    /// True when fired by a Plasma Arcanist mage (renders vivid violet-magenta arcane energy).
    pub is_arcane: bool,
    /// True when fired as a Boss ground-skimming molten shockwave (glides across deck).
    pub is_magma_wave: bool,
    /// True when fired by Sub-Zero Warden Borealis (renders glacial frost-blue energy).
    pub is_cryo_wave: bool,
}

/// Smash Arena blast-zone volume (`min..=max`); leaving triggers a KO flash, SFX, and respawn.
#[derive(Component, Debug, Clone, Copy)]
pub struct BlastZone {
    pub min: Vec2,
    pub max: Vec2,
    pub arena_center: Vec2,
}

impl BlastZone {
    pub fn contains(&self, pos: Vec2) -> bool {
        pos.x >= self.min.x && pos.x <= self.max.x && pos.y >= self.min.y && pos.y <= self.max.y
    }

    /// Classifies which edge of the blast zone `pos` crossed for directional KO flash beams.
    pub fn crossed_edge(&self, pos: Vec2) -> BlastEdge {
        if pos.x > self.max.x {
            BlastEdge::Right
        } else if pos.x < self.min.x {
            BlastEdge::Left
        } else if pos.y > self.max.y {
            BlastEdge::Top
        } else {
            BlastEdge::Bottom
        }
    }
}

/// Edge of the [`BlastZone`] where a Smash KO occurred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlastEdge {
    Left,
    Right,
    Top,
    Bottom,
}

/// Pending 3-second Smash respawn entry tracked deterministically inside `FixedUpdate`.
#[derive(Component, Debug, Clone)]
pub struct PendingEnemyRespawn {
    pub archetype: EnemyArchetype,
    pub home_pos: Vec2,
    pub timer: Timer,
}

/// Message emitted whenever an arena fighter, boss, OR the player is KO'd.
#[derive(Message, Debug, Clone, Copy)]
pub struct BlastZoneKoEvent {
    pub is_player: bool,
    pub archetype: Option<EnemyArchetype>,
    pub ko_pos: Vec2,
    pub edge: BlastEdge,
    pub final_percent: f32,
}

impl BlastZoneKoEvent {
    pub fn display_name(self) -> &'static str {
        if self.is_player {
            "P1 LEVY"
        } else {
            self.archetype
                .map_or("FIGHTER", EnemyArchetype::display_name)
        }
    }
}

/// Tracks live stage KO score, turret takedowns, and collectible progress (for S/A/B/C Ranking!).
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct ArenaScore {
    pub ko_count: u32,
    pub turrets_destroyed: u32,
    pub encounters_cleared: u32,
    pub cores_collected: u32,
    pub cores_total: u32,
    pub relics_collected: u32,
    pub relics_total: u32,
    pub walls_broken: u32,
}

impl ArenaScore {
    /// Computes classic Mega Man X / Sonic stage clear grade (`S`, `A`, `B`, or `C`).
    pub fn compute_stage_rank(&self, clear_secs: f32, best_combo: u32) -> &'static str {
        let mut pts: u32 = 0;
        if clear_secs <= 185.0 {
            pts += 3;
        } else if clear_secs <= 260.0 {
            pts += 2;
        } else {
            pts += 1;
        }
        pts += self.relics_collected.min(3) * 2;
        if self.cores_total > 0 && self.cores_collected * 2 >= self.cores_total {
            pts += 2;
        } else if self.cores_collected >= 5 {
            pts += 1;
        }
        if best_combo >= 5 {
            pts += 2;
        } else if best_combo >= 3 {
            pts += 1;
        }
        if pts >= 10 {
            "S"
        } else if pts >= 7 {
            "A"
        } else if pts >= 4 {
            "B"
        } else {
            "C"
        }
    }
}

/// Helper spawning a destructible cracked Foundry [`BreakableWall`] blocking secret vaults & shortcuts.
pub fn spawn_breakable_wall(commands: &mut Commands, center: Vec2, size: Vec2, hp: f32) -> Entity {
    commands
        .spawn((
            Name::new("BreakableWall"),
            BreakableWall::new(center, size, hp),
            SolidRect::new(center, size),
            Team::Enemy,
            Hurtbox {
                offset: Vec2::ZERO,
                radius: (size.x.max(size.y) * 0.50).clamp(18.0, 44.0),
            },
            DamagePercent(0.0),
            KnockbackWeight(0.0),
            KinematicBody {
                position: center,
                half_extents: size * 0.5,
                state: MovementState::Grounded,
                facing: -1.0,
                ..default()
            },
            Transform::from_xyz(center.x, center.y, -2.0),
            Visibility::default(),
        ))
        .id()
}

/// Spawns a complete headless-safe [`EnemyFighter`] (respawns after 3s on KO by default).
pub fn spawn_enemy_fighter(
    commands: &mut Commands,
    archetype: EnemyArchetype,
    home_pos: Vec2,
    spawn_invuln: bool,
) -> Entity {
    spawn_enemy_internal(commands, archetype, home_pos, spawn_invuln, true, None)
}

/// Spawns a non-respawning stage progression or Ambush Wave [`EnemyFighter`].
pub fn spawn_stage_enemy(
    commands: &mut Commands,
    archetype: EnemyArchetype,
    home_pos: Vec2,
    spawn_invuln: bool,
    leash_bounds: Option<(f32, f32)>,
) -> Entity {
    spawn_enemy_internal(commands, archetype, home_pos, spawn_invuln, false, leash_bounds)
}

/// Spawns the towering Stage 1 Boss (*IRON TITAN: VULCANUS*).
pub fn spawn_foundry_boss(
    commands: &mut Commands,
    home_pos: Vec2,
    arena_min_x: f32,
    arena_max_x: f32,
) -> Entity {
    let e = spawn_enemy_internal(
        commands,
        EnemyArchetype::FoundryBoss,
        home_pos,
        true,
        false,
        Some((arena_min_x + 36.0, arena_max_x - 36.0)),
    );
    commands
        .entity(e)
        .insert(BossState::new(arena_min_x + 36.0, arena_max_x - 36.0));
    e
}

/// Spawns the towering Stage 2 Boss (*SUB-ZERO WARDEN: BOREALIS*).
pub fn spawn_cryo_boss(
    commands: &mut Commands,
    home_pos: Vec2,
    arena_min_x: f32,
    arena_max_x: f32,
) -> Entity {
    let e = spawn_enemy_internal(
        commands,
        EnemyArchetype::CryoBoss,
        home_pos,
        true,
        false,
        Some((arena_min_x + 36.0, arena_max_x - 36.0)),
    );
    commands
        .entity(e)
        .insert(BossState::new(arena_min_x + 36.0, arena_max_x - 36.0));
    e
}

fn spawn_enemy_internal(
    commands: &mut Commands,
    archetype: EnemyArchetype,
    home_pos: Vec2,
    spawn_invuln: bool,
    respawns: bool,
    leash_bounds: Option<(f32, f32)>,
) -> Entity {
    let half = archetype.half_extents();
    let mut fighter = EnemyFighter::new(archetype, home_pos);
    fighter.respawns = respawns;
    fighter.leash_bounds = leash_bounds;

    let hurt_radius = match archetype {
        EnemyArchetype::FoundryBoss | EnemyArchetype::CryoBoss => 26.0,
        EnemyArchetype::HeavyBrawler => 19.0,
        _ => 16.0,
    };
    let kb_weight = match archetype {
        EnemyArchetype::LightStriker => 1.15,
        EnemyArchetype::HeavyBrawler => 0.72,
        EnemyArchetype::ArcaneZoner => 1.10,
        EnemyArchetype::ShadowAssassin => 1.12,
        EnemyArchetype::FoundryBoss => 0.38,
        EnemyArchetype::CryoBoss => 0.42,
    };

    let mut ec = commands.spawn((
        Name::new(archetype.display_name()),
        fighter,
        EnemyHealth::new(archetype.max_hp()),
        Team::Enemy,
        Hurtbox {
            offset: Vec2::ZERO,
            radius: hurt_radius,
        },
        DamagePercent(0.0),
        KnockbackWeight(kb_weight),
        KinematicBody {
            position: home_pos,
            half_extents: half,
            facing: -1.0,
            ..default()
        },
        Transform::from_xyz(home_pos.x, home_pos.y, 0.0),
        Visibility::default(),
    ));
    if spawn_invuln {
        ec.insert(Invulnerable::new(0.85));
    }
    ec.id()
}

/// Helper spawning a Zone B wall-mounted [`WallTurret`] entity.
pub fn spawn_wall_turret(commands: &mut Commands, pos: Vec2, facing: f32) -> Entity {
    commands
        .spawn((
            Name::new("WallTurret"),
            WallTurret::new(pos, facing),
            Team::Enemy,
            Hurtbox {
                offset: Vec2::ZERO,
                radius: 14.0,
            },
            DamagePercent(0.0),
            KinematicBody {
                position: pos,
                half_extents: Vec2::new(10.0, 10.0),
                state: MovementState::Grounded,
                facing,
                ..default()
            },
            Transform::from_xyz(pos.x, pos.y, 0.0),
            Visibility::default(),
        ))
        .id()
}

/// Plugin managing enemy AI, Lockdown Ambush Arenas, Boss Fight, wall turrets, hazards, and KOs.
pub struct EnemiesPlugin;

impl Plugin for EnemiesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ArenaScore>()
            .init_resource::<EncounterStatus>()
            .init_resource::<PlayerCheckpoint>()
            .add_message::<BlastZoneKoEvent>()
            .add_systems(
                FixedUpdate,
                (
                    update_player_checkpoint,
                    update_checkpoint_beacons,
                    update_collectible_items,
                    update_lockdown_encounters,
                    update_enemy_fighter_ai,
                    update_boss_ai,
                    tick_pending_arcane_pillars,
                    update_wall_turrets,
                    advance_turret_projectiles,
                )
                    .chain()
                    .in_set(CombatPrePhysicsSystems),
            )
            .add_systems(
                FixedUpdate,
                (
                    resolve_spike_hazards,
                    update_timed_hazard_beams,
                    update_breakable_walls,
                    apply_enemy_hp_damage_and_kos,
                    check_blast_zone_kos,
                    tick_pending_enemy_respawns,
                )
                    .chain()
                    .in_set(CombatPostPhysicsSystems),
            )
            .add_systems(OnEnter(AppState::Title), reset_encounter_state_on_title)
            .add_systems(Update, reset_arena_score_on_restart);
    }
}

fn reset_encounter_state_on_title(
    mut score: ResMut<ArenaScore>,
    mut enc_status: ResMut<EncounterStatus>,
    mut checkpoint: ResMut<PlayerCheckpoint>,
) {
    let cores_total = score.cores_total;
    let relics_total = score.relics_total;
    *score = ArenaScore {
        cores_total,
        relics_total,
        ..default()
    };
    *enc_status = EncounterStatus::default();
    *checkpoint = PlayerCheckpoint::default();
}

fn reset_arena_score_on_restart(
    mut restarts: MessageReader<StageRestartEvent>,
    mut switches: MessageReader<StageSwitchEvent>,
    mut score: ResMut<ArenaScore>,
    mut enc_status: ResMut<EncounterStatus>,
    mut checkpoint: ResMut<PlayerCheckpoint>,
) {
    let switched = switches.read().next().is_some();
    let restarted = restarts.read().next().is_some();
    if switched {
        *score = ArenaScore::default();
        *enc_status = EncounterStatus::default();
        *checkpoint = PlayerCheckpoint::default();
    } else if restarted {
        let cores_total = score.cores_total;
        let relics_total = score.relics_total;
        *score = ArenaScore {
            cores_total,
            relics_total,
            ..default()
        };
        *enc_status = EncounterStatus::default();
        *checkpoint = PlayerCheckpoint::default();
    }
}

/// Advances fallback progression checkpoints on grounded landings across all 8 Sectors
/// (complemented by explicit glowing [`CheckpointBeacon`] pylons!).
fn update_player_checkpoint(
    campaign: Res<CampaignState>,
    players: Query<&KinematicBody, With<Player>>,
    mut checkpoint: ResMut<PlayerCheckpoint>,
) {
    let Ok(body) = players.single() else {
        return;
    };
    if body.state != MovementState::Grounded {
        return;
    }
    let stage_cps: &[(f32, Vec2)] = match campaign.active_stage {
        StageId::Stage1Foundry => &[
            (335.0, Vec2::new(380.0, 56.0)),
            (680.0, Vec2::new(720.0, 248.0)),
            (1780.0, Vec2::new(1810.0, 248.0)),
            (3490.0, Vec2::new(3530.0, 248.0)),
            (4860.0, Vec2::new(4940.0, 368.0)),
            (6630.0, Vec2::new(6680.0, 248.0)),
            (7480.0, Vec2::new(7540.0, 248.0)),
            (8300.0, Vec2::new(8360.0, 248.0)),
        ],
        StageId::Stage2Cryo => &[
            (340.0, Vec2::new(390.0, 48.0)),
            (840.0, Vec2::new(890.0, 248.0)),
            (1820.0, Vec2::new(1870.0, 248.0)),
            (3500.0, Vec2::new(3560.0, 248.0)),
            (5320.0, Vec2::new(5380.0, 348.0)),
            (6700.0, Vec2::new(6750.0, 248.0)),
            (7500.0, Vec2::new(7560.0, 248.0)),
            (8300.0, Vec2::new(8360.0, 248.0)),
        ],
    };
    for &(threshold_x, cp_pos) in stage_cps {
        if body.position.x >= threshold_x && checkpoint.0.x < cp_pos.x {
            checkpoint.0 = cp_pos;
        }
    }
}

/// Activates glowing [`CheckpointBeacon`] pylons when the player passes them,
/// updating respawn point, healing `-25%`, and flashing a crisp checkpoint banner!
#[allow(clippy::type_complexity)]
fn update_checkpoint_beacons(
    mut checkpoint: ResMut<PlayerCheckpoint>,
    mut status: ResMut<EncounterStatus>,
    mut players: Query<(Entity, &KinematicBody, &mut DamagePercent), With<Player>>,
    mut beacons: Query<&mut CheckpointBeacon>,
    mut move_events: MessageWriter<MovementEvent>,
    mut vis_events: MessageWriter<CombatVisualEvent>,
) {
    let Ok((player_entity, body, mut pct)) = players.single_mut() else {
        return;
    };
    for mut beacon in &mut beacons {
        if beacon.activated {
            continue;
        }
        let dx = (body.position.x - beacon.pos.x).abs();
        let dy = (body.position.y - (beacon.pos.y + 22.0)).abs();
        if dx <= 28.0 && dy <= 42.0 {
            beacon.activated = true;
            let safe_spawn = beacon.pos + Vec2::new(0.0, 26.0);
            if safe_spawn.x >= checkpoint.0.x {
                checkpoint.0 = safe_spawn;
            }
            pct.0 = (pct.0 - 25.0).max(0.0);
            status.banner_text = format!("CHECKPOINT {} ACTIVATED  •  -25% REPAIRED!", beacon.id);
            status.banner_timer = 2.4;
            move_events.write(MovementEvent {
                entity: player_entity,
                kind: MovementFxKind::Checkpoint,
                pos: beacon.pos + Vec2::new(0.0, 20.0),
                dir: Vec2::Y,
            });
            vis_events.write(CombatVisualEvent::ShockwaveRing {
                pos: beacon.pos + Vec2::new(0.0, 20.0),
                max_radius: 48.0,
                color_rgb: [0.35, 4.2, 5.4],
            });
        }
    }
}

/// Collects floating Energy Cores, Secret Golden Data Relics, and Emerald Heal Canisters!
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn update_collectible_items(
    mut commands: Commands,
    campaign: Res<CampaignState>,
    mut score: ResMut<ArenaScore>,
    mut status: ResMut<EncounterStatus>,
    mut players: Query<(Entity, &KinematicBody, &mut DamagePercent), With<Player>>,
    items: Query<(Entity, &CollectibleItem)>,
    mut move_events: MessageWriter<MovementEvent>,
    mut vis_events: MessageWriter<CombatVisualEvent>,
    mut story_events: MessageWriter<StoryTriggerEvent>,
) {
    let Ok((player_entity, body, mut pct)) = players.single_mut() else {
        return;
    };
    for (item_entity, item) in &items {
        let r = item.pickup_radius() + 14.0;
        if body.position.distance_squared(item.pos) > r * r {
            continue;
        }
        match item.kind {
            CollectibleKind::EnergyCore => {
                score.cores_collected += 1;
                pct.0 = (pct.0 - 5.0).max(0.0);
                move_events.write(MovementEvent {
                    entity: player_entity,
                    kind: MovementFxKind::CollectCore,
                    pos: item.pos,
                    dir: Vec2::Y,
                });
            }
            CollectibleKind::SecretRelic => {
                score.relics_collected += 1;
                pct.0 = (pct.0 - 20.0).max(0.0);
                let idx = stage1_relic_index_for_x(item.pos.x);
                let keepsake = stage_relic_names(campaign.active_stage)[idx];
                let beat = relic_beat_for_stage(campaign.active_stage, idx);
                story_events.write(StoryTriggerEvent(beat));
                status.banner_text = format!(
                    "1700s KEEPSAKE: {}  ({}/{})",
                    keepsake,
                    score.relics_collected,
                    score.relics_total.max(score.relics_collected)
                );
                status.banner_timer = 3.6;
                move_events.write(MovementEvent {
                    entity: player_entity,
                    kind: MovementFxKind::CollectRelic,
                    pos: item.pos,
                    dir: Vec2::Y,
                });
                vis_events.write(CombatVisualEvent::CounterTrigger { pos: item.pos });
            }
            CollectibleKind::HealCanister => {
                pct.0 = (pct.0 - 40.0).max(0.0);
                status.banner_text = "NANO-REPAIR CANISTER  •  -40% DAMAGE!".to_string();
                status.banner_timer = 2.0;
                move_events.write(MovementEvent {
                    entity: player_entity,
                    kind: MovementFxKind::CollectHeal,
                    pos: item.pos,
                    dir: Vec2::Y,
                });
                vis_events.write(CombatVisualEvent::ShockwaveRing {
                    pos: item.pos,
                    max_radius: 40.0,
                    color_rgb: [0.4, 4.8, 2.2],
                });
            }
        }
        commands.entity(item_entity).despawn();
    }
}

/// Processes melee/Buster damage against cracked [`BreakableWall`] bulkheads, shattering them open!
/// Equipping the **Forge Diamond** applies `2.0x` thermal melt damage against ice & steel bulkheads!
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn update_breakable_walls(
    mut commands: Commands,
    campaign: Res<CampaignState>,
    mut hit_events: MessageReader<CombatHitEvent>,
    mut score: ResMut<ArenaScore>,
    mut status: ResMut<EncounterStatus>,
    mut move_events: MessageWriter<MovementEvent>,
    mut vis_events: MessageWriter<CombatVisualEvent>,
    mut walls: Query<(Entity, &mut BreakableWall, &mut KinematicBody, &mut Transform)>,
) {
    // Keep unbroken walls strictly anchored in place
    for (entity, wall, mut body, mut tf) in &mut walls {
        body.position = wall.center;
        body.velocity = Vec2::ZERO;
        body.ground_speed = 0.0;
        body.state = MovementState::Grounded;
        tf.translation.x = wall.center.x;
        tf.translation.y = wall.center.y;
        commands.entity(entity).remove::<Hitstun>();
    }

    let thermal_mult = if campaign.active_diamond == AspectDiamond::Forge {
        2.0
    } else {
        1.0
    };
    let is_cryo = campaign.active_stage == StageId::Stage2Cryo;

    let mut broken: HashSet<Entity> = HashSet::new();
    for ev in hit_events.read() {
        if broken.contains(&ev.target) {
            continue;
        }
        let Ok((wall_entity, mut wall, _, _)) = walls.get_mut(ev.target) else {
            continue;
        };
        wall.hp = (wall.hp - ev.damage * thermal_mult).max(0.0);
        if wall.hp <= 0.0 {
            broken.insert(wall_entity);
            score.walls_broken += 1;
            status.banner_text = if thermal_mult > 1.5 && is_cryo {
                "FORGE MELT! CRYO BULKHEAD SHATTERED  •  SECRET PATH OPEN!".to_string()
            } else {
                "BULKHEAD SHATTERED  •  SECRET PATH OPEN!".to_string()
            };
            status.banner_timer = 2.4;
            move_events.write(MovementEvent {
                entity: ev.attacker,
                kind: MovementFxKind::WallBreak,
                pos: wall.center,
                dir: Vec2::Y,
            });
            vis_events.write(CombatVisualEvent::GroundSlam { pos: wall.center });
            vis_events.write(CombatVisualEvent::ShockwaveRing {
                pos: wall.center,
                max_radius: 54.0,
                color_rgb: if is_cryo {
                    [0.45, 3.8, 5.6]
                } else {
                    [5.0, 2.4, 0.4]
                },
            });
            commands.entity(wall_entity).despawn();
        }
    }
}

/// Cycles rhythmic Mega Man X [`TimedHazardBeam`] magma vents (`Off -> Telegraph -> Active`)
/// and applies non-instant-death Smash knockback + brief i-frames on contact while active.
#[allow(clippy::type_complexity)]
fn update_timed_hazard_beams(
    mut commands: Commands,
    fixed_time: Res<Time<Fixed>>,
    mut beams: Query<&mut TimedHazardBeam>,
    mut players: Query<
        (
            Entity,
            &mut KinematicBody,
            &mut DamagePercent,
            Option<&Invulnerable>,
        ),
        With<Player>,
    >,
    mut hit_events: MessageWriter<CombatHitEvent>,
) {
    let dt = fixed_time.delta_secs();
    for mut beam in &mut beams {
        beam.timer = (beam.timer + dt).rem_euclid(beam.cycle_secs());
    }

    for (player_entity, mut body, mut percent, invuln) in &mut players {
        if invuln.is_some_and(Invulnerable::is_active) {
            continue;
        }
        for beam in &beams {
            if beam.phase() == HazardBeamPhase::Active
                && beam.overlaps(body.position, body.half_extents)
            {
                percent.0 += beam.damage;
                let push_dir = if body.position.x >= beam.center.x {
                    1.0
                } else {
                    -1.0
                };
                let kb_vec = Vec2::new(push_dir * 165.0, 290.0);
                body.velocity = kb_vec;
                body.ground_speed = kb_vec.x;
                body.state = MovementState::Airborne;
                body.on_one_way = false;
                body.coyote_timer = 0.0;

                commands.entity(player_entity).insert((
                    Invulnerable::new(1.05),
                    Hitstun::new(0.24),
                    Hitstop::from_frames(4),
                ));
                hit_events.write(CombatHitEvent {
                    attacker: player_entity,
                    target: player_entity,
                    hit_pos: body.position,
                    damage: beam.damage,
                    target_percent_after: percent.0,
                    knockback_mag: kb_vec.length(),
                    knockback_vec: kb_vec,
                    hitstop_frames: 4,
                    is_charged: true,
                });
                break;
            }
        }
    }
}

/// Manages Lockdown Ambush Arenas and the End-of-Level Boss Chamber:
/// slams shut laser gates, spawns waves of enemies, tracks clears, unlocks gates, and triggers Victory!
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn update_lockdown_encounters(
    mut commands: Commands,
    fixed_time: Res<Time<Fixed>>,
    campaign: Res<CampaignState>,
    mut score: ResMut<ArenaScore>,
    mut status: ResMut<EncounterStatus>,
    mut state_commands: MessageWriter<GameStateCommand>,
    mut vis_events: MessageWriter<CombatVisualEvent>,
    mut story_events: MessageWriter<StoryTriggerEvent>,
    mut players: Query<(&KinematicBody, &mut DamagePercent), With<Player>>,
    alive_enemies: Query<Entity, With<EnemyFighter>>,
    mut encounters: Query<&mut LockdownEncounter>,
) {
    let is_stage2 = campaign.active_stage == StageId::Stage2Cryo;
    let gate_ring_rgb = if is_stage2 {
        [0.45, 3.8, 5.6]
    } else {
        [4.8, 0.55, 0.45]
    };
    let dt = fixed_time.delta_secs();
    if status.banner_timer > 0.0 {
        status.banner_timer = (status.banner_timer - dt).max(0.0);
        if status.banner_timer <= 0.0 {
            status.banner_text.clear();
        }
    }

    if let Some(ref mut v_timer) = status.victory_delay_timer {
        *v_timer -= dt;
        if *v_timer <= 0.0 {
            status.victory_delay_timer = None;
            state_commands.write(GameStateCommand::StageCleared);
        }
    }

    let Ok((player_body, mut player_pct)) = players.single_mut() else {
        return;
    };

    let mut any_active = false;

    for mut enc in &mut encounters {
        match enc.phase {
            EncounterPhase::Idle => {
                if player_body.position.x >= enc.trigger_x
                    && player_body.position.x <= enc.max_x
                    && player_body.position.y >= enc.floor_y - 35.0
                {
                    enc.phase = EncounterPhase::Active;
                    enc.current_wave = 0;

                    // Slam shut glowing laser lockdown gates!
                    let gate_y = enc.floor_y + 140.0;
                    let left_gate = commands
                        .spawn((
                            Name::new(format!("LockdownGate_{}_L", enc.id)),
                            LockdownGate {
                                encounter_id: enc.id,
                                is_boss: enc.is_boss,
                            },
                            SolidRect::new(Vec2::new(enc.min_x + 8.0, gate_y), Vec2::new(16.0, 280.0)),
                        ))
                        .id();
                    enc.gate_entities.push(left_gate);
                    vis_events.write(CombatVisualEvent::ShockwaveRing {
                        pos: Vec2::new(enc.min_x + 8.0, enc.floor_y + 42.0),
                        max_radius: 46.0,
                        color_rgb: gate_ring_rgb,
                    });

                    if enc.spawn_right_gate {
                        let right_gate = commands
                            .spawn((
                                Name::new(format!("LockdownGate_{}_R", enc.id)),
                                LockdownGate {
                                    encounter_id: enc.id,
                                    is_boss: enc.is_boss,
                                },
                                SolidRect::new(
                                    Vec2::new(enc.max_x - 8.0, gate_y),
                                    Vec2::new(16.0, 280.0),
                                ),
                            ))
                            .id();
                        enc.gate_entities.push(right_gate);
                        vis_events.write(CombatVisualEvent::ShockwaveRing {
                            pos: Vec2::new(enc.max_x - 8.0, enc.floor_y + 42.0),
                            max_radius: 46.0,
                            color_rgb: gate_ring_rgb,
                        });
                    }

                    // Spawn Wave 0 enemies (or Boss!)
                    spawn_encounter_wave(&mut commands, &mut enc, 0, &mut vis_events);

                    if enc.is_boss {
                        let intro_beat = if is_stage2 {
                            StoryBeat::BossBorealisIntro
                        } else {
                            StoryBeat::BossVulcanusIntro
                        };
                        story_events.write(StoryTriggerEvent(intro_beat));
                    }

                    status.banner_text = if enc.is_boss {
                        format!("BOSS LOCKDOWN  •  {}", enc.title)
                    } else {
                        format!(
                            "AMBUSH! {}  •  WAVE 1/{}",
                            enc.title,
                            enc.total_waves()
                        )
                    };
                    status.banner_timer = 2.8;
                }
            }
            EncounterPhase::Active => {
                any_active = true;
                // Prune defeated enemies
                enc.active_enemies
                    .retain(|&e| alive_enemies.get(e).is_ok());

                if enc.active_enemies.is_empty() {
                    if enc.current_wave + 1 < enc.waves.len() {
                        enc.current_wave += 1;
                        let wave_idx = enc.current_wave;
                        spawn_encounter_wave(&mut commands, &mut enc, wave_idx, &mut vis_events);
                        status.banner_text = format!(
                            "{}  •  WAVE {}/{} — REINFORCEMENTS!",
                            enc.title,
                            wave_idx + 1,
                            enc.total_waves()
                        );
                        status.banner_timer = 2.4;
                    } else {
                        // ENCOUNTER CLEARED! Shatter laser gates & reward player!
                        enc.phase = EncounterPhase::Cleared;
                        score.encounters_cleared += 1;
                        for gate in enc.gate_entities.drain(..) {
                            if let Ok(mut ec) = commands.get_entity(gate) {
                                ec.despawn();
                            }
                        }
                        vis_events.write(CombatVisualEvent::CounterTrigger {
                            pos: Vec2::new(enc.max_x - 16.0, enc.floor_y + 48.0),
                        });

                        // Heal player Smash % upon clearing an ambush!
                        player_pct.0 = if enc.id >= 2 {
                            0.0
                        } else {
                            (player_pct.0 - 30.0).max(0.0)
                        };

                        if enc.is_boss {
                            if is_stage2 {
                                story_events
                                    .write(StoryTriggerEvent(StoryBeat::BossBorealisDefeated));
                                status.banner_text =
                                    "STASIS DIAMOND RECOVERED  •  STAGE 2 CLEAR!".to_string();
                            } else {
                                story_events
                                    .write(StoryTriggerEvent(StoryBeat::BossVulcanusDefeated));
                                status.banner_text =
                                    "FORGE DIAMOND RECOVERED  •  STAGE CLEAR!".to_string();
                            }
                            status.banner_timer = 4.0;
                            status.victory_delay_timer = Some(1.15);
                        } else {
                            status.banner_text =
                                format!("{} CLEARED  •  GATE OPEN — ADVANCE!", enc.title);
                            status.banner_timer = 2.8;
                        }
                        any_active = false;
                        continue;
                    }
                }

                status.active_title = Some(enc.title.clone());
                status.is_boss = enc.is_boss;
                status.current_wave = enc.current_wave + 1;
                status.total_waves = enc.total_waves();
                status.enemies_remaining = enc.active_enemies.len();
                status.camera_lock_center = Some(enc.arena_center);
            }
            EncounterPhase::Cleared => {}
        }
    }

    if !any_active {
        status.active_title = None;
        status.is_boss = false;
        status.enemies_remaining = 0;
        status.camera_lock_center = None;
    }
}

fn spawn_encounter_wave(
    commands: &mut Commands,
    enc: &mut LockdownEncounter,
    wave_idx: usize,
    vis_events: &mut MessageWriter<CombatVisualEvent>,
) {
    let Some(wave) = enc.waves.get(wave_idx) else {
        return;
    };
    let bounds = Some((enc.min_x + 28.0, enc.max_x - 28.0));
    for &(arch, pos) in wave {
        let e = match arch {
            EnemyArchetype::FoundryBoss => spawn_foundry_boss(commands, pos, enc.min_x, enc.max_x),
            EnemyArchetype::CryoBoss => spawn_cryo_boss(commands, pos, enc.min_x, enc.max_x),
            _ => spawn_stage_enemy(commands, arch, pos, true, bounds),
        };
        enc.active_enemies.push(e);
        vis_events.write(CombatVisualEvent::EnemyBlink {
            from: pos + Vec2::new(0.0, 56.0),
            to: pos,
        });
    }
}

/// Subtracts damage from [`EnemyHealth`] on confirmed hits, triggers Boss Phase 2 Overdrive,
/// and detonates a dramatic Smash KO burst when an enemy's HP reaches `0`.
#[allow(clippy::type_complexity)]
fn apply_enemy_hp_damage_and_kos(
    mut commands: Commands,
    mut hit_events: MessageReader<CombatHitEvent>,
    mut score: ResMut<ArenaScore>,
    mut enc_status: ResMut<EncounterStatus>,
    mut ko_events: MessageWriter<BlastZoneKoEvent>,
    mut vis_events: MessageWriter<CombatVisualEvent>,
    mut enemies: Query<(
        &EnemyFighter,
        &KinematicBody,
        &DamagePercent,
        &mut EnemyHealth,
        Option<&mut BossState>,
    )>,
) {
    let mut defeated_set: HashSet<Entity> = HashSet::new();

    for ev in hit_events.read() {
        if defeated_set.contains(&ev.target) {
            continue;
        }
        let Ok((fighter, body, percent, mut hp, maybe_boss)) = enemies.get_mut(ev.target) else {
            continue;
        };
        hp.current = (hp.current - ev.damage).max(0.0);

        // Trigger dramatic Phase 2 Overdrive on Boss! (Vulcanus <= 50% HP, Borealis <= 55% HP)
        let phase2_threshold = if fighter.archetype == EnemyArchetype::CryoBoss {
            0.55
        } else {
            0.50
        };
        if let Some(mut boss) = maybe_boss
            && !boss.enraged_triggered
            && hp.current > 0.0
            && hp.current <= hp.max * phase2_threshold
        {
            boss.enraged_triggered = true;
            boss.phase = 2;
            commands
                .entity(ev.target)
                .insert(Invulnerable::new(0.45));
            vis_events.write(CombatVisualEvent::CounterTrigger { pos: body.position });
            let is_cryo = fighter.archetype == EnemyArchetype::CryoBoss;
            vis_events.write(CombatVisualEvent::ShockwaveRing {
                pos: body.position,
                max_radius: 86.0,
                color_rgb: if is_cryo {
                    [0.45, 3.8, 5.6]
                } else {
                    [5.2, 2.2, 0.35]
                },
            });
            enc_status.banner_text = if is_cryo {
                "BOREALIS: ABSOLUTE ZERO OVERDRIVE  •  PHASE 2!".to_string()
            } else {
                "VULCANUS OVERDRIVE  •  PHASE 2!".to_string()
            };
            enc_status.banner_timer = 2.8;
        }

        if hp.current <= 0.0 {
            defeated_set.insert(ev.target);
            score.ko_count += 1;

            let edge = if ev.knockback_vec.x >= 0.0 {
                BlastEdge::Right
            } else {
                BlastEdge::Left
            };
            ko_events.write(BlastZoneKoEvent {
                is_player: false,
                archetype: Some(fighter.archetype),
                ko_pos: body.position,
                edge,
                final_percent: percent.0,
            });
            let is_boss = fighter.archetype.is_boss();
            let ko_rgb = if fighter.archetype == EnemyArchetype::CryoBoss {
                [0.45, 4.0, 5.8]
            } else {
                [5.2, 2.8, 0.45]
            };
            vis_events.write(CombatVisualEvent::ShockwaveRing {
                pos: body.position,
                max_radius: if is_boss { 110.0 } else { 44.0 },
                color_rgb: ko_rgb,
            });
            if is_boss {
                vis_events.write(CombatVisualEvent::CounterTrigger { pos: body.position });
                vis_events.write(CombatVisualEvent::GroundSlam { pos: body.position });
            }

            if fighter.respawns {
                commands.spawn((
                    Name::new("PendingEnemyRespawn"),
                    PendingEnemyRespawn {
                        archetype: fighter.archetype,
                        home_pos: fighter.home_pos,
                        timer: Timer::from_seconds(ENEMY_KO_RESPAWN_SECS, TimerMode::Once),
                    },
                ));
            }
            commands.entity(ev.target).despawn();
        }
    }
}

/// Multi-phase End-of-Level Boss AI for *IRON TITAN: VULCANUS*!
#[allow(clippy::type_complexity)]
fn update_boss_ai(
    mut commands: Commands,
    fixed_time: Res<Time<Fixed>>,
    players: Query<&KinematicBody, (With<Player>, Without<EnemyFighter>)>,
    mut vis_events: MessageWriter<CombatVisualEvent>,
    mut bosses: Query<(
        Entity,
        &mut EnemyFighter,
        &mut BossState,
        &mut KinematicBody,
        Option<&Hitstop>,
        Option<&Hitstun>,
    )>,
) {
    let dt = fixed_time.delta_secs();
    let Ok(p_body) = players.single() else {
        return;
    };
    let p_pos = p_body.position;

    for (entity, mut fighter, mut boss, mut body, hitstop, hitstun) in &mut bosses {
        if hitstop.is_some_and(Hitstop::is_active) {
            continue;
        }
        fighter.jump_cooldown = (fighter.jump_cooldown - dt).max(0.0);
        fighter.special_cooldown = (fighter.special_cooldown - dt).max(0.0);

        // Keep boss cleanly contained inside the Boss Sanctum walls
        body.position.x = body.position.x.clamp(boss.arena_min_x, boss.arena_max_x);
        let is_cryo = fighter.archetype == EnemyArchetype::CryoBoss;
        let boss_rgb: [f32; 3] = if is_cryo {
            [0.42, 3.8, 5.6]
        } else {
            [5.2, 2.0, 0.35]
        };

        // Detect touchdown from Leap Tremor / Permafrost Shatter Slam -> unleash dual floor waves!
        if boss.slam_pending && body.state == MovementState::Grounded && body.velocity.y <= 0.0 {
            boss.slam_pending = false;
            let slam_pos = Vec2::new(body.position.x, body.feet_y() + 10.0);
            commands.spawn((
                Name::new(if is_cryo {
                    "BorealisPermafrostSlam"
                } else {
                    "BossTremorSlamHitbox"
                }),
                Hitbox {
                    owner: entity,
                    team: Team::Enemy,
                    offset: Vec2::ZERO,
                    radius: if is_cryo { 72.0 } else { 68.0 },
                    damage: 16.0,
                    base_kb: 255.0,
                    kb_scale: 2.8,
                    momentum_mult: 0.0,
                    angle_deg: 68.0,
                    attacker_speed: 0.0,
                    facing: body.facing,
                    lifetime: Timer::from_seconds(0.14, TimerMode::Once),
                    piercing: true,
                    despawn_on_expire: true,
                    recoil_bounce_vy: None,
                    hit_entities: HashSet::new(),
                    is_charged: true,
                    min_hitstop_frames: 7,
                },
                Transform::from_xyz(slam_pos.x, slam_pos.y, 1.5),
            ));
            // Roll twin ground-skimming magma or glacial frost waves left and right across the Boss Deck!
            for dir in [-1.0_f32, 1.0_f32] {
                let speed = if is_cryo {
                    if boss.phase >= 2 { 320.0 } else { 260.0 }
                } else if boss.phase >= 2 {
                    275.0
                } else {
                    225.0
                };
                commands.spawn((
                    Name::new(if is_cryo {
                        "BossCryoFrostWave"
                    } else {
                        "BossMagmaWave"
                    }),
                    TurretProjectile {
                        velocity: Vec2::new(dir * speed, 0.0),
                        is_arcane: false,
                        is_magma_wave: !is_cryo,
                        is_cryo_wave: is_cryo,
                    },
                    Hitbox {
                        owner: entity,
                        team: Team::Enemy,
                        offset: Vec2::ZERO,
                        radius: 12.0,
                        damage: 11.0,
                        base_kb: 185.0,
                        kb_scale: 1.9,
                        momentum_mult: 0.0,
                        angle_deg: 58.0,
                        attacker_speed: 0.0,
                        facing: dir,
                        lifetime: Timer::from_seconds(1.85, TimerMode::Once),
                        piercing: false,
                        despawn_on_expire: true,
                        recoil_bounce_vy: None,
                        hit_entities: HashSet::new(),
                        is_charged: true,
                        min_hitstop_frames: 4,
                    },
                    Transform::from_xyz(slam_pos.x + dir * 24.0, body.feet_y() + 14.0, 3.2),
                    Visibility::default(),
                ));
            }
            vis_events.write(CombatVisualEvent::GroundSlam { pos: slam_pos });
            vis_events.write(CombatVisualEvent::ShockwaveRing {
                pos: slam_pos,
                max_radius: 78.0,
                color_rgb: boss_rgb,
            });
            fighter.state = EnemyAiState::Recovery {
                remaining: if boss.phase >= 2 { 0.58 } else { 0.86 },
            };
            continue;
        }

        // Boss resists micro-stagger during attacks, only cancels Chase/Patrol on hitstun
        if hitstun.is_some_and(Hitstun::is_active)
            && matches!(fighter.state, EnemyAiState::Patrol | EnemyAiState::Chase)
        {
            continue;
        }

        let delta = p_pos - body.position;
        let dist = delta.length();
        let phase_speed = if boss.phase >= 2 { 1.28 } else { 1.0 };

        match fighter.state {
            EnemyAiState::Patrol => {
                fighter.state = EnemyAiState::Chase;
            }
            EnemyAiState::Chase => {
                let desired_dir = if delta.x.abs() > 10.0 {
                    delta.x.signum()
                } else {
                    body.facing
                };
                body.facing = desired_dir;

                let spd = desired_dir * fighter.archetype.move_speed() * phase_speed;
                if body.state == MovementState::Grounded {
                    body.ground_speed = spd;
                    body.velocity.x = spd;
                } else {
                    body.velocity.x = body.velocity.x.lerp(spd, (dt * 4.0).min(1.0));
                }

                // Choose next telegraphed Boss pattern!
                let close_range = if is_cryo { 88.0 } else { 82.0 };
                if dist <= close_range && delta.y.abs() < 48.0 {
                    // Close range: Greataxe / Cryo Halberd Cleave (prime Down-B Counter punish!)
                    body.ground_speed = 0.0;
                    body.velocity.x *= 0.2;
                    fighter.attack_variant = 0;
                    fighter.state = EnemyAiState::Telegraph {
                        remaining: if boss.phase >= 2 { 0.40 } else { 0.50 },
                    };
                } else if fighter.special_cooldown <= 0.0 && body.state == MovementState::Grounded {
                    body.ground_speed = 0.0;
                    body.velocity.x = 0.0;
                    let pattern_count = if is_cryo {
                        if boss.phase >= 2 { 5 } else { 3 }
                    } else if boss.phase >= 2 {
                        4
                    } else {
                        2
                    };
                    boss.pattern_step = (boss.pattern_step + 1) % pattern_count;
                    match boss.pattern_step {
                        0 => {
                            // Pattern A: Magma Bull-Rush (Vulcanus) / Permafrost Flash-Glide (Borealis)
                            fighter.attack_variant = 3;
                            fighter.state = EnemyAiState::Telegraph {
                                remaining: if boss.phase >= 2 { 0.44 } else { 0.56 },
                            };
                        }
                        1 => {
                            // Pattern B: Leap Tremor Slam / Permafrost Shatter Slam!
                            fighter.attack_variant = 1;
                            fighter.state = EnemyAiState::Telegraph {
                                remaining: if boss.phase >= 2 { 0.42 } else { 0.52 },
                            };
                        }
                        2 => {
                            // Pattern C: Forge Volley (Vulcanus P2) / Cryo Prism Laser Fan (Borealis P1+P2)!
                            let aim_dir = (p_pos - body.position)
                                .normalize_or(Vec2::new(body.facing, 0.0));
                            boss.locked_aim_dir = aim_dir;
                            let origin = body.position + Vec2::new(body.facing * 26.0, 10.0);
                            fighter.attack_variant = 2;
                            fighter.state = EnemyAiState::Telegraph { remaining: 0.48 };
                            vis_events.write(CombatVisualEvent::BossLaserTelegraph {
                                origin,
                                target_dir: aim_dir,
                                duration: 0.48,
                            });
                            vis_events.write(CombatVisualEvent::ShockwaveRing {
                                pos: origin,
                                max_radius: 34.0,
                                color_rgb: boss_rgb,
                            });
                        }
                        3 => {
                            // Pattern D (Phase 2): TITAN CYCLONE / BLIZZARD HALBERD WHIRLWIND (360° sword_spin)!
                            fighter.attack_variant = 6;
                            fighter.state = EnemyAiState::Telegraph { remaining: 0.50 };
                            vis_events.write(CombatVisualEvent::ShockwaveRing {
                                pos: body.position,
                                max_radius: 52.0,
                                color_rgb: boss_rgb,
                            });
                        }
                        _ => {
                            // Pattern E (Borealis Phase 2): STASIS FROST PILLAR ERUPTIONS!
                            fighter.attack_variant = 7;
                            fighter.state = EnemyAiState::Telegraph { remaining: 0.46 };
                            vis_events.write(CombatVisualEvent::ShockwaveRing {
                                pos: body.position + Vec2::new(0.0, 12.0),
                                max_radius: 44.0,
                                color_rgb: boss_rgb,
                            });
                        }
                    }
                }
            }
            EnemyAiState::Telegraph { mut remaining } => {
                body.ground_speed *= (1.0 - dt * 10.0).max(0.0);
                remaining -= dt;
                if remaining <= 0.0 {
                    match fighter.attack_variant {
                        // 0: Titan Greataxe / Glacial Halberd Cleave!
                        0 => {
                            let lunge = if is_cryo { 170.0 } else { 145.0 } * phase_speed;
                            body.ground_speed = body.facing * lunge;
                            body.velocity.x = body.ground_speed;
                            let offset = Vec2::new(if is_cryo { 42.0 } else { 38.0 }, 4.0);
                            let pos = body.position + Vec2::new(offset.x * body.facing, offset.y);
                            commands.spawn((
                                Name::new("BossGreataxeCleave"),
                                AttachedHitbox,
                                Hitbox {
                                    owner: entity,
                                    team: Team::Enemy,
                                    offset,
                                    radius: if is_cryo { 33.0 } else { 30.0 },
                                    damage: 16.0,
                                    base_kb: 245.0,
                                    kb_scale: 2.7,
                                    momentum_mult: 0.25,
                                    angle_deg: 34.0,
                                    attacker_speed: body.speed(),
                                    facing: body.facing,
                                    lifetime: Timer::from_seconds(0.18, TimerMode::Once),
                                    piercing: true,
                                    despawn_on_expire: true,
                                    recoil_bounce_vy: None,
                                    hit_entities: HashSet::new(),
                                    is_charged: true,
                                    min_hitstop_frames: 6,
                                },
                                Transform::from_xyz(pos.x, pos.y, 1.5),
                            ));
                            vis_events.write(CombatVisualEvent::SlashArc {
                                pos,
                                facing: body.facing,
                                angle_rad: 0.05,
                                radius: if is_cryo { 48.0 } else { 44.0 },
                                heavy: true,
                                color_rgb: boss_rgb,
                            });
                            fighter.state = EnemyAiState::Attacking { remaining: 0.22 };
                        }
                        // 1: Leap Foundry Tremor / Permafrost Shatter Slam!
                        1 => {
                            let jump_vx = (delta.x * 1.45).clamp(-260.0, 260.0);
                            body.velocity = Vec2::new(jump_vx, 485.0);
                            body.ground_speed = jump_vx;
                            body.state = MovementState::Airborne;
                            boss.slam_pending = true;
                            fighter.special_cooldown = if boss.phase >= 2 { 1.9 } else { 2.7 };
                            vis_events.write(CombatVisualEvent::ShockwaveRing {
                                pos: body.position,
                                max_radius: 44.0,
                                color_rgb: boss_rgb,
                            });
                            fighter.state = EnemyAiState::Attacking { remaining: 0.85 };
                        }
                        // 2: Forge Volley (Vulcanus) / Cryo Prism Laser Fan (Borealis)!
                        2 => {
                            let base_dir = boss
                                .locked_aim_dir
                                .normalize_or(Vec2::new(body.facing, 0.0));
                            let base_angle = base_dir.y.atan2(base_dir.x);
                            let origin = body.position + Vec2::new(body.facing * 26.0, 10.0);
                            let spreads: &[f32] = if is_cryo && boss.phase >= 2 {
                                &[-0.44, -0.22, 0.0, 0.22, 0.44]
                            } else {
                                &[-0.30, 0.0, 0.30]
                            };
                            let proj_speed = if is_cryo { 275.0 } else { 245.0 };
                            for &spread in spreads {
                                let a = base_angle + spread;
                                let vel = Vec2::new(a.cos(), a.sin()) * proj_speed;
                                commands.spawn((
                                    Name::new(if is_cryo {
                                        "BossCryoPrismBolt"
                                    } else {
                                        "BossForgeOrb"
                                    }),
                                    TurretProjectile {
                                        velocity: vel,
                                        is_arcane: false,
                                        is_magma_wave: !is_cryo,
                                        is_cryo_wave: is_cryo,
                                    },
                                    Hitbox {
                                        owner: entity,
                                        team: Team::Enemy,
                                        offset: Vec2::ZERO,
                                        radius: 9.0,
                                        damage: 10.0,
                                        base_kb: 175.0,
                                        kb_scale: 1.8,
                                        momentum_mult: 0.0,
                                        angle_deg: 36.0,
                                        attacker_speed: 0.0,
                                        facing: vel.x.signum(),
                                        lifetime: Timer::from_seconds(2.0, TimerMode::Once),
                                        piercing: false,
                                        despawn_on_expire: true,
                                        recoil_bounce_vy: None,
                                        hit_entities: HashSet::new(),
                                        is_charged: true,
                                        min_hitstop_frames: 4,
                                    },
                                    Transform::from_xyz(origin.x, origin.y, 3.2),
                                    Visibility::default(),
                                ));
                            }
                            vis_events.write(CombatVisualEvent::ShockwaveRing {
                                pos: origin,
                                max_radius: 44.0,
                                color_rgb: boss_rgb,
                            });
                            fighter.special_cooldown = if boss.phase >= 2 { 1.95 } else { 2.4 };
                            fighter.state = EnemyAiState::Attacking { remaining: 0.25 };
                        }
                        // 6: Phase 2 TITAN CYCLONE / BLIZZARD HALBERD WHIRLWIND (360° dual-sided spin!)
                        6 => {
                            let spin_spd = body.facing * if is_cryo { 230.0 } else { 195.0 };
                            body.ground_speed = spin_spd;
                            body.velocity.x = spin_spd;
                            commands.spawn((
                                Name::new("BossCycloneSpinHitbox"),
                                AttachedHitbox,
                                DualSidedHitbox,
                                Hitbox {
                                    owner: entity,
                                    team: Team::Enemy,
                                    offset: Vec2::new(0.0, 6.0),
                                    radius: 44.0,
                                    damage: 15.0,
                                    base_kb: 265.0,
                                    kb_scale: 2.8,
                                    momentum_mult: 0.20,
                                    angle_deg: 48.0,
                                    attacker_speed: spin_spd.abs(),
                                    facing: body.facing,
                                    lifetime: Timer::from_seconds(0.68, TimerMode::Once),
                                    piercing: true,
                                    despawn_on_expire: true,
                                    recoil_bounce_vy: None,
                                    hit_entities: HashSet::new(),
                                    is_charged: true,
                                    min_hitstop_frames: 6,
                                },
                                Transform::from_xyz(body.position.x, body.position.y + 6.0, 1.5),
                            ));
                            vis_events.write(CombatVisualEvent::CycloneSpin {
                                pos: body.position + Vec2::new(0.0, 6.0),
                                radius: 52.0,
                                color_rgb: boss_rgb,
                            });
                            fighter.special_cooldown = 2.1;
                            fighter.state = EnemyAiState::Attacking { remaining: 0.72 };
                        }
                        // 7: Borealis Phase 2 STASIS FROST PILLAR ERUPTIONS (twin telegraphed ice pillars!)
                        7 => {
                            let floor_y = body.feet_y();
                            for offset_x in [-72.0_f32, 0.0, 72.0] {
                                let px = (p_pos.x + offset_x)
                                    .clamp(boss.arena_min_x + 12.0, boss.arena_max_x - 12.0);
                                let rune_pos = Vec2::new(px, floor_y + 2.0);
                                commands.spawn((
                                    Name::new("BorealisStasisPillar"),
                                    PendingArcanePillar {
                                        owner: entity,
                                        pos: rune_pos,
                                        timer: 0.68,
                                        is_cryo: true,
                                    },
                                ));
                                vis_events.write(CombatVisualEvent::ArcanePillarTelegraph {
                                    pos: rune_pos,
                                    duration: 0.68,
                                });
                            }
                            fighter.special_cooldown = 2.4;
                            fighter.state = EnemyAiState::Attacking { remaining: 0.30 };
                        }
                        // 3: Magma Bull-Rush / Permafrost Flash-Glide Charge!
                        _ => {
                            let charge_spd = if is_cryo {
                                if boss.phase >= 2 { 415.0 } else { 355.0 }
                            } else if boss.phase >= 2 {
                                365.0
                            } else {
                                310.0
                            };
                            body.ground_speed = body.facing * charge_spd;
                            body.velocity.x = body.ground_speed;
                            let offset = Vec2::new(24.0, 0.0);
                            let pos = body.position + Vec2::new(offset.x * body.facing, offset.y);
                            commands.spawn((
                                Name::new("BossBullRushHitbox"),
                                AttachedHitbox,
                                Hitbox {
                                    owner: entity,
                                    team: Team::Enemy,
                                    offset,
                                    radius: 26.0,
                                    damage: 15.0,
                                    base_kb: 240.0,
                                    kb_scale: 2.5,
                                    momentum_mult: 0.20,
                                    angle_deg: 42.0,
                                    attacker_speed: charge_spd,
                                    facing: body.facing,
                                    lifetime: Timer::from_seconds(0.46, TimerMode::Once),
                                    piercing: true,
                                    despawn_on_expire: true,
                                    recoil_bounce_vy: None,
                                    hit_entities: HashSet::new(),
                                    is_charged: true,
                                    min_hitstop_frames: 6,
                                },
                                Transform::from_xyz(pos.x, pos.y, 1.5),
                            ));
                            vis_events.write(CombatVisualEvent::SideBlitz {
                                pos: body.position,
                                facing: body.facing,
                            });
                            fighter.special_cooldown = if boss.phase >= 2 { 1.8 } else { 2.5 };
                            fighter.state = EnemyAiState::Attacking { remaining: 0.48 };
                        }
                    }
                } else {
                    fighter.state = EnemyAiState::Telegraph { remaining };
                }
            }
            EnemyAiState::Attacking { mut remaining } => {
                // Keep Bull-Rush & Cyclone Spin momentum strong until timer expires or reaching arena edge
                if fighter.attack_variant == 3 || fighter.attack_variant == 6 {
                    let prev_rem = remaining;
                    if fighter.attack_variant == 6 && prev_rem > 0.36 && (prev_rem - dt) <= 0.36 {
                        vis_events.write(CombatVisualEvent::CycloneSpin {
                            pos: body.position + Vec2::new(0.0, 6.0),
                            radius: 52.0,
                            color_rgb: boss_rgb,
                        });
                    }
                    let at_wall = (body.position.x <= boss.arena_min_x + 4.0 && body.facing < 0.0)
                        || (body.position.x >= boss.arena_max_x - 4.0 && body.facing > 0.0);
                    if at_wall {
                        body.ground_speed = 0.0;
                        body.velocity.x = 0.0;
                        remaining = 0.0;
                    }
                }
                remaining -= dt;
                if remaining <= 0.0 {
                    boss.slam_pending = false;
                    fighter.state = EnemyAiState::Recovery {
                        remaining: if boss.phase >= 2 { 0.56 } else { 0.82 },
                    };
                } else {
                    fighter.state = EnemyAiState::Attacking { remaining };
                }
            }
            EnemyAiState::Recovery { mut remaining } => {
                body.ground_speed *= (1.0 - dt * 8.0).max(0.0);
                remaining -= dt;
                if remaining <= 0.0 {
                    fighter.attack_variant = 0;
                    fighter.state = EnemyAiState::Chase;
                } else {
                    fighter.state = EnemyAiState::Recovery { remaining };
                }
            }
        }
    }
}

/// Returns `true` if there is a walkable solid deck, one-way catwalk, or slope surface
/// beneath `probe_x` near `feet_y`, preventing AI fighters from voluntarily walking,
/// kiting, blinking, or lunging off platform ledges.
fn has_walkable_ground_at(
    probe_x: f32,
    feet_y: f32,
    solids: &[SolidRect],
    one_ways: &[OneWayPlatform],
    slopes: &[SlopeSegment],
) -> bool {
    let min_top = feet_y - 26.0;
    let max_top = feet_y + 18.0;

    if solids.iter().any(|s| {
        let min = s.min();
        let max = s.max();
        probe_x >= min.x + 2.0 && probe_x <= max.x - 2.0 && max.y >= min_top && max.y <= max_top
    }) {
        return true;
    }

    if one_ways.iter().any(|p| {
        let top = p.top_y();
        probe_x >= p.min_x() + 2.0
            && probe_x <= p.max_x() - 2.0
            && top >= min_top
            && top <= max_top
    }) {
        return true;
    }

    slopes.iter().any(|sl| {
        sl.sample_y(probe_x)
            .is_some_and(|sy| sy >= min_top && sy <= max_top)
    })
}

/// Returns `true` if a solid wall blocks horizontal movement at `(probe_x, body_y)`.
fn has_blocking_wall_at(probe_x: f32, body_y: f32, solids: &[SolidRect]) -> bool {
    solids.iter().any(|s| {
        let min = s.min();
        let max = s.max();
        probe_x >= min.x && probe_x <= max.x && body_y >= min.y + 4.0 && body_y <= max.y - 4.0
    })
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn update_enemy_fighter_ai(
    mut commands: Commands,
    fixed_time: Res<Time<Fixed>>,
    solids_q: Query<&SolidRect>,
    one_ways_q: Query<&OneWayPlatform>,
    slopes_q: Query<&SlopeSegment>,
    players: Query<&KinematicBody, (With<Player>, Without<EnemyFighter>)>,
    mut vis_events: MessageWriter<CombatVisualEvent>,
    mut enemies: Query<
        (
            Entity,
            &mut EnemyFighter,
            &mut KinematicBody,
            Option<&Hitstop>,
            Option<&Hitstun>,
        ),
        Without<BossState>,
    >,
) {
    let dt = fixed_time.delta_secs();
    let player_pos = players.single().map(|p| p.position).ok();
    let solids: Vec<SolidRect> = solids_q.iter().copied().collect();
    let one_ways: Vec<OneWayPlatform> = one_ways_q.iter().copied().collect();
    let slopes: Vec<SlopeSegment> = slopes_q.iter().copied().collect();
    let has_geometry = !solids.is_empty() || !one_ways.is_empty() || !slopes.is_empty();

    for (entity, mut fighter, mut body, hitstop, hitstun) in &mut enemies {
        if hitstop.is_some_and(Hitstop::is_active) {
            continue;
        }
        fighter.jump_cooldown = (fighter.jump_cooldown - dt).max(0.0);
        fighter.special_cooldown = (fighter.special_cooldown - dt).max(0.0);

        if hitstun.is_some_and(Hitstun::is_active) {
            // Interrupt telegraph cleanly when player lands a hit or Buster pellet!
            fighter.state = EnemyAiState::Chase;
            continue;
        }

        let Some(p_pos) = player_pos else {
            continue;
        };

        let delta = p_pos - body.position;
        let dist = delta.length();
        let aggro_range = 480.0;

        // Keep AI from voluntarily walking off its island / arena deck ledges
        let (arena_min_x, arena_max_x) = if let Some((mn, mx)) = fighter.leash_bounds {
            (mn, mx)
        } else {
            let leash = if fighter.home_pos.x < 635.0 { 72.0 } else { 185.0 };
            (fighter.home_pos.x - leash, fighter.home_pos.x + leash)
        };

        let body_pos = body.position;
        let body_half_x = body.half_extents.x;
        let feet_y = body.feet_y();
        let can_step_dir = |dir: f32, lookahead: f32| -> bool {
            if !has_geometry || dir.abs() < 0.1 {
                return true;
            }
            let probe_x = body_pos.x + dir.signum() * (body_half_x + lookahead);
            has_walkable_ground_at(probe_x, feet_y, &solids, &one_ways, &slopes)
                && !has_blocking_wall_at(probe_x, body_pos.y, &solids)
        };

        match fighter.state {
            EnemyAiState::Patrol => {
                // Active sentinel pacing around home post, strictly honoring real platform ledges & walls!
                let patrol_half = 44.0_f32;
                let p_min = (fighter.home_pos.x - patrol_half).max(arena_min_x + 8.0);
                let p_max = (fighter.home_pos.x + patrol_half).min(arena_max_x - 8.0);
                if p_max - p_min > 18.0 && body.state == MovementState::Grounded {
                    if body.position.x <= p_min {
                        body.facing = 1.0;
                    } else if body.position.x >= p_max {
                        body.facing = -1.0;
                    }
                    if !can_step_dir(body.facing, 10.0) {
                        body.facing = -body.facing;
                    }
                    if can_step_dir(body.facing, 10.0) {
                        let pace = body.facing * fighter.archetype.move_speed() * 0.42;
                        body.ground_speed = pace;
                        body.velocity.x = pace;
                    } else {
                        body.ground_speed = 0.0;
                        body.velocity.x = 0.0;
                    }
                } else {
                    body.ground_speed *= (1.0 - dt * 6.0).max(0.0);
                }
                if dist <= aggro_range {
                    fighter.state = EnemyAiState::Chase;
                }
            }
            EnemyAiState::Chase => {
                if dist > aggro_range * 1.25 {
                    fighter.state = EnemyAiState::Patrol;
                    continue;
                }

                let desired_dir = if delta.x.abs() > 8.0 {
                    delta.x.signum()
                } else {
                    body.facing
                };
                body.facing = desired_dir;

                // ArcaneZoner kites back to mid-range and Phase-Blinks safely onto walkable floor when rushed!
                if fighter.archetype == EnemyArchetype::ArcaneZoner {
                    if dist < 72.0 && fighter.special_cooldown <= 0.0 {
                        let from = body.position;
                        let retreat_dir = -desired_dir;
                        let mut target_x =
                            (body.position.x + retreat_dir * 125.0).clamp(arena_min_x, arena_max_x);
                        if has_geometry {
                            // Step inward until landing point sits safely on walkable ground
                            let mut found_safe = None;
                            for frac in [1.0_f32, 0.78, 0.56, 0.35] {
                                let cand = (body.position.x + retreat_dir * 125.0 * frac)
                                    .clamp(arena_min_x, arena_max_x);
                                if has_walkable_ground_at(cand, feet_y, &solids, &one_ways, &slopes)
                                    && !has_blocking_wall_at(cand, body.position.y, &solids)
                                {
                                    found_safe = Some(cand);
                                    break;
                                }
                            }
                            if let Some(safe) = found_safe {
                                target_x = safe;
                            } else {
                                target_x = body.position.x;
                            }
                        }
                        if (target_x - body.position.x).abs() > 16.0 {
                            body.position.x = target_x;
                            body.velocity = Vec2::new(retreat_dir * 32.0, 85.0);
                            body.ground_speed = 0.0;
                            body.state = MovementState::Airborne;
                            fighter.special_cooldown = 2.6;
                            commands.entity(entity).insert(Invulnerable::new(0.20));
                            vis_events.write(CombatVisualEvent::EnemyBlink {
                                from,
                                to: body.position,
                            });
                            vis_events.write(CombatVisualEvent::ShockwaveRing {
                                pos: body.position,
                                max_radius: 30.0,
                                color_rgb: [3.6, 0.55, 5.2],
                            });
                            continue;
                        }
                    }

                    let move_dir = if dist < 115.0 {
                        -desired_dir
                    } else if dist > 235.0 {
                        desired_dir
                    } else {
                        0.0
                    };
                    let at_edge = (body.position.x <= arena_min_x && move_dir < 0.0)
                        || (body.position.x >= arena_max_x && move_dir > 0.0)
                        || (body.state == MovementState::Grounded
                            && move_dir != 0.0
                            && !can_step_dir(move_dir, 12.0));
                    let target_spd = if at_edge {
                        0.0
                    } else {
                        move_dir * fighter.archetype.move_speed()
                    };
                    if body.state == MovementState::Grounded {
                        body.ground_speed = target_spd;
                        body.velocity.x = target_spd;
                    } else {
                        body.velocity.x = body.velocity.x.lerp(target_spd, (dt * 4.0).min(1.0));
                    }

                    if dist <= fighter.archetype.attack_range()
                        && delta.y.abs() < 115.0
                        && fighter.special_cooldown <= 0.0
                    {
                        body.ground_speed = 0.0;
                        body.velocity.x *= 0.20;
                        // Alternate: 1st cast = Aimed Arcane Orb (2), 2nd cast = Arcane Pillar Eruption (7)!
                        if fighter.pattern_counter.is_multiple_of(2) {
                            fighter.attack_variant = 2;
                        } else {
                            fighter.attack_variant = 7;
                        }
                        fighter.pattern_counter = fighter.pattern_counter.wrapping_add(1);
                        fighter.state = EnemyAiState::Telegraph {
                            remaining: fighter.archetype.telegraph_secs(),
                        };
                    }
                    continue;
                }

                // Stop cleanly before walking off any real platform/deck ledge; otherwise pursue aggressively
                let at_edge = (body.position.x <= arena_min_x && desired_dir < 0.0)
                    || (body.position.x >= arena_max_x && desired_dir > 0.0)
                    || (body.state == MovementState::Grounded && !can_step_dir(desired_dir, 12.0));
                let target_spd = if at_edge {
                    0.0
                } else {
                    desired_dir * fighter.archetype.move_speed()
                };

                if body.state == MovementState::Grounded {
                    body.ground_speed = target_spd;
                    body.velocity.x = target_spd;

                    // LightStriker & ShadowAssassin hop onto drop-through platforms to pressure P1!
                    let can_hop = matches!(
                        fighter.archetype,
                        EnemyArchetype::LightStriker | EnemyArchetype::ShadowAssassin
                    );
                    if can_hop
                        && delta.y > 40.0
                        && delta.x.abs() < 110.0
                        && (can_step_dir(desired_dir, 14.0) || delta.x.abs() < 44.0)
                        && fighter.jump_cooldown <= 0.0
                    {
                        body.velocity.y = 405.0;
                        body.state = MovementState::Airborne;
                        fighter.jump_cooldown = if fighter.archetype == EnemyArchetype::ShadowAssassin
                        {
                            0.95
                        } else {
                            1.40
                        };
                    }
                } else {
                    body.velocity.x = body.velocity.x.lerp(target_spd, (dt * 4.5).min(1.0));
                }

                match fighter.archetype {
                    EnemyArchetype::HeavyBrawler => {
                        if dist <= 78.0
                            && delta.y.abs() < 38.0
                            && body.state == MovementState::Grounded
                            && fighter.special_cooldown <= 0.0
                        {
                            body.ground_speed = 0.0;
                            body.velocity.x = 0.0;
                            // Alternate between Seismic Ground Stomp (1) & 360° Whirlwind Greataxe Spin (6)!
                            if fighter.pattern_counter.is_multiple_of(2) {
                                fighter.attack_variant = 1;
                                fighter.state = EnemyAiState::Telegraph { remaining: 0.56 };
                            } else {
                                fighter.attack_variant = 6;
                                fighter.state = EnemyAiState::Telegraph { remaining: 0.56 };
                                vis_events.write(CombatVisualEvent::ShockwaveRing {
                                    pos: body.position,
                                    max_radius: 36.0,
                                    color_rgb: [5.0, 2.0, 0.35],
                                });
                            }
                            fighter.pattern_counter = fighter.pattern_counter.wrapping_add(1);
                        } else if dist <= fighter.archetype.attack_range() && delta.y.abs() < 36.0 {
                            body.ground_speed = 0.0;
                            body.velocity.x *= 0.25;
                            fighter.attack_variant = 0;
                            fighter.state = EnemyAiState::Telegraph {
                                remaining: fighter.archetype.telegraph_secs(),
                            };
                        }
                    }
                    EnemyArchetype::ShadowAssassin => {
                        if (56.0..=172.0).contains(&dist)
                            && delta.y.abs() < 46.0
                            && fighter.special_cooldown <= 0.0
                        {
                            body.ground_speed *= 0.2;
                            body.velocity.x *= 0.2;
                            // Alternate Shadow Dash-Through (3) & Smoke Backflip Kunai Throw (8)!
                            if fighter.pattern_counter.is_multiple_of(2) {
                                fighter.attack_variant = 3;
                                fighter.state = EnemyAiState::Telegraph {
                                    remaining: fighter.archetype.telegraph_secs(),
                                };
                            } else {
                                fighter.attack_variant = 8;
                                fighter.state = EnemyAiState::Telegraph { remaining: 0.26 };
                            }
                            fighter.pattern_counter = fighter.pattern_counter.wrapping_add(1);
                        } else if body.state == MovementState::Airborne
                            && delta.y < -30.0
                            && delta.x.abs() < 64.0
                            && fighter.special_cooldown <= 0.0
                        {
                            fighter.attack_variant = 3;
                            fighter.state = EnemyAiState::Telegraph { remaining: 0.22 };
                        } else if dist <= 44.0 && delta.y.abs() < 34.0 {
                            body.ground_speed = 0.0;
                            body.velocity.x *= 0.25;
                            fighter.attack_variant = 0;
                            fighter.state = EnemyAiState::Telegraph { remaining: 0.28 };
                        }
                    }
                    EnemyArchetype::LightStriker => {
                        if dist <= 34.0
                            && delta.y.abs() < 30.0
                            && body.state == MovementState::Grounded
                            && fighter.special_cooldown <= 0.0
                        {
                            // Agile Backstep Evade Roll — only if walkable ground exists behind!
                            let back = -desired_dir;
                            let safe_x = (body.position.x + back * 36.0).clamp(arena_min_x, arena_max_x);
                            if (safe_x - body.position.x).abs() > 12.0 && can_step_dir(back, 22.0) {
                                body.ground_speed = back * 210.0;
                                body.velocity.x = body.ground_speed;
                                fighter.attack_variant = 5;
                                fighter.special_cooldown = 2.4;
                                commands.entity(entity).insert(Invulnerable::new(0.18));
                                vis_events.write(CombatVisualEvent::ShockwaveRing {
                                    pos: body.position,
                                    max_radius: 22.0,
                                    color_rgb: [4.6, 1.4, 0.8],
                                });
                                fighter.state = EnemyAiState::Attacking { remaining: 0.22 };
                                continue;
                            }
                        }
                        if dist <= fighter.archetype.attack_range() && delta.y.abs() < 36.0 {
                            body.ground_speed = 0.0;
                            body.velocity.x *= 0.25;
                            fighter.attack_variant = 0;
                            fighter.state = EnemyAiState::Telegraph {
                                remaining: fighter.archetype.telegraph_secs(),
                            };
                        }
                    }
                    EnemyArchetype::ArcaneZoner
                    | EnemyArchetype::FoundryBoss
                    | EnemyArchetype::CryoBoss => {
                        if dist <= fighter.archetype.attack_range() && delta.y.abs() < 36.0 {
                            body.ground_speed = 0.0;
                            body.velocity.x *= 0.25;
                            fighter.attack_variant = 0;
                            fighter.state = EnemyAiState::Telegraph {
                                remaining: fighter.archetype.telegraph_secs(),
                            };
                        }
                    }
                }
            }
            EnemyAiState::Telegraph { mut remaining } => {
                body.ground_speed *= (1.0 - dt * 10.0).max(0.0);
                remaining -= dt;
                if remaining <= 0.0 {
                    match fighter.attack_variant {
                        1 => {
                            let slam_pos = Vec2::new(body.position.x, body.feet_y() + 10.0);
                            commands.spawn((
                                Name::new("BrawlerSeismicStomp"),
                                Hitbox {
                                    owner: entity,
                                    team: Team::Enemy,
                                    offset: Vec2::ZERO,
                                    radius: 52.0,
                                    damage: 16.0,
                                    base_kb: 235.0,
                                    kb_scale: 2.9,
                                    momentum_mult: 0.0,
                                    angle_deg: 74.0,
                                    attacker_speed: 0.0,
                                    facing: body.facing,
                                    lifetime: Timer::from_seconds(0.14, TimerMode::Once),
                                    piercing: true,
                                    despawn_on_expire: true,
                                    recoil_bounce_vy: None,
                                    hit_entities: HashSet::new(),
                                    is_charged: true,
                                    min_hitstop_frames: 6,
                                },
                                Transform::from_xyz(slam_pos.x, slam_pos.y, 1.5),
                            ));
                            vis_events.write(CombatVisualEvent::GroundSlam { pos: slam_pos });
                            vis_events.write(CombatVisualEvent::ShockwaveRing {
                                pos: slam_pos,
                                max_radius: 58.0,
                                color_rgb: [4.8, 1.6, 0.35],
                            });
                            fighter.special_cooldown = 3.2;
                            fighter.state = EnemyAiState::Attacking { remaining: 0.22 };
                        }
                        2 => {
                            let aim_dir = (p_pos - body.position)
                                .normalize_or(Vec2::new(body.facing, 0.0));
                            let origin = body.position + Vec2::new(body.facing * 18.0, 6.0);
                            commands.spawn((
                                Name::new("ArcanePlasmaOrb"),
                                TurretProjectile {
                                    velocity: aim_dir * 235.0,
                                    is_arcane: true,
                                    is_magma_wave: false,
                                    is_cryo_wave: false,
                                },
                                Hitbox {
                                    owner: entity,
                                    team: Team::Enemy,
                                    offset: Vec2::ZERO,
                                    radius: 8.0,
                                    damage: 9.0,
                                    base_kb: 165.0,
                                    kb_scale: 1.8,
                                    momentum_mult: 0.0,
                                    angle_deg: 34.0,
                                    attacker_speed: 0.0,
                                    facing: aim_dir.x.signum(),
                                    lifetime: Timer::from_seconds(1.8, TimerMode::Once),
                                    piercing: false,
                                    despawn_on_expire: true,
                                    recoil_bounce_vy: None,
                                    hit_entities: HashSet::new(),
                                    is_charged: true,
                                    min_hitstop_frames: 4,
                                },
                                Transform::from_xyz(origin.x, origin.y, 3.0),
                                Visibility::default(),
                            ));
                            vis_events.write(CombatVisualEvent::ShockwaveRing {
                                pos: origin,
                                max_radius: 26.0,
                                color_rgb: [3.8, 0.55, 5.2],
                            });
                            fighter.special_cooldown = 1.65;
                            fighter.state = EnemyAiState::Attacking { remaining: 0.20 };
                        }
                        3 => {
                            let dive = body.state == MovementState::Airborne && delta.y < -20.0;
                            let dash_vx = if dive || can_step_dir(body.facing, 18.0) {
                                body.facing * 305.0
                            } else {
                                0.0
                            };
                            body.ground_speed = dash_vx;
                            body.velocity.x = dash_vx;
                            if dive {
                                body.velocity.y = -320.0;
                            }
                            let offset = Vec2::new(22.0, if dive { -8.0 } else { 2.0 });
                            let pos = body.position + Vec2::new(offset.x * body.facing, offset.y);
                            commands.spawn((
                                Name::new("ShadowBlitzHitbox"),
                                AttachedHitbox,
                                Hitbox {
                                    owner: entity,
                                    team: Team::Enemy,
                                    offset,
                                    radius: 16.0,
                                    damage: 10.0,
                                    base_kb: 175.0,
                                    kb_scale: 2.1,
                                    momentum_mult: 0.25,
                                    angle_deg: if dive { -45.0 } else { 32.0 },
                                    attacker_speed: body.speed(),
                                    facing: body.facing,
                                    lifetime: Timer::from_seconds(0.18, TimerMode::Once),
                                    piercing: true,
                                    despawn_on_expire: true,
                                    recoil_bounce_vy: None,
                                    hit_entities: HashSet::new(),
                                    is_charged: true,
                                    min_hitstop_frames: 4,
                                },
                                Transform::from_xyz(pos.x, pos.y, 1.0),
                            ));
                            vis_events.write(CombatVisualEvent::SideBlitz {
                                pos: body.position,
                                facing: body.facing,
                            });
                            vis_events.write(CombatVisualEvent::SlashArc {
                                pos,
                                facing: body.facing,
                                angle_rad: if dive { -0.65 } else { 0.0 },
                                radius: 26.0,
                                heavy: true,
                                color_rgb: [4.6, 0.45, 3.6],
                            });
                            fighter.special_cooldown = 2.4;
                            fighter.state = EnemyAiState::Attacking { remaining: 0.20 };
                        }
                        // 6: Iron Brawler 360° Whirlwind Greataxe Spin!
                        6 => {
                            body.ground_speed = if body.state != MovementState::Grounded
                                || can_step_dir(body.facing, 14.0)
                            {
                                body.facing * 125.0
                            } else {
                                0.0
                            };
                            body.velocity.x = body.ground_speed;
                            commands.spawn((
                                Name::new("BrawlerWhirlwindHitbox"),
                                AttachedHitbox,
                                DualSidedHitbox,
                                Hitbox {
                                    owner: entity,
                                    team: Team::Enemy,
                                    offset: Vec2::new(0.0, 2.0),
                                    radius: 34.0,
                                    damage: 13.0,
                                    base_kb: 235.0,
                                    kb_scale: 2.7,
                                    momentum_mult: 0.18,
                                    angle_deg: 50.0,
                                    attacker_speed: 125.0,
                                    facing: body.facing,
                                    lifetime: Timer::from_seconds(0.48, TimerMode::Once),
                                    piercing: true,
                                    despawn_on_expire: true,
                                    recoil_bounce_vy: None,
                                    hit_entities: HashSet::new(),
                                    is_charged: true,
                                    min_hitstop_frames: 5,
                                },
                                Transform::from_xyz(body.position.x, body.position.y + 2.0, 1.5),
                            ));
                            vis_events.write(CombatVisualEvent::CycloneSpin {
                                pos: body.position + Vec2::new(0.0, 2.0),
                                radius: 38.0,
                                color_rgb: [5.0, 1.9, 0.35],
                            });
                            fighter.special_cooldown = 3.2;
                            fighter.state = EnemyAiState::Attacking { remaining: 0.50 };
                        }
                        // 7: Plasma Arcanist Arcane Pillar Eruption (telegraphed ground rune under P1!)
                        7 => {
                            let rune_pos = Vec2::new(p_pos.x.clamp(arena_min_x, arena_max_x), p_pos.y - 16.0);
                            commands.spawn((
                                Name::new("PendingArcanePillar"),
                                PendingArcanePillar {
                                    owner: entity,
                                    pos: rune_pos,
                                    timer: 0.62,
                                    is_cryo: false,
                                },
                            ));
                            vis_events.write(CombatVisualEvent::ArcanePillarTelegraph {
                                pos: rune_pos,
                                duration: 0.62,
                            });
                            fighter.special_cooldown = 2.2;
                            fighter.state = EnemyAiState::Attacking { remaining: 0.24 };
                        }
                        // 8: Shadow Assassin Smoke Backflip + Crimson Kunai Bolt (stay on platform!)
                        8 => {
                            let back = -body.facing;
                            if can_step_dir(back, 36.0) {
                                body.ground_speed = back * 185.0;
                                body.velocity = Vec2::new(back * 185.0, 165.0);
                            } else {
                                body.ground_speed = 0.0;
                                body.velocity = Vec2::new(0.0, 155.0);
                            }
                            body.state = MovementState::Airborne;
                            commands.entity(entity).insert(Invulnerable::new(0.16));
                            let aim_dir = (p_pos - body.position)
                                .normalize_or(Vec2::new(body.facing, 0.0));
                            let origin = body.position + Vec2::new(body.facing * 16.0, 4.0);
                            commands.spawn((
                                Name::new("AssassinKunaiBolt"),
                                TurretProjectile {
                                    velocity: aim_dir * 310.0,
                                    is_arcane: false,
                                    is_magma_wave: false,
                                    is_cryo_wave: false,
                                },
                                Hitbox {
                                    owner: entity,
                                    team: Team::Enemy,
                                    offset: Vec2::ZERO,
                                    radius: 7.0,
                                    damage: 8.0,
                                    base_kb: 155.0,
                                    kb_scale: 1.6,
                                    momentum_mult: 0.0,
                                    angle_deg: 28.0,
                                    attacker_speed: 0.0,
                                    facing: aim_dir.x.signum(),
                                    lifetime: Timer::from_seconds(1.2, TimerMode::Once),
                                    piercing: false,
                                    despawn_on_expire: true,
                                    recoil_bounce_vy: None,
                                    hit_entities: HashSet::new(),
                                    is_charged: false,
                                    min_hitstop_frames: 3,
                                },
                                Transform::from_xyz(origin.x, origin.y, 3.0),
                                Visibility::default(),
                            ));
                            vis_events.write(CombatVisualEvent::ShockwaveRing {
                                pos: body.position,
                                max_radius: 24.0,
                                color_rgb: [4.6, 0.45, 3.4],
                            });
                            fighter.special_cooldown = 2.1;
                            fighter.state = EnemyAiState::Attacking { remaining: 0.24 };
                        }
                        _ => {
                            let is_heavy = fighter.archetype == EnemyArchetype::HeavyBrawler;
                            let lunge = if is_heavy { 110.0 } else { 190.0 };
                            body.ground_speed = if body.state != MovementState::Grounded
                                || can_step_dir(body.facing, 12.0)
                            {
                                body.facing * lunge
                            } else {
                                0.0
                            };
                            body.velocity.x = body.ground_speed;

                            let offset = Vec2::new(if is_heavy { 26.0 } else { 22.0 }, 2.0);
                            let pos = body.position + Vec2::new(offset.x * body.facing, offset.y);
                            commands.spawn((
                                Name::new("EnemyMeleeHitbox"),
                                AttachedHitbox,
                                Hitbox {
                                    owner: entity,
                                    team: Team::Enemy,
                                    offset,
                                    radius: if is_heavy { 18.0 } else { 14.0 },
                                    damage: if is_heavy { 14.0 } else { 8.0 },
                                    base_kb: if is_heavy { 210.0 } else { 145.0 },
                                    kb_scale: if is_heavy { 2.6 } else { 1.8 },
                                    momentum_mult: 0.25,
                                    angle_deg: 35.0,
                                    attacker_speed: body.speed(),
                                    facing: body.facing,
                                    lifetime: Timer::from_seconds(0.14, TimerMode::Once),
                                    piercing: true,
                                    despawn_on_expire: true,
                                    recoil_bounce_vy: None,
                                    hit_entities: HashSet::new(),
                                    is_charged: is_heavy,
                                    min_hitstop_frames: if is_heavy { 5 } else { 3 },
                                },
                                Transform::from_xyz(pos.x, pos.y, 1.0),
                            ));
                            vis_events.write(CombatVisualEvent::SlashArc {
                                pos,
                                facing: body.facing,
                                angle_rad: 0.0,
                                radius: if is_heavy { 26.0 } else { 20.0 },
                                heavy: is_heavy,
                                color_rgb: if is_heavy {
                                    [4.8, 1.4, 0.35]
                                } else {
                                    [4.6, 0.65, 0.55]
                                },
                            });

                            fighter.state = EnemyAiState::Attacking { remaining: 0.16 };
                        }
                    }
                } else {
                    fighter.state = EnemyAiState::Telegraph { remaining };
                }
            }
            EnemyAiState::Attacking { mut remaining } => {
                // Never let a grounded enemy melee lunge, dash, whirlwind, or evade step off a cliff!
                if body.state == MovementState::Grounded && body.ground_speed.abs() > 1.0 {
                    let move_dir = body.ground_speed.signum();
                    let at_edge = (body.position.x <= arena_min_x + 4.0 && move_dir < 0.0)
                        || (body.position.x >= arena_max_x - 4.0 && move_dir > 0.0)
                        || !can_step_dir(move_dir, 10.0);
                    if at_edge {
                        body.ground_speed = 0.0;
                        body.velocity.x = 0.0;
                    }
                }
                remaining -= dt;
                if remaining <= 0.0 {
                    // Rogue Striker chains a crisp 2nd cross-slash (`variant 4`) if player stays close!
                    if fighter.archetype == EnemyArchetype::LightStriker
                        && fighter.attack_variant == 0
                        && dist <= 56.0
                        && delta.y.abs() < 36.0
                    {
                        fighter.attack_variant = 4;
                        body.facing = if delta.x.abs() > 4.0 {
                            delta.x.signum()
                        } else {
                            body.facing
                        };
                        body.ground_speed = if body.state != MovementState::Grounded
                            || can_step_dir(body.facing, 12.0)
                        {
                            body.facing * 175.0
                        } else {
                            0.0
                        };
                        body.velocity.x = body.ground_speed;
                        let offset = Vec2::new(24.0, 4.0);
                        let pos = body.position + Vec2::new(offset.x * body.facing, offset.y);
                        commands.spawn((
                            Name::new("RogueComboFinisherHitbox"),
                            AttachedHitbox,
                            Hitbox {
                                owner: entity,
                                team: Team::Enemy,
                                offset,
                                radius: 16.0,
                                damage: 9.0,
                                base_kb: 185.0,
                                kb_scale: 2.2,
                                momentum_mult: 0.25,
                                angle_deg: 46.0,
                                attacker_speed: body.speed(),
                                facing: body.facing,
                                lifetime: Timer::from_seconds(0.14, TimerMode::Once),
                                piercing: true,
                                despawn_on_expire: true,
                                recoil_bounce_vy: None,
                                hit_entities: HashSet::new(),
                                is_charged: false,
                                min_hitstop_frames: 4,
                            },
                            Transform::from_xyz(pos.x, pos.y, 1.0),
                        ));
                        vis_events.write(CombatVisualEvent::SlashArc {
                            pos,
                            facing: body.facing,
                            angle_rad: 0.45,
                            radius: 24.0,
                            heavy: true,
                            color_rgb: [5.0, 1.1, 0.55],
                        });
                        fighter.state = EnemyAiState::Attacking { remaining: 0.16 };
                        continue;
                    }

                    let rec = match fighter.archetype {
                        EnemyArchetype::HeavyBrawler => 0.78,
                        EnemyArchetype::ArcaneZoner => 0.58,
                        EnemyArchetype::ShadowAssassin => 0.42,
                        EnemyArchetype::LightStriker => 0.42,
                        EnemyArchetype::FoundryBoss | EnemyArchetype::CryoBoss => 0.72,
                    };
                    fighter.state = EnemyAiState::Recovery { remaining: rec };
                } else {
                    fighter.state = EnemyAiState::Attacking { remaining };
                }
            }
            EnemyAiState::Recovery { mut remaining } => {
                body.ground_speed *= (1.0 - dt * 8.0).max(0.0);
                remaining -= dt;
                if remaining <= 0.0 {
                    fighter.attack_variant = 0;
                    fighter.state = EnemyAiState::Chase;
                } else {
                    fighter.state = EnemyAiState::Recovery { remaining };
                }
            }
        }
    }
}

/// Detonates delayed Plasma Arcanist ground runes into vertical violet Arcane Pillar Eruptions!
fn tick_pending_arcane_pillars(
    mut commands: Commands,
    fixed_time: Res<Time<Fixed>>,
    mut pillars: Query<(Entity, &mut PendingArcanePillar)>,
    mut vis_events: MessageWriter<CombatVisualEvent>,
) {
    let dt = fixed_time.delta_secs();
    for (entity, mut pillar) in &mut pillars {
        pillar.timer -= dt;
        if pillar.timer <= 0.0 {
            let erupt_pos = pillar.pos + Vec2::new(0.0, 32.0);
            commands.spawn((
                Name::new("ArcanePillarHitbox"),
                DualSidedHitbox,
                Hitbox {
                    owner: pillar.owner,
                    team: Team::Enemy,
                    offset: Vec2::ZERO,
                    radius: 26.0,
                    damage: 12.0,
                    base_kb: 225.0,
                    kb_scale: 2.3,
                    momentum_mult: 0.0,
                    angle_deg: 82.0,
                    attacker_speed: 0.0,
                    facing: 1.0,
                    lifetime: Timer::from_seconds(0.16, TimerMode::Once),
                    piercing: true,
                    despawn_on_expire: true,
                    recoil_bounce_vy: None,
                    hit_entities: HashSet::new(),
                    is_charged: true,
                    min_hitstop_frames: 5,
                },
                Transform::from_xyz(erupt_pos.x, erupt_pos.y, 2.0),
            ));
            vis_events.write(CombatVisualEvent::ArcanePillarErupt { pos: pillar.pos });
            vis_events.write(CombatVisualEvent::ShockwaveRing {
                pos: pillar.pos + Vec2::new(0.0, 10.0),
                max_radius: 42.0,
                color_rgb: if pillar.is_cryo {
                    [0.42, 3.8, 5.6]
                } else {
                    [4.2, 0.75, 5.4]
                },
            });
            commands.entity(entity).despawn();
        }
    }
}

#[allow(clippy::type_complexity)]
fn update_wall_turrets(
    mut commands: Commands,
    fixed_time: Res<Time<Fixed>>,
    players: Query<&KinematicBody, (With<Player>, Without<WallTurret>)>,
    mut score: ResMut<ArenaScore>,
    mut turrets: Query<(
        Entity,
        &mut WallTurret,
        &mut KinematicBody,
        &mut DamagePercent,
    )>,
) {
    let dt = fixed_time.delta_secs();
    let player_pos = players.single().map(|p| p.position).ok();

    for (entity, mut turret, mut body, mut percent) in &mut turrets {
        body.position = turret.home_pos;
        body.velocity = Vec2::ZERO;
        body.ground_speed = 0.0;
        body.state = MovementState::Grounded;

        if turret.disabled_timer > 0.0 {
            turret.disabled_timer = (turret.disabled_timer - dt).max(0.0);
            if turret.disabled_timer <= 0.0 {
                percent.0 = 0.0;
                turret.fire_timer = turret.fire_interval * 0.7;
            }
            continue;
        }

        if percent.0 >= turret.hp_threshold {
            turret.disabled_timer = 6.5;
            percent.0 = 0.0;
            score.turrets_destroyed += 1;
            commands
                .entity(entity)
                .remove::<Hitstun>()
                .remove::<Hitstop>()
                .insert(Invulnerable::new(6.5));
            continue;
        }

        let in_range = player_pos.is_some_and(|p| (p - turret.home_pos).length() < 340.0);
        if !in_range {
            turret.fire_timer = turret.fire_interval * 0.6;
            continue;
        }

        turret.fire_timer -= dt;
        if turret.fire_timer <= 0.0 {
            turret.fire_timer = turret.fire_interval;
            let origin = turret.home_pos + Vec2::new(turret.facing * 14.0, 0.0);
            commands.spawn((
                Name::new("TurretOrb"),
                TurretProjectile {
                    velocity: Vec2::new(turret.facing * 205.0, 0.0),
                    is_arcane: false,
                    is_magma_wave: false,
                    is_cryo_wave: false,
                },
                Hitbox {
                    owner: entity,
                    team: Team::Enemy,
                    offset: Vec2::ZERO,
                    radius: 6.0,
                    damage: 7.0,
                    base_kb: 140.0,
                    kb_scale: 1.2,
                    momentum_mult: 0.0,
                    angle_deg: 30.0,
                    attacker_speed: 0.0,
                    facing: turret.facing,
                    lifetime: Timer::from_seconds(1.5, TimerMode::Once),
                    piercing: false,
                    despawn_on_expire: true,
                    recoil_bounce_vy: None,
                    hit_entities: HashSet::new(),
                    is_charged: false,
                    min_hitstop_frames: 3,
                },
                Transform::from_xyz(origin.x, origin.y, 3.0),
                Visibility::default(),
            ));
        }
    }
}

fn advance_turret_projectiles(
    mut commands: Commands,
    fixed_time: Res<Time<Fixed>>,
    solids: Query<&SolidRect>,
    mut orbs: Query<(Entity, &TurretProjectile, &Hitbox, &mut Transform)>,
) {
    let dt = fixed_time.delta_secs();
    let solid_list: Vec<SolidRect> = solids.iter().copied().collect();

    for (entity, proj, hb, mut tf) in &mut orbs {
        tf.translation.x += proj.velocity.x * dt;
        tf.translation.y += proj.velocity.y * dt;

        let p = tf.translation.truncate();
        let hit_wall = solid_list.iter().any(|rect| {
            // Ground-skimming magma & cryo frost waves ignore floor decks underneath them, only stopping at walls/gates
            if (proj.is_magma_wave || proj.is_cryo_wave)
                && rect.half_extents.x > rect.half_extents.y * 1.4
            {
                return false;
            }
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

/// Applies non-instant-death spike hazard knockback + brief i-frames (`SPEC.MD` Zone B).
#[allow(clippy::type_complexity)]
fn resolve_spike_hazards(
    mut commands: Commands,
    spikes: Query<&SpikeHazard>,
    mut players: Query<
        (
            Entity,
            &mut KinematicBody,
            &mut DamagePercent,
            Option<&Invulnerable>,
        ),
        With<Player>,
    >,
    mut hit_events: MessageWriter<CombatHitEvent>,
) {
    for (player_entity, mut body, mut percent, invuln) in &mut players {
        if invuln.is_some_and(Invulnerable::is_active) {
            continue;
        }
        for spike in &spikes {
            if spike.overlaps(body.position, body.half_extents) {
                percent.0 += spike.damage;
                let kb_vec = Vec2::new(-body.facing * 135.0, spike.knockback_vy);
                body.velocity = kb_vec;
                body.ground_speed = kb_vec.x;
                body.state = MovementState::Airborne;
                body.on_one_way = false;
                body.coyote_timer = 0.0;

                commands.entity(player_entity).insert((
                    Invulnerable::new(1.1),
                    Hitstun::new(0.22),
                    Hitstop::from_frames(3),
                ));

                hit_events.write(CombatHitEvent {
                    attacker: player_entity,
                    target: player_entity,
                    hit_pos: body.position - Vec2::new(0.0, body.half_extents.y * 0.5),
                    damage: spike.damage,
                    target_percent_after: percent.0,
                    knockback_mag: kb_vec.length(),
                    knockback_vec: kb_vec,
                    hitstop_frames: 3,
                    is_charged: false,
                });
                break;
            }
        }
    }
}

/// Checks if any [`EnemyFighter`] has been knocked outside a [`BlastZone`] or off a chasm pit,
/// triggering a Smash KO event, score increment, and optional 3-second respawn.
fn check_blast_zone_kos(
    mut commands: Commands,
    blast_zones: Query<&BlastZone>,
    enemies: Query<(Entity, &EnemyFighter, &KinematicBody, &DamagePercent)>,
    mut score: ResMut<ArenaScore>,
    mut ko_events: MessageWriter<BlastZoneKoEvent>,
) {
    let maybe_zone = blast_zones.iter().next();

    for (entity, fighter, body, percent) in &enemies {
        let ko_edge = if let Some(zone) = maybe_zone
            && zone.contains(fighter.home_pos)
        {
            if !zone.contains(body.position) {
                Some(zone.crossed_edge(body.position))
            } else {
                None
            }
        } else if body.position.y < -150.0 {
            Some(BlastEdge::Bottom)
        } else if body.position.y > 560.0 {
            Some(BlastEdge::Top)
        } else if body.position.x < -380.0 {
            Some(BlastEdge::Left)
        } else {
            None
        };

        if let Some(edge) = ko_edge {
            score.ko_count += 1;

            ko_events.write(BlastZoneKoEvent {
                is_player: false,
                archetype: Some(fighter.archetype),
                ko_pos: body.position,
                edge,
                final_percent: percent.0,
            });

            if fighter.respawns {
                commands.spawn((
                    Name::new("PendingEnemyRespawn"),
                    PendingEnemyRespawn {
                        archetype: fighter.archetype,
                        home_pos: fighter.home_pos,
                        timer: Timer::from_seconds(ENEMY_KO_RESPAWN_SECS, TimerMode::Once),
                    },
                ));
            }

            commands.entity(entity).despawn();
        }
    }
}

fn tick_pending_enemy_respawns(
    mut commands: Commands,
    fixed_time: Res<Time<Fixed>>,
    mut pending: Query<(Entity, &mut PendingEnemyRespawn)>,
) {
    let dt = Duration::from_secs_f32(fixed_time.delta_secs());
    for (entity, mut respawn) in &mut pending {
        respawn.timer.tick(dt);
        if respawn.timer.is_finished() {
            spawn_enemy_fighter(&mut commands, respawn.archetype, respawn.home_pos, true);
            commands.entity(entity).despawn();
        }
    }
}
