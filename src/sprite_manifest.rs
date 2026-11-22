//! Pure, headless-safe sprite-sheet manifest schema (`<name>.ron`).
//!
//! Shared between the Blender sprite baker (`tools/sprite_baker/`), the 2D normal-mapped
//! rendering pipeline (`src/rendering/`), and deterministic combat hitbox checks
//! (`src/combat/`) without coupling logic modules to rendering assets.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use bevy::math::Vec4;
use serde::{Deserialize, Serialize};

/// Per-frame melee hitbox emitted from projected 3D bone coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameHitbox {
    /// Global zero-based frame index inside the sprite atlas.
    pub frame: u32,
    /// Offset in sprite pixels relative to frame center (`+X` forward/right, `+Y` up).
    pub offset: (f32, f32),
    /// Circular hitbox radius in pixels.
    pub radius: f32,
    /// Damage added to target's Smash `DamagePercent`.
    pub damage: f32,
    /// Base Smash knockback speed (`px/s`).
    pub base_kb: f32,
    /// Knockback scaling per target `%`.
    pub kb_scale: f32,
    /// Launch angle in degrees (`0°` = horizontal forward, `90°` = straight up, negative = meteor spike).
    pub angle_deg: f32,
}

/// Animation clip definition inside a [`SpriteSheetManifest`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnimationDef {
    /// Inclusive first frame index in the atlas.
    pub start: u32,
    /// Inclusive last frame index in the atlas.
    pub end: u32,
    /// Playback speed in frames per second.
    pub fps: f32,
    /// Whether playback wraps from `end` back to `start`.
    pub looping: bool,
    /// Optional active combat hitboxes bound to specific frames of this animation.
    #[serde(default)]
    pub active_hitboxes: Vec<FrameHitbox>,
}

impl AnimationDef {
    /// Total number of frames in this clip (`end - start + 1`).
    pub fn frame_count(&self) -> u32 {
        self.end.saturating_sub(self.start) + 1
    }

    /// Returns all active hitboxes for a specific global atlas frame.
    pub fn hitboxes_for_frame(&self, global_frame: u32) -> impl Iterator<Item = &FrameHitbox> {
        self.active_hitboxes
            .iter()
            .filter(move |hb| hb.frame == global_frame)
    }
}

/// Projected 2D bone coordinates (`+X` right, `+Y` up from frame center) for a single frame.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BoneFrameSockets {
    #[serde(default)]
    pub hand_r: Option<(f32, f32)>,
    #[serde(default)]
    pub hand_l: Option<(f32, f32)>,
    #[serde(default)]
    pub foot_r: Option<(f32, f32)>,
    #[serde(default)]
    pub foot_l: Option<(f32, f32)>,
    #[serde(default)]
    pub buster_muzzle: Option<(f32, f32)>,
    #[serde(default)]
    pub chest: Option<(f32, f32)>,
}

/// Top-level sprite atlas manifest matching `SPEC.MD` Section 3.3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpriteSheetManifest {
    /// Pixel dimensions `(width, height)` of a single sprite frame (e.g. `(96, 96)`).
    pub frame_size: (u32, u32),
    /// Number of columns in the packed atlas PNGs.
    pub columns: u32,
    /// Number of rows in the packed atlas PNGs.
    pub rows: u32,
    /// Named animations keyed by gameplay state (`"idle"`, `"run"`, `"f_air"`, etc.).
    pub animations: BTreeMap<String, AnimationDef>,
    /// Optional per-frame projected 2D bone positions keyed by global frame index.
    #[serde(default)]
    pub bone_sockets: BTreeMap<u32, BoneFrameSockets>,
}

impl SpriteSheetManifest {
    /// Parses a [`SpriteSheetManifest`] from a RON string.
    pub fn from_ron_str(source: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(source)
    }

    /// Loads and validates a [`SpriteSheetManifest`] from disk.
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self, String> {
        let path_ref = path.as_ref();
        let text = fs::read_to_string(path_ref)
            .map_err(|err| format!("Failed reading manifest {}: {}", path_ref.display(), err))?;
        let manifest = Self::from_ron_str(&text)
            .map_err(|err| format!("Failed parsing RON manifest {}: {}", path_ref.display(), err))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Total cell capacity of the packed grid (`columns * rows`).
    pub fn capacity(&self) -> u32 {
        self.columns.saturating_mul(self.rows)
    }

    /// Computes normalized UV `(u_offset, v_offset, u_scale, v_scale)` for `frame_index`.
    pub fn frame_uv_rect(&self, frame_index: u32) -> Vec4 {
        let cols = self.columns.max(1);
        let rows = self.rows.max(1);
        let clamped = frame_index.min(cols.saturating_mul(rows).saturating_sub(1));
        let col = clamped % cols;
        let row = clamped / cols;
        let u_scale = 1.0 / cols as f32;
        let v_scale = 1.0 / rows as f32;
        Vec4::new(col as f32 * u_scale, row as f32 * v_scale, u_scale, v_scale)
    }

    /// Validates structural invariants so malformed manifests fail loudly in tests.
    pub fn validate(&self) -> Result<(), String> {
        if self.frame_size.0 == 0 || self.frame_size.1 == 0 {
            return Err(format!("Invalid zero frame_size: {:?}", self.frame_size));
        }
        if self.columns == 0 || self.rows == 0 {
            return Err(format!(
                "Invalid zero atlas grid dimensions: {}x{}",
                self.columns, self.rows
            ));
        }
        if self.animations.is_empty() {
            return Err("Manifest contains zero animations".to_string());
        }

        let cap = self.capacity();
        for (name, anim) in &self.animations {
            if anim.start > anim.end {
                return Err(format!(
                    "Animation '{}' has start ({}) > end ({})",
                    name, anim.start, anim.end
                ));
            }
            if anim.end >= cap {
                return Err(format!(
                    "Animation '{}' end frame {} exceeds atlas capacity {}",
                    name, anim.end, cap
                ));
            }
            if anim.fps <= 0.0 || !anim.fps.is_finite() {
                return Err(format!("Animation '{}' has invalid fps {}", name, anim.fps));
            }
            for hb in &anim.active_hitboxes {
                if hb.frame < anim.start || hb.frame > anim.end {
                    return Err(format!(
                        "Animation '{}' hitbox frame {} outside {}..={}",
                        name, hb.frame, anim.start, anim.end
                    ));
                }
                if hb.radius <= 0.0 {
                    return Err(format!(
                        "Animation '{}' hitbox frame {} has non-positive radius {}",
                        name, hb.frame, hb.radius
                    ));
                }
            }
        }
        Ok(())
    }
}
