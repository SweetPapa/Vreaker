//! Arc-length-parameterized 360° Sonic loop spline (`LoopSpline`) using Catmull-Rom
//! interpolation so traversal speed in `px/s` is uniform around the entire curve.

use bevy::prelude::*;

/// Evaluated sample along a [`LoopSpline`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplineSample {
    /// World point along the track surface where the player's feet travel.
    pub position: Vec2,
    /// Unit forward tangent along the spline direction.
    pub tangent: Vec2,
    /// Unit inward/upward track normal perpendicular to `tangent` (`(-t.y, t.x)`).
    pub normal: Vec2,
    /// Visual rotation angle in radians (`tangent.y.atan2(tangent.x)`).
    pub angle_rad: f32,
}

/// 360° loop-de-loop spline authored via ordered control points (entry, apex, exit).
#[derive(Component, Debug, Clone)]
pub struct LoopSpline {
    /// Unique identifier stored inside `MovementState::LoopLocked`.
    pub id: u32,
    /// Ordered control points defining the track centerline.
    pub control_points: Vec<Vec2>,
    /// Minimum entry speed (`px/s`) required along the entry tangent to lock onto the loop.
    pub min_speed: f32,
    /// Speed threshold (`px/s`) below which the player stalls and detaches near the apex.
    pub stall_speed: f32,
    /// Capture radius (`px`) around the entry point.
    pub entry_radius: f32,
    /// Uniformly sampled arc-length table for constant-speed progress lookup.
    samples: Vec<(f32, Vec2)>,
    total_length: f32,
}

impl LoopSpline {
    /// Builds a smooth Catmull-Rom [`LoopSpline`] from ordered control points.
    pub fn from_control_points(
        id: u32,
        control_points: Vec<Vec2>,
        min_speed: f32,
        stall_speed: f32,
        entry_radius: f32,
    ) -> Self {
        let mut spline = Self {
            id,
            control_points,
            min_speed,
            stall_speed,
            entry_radius,
            samples: Vec::new(),
            total_length: 1.0,
        };
        spline.rebuild_lut(160);
        spline
    }

    /// Generates a classic Sonic 360° counter-clockwise loop-de-loop:
    /// enters horizontally at `(center.x - lead, center.y - radius)` moving `+X`,
    /// climbs the right wall, runs inverted across the top apex, descends the left wall,
    /// and blasts out horizontally at `(center.x + lead, center.y - radius)` moving `+X`.
    pub fn circular_loop(
        id: u32,
        center: Vec2,
        radius: f32,
        approach_lead: f32,
        min_speed: f32,
    ) -> Self {
        let bottom_y = center.y - radius;
        let mut pts = Vec::with_capacity(19);
        pts.push(Vec2::new(center.x - approach_lead, bottom_y));

        // Sample 17 points from -90° (bottom) counter-clockwise +360° back to +270° (bottom),
        // with a tiny horizontal helix offset so entry and exit visual tracks look clean.
        let steps = 16;
        for i in 0..=steps {
            let frac = i as f32 / steps as f32;
            let theta = -std::f32::consts::FRAC_PI_2 + frac * std::f32::consts::TAU;
            let x = center.x + radius * theta.cos();
            let y = center.y + radius * theta.sin();
            pts.push(Vec2::new(x, y));
        }

        pts.push(Vec2::new(center.x + approach_lead, bottom_y));

        Self::from_control_points(id, pts, min_speed, 155.0, 26.0)
    }

    /// Total arc length of the loop spline in virtual pixels.
    pub fn total_length(&self) -> f32 {
        self.total_length
    }

    /// Entry trigger position (`progress = 0.0`).
    pub fn entry_point(&self) -> Vec2 {
        self.samples
            .first()
            .map(|(_, p)| *p)
            .unwrap_or(Vec2::ZERO)
    }

    /// Unit entry tangent at `progress = 0.0`.
    pub fn entry_tangent(&self) -> Vec2 {
        self.sample(0.0).tangent
    }

    /// Unit exit tangent at `progress = 1.0`.
    pub fn exit_tangent(&self) -> Vec2 {
        self.sample(1.0).tangent
    }

    /// Returns sampled polyline points for visual rendering of the track curve.
    pub fn polyline_points(&self, subdivisions: usize) -> Vec<Vec2> {
        let n = subdivisions.max(8);
        (0..=n)
            .map(|i| self.sample(i as f32 / n as f32).position)
            .collect()
    }

    /// Samples position, tangent, inward normal, and visual angle at normalized `progress` in `[0.0, 1.0]`.
    pub fn sample(&self, progress: f32) -> SplineSample {
        if self.samples.len() < 2 {
            return SplineSample {
                position: self.entry_point(),
                tangent: Vec2::X,
                normal: Vec2::Y,
                angle_rad: 0.0,
            };
        }

        let clamped = progress.clamp(0.0, 1.0);
        let target_dist = clamped * self.total_length;

        // Binary search the arc-length LUT
        let idx = match self
            .samples
            .binary_search_by(|(d, _)| d.total_cmp(&target_dist))
        {
            Ok(i) => i.min(self.samples.len() - 2),
            Err(i) => i.saturating_sub(1).min(self.samples.len() - 2),
        };

        let (d0, p0) = self.samples[idx];
        let (d1, p1) = self.samples[idx + 1];
        let seg_len = (d1 - d0).max(1e-5);
        let local_t = ((target_dist - d0) / seg_len).clamp(0.0, 1.0);
        let position = p0.lerp(p1, local_t);

        // Compute smooth finite-difference tangent using neighbors
        let prev_p = self.samples[idx.saturating_sub(1)].1;
        let next_p = self.samples[(idx + 2).min(self.samples.len() - 1)].1;
        let raw_tangent = if (idx == 0 && local_t < 0.05) || (idx + 1 >= self.samples.len() - 1) {
            p1 - p0
        } else {
            next_p - prev_p
        };
        let tangent = raw_tangent.normalize_or(Vec2::X);
        let normal = Vec2::new(-tangent.y, tangent.x);
        let angle_rad = tangent.y.atan2(tangent.x);

        SplineSample {
            position,
            tangent,
            normal,
            angle_rad,
        }
    }

    fn rebuild_lut(&mut self, total_subdivisions: usize) {
        self.samples.clear();
        let n = self.control_points.len();
        if n == 0 {
            self.total_length = 1.0;
            return;
        }
        if n == 1 {
            self.samples.push((0.0, self.control_points[0]));
            self.total_length = 1.0;
            return;
        }

        let seg_count = n - 1;
        let per_seg = (total_subdivisions / seg_count).max(4);
        let mut accum = 0.0_f32;
        let mut prev_pt = self.control_points[0];
        self.samples.push((0.0, prev_pt));

        for seg in 0..seg_count {
            let p0 = self.control_points[seg.saturating_sub(1)];
            let p1 = self.control_points[seg];
            let p2 = self.control_points[seg + 1];
            let p3 = self.control_points[(seg + 2).min(n - 1)];

            for step in 1..=per_seg {
                let t = step as f32 / per_seg as f32;
                let pt = catmull_rom(p0, p1, p2, p3, t);
                accum += pt.distance(prev_pt);
                self.samples.push((accum, pt));
                prev_pt = pt;
            }
        }

        self.total_length = accum.max(1.0);
    }
}

/// Standard centripetal-friendly uniform Catmull-Rom interpolation between `p1` and `p2`.
fn catmull_rom(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, t: f32) -> Vec2 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}
