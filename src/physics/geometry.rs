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

/// Result of a dual foot downward ground-sensor cast.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroundHit {
    pub surface_y: f32,
    pub normal: Vec2,
    pub is_one_way: bool,
    pub is_slope: bool,
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
        for rect in solids {
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
                    });
                }
            }
        }

        // 2. Walkable slope segments (up to 45°)
        for slope in slopes {
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
                        });
                    }
                }
            }
        }
    }

    best
}

/// Resolves horizontal overlap against solid AABB walls, returning `(new_x, hit_wall_dir)`.
pub fn resolve_horizontal_solids(
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
        if overlap > 0.0 {
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
