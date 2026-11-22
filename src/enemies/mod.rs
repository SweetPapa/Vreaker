//! Phase 4 Enemy AI (Light Fast-Mover, Heavy Fighter, Zone B Wall Turrets),
//! spike hazards, Smash Blast-Zone KOs, and 3-second deterministic respawns.
//!
//! Completely decoupled from rendering (`MinimalPlugins`-safe).

use std::time::Duration;

use bevy::prelude::*;

use crate::combat::{
    AttachedHitbox, CombatHitEvent, CombatPostPhysicsSystems, CombatPrePhysicsSystems,
    DamagePercent, Hitbox, Hitstop, Hitstun, Hurtbox, Invulnerable, KnockbackWeight, Team,
};
use crate::physics::{KinematicBody, MovementState, SolidRect, SpikeHazard};
use crate::player::Player;
use crate::state::StageRestartEvent;

/// Respawn delay in seconds after knocking an arena fighter into a Smash blast zone (`SPEC.MD`).
pub const ENEMY_KO_RESPAWN_SECS: f32 = 3.0;

/// Arena AI fighter archetype (`SPEC.MD` Section 6: 1 light fast-mover, 1 heavy punching bag).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnemyArchetype {
    /// Agile fast-mover rendered with baked Rogue sprite sheets.
    LightStriker,
    /// Heavy punching-bag bruiser rendered with baked Barbarian sprite sheets.
    HeavyBrawler,
}

impl EnemyArchetype {
    pub fn display_name(self) -> &'static str {
        match self {
            EnemyArchetype::LightStriker => "ROGUE STRIKER",
            EnemyArchetype::HeavyBrawler => "IRON BRAWLER",
        }
    }

    pub fn move_speed(self) -> f32 {
        match self {
            EnemyArchetype::LightStriker => 185.0,
            EnemyArchetype::HeavyBrawler => 105.0,
        }
    }

    pub fn attack_range(self) -> f32 {
        match self {
            EnemyArchetype::LightStriker => 42.0,
            EnemyArchetype::HeavyBrawler => 50.0,
        }
    }

    pub fn telegraph_secs(self) -> f32 {
        match self {
            EnemyArchetype::LightStriker => 0.34,
            EnemyArchetype::HeavyBrawler => 0.52,
        }
    }

    pub fn half_extents(self) -> Vec2 {
        match self {
            EnemyArchetype::LightStriker => Vec2::new(10.0, 18.0),
            EnemyArchetype::HeavyBrawler => Vec2::new(12.0, 19.0),
        }
    }
}

/// Finite state machine for Zone C Smash Arena AI fighters.
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

/// Arena AI fighter component.
#[derive(Component, Debug, Clone)]
pub struct EnemyFighter {
    pub archetype: EnemyArchetype,
    pub home_pos: Vec2,
    pub state: EnemyAiState,
    pub jump_cooldown: f32,
}

impl EnemyFighter {
    pub fn new(archetype: EnemyArchetype, home_pos: Vec2) -> Self {
        Self {
            archetype,
            home_pos,
            state: EnemyAiState::Patrol,
            jump_cooldown: 0.0,
        }
    }

