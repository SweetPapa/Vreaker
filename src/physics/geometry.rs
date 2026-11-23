//! Pure 2D collision geometry primitives and deterministic sensor queries.
//!
//! Completely decoupled from rendering (`MinimalPlugins`-safe).

use bevy::prelude::*;

/// Maximum terrain slope angle (`45°`) that the player snaps to naturally while grounded.
pub const MAX_WALKABLE_SLOPE_COS: f32 = std::f32::consts::FRAC_1_SQRT_2; // cos(45°) ≈ 0.7071

/// Axis-aligned solid block (floors, walls, ceilings, solid platforms).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct SolidRect {
    pub center: Vec2,
    pub half_extents: Vec2,
}

/// Marker identifying a low-friction Mirror-Ice solid deck or cryo slope (`Stage 2: Cryo Conduit`),
/// granting silky Smash momentum slides, faster sprint ceilings, and accelerated downhill slopes!
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IceSurface;

impl SolidRect {
    pub fn new(center: Vec2, size: Vec2) -> Self {
        Self {
            center,
            half_extents: size * 0.5,
        }
    }

    pub fn from_min_max(min: Vec2, max: Vec2) -> Self {
        Self {
            center: (min + max) * 0.5,
            half_extents: (max - min) * 0.5,
        }
    }

    pub fn min(&self) -> Vec2 {
        self.center - self.half_extents
    }

    pub fn max(&self) -> Vec2 {
        self.center + self.half_extents
    }
}

/// Angled walkable terrain slope segment (`start.x < end.x`) for Sonic momentum & alignment.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct SlopeSegment {
    pub start: Vec2,
    pub end: Vec2,
}

impl SlopeSegment {
    /// Creates a slope segment ordered left-to-right (`start.x <= end.x`).
    pub fn new(a: Vec2, b: Vec2) -> Self {
        if a.x <= b.x {
            Self { start: a, end: b }
        } else {
            Self { start: b, end: a }
        }
    }

    /// Unit surface normal pointing upward (`normal.y > 0`).
    pub fn normal(&self) -> Vec2 {
        let d = self.end - self.start;
        let len = d.length();
        if len <= 1e-5 {
            return Vec2::Y;
        }
        let n = Vec2::new(-d.y / len, d.x / len);
        if n.y >= 0.0 { n } else { -n }
    }

    /// Slope inclination angle `theta` in radians (`> 0` uphill right, `< 0` downhill right).
    pub fn angle_rad(&self) -> f32 {
        let d = self.end - self.start;
        d.y.atan2(d.x)
    }

    /// Evaluates surface Y at `x` if `x` lies within the segment's horizontal span.
    pub fn sample_y(&self, x: f32) -> Option<f32> {
        let dx = self.end.x - self.start.x;
        if dx.abs() <= 1e-5 || x < self.start.x - 0.5 || x > self.end.x + 0.5 {
            return None;
        }
        let t = ((x - self.start.x) / dx).clamp(0.0, 1.0);
        Some(self.start.y + (self.end.y - self.start.y) * t)
    }
}

/// Smash-style one-way drop-through platform solid only from above.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct OneWayPlatform {
    pub center: Vec2,
    pub half_width: f32,
    pub thickness: f32,
}

impl OneWayPlatform {
    pub fn new(center: Vec2, width: f32, thickness: f32) -> Self {
        Self {
            center,
            half_width: width * 0.5,
            thickness,
        }
    }

    pub fn top_y(&self) -> f32 {
        self.center.y + self.thickness * 0.5
    }

    pub fn min_x(&self) -> f32 {
        self.center.x - self.half_width
    }

    pub fn max_x(&self) -> f32 {
        self.center.x + self.half_width
    }
}

/// Sonic-style speed booster pad that accelerates grounded/passing players cleanly into loops.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct BoostPad {
    pub center: Vec2,
    pub half_extents: Vec2,
    pub direction: Vec2,
    pub target_speed: f32,
}

