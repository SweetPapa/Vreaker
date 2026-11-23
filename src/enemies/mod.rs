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
use crate::physics::{KinematicBody, MovementState, SolidRect, SpikeHazard};
use crate::player::{DEFAULT_PLAYER_SPAWN, Player};
use crate::state::{AppState, GameStateCommand, StageRestartEvent};

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
    /// Towering 144px End-of-Level Boss (*IRON TITAN: VULCANUS*) with 2-phase melee & shockwave kit.
    FoundryBoss,
}

impl EnemyArchetype {
    pub fn display_name(self) -> &'static str {
        match self {
            EnemyArchetype::LightStriker => "ROGUE STRIKER",
            EnemyArchetype::HeavyBrawler => "IRON BRAWLER",
            EnemyArchetype::ArcaneZoner => "PLASMA ARCANIST",
            EnemyArchetype::ShadowAssassin => "SHADOW ASSASSIN",
            EnemyArchetype::FoundryBoss => "IRON TITAN: VULCANUS",
        }
    }

    pub fn move_speed(self) -> f32 {
        match self {
            EnemyArchetype::LightStriker => 185.0,
            EnemyArchetype::HeavyBrawler => 105.0,
            EnemyArchetype::ArcaneZoner => 118.0,
            EnemyArchetype::ShadowAssassin => 215.0,
            EnemyArchetype::FoundryBoss => 126.0,
        }
    }

    pub fn attack_range(self) -> f32 {
        match self {
            EnemyArchetype::LightStriker => 42.0,
            EnemyArchetype::HeavyBrawler => 54.0,
            EnemyArchetype::ArcaneZoner => 260.0,
            EnemyArchetype::ShadowAssassin => 165.0,
            EnemyArchetype::FoundryBoss => 78.0,
        }
    }

    pub fn telegraph_secs(self) -> f32 {
        match self {
            EnemyArchetype::LightStriker => 0.34,
            EnemyArchetype::HeavyBrawler => 0.52,
            EnemyArchetype::ArcaneZoner => 0.48,
            EnemyArchetype::ShadowAssassin => 0.30,
            EnemyArchetype::FoundryBoss => 0.48,
        }
    }

    pub fn half_extents(self) -> Vec2 {
        match self {
            EnemyArchetype::LightStriker => Vec2::new(10.0, 18.0),
            EnemyArchetype::HeavyBrawler => Vec2::new(12.0, 19.0),
            EnemyArchetype::ArcaneZoner => Vec2::new(10.0, 18.0),
            EnemyArchetype::ShadowAssassin => Vec2::new(10.0, 18.0),
            EnemyArchetype::FoundryBoss => Vec2::new(20.0, 30.0),
        }
    }

    pub fn max_hp(self) -> f32 {
        match self {
            EnemyArchetype::LightStriker => 42.0,
            EnemyArchetype::HeavyBrawler => 78.0,
            EnemyArchetype::ArcaneZoner => 38.0,
            EnemyArchetype::ShadowAssassin => 44.0,
            EnemyArchetype::FoundryBoss => 240.0,
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

/// Delayed ground-rune Arcane Pillar Eruption summoned by [`EnemyArchetype::ArcaneZoner`].
#[derive(Component, Debug, Clone)]
pub struct PendingArcanePillar {
    pub owner: Entity,
    pub pos: Vec2,
    pub timer: f32,
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
/// or [`EnemyArchetype::FoundryBoss`] ground shockwave.
#[derive(Component, Debug, Clone, Copy)]
pub struct TurretProjectile {
    pub velocity: Vec2,
    /// True when fired by a Plasma Arcanist mage (renders vivid violet-magenta arcane energy).
    pub is_arcane: bool,
    /// True when fired as a Boss ground-skimming molten shockwave (glides across deck).
    pub is_magma_wave: bool,
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
            "P1 BREAKER"
        } else {
            self.archetype
                .map_or("FIGHTER", EnemyArchetype::display_name)
        }
    }
}

/// Tracks live stage KO score & turret takedowns.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct ArenaScore {
    pub ko_count: u32,
    pub turrets_destroyed: u32,
    pub encounters_cleared: u32,
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

/// Spawns the towering End-of-Level Boss (*IRON TITAN: VULCANUS*).
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
        EnemyArchetype::FoundryBoss => 26.0,
        EnemyArchetype::HeavyBrawler => 19.0,
        _ => 16.0,
    };
    let kb_weight = match archetype {
        EnemyArchetype::LightStriker => 1.15,
        EnemyArchetype::HeavyBrawler => 0.72,
        EnemyArchetype::ArcaneZoner => 1.10,
        EnemyArchetype::ShadowAssassin => 1.12,
        EnemyArchetype::FoundryBoss => 0.38,
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
    *score = ArenaScore::default();
    *enc_status = EncounterStatus::default();
    *checkpoint = PlayerCheckpoint::default();
}