    pub fn is_telegraphing(&self) -> bool {
        matches!(self.state, EnemyAiState::Telegraph { .. })
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

/// Enemy energy bolt fired by a [`WallTurret`].
#[derive(Component, Debug, Clone, Copy)]
pub struct TurretProjectile {
    pub velocity: Vec2,
}

/// Smash Arena blast-zone volume (`min..=max`); leaving triggers a KO flash, SFX, and 3s respawn.
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

/// Message emitted whenever an arena fighter is launched across a [`BlastZone`] edge.
#[derive(Message, Debug, Clone, Copy)]
pub struct BlastZoneKoEvent {
    pub archetype: EnemyArchetype,
    pub ko_pos: Vec2,
    pub edge: BlastEdge,
    pub final_percent: f32,
}

/// Tracks live Zone C Smash KO score & turret takedowns.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct ArenaScore {
    pub ko_count: u32,
    pub turrets_destroyed: u32,
}

/// Helper spawning a complete headless-safe [`EnemyFighter`] entity (decorated visually by `RenderingPlugin`).
pub fn spawn_enemy_fighter(
    commands: &mut Commands,
    archetype: EnemyArchetype,
    home_pos: Vec2,
    spawn_invuln: bool,
) -> Entity {
    let half = archetype.half_extents();
    let mut ec = commands.spawn((
        Name::new(archetype.display_name()),
        EnemyFighter::new(archetype, home_pos),
        Team::Enemy,
        Hurtbox {
            offset: Vec2::ZERO,
            radius: if archetype == EnemyArchetype::HeavyBrawler {
                19.0
            } else {
                16.0
            },
        },
        DamagePercent(0.0),
        KnockbackWeight(match archetype {
            EnemyArchetype::LightStriker => 1.15,
            EnemyArchetype::HeavyBrawler => 0.72,
        }),
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
        ec.insert(Invulnerable::new(1.0));
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

/// Plugin managing enemy AI, Zone B wall turrets, spike hazards, and Smash blast-zone KOs.
pub struct EnemiesPlugin;

impl Plugin for EnemiesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ArenaScore>()
            .add_message::<BlastZoneKoEvent>()
            .add_systems(
                FixedUpdate,
                (
                    update_enemy_fighter_ai,
                    update_wall_turrets,
                    advance_turret_projectiles,
                )
                    .in_set(CombatPrePhysicsSystems),
            )
            .add_systems(
                FixedUpdate,
                (
                    resolve_spike_hazards,
                    check_blast_zone_kos,
                    tick_pending_enemy_respawns,
                )
                    .chain()
                    .in_set(CombatPostPhysicsSystems),
            )
            .add_systems(Update, reset_arena_score_on_restart);
    }
}

fn reset_arena_score_on_restart(
    mut restarts: MessageReader<StageRestartEvent>,
    mut score: ResMut<ArenaScore>,
) {
    if restarts.read().next().is_some() {
        *score = ArenaScore::default();
    }
}

#[allow(clippy::type_complexity)]
fn update_enemy_fighter_ai(
    mut commands: Commands,
    fixed_time: Res<Time<Fixed>>,
    players: Query<&KinematicBody, (With<Player>, Without<EnemyFighter>)>,
    mut enemies: Query<(
        Entity,
        &mut EnemyFighter,
        &mut KinematicBody,
        Option<&Hitstop>,
        Option<&Hitstun>,
    )>,
) {
    let dt = fixed_time.delta_secs();
    let player_pos = players.single().map(|p| p.position).ok();

    for (entity, mut fighter, mut body, hitstop, hitstun) in &mut enemies {
        if hitstop.is_some_and(Hitstop::is_active) {
            continue;
        }
        fighter.jump_cooldown = (fighter.jump_cooldown - dt).max(0.0);

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
        let aggro_range = 440.0;

        // Keep AI from walking itself off the Battlefield ledges voluntarily
        let arena_min_x = fighter.home_pos.x - 190.0;
        let arena_max_x = fighter.home_pos.x + 190.0;

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

                    // Light fast-mover hops up to pressure player on drop-through platforms
                    if fighter.archetype == EnemyArchetype::LightStriker
                        && delta.y > 42.0
                        && delta.x.abs() < 120.0
                        && fighter.jump_cooldown <= 0.0
                    {
                        body.velocity.y = 395.0;
                        body.state = MovementState::Airborne;
                        fighter.jump_cooldown = 1.4;
                    }
                } else {
                    body.velocity.x = body.velocity.x.lerp(target_spd, (dt * 4.0).min(1.0));
                }

                if dist <= fighter.archetype.attack_range() && delta.y.abs() < 36.0 {
                    body.ground_speed = 0.0;
                    body.velocity.x *= 0.25;
                    fighter.state = EnemyAiState::Telegraph {
                        remaining: fighter.archetype.telegraph_secs(),
                    };
                }
            }
            EnemyAiState::Telegraph { mut remaining } => {
                body.ground_speed *= (1.0 - dt * 10.0).max(0.0);
                remaining -= dt;
                if remaining <= 0.0 {
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
                            hit_entities: Vec::new(),
                            is_charged: is_heavy,
                        },
                        Transform::from_xyz(pos.x, pos.y, 1.0),
                    ));

                    fighter.state = EnemyAiState::Attacking { remaining: 0.16 };
                } else {
                    fighter.state = EnemyAiState::Telegraph { remaining };
                }
            }
            EnemyAiState::Attacking { mut remaining } => {
                remaining -= dt;
                if remaining <= 0.0 {
                    let rec = if fighter.archetype == EnemyArchetype::HeavyBrawler {
                        0.75
                    } else {
                        0.45
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
                    fighter.state = EnemyAiState::Chase;
                } else {
                    fighter.state = EnemyAiState::Recovery { remaining };
                }
            }
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
        // Keep wall turrets firmly bolted to their shaft wall mount
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

        // Taking >= hp_threshold damage (2 Buster pellets or 1 Charged Shot!) destroys the turret!
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
                    hit_entities: Vec::new(),
                    is_charged: false,
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

/// Checks if any Zone C [`EnemyFighter`] has been knocked outside a [`BlastZone`],
/// triggering a Smash KO event, score increment, and deterministic 3-second respawn.
fn check_blast_zone_kos(
    mut commands: Commands,
    blast_zones: Query<&BlastZone>,
    enemies: Query<(Entity, &EnemyFighter, &KinematicBody, &DamagePercent)>,
    mut score: ResMut<ArenaScore>,
    mut ko_events: MessageWriter<BlastZoneKoEvent>,
) {
    let Some(zone) = blast_zones.iter().next() else {
        return;
    };

    for (entity, fighter, body, percent) in &enemies {
        if !zone.contains(body.position) {
            let edge = zone.crossed_edge(body.position);
            score.ko_count += 1;

            ko_events.write(BlastZoneKoEvent {
                archetype: fighter.archetype,
                ko_pos: body.position,
                edge,
                final_percent: percent.0,
            });

            commands.spawn((
                Name::new("PendingEnemyRespawn"),
                PendingEnemyRespawn {
                    archetype: fighter.archetype,
                    home_pos: fighter.home_pos,
                    timer: Timer::from_seconds(ENEMY_KO_RESPAWN_SECS, TimerMode::Once),
                },
            ));

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