impl BoostPad {
    pub fn new(center: Vec2, size: Vec2, direction: Vec2, target_speed: f32) -> Self {
        Self {
            center,
            half_extents: size * 0.5,
            direction: direction.normalize_or(Vec2::X),
            target_speed,
        }
    }

    pub fn overlaps(&self, body_center: Vec2, body_half: Vec2) -> bool {
        let dx = (body_center.x - self.center.x).abs();
        let dy = (body_center.y - self.center.y).abs();
        dx <= self.half_extents.x + body_half.x && dy <= self.half_extents.y + body_half.y
    }
}

/// Deterministic Mega Man oscillating moving platform driving an attached [`OneWayPlatform`].
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct MovingPlatform {
    /// Starting center position (`t = 0`).
    pub origin: Vec2,
    /// Total end-to-end travel offset (`origin` <-> `origin + travel`).
    pub travel: Vec2,
    /// Oscillation angular frequency (`rad/s`).
    pub speed: f32,
    /// Current oscillation phase (`rad`).
    pub phase: f32,
    /// Displacement applied during the most recent `FixedUpdate` tick (used to carry grounded riders).
    pub last_delta: Vec2,
}

impl MovingPlatform {
    pub fn new(origin: Vec2, travel: Vec2, speed: f32) -> Self {
        Self {
            origin,
            travel,
            speed,
            phase: 0.0,
            last_delta: Vec2::ZERO,
        }
    }

    /// Evaluates the platform center at its current `phase`.
    pub fn current_center(&self) -> Vec2 {
        let frac = 0.5 - 0.5 * self.phase.cos();
        self.origin + self.travel * frac
    }
}

/// Non-instant-death spike hazard dealing small damage, Smash knockback, and brief i-frames.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct SpikeHazard {
    pub center: Vec2,
    pub half_extents: Vec2,
    pub damage: f32,
    pub knockback_vy: f32,
}

impl SpikeHazard {
    pub fn new(center: Vec2, size: Vec2) -> Self {
        Self {
            center,
            half_extents: size * 0.5,
            damage: 8.0,
            knockback_vy: 330.0,
        }
    }

    pub fn overlaps(&self, body_center: Vec2, body_half: Vec2) -> bool {
        let dx = (body_center.x - self.center.x).abs();
        let dy = (body_center.y - self.center.y).abs();
        dx <= self.half_extents.x + body_half.x * 0.85
            && dy <= self.half_extents.y + body_half.y * 0.9
    }
}

/// Pneumatic Foundry Spring Launcher that catapults fighters high into upper routes or aerial combos!
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct JumpPad {
    pub center: Vec2,
    pub half_extents: Vec2,
    pub launch_vy: f32,
}

impl JumpPad {
    pub fn new(center: Vec2, size: Vec2, launch_vy: f32) -> Self {
        Self {
            center,
            half_extents: size * 0.5,
            launch_vy: launch_vy.max(460.0),
        }
    }

    pub fn overlaps(&self, body_center: Vec2, body_half: Vec2) -> bool {
        let dx = (body_center.x - self.center.x).abs();
        let dy = (body_center.y - self.center.y).abs();
        dx <= self.half_extents.x + body_half.x && dy <= self.half_extents.y + body_half.y
    }
}

/// Interactive glowing Foundry Checkpoint Pylon (ignites cyan on touch, saves respawn, heals -25%).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct CheckpointBeacon {
    pub id: u32,
    pub pos: Vec2,
    pub activated: bool,
}

impl CheckpointBeacon {
    pub fn new(id: u32, pos: Vec2) -> Self {
        Self {
            id,
            pos,
            activated: false,
        }
    }
}

/// Kind of stage collectible pickup scattered along sightlines, combat lanes, and secret vaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CollectibleKind {
    /// Floating Foundry Energy Core (jump sightline breadcrumb, -5% Smash heal + score).
    EnergyCore,
    /// Hidden Golden Data Relic (3 hidden per stage — unlocks S-Rank!).
    SecretRelic,
    /// Emerald Nano-Repair Canister (-40% Smash damage heal!).
    HealCanister,
}