fn reset_arena_score_on_restart(
    mut restarts: MessageReader<StageRestartEvent>,
    mut score: ResMut<ArenaScore>,
    mut enc_status: ResMut<EncounterStatus>,
    mut checkpoint: ResMut<PlayerCheckpoint>,
) {
    if restarts.read().next().is_some() {
        *score = ArenaScore::default();
        *enc_status = EncounterStatus::default();
        *checkpoint = PlayerCheckpoint::default();
    }
}

/// Advances the player's safe Angel Respawn checkpoint as they progress deeper into the stage.
fn update_player_checkpoint(
    players: Query<&KinematicBody, With<Player>>,
    mut checkpoint: ResMut<PlayerCheckpoint>,
) {
    let Ok(body) = players.single() else {
        return;
    };
    if body.state != MovementState::Grounded {
        return;
    }
    // Advance safe respawn checkpoints at major stage landings across all 5 Acts
    for (threshold_x, cp_pos) in [
        (300.0, Vec2::new(340.0, 56.0)),
        (650.0, Vec2::new(710.0, 248.0)),
        (1470.0, Vec2::new(1520.0, 248.0)),
        (1860.0, Vec2::new(1900.0, 248.0)),
        (2590.0, Vec2::new(2635.0, 248.0)),
        (2960.0, Vec2::new(3005.0, 368.0)),
        (3930.0, Vec2::new(3980.0, 248.0)),
        (4310.0, Vec2::new(4365.0, 248.0)),
    ] {
        if body.position.x >= threshold_x && checkpoint.0.x < cp_pos.x {
            checkpoint.0 = cp_pos;
        }
    }
}