/// Collectible world pickup entity.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct CollectibleItem {
    pub kind: CollectibleKind,
    pub pos: Vec2,
    pub bob_phase: f32,
}

impl CollectibleItem {
    pub fn new(kind: CollectibleKind, pos: Vec2) -> Self {
        Self {
            kind,
            pos,
            bob_phase: (pos.x * 0.03 + pos.y * 0.02).rem_euclid(std::f32::consts::TAU),
        }
    }

    pub fn pickup_radius(&self) -> f32 {
        match self.kind {
            CollectibleKind::EnergyCore => 16.0,
            CollectibleKind::SecretRelic => 20.0,
            CollectibleKind::HealCanister => 18.0,
        }
    }
}

/// Cracked destructible Foundry bulkhead blocking secret vaults & speedrun shortcuts (`SolidRect` + `Hurtbox`).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct BreakableWall {
    pub hp: f32,
    pub max_hp: f32,
    pub center: Vec2,
    pub size: Vec2,
}

impl BreakableWall {
    pub fn new(center: Vec2, size: Vec2, max_hp: f32) -> Self {
        Self {
            hp: max_hp.max(10.0),
            max_hp: max_hp.max(10.0),
            center,
            size,
        }
    }

    pub fn hp_ratio(&self) -> f32 {
        (self.hp / self.max_hp.max(1.0)).clamp(0.0, 1.0)
    }
}

/// Lifecycle phase of a rhythmic [`TimedHazardBeam`] magma/plasma vent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HazardBeamPhase {
    Off,
    Telegraph,
    Active,
}

/// Rhythmic Mega Man X magma/laser Foundry vent cycling cleanly: `Off -> Telegraph -> Active`.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct TimedHazardBeam {
    pub center: Vec2,
    pub half_extents: Vec2,
    pub off_secs: f32,
    pub telegraph_secs: f32,
    pub active_secs: f32,
    pub timer: f32,
    pub damage: f32,
}

impl TimedHazardBeam {
    pub fn new(center: Vec2, size: Vec2, phase_offset: f32) -> Self {
        let off_secs = 1.65;
        let telegraph_secs = 0.60;
        let active_secs = 1.15;
        let total = off_secs + telegraph_secs + active_secs;
        Self {
            center,
            half_extents: size * 0.5,
            off_secs,
            telegraph_secs,
            active_secs,
            timer: phase_offset.rem_euclid(total),
            damage: 11.0,
        }
    }

    pub fn cycle_secs(&self) -> f32 {
        self.off_secs + self.telegraph_secs + self.active_secs
    }

    pub fn phase(&self) -> HazardBeamPhase {
        let t = self.timer.rem_euclid(self.cycle_secs());
        if t < self.off_secs {
            HazardBeamPhase::Off
        } else if t < self.off_secs + self.telegraph_secs {
            HazardBeamPhase::Telegraph
        } else {
            HazardBeamPhase::Active
        }
    }

    pub fn overlaps(&self, body_center: Vec2, body_half: Vec2) -> bool {
        let dx = (body_center.x - self.center.x).abs();
        let dy = (body_center.y - self.center.y).abs();
        dx <= self.half_extents.x + body_half.x * 0.8 && dy <= self.half_extents.y + body_half.y * 0.85
    }
}

/// Result of a dual foot downward ground-sensor cast.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroundHit {
    pub surface_y: f32,
    pub normal: Vec2,
    pub is_one_way: bool,
    pub is_slope: bool,
    pub is_ice: bool,
}

/// Casts dual downward ground sensors from left/right foot bounds against solid blocks,
/// walkable slopes (`<= 45°`), and one-way platforms.
#[allow(clippy::too_many_arguments)]
pub fn cast_dual_foot_sensors(
    pos: Vec2,
    half_extents: Vec2,
    prev_feet_y: f32,
    vel_y: f32,
    was_grounded: bool,
    drop_through_active: bool,
    solids: &[SolidRect],
    slopes: &[SlopeSegment],
    one_ways: &[OneWayPlatform],
) -> Option<GroundHit> {
    cast_dual_foot_sensors_with_ice(
        pos,
        half_extents,
        prev_feet_y,
        vel_y,
        was_grounded,
        drop_through_active,
        solids,
        &[],
        slopes,
        &[],
        one_ways,
    )
}

/// Extended dual downward ground-sensor cast tracking whether the landed solid or slope
/// carries the low-friction [`IceSurface`] surface property.
#[allow(clippy::too_many_arguments)]
pub fn cast_dual_foot_sensors_with_ice(
    pos: Vec2,
    half_extents: Vec2,
    prev_feet_y: f32,
    vel_y: f32,
    was_grounded: bool,
    drop_through_active: bool,
    solids: &[SolidRect],
    solid_ice: &[bool],
    slopes: &[SlopeSegment],
    slope_ice: &[bool],
    one_ways: &[OneWayPlatform],
) -> Option<GroundHit> {
    let feet_y = pos.y - half_extents.y;
    let sensor_inset = (half_extents.x * 0.72).max(2.0);
    let foot_xs = [pos.x - sensor_inset, pos.x + sensor_inset];

    // Allow generous downward snap when already grounded (for high-speed downhill slopes),
    // and tight capture when airborne & falling.
    let snap_below = if was_grounded { 14.0 } else { 2.5 };
    let step_above = if was_grounded { 12.0 } else { 4.0 };
    let max_surface_y = prev_feet_y.max(feet_y) + step_above;
    let min_surface_y = feet_y - snap_below;

    let mut best: Option<GroundHit> = None;
    let mut consider = |hit: GroundHit| {
        if hit.surface_y < min_surface_y || hit.surface_y > max_surface_y {
            return;
        }
        let replace = match best {
            None => true,
            Some(prev) => {
                // Prefer higher surface, breaking ties in favor of slopes for smooth transitions.
                hit.surface_y > prev.surface_y + 0.01
                    || ((hit.surface_y - prev.surface_y).abs() <= 0.01 && hit.is_slope)
            }
        };
        if replace {
            best = Some(hit);
        }
    };

    for &fx in &foot_xs {
        // 1. Solid AABB top faces
        for (idx, rect) in solids.iter().enumerate() {
            let min = rect.min();
            let max = rect.max();
            if fx >= min.x && fx <= max.x {
                // Require previous feet to be at/above top face (with small step margin)
                // so lateral wall contacts never snap the player onto the roof.
                if prev_feet_y >= max.y - step_above && (was_grounded || vel_y <= 0.0) {
                    consider(GroundHit {
                        surface_y: max.y,
                        normal: Vec2::Y,
                        is_one_way: false,
                        is_slope: false,
                        is_ice: solid_ice.get(idx).copied().unwrap_or(false),
                    });
                }
            }
        }

        // 2. Walkable slope segments (up to 45°)
        for (idx, slope) in slopes.iter().enumerate() {
            let normal = slope.normal();
            if normal.y < MAX_WALKABLE_SLOPE_COS - 1e-3 {
                continue;
            }
            if let Some(sy) = slope.sample_y(fx)
                && (was_grounded || vel_y <= 25.0)
            {
                consider(GroundHit {
                    surface_y: sy,
                    normal,
                    is_one_way: false,
                    is_slope: true,
                    is_ice: slope_ice.get(idx).copied().unwrap_or(false),
                });
            }
        }

        // 3. One-way drop-through platforms (disabled while drop-through timer is active)
        if !drop_through_active && (was_grounded || vel_y <= 0.0) {
            for plat in one_ways {
                if fx >= plat.min_x() && fx <= plat.max_x() {
                    let top = plat.top_y();
                    if prev_feet_y >= top - 4.0 {
                        consider(GroundHit {
                            surface_y: top,
                            normal: Vec2::Y,
                            is_one_way: true,
                            is_slope: false,
                            is_ice: false,
                        });
                    }
                }
            }
        }
    }

    best
}