/// Manages Lockdown Ambush Arenas and the End-of-Level Boss Chamber:
/// slams shut laser gates, spawns waves of enemies, tracks clears, unlocks gates, and triggers Victory!
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn update_lockdown_encounters(
    mut commands: Commands,
    fixed_time: Res<Time<Fixed>>,
    mut score: ResMut<ArenaScore>,
    mut status: ResMut<EncounterStatus>,
    mut state_commands: MessageWriter<GameStateCommand>,
    mut vis_events: MessageWriter<CombatVisualEvent>,
    mut players: Query<(&KinematicBody, &mut DamagePercent), With<Player>>,
    alive_enemies: Query<Entity, With<EnemyFighter>>,
    mut encounters: Query<&mut LockdownEncounter>,
) {
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
                        color_rgb: [4.8, 0.55, 0.45],
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
                            color_rgb: [4.8, 0.55, 0.45],
                        });
                    }

                    // Spawn Wave 0 enemies (or Boss!)
                    spawn_encounter_wave(&mut commands, &mut enc, 0, &mut vis_events);

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
                            status.banner_text = "IRON TITAN DEFEATED  •  STAGE CLEAR!".to_string();
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
        let e = if arch == EnemyArchetype::FoundryBoss {
            spawn_foundry_boss(commands, pos, enc.min_x, enc.max_x)
        } else {
            spawn_stage_enemy(commands, arch, pos, true, bounds)
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

        // Trigger dramatic Phase 2 Overdrive on Boss at <= 50% HP!
        if let Some(mut boss) = maybe_boss
            && !boss.enraged_triggered
            && hp.current > 0.0
            && hp.current <= hp.max * 0.50
        {
            boss.enraged_triggered = true;
            boss.phase = 2;
            commands
                .entity(ev.target)
                .insert(Invulnerable::new(0.45));
            vis_events.write(CombatVisualEvent::CounterTrigger { pos: body.position });
            vis_events.write(CombatVisualEvent::ShockwaveRing {
                pos: body.position,
                max_radius: 86.0,
                color_rgb: [5.2, 2.2, 0.35],
            });
            enc_status.banner_text = "VULCANUS OVERDRIVE  •  PHASE 2!".to_string();
            enc_status.banner_timer = 2.6;
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
            vis_events.write(CombatVisualEvent::ShockwaveRing {
                pos: body.position,
                max_radius: if fighter.archetype == EnemyArchetype::FoundryBoss {
                    110.0
                } else {
                    44.0
                },
                color_rgb: [5.2, 2.8, 0.45],
            });
            if fighter.archetype == EnemyArchetype::FoundryBoss {
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

        // Detect touchdown from Leap Tremor Slam -> unleash dual floor shockwaves + wide slam hitbox!
        if boss.slam_pending && body.state == MovementState::Grounded && body.velocity.y <= 0.0 {
            boss.slam_pending = false;
            let slam_pos = Vec2::new(body.position.x, body.feet_y() + 10.0);
            commands.spawn((
                Name::new("BossTremorSlamHitbox"),
                Hitbox {
                    owner: entity,
                    team: Team::Enemy,
                    offset: Vec2::ZERO,
                    radius: 68.0,
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
            // Roll twin ground-skimming magma shockwaves left and right across the Boss Deck!
            for dir in [-1.0_f32, 1.0_f32] {
                let speed = if boss.phase >= 2 { 275.0 } else { 225.0 };
                commands.spawn((
                    Name::new("BossMagmaWave"),
                    TurretProjectile {
                        velocity: Vec2::new(dir * speed, 0.0),
                        is_arcane: false,
                        is_magma_wave: true,
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
                max_radius: 76.0,
                color_rgb: [5.2, 2.0, 0.35],
            });
            fighter.state = EnemyAiState::Recovery {
                remaining: if boss.phase >= 2 { 0.62 } else { 0.90 },
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
                if dist <= 82.0 && delta.y.abs() < 48.0 {
                    // Close range: Greataxe Cleave (prime Down-B Counter punish!)
                    body.ground_speed = 0.0;
                    body.velocity.x *= 0.2;
                    fighter.attack_variant = 0;
                    fighter.state = EnemyAiState::Telegraph {
                        remaining: if boss.phase >= 2 { 0.42 } else { 0.54 },
                    };
                } else if fighter.special_cooldown <= 0.0 && body.state == MovementState::Grounded {
                    body.ground_speed = 0.0;
                    body.velocity.x = 0.0;
                    boss.pattern_step = (boss.pattern_step + 1) % if boss.phase >= 2 { 4 } else { 2 };
                    match boss.pattern_step {
                        0 => {
                            // Pattern A: Magma Bull-Rush Charge across the sanctum!
                            fighter.attack_variant = 3;
                            fighter.state = EnemyAiState::Telegraph {
                                remaining: if boss.phase >= 2 { 0.48 } else { 0.60 },
                            };
                        }
                        1 => {
                            // Pattern B: Leap Foundry Tremor Slam!
                            fighter.attack_variant = 1;
                            fighter.state = EnemyAiState::Telegraph {
                                remaining: if boss.phase >= 2 { 0.44 } else { 0.55 },
                            };
                        }
                        2 => {
                            // Pattern C (Phase 2): 3-Way Forge Volley Laser Barrage with 0.48s charge sightline!
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
                                color_rgb: [5.4, 3.6, 0.55],
                            });
                        }
                        _ => {
                            // Pattern D (Phase 2): TITAN CYCLONE GREATAXE WHIRLWIND (360° sword_spin)!
                            fighter.attack_variant = 6;
                            fighter.state = EnemyAiState::Telegraph { remaining: 0.54 };
                            vis_events.write(CombatVisualEvent::ShockwaveRing {
                                pos: body.position,
                                max_radius: 52.0,
                                color_rgb: [5.4, 2.2, 0.32],
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
                        // 0: Titan Greataxe Cleave!
                        0 => {
                            let lunge = 145.0 * phase_speed;
                            body.ground_speed = body.facing * lunge;
                            body.velocity.x = body.ground_speed;
                            let offset = Vec2::new(38.0, 4.0);
                            let pos = body.position + Vec2::new(offset.x * body.facing, offset.y);
                            commands.spawn((
                                Name::new("BossGreataxeCleave"),
                                AttachedHitbox,
                                Hitbox {
                                    owner: entity,
                                    team: Team::Enemy,
                                    offset,
                                    radius: 30.0,
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
                                radius: 44.0,
                                heavy: true,
                                color_rgb: [5.2, 2.0, 0.35],
                            });
                            fighter.state = EnemyAiState::Attacking { remaining: 0.22 };
                        }
                        // 1: Leap Foundry Tremor Slam!
                        1 => {
                            let jump_vx = (delta.x * 1.45).clamp(-260.0, 260.0);
                            body.velocity = Vec2::new(jump_vx, 485.0);
                            body.ground_speed = jump_vx;
                            body.state = MovementState::Airborne;
                            boss.slam_pending = true;
                            fighter.special_cooldown = if boss.phase >= 2 { 2.0 } else { 2.8 };
                            vis_events.write(CombatVisualEvent::ShockwaveRing {
                                pos: body.position,
                                max_radius: 44.0,
                                color_rgb: [5.0, 2.4, 0.4],
                            });
                            fighter.state = EnemyAiState::Attacking { remaining: 0.85 };
                        }
                        // 2: Phase 2 Forge Volley 3-Way Molten Fan along the telegraphed warning sightlines!
                        2 => {
                            let base_dir = boss
                                .locked_aim_dir
                                .normalize_or(Vec2::new(body.facing, 0.0));
                            let base_angle = base_dir.y.atan2(base_dir.x);
                            let origin = body.position + Vec2::new(body.facing * 26.0, 10.0);
                            for spread in [-0.30_f32, 0.0, 0.30] {
                                let a = base_angle + spread;
                                let vel = Vec2::new(a.cos(), a.sin()) * 245.0;
                                commands.spawn((
                                    Name::new("BossForgeOrb"),
                                    TurretProjectile {
                                        velocity: vel,
                                        is_arcane: false,
                                        is_magma_wave: true,
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
                                max_radius: 42.0,
                                color_rgb: [5.2, 1.8, 0.3],
                            });
                            fighter.special_cooldown = 2.1;
                            fighter.state = EnemyAiState::Attacking { remaining: 0.25 };
                        }
                        // 6: Phase 2 TITAN CYCLONE GREATAXE WHIRLWIND (360° dual-sided spin!)
                        6 => {
                            let spin_spd = body.facing * 195.0;
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
                                    attacker_speed: 195.0,
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
                                color_rgb: [5.4, 2.1, 0.32],
                            });
                            fighter.special_cooldown = 2.2;
                            fighter.state = EnemyAiState::Attacking { remaining: 0.72 };
                        }
                        // 3: Magma Bull-Rush Charge!
                        _ => {
                            let charge_spd = if boss.phase >= 2 { 365.0 } else { 310.0 };
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
                            fighter.special_cooldown = if boss.phase >= 2 { 1.9 } else { 2.6 };
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
                            color_rgb: [5.4, 2.6, 0.35],
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

#[allow(clippy::type_complexity)]
fn update_enemy_fighter_ai(
    mut commands: Commands,
    fixed_time: Res<Time<Fixed>>,
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

        match fighter.state {
            EnemyAiState::Patrol => {
                body.ground_speed *= (1.0 - dt * 6.0).max(0.0);
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

                // ArcaneZoner kites back to mid-range and Phase-Blinks away when rushed!
                if fighter.archetype == EnemyArchetype::ArcaneZoner {
                    if dist < 72.0 && fighter.special_cooldown <= 0.0 {
                        let from = body.position;
                        let retreat_dir = -desired_dir;
                        let target_x =
                            (body.position.x + retreat_dir * 125.0).clamp(arena_min_x, arena_max_x);
                        body.position.x = target_x;
                        body.velocity = Vec2::new(retreat_dir * 45.0, 95.0);
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

                    let move_dir = if dist < 115.0 {
                        -desired_dir
                    } else if dist > 235.0 {
                        desired_dir
                    } else {
                        0.0
                    };
                    let at_edge = (body.position.x <= arena_min_x && move_dir < 0.0)
                        || (body.position.x >= arena_max_x && move_dir > 0.0);
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

                // Stop before walking off deck edge; otherwise pursue aggressively
                let at_edge = (body.position.x <= arena_min_x && desired_dir < 0.0)
                    || (body.position.x >= arena_max_x && desired_dir > 0.0);
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
                        && delta.x.abs() < 135.0
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
                            // Agile Backstep Evade Roll! Leaps safely back before rushing in!
                            let back = -desired_dir;
                            let safe_x = (body.position.x + back * 22.0).clamp(arena_min_x, arena_max_x);
                            if (safe_x - body.position.x).abs() > 8.0 {
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
                    EnemyArchetype::ArcaneZoner | EnemyArchetype::FoundryBoss => {
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
                            let dash_vx = body.facing * 305.0;
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
                            body.ground_speed = body.facing * 125.0;
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
                                },
                            ));
                            vis_events.write(CombatVisualEvent::ArcanePillarTelegraph {
                                pos: rune_pos,
                                duration: 0.62,
                            });
                            fighter.special_cooldown = 2.2;
                            fighter.state = EnemyAiState::Attacking { remaining: 0.24 };
                        }
                        // 8: Shadow Assassin Smoke Backflip + Crimson Kunai Bolt!
                        8 => {
                            let back = -body.facing;
                            body.ground_speed = back * 185.0;
                            body.velocity = Vec2::new(back * 185.0, 165.0);
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
                            body.ground_speed = body.facing * lunge;
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
                // Clamp advancing Brawler whirlwind & Rogue backstep safely inside arena bounds
                if fighter.attack_variant == 6 || fighter.attack_variant == 5 {
                    let at_edge = (body.position.x <= arena_min_x + 4.0 && body.ground_speed < 0.0)
                        || (body.position.x >= arena_max_x - 4.0 && body.ground_speed > 0.0);
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
                        body.ground_speed = body.facing * 175.0;
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
                        EnemyArchetype::FoundryBoss => 0.72,
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
                color_rgb: [4.2, 0.75, 5.4],
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
            // Ground-skimming magma waves ignore floor decks underneath them, only stopping at walls/gates
            if proj.is_magma_wave && rect.half_extents.x > rect.half_extents.y * 1.4 {
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