/// Resolves horizontal overlap against solid AABB walls using separated-axis swept bounds
/// (`prev_x` -> `pos.x`), returning `(new_x, hit_wall_dir)`.
///
/// Crucially ignores `SolidRect`s that the body was already spanning horizontally (e.g. a wide
/// floor beneath a falling body) so vertical landings can NEVER eject a fighter horizontally.
pub fn resolve_horizontal_solids(
    prev_x: f32,
    pos: Vec2,
    half_extents: Vec2,
    solids: &[SolidRect],
    step_clearance: f32,
) -> (f32, Option<i8>) {
    let mut x = pos.x;
    let mut hit_dir: Option<i8> = None;
    let body_bottom = pos.y - half_extents.y + step_clearance;
    let body_top = pos.y + half_extents.y - 2.0;

    for rect in solids {
        let min = rect.min();
        let max = rect.max();
        if body_top <= min.y || body_bottom >= max.y {
            continue;
        }
        let overlap = (half_extents.x + rect.half_extents.x) - (x - rect.center.x).abs();
        if overlap <= 0.0 {
            continue;
        }

        // Only push out horizontally if the body approached from outside the left or right face
        // (small 6px margin allows minor sub-step / moving-platform seam resolution without
        // ever teleporting a body across a wide floor).
        let came_from_left = prev_x + half_extents.x <= min.x + 6.0;
        let came_from_right = prev_x - half_extents.x >= max.x - 6.0;

        if came_from_left {
            x = min.x - half_extents.x;
            hit_dir = Some(1);
        } else if came_from_right {
            x = max.x + half_extents.x;
            hit_dir = Some(-1);
        } else if overlap <= 8.0 {
            // Shallow lateral penetration (e.g., thin wall contact): push out via nearest edge
            if x < rect.center.x {
                x = min.x - half_extents.x;
                hit_dir = Some(1);
            } else {
                x = max.x + half_extents.x;
                hit_dir = Some(-1);
            }
        }
    }

    (x, hit_dir)
}

/// Resolves upward ceiling contact against solid AABB undersides.
pub fn resolve_ceiling_solids(
    pos: Vec2,
    half_extents: Vec2,
    vel_y: f32,
    solids: &[SolidRect],
) -> (f32, bool) {
    if vel_y <= 0.0 {
        return (pos.y, false);
    }

    let mut y = pos.y;
    let mut hit_ceiling = false;
    let body_left = pos.x - half_extents.x + 2.0;
    let body_right = pos.x + half_extents.x - 2.0;

    for rect in solids {
        let min = rect.min();
        let max = rect.max();
        if body_right <= min.x || body_left >= max.x {
            continue;
        }
        let body_top = y + half_extents.y;
        let body_bottom = y - half_extents.y;
        if body_top > min.y && body_bottom < min.y {
            y = min.y - half_extents.y;
            hit_ceiling = true;
        }
    }

    (y, hit_ceiling)
}

/// Probes left (`-1`) and right (`+1`) wall sensors for Mega Man wall-slide / wall-jump.
pub fn probe_wall_contact(
    pos: Vec2,
    half_extents: Vec2,
    preferred_dir: f32,
    solids: &[SolidRect],
) -> Option<i8> {
    const PROBE_MARGIN: f32 = 2.5;
    let torso_bottom = pos.y - half_extents.y * 0.45;
    let torso_top = pos.y + half_extents.y * 0.55;

    let mut left_hit = false;
    let mut right_hit = false;

    for rect in solids {
        let min = rect.min();
        let max = rect.max();
        if torso_top <= min.y || torso_bottom >= max.y {
            continue;
        }
        let right_edge = pos.x + half_extents.x;
        let left_edge = pos.x - half_extents.x;

        if right_edge + PROBE_MARGIN >= min.x && right_edge <= max.x {
            right_hit = true;
        }
        if left_edge - PROBE_MARGIN <= max.x && left_edge >= min.x {
            left_hit = true;
        }
    }

    match (left_hit, right_hit) {
        (true, true) => {
            if preferred_dir < 0.0 {
                Some(-1)
            } else {
                Some(1)
            }
        }
        (true, false) => Some(-1),
        (false, true) => Some(1),
        (false, false) => None,
    }
}
