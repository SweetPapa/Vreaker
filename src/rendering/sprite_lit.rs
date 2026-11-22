//! Custom Bevy `Material2d` for normal-mapped, HDR-emissive pixel-art sprites
//! with up to 16 dynamic 2D point lights (`assets/shaders/sprite_lit.wgsl`).

use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, MeshMaterial2d};

use crate::sprite_manifest::SpriteSheetManifest;

/// Maximum number of dynamic 2D point lights supported simultaneously by `sprite_lit.wgsl`.
pub const MAX_POINT_LIGHTS_2D: usize = 16;

/// Dynamic 2D point light in world pixel coordinates that illuminates all [`SpriteLitMaterial`] quads.
#[derive(Component, Debug, Clone, Copy)]
pub struct PointLight2d {
    /// Linear HDR RGB color of the light.
    pub color: LinearRgba,
    /// Scalar intensity multiplier applied to `color`.
    pub intensity: f32,
    /// Effective illumination radius in virtual world pixels.
    pub radius: f32,
    /// Virtual Z elevation above the 2D sprite plane (`> 0` lights front faces cleanly).
    pub height: f32,
    /// Distance falloff exponent (`1.0` linear, `2.0` quadratic).
    pub falloff: f32,
}

impl Default for PointLight2d {
    fn default() -> Self {
        Self {
            color: LinearRgba::new(1.0, 0.9, 0.75, 1.0),
            intensity: 2.0,
            radius: 180.0,
            height: 36.0,
            falloff: 1.6,
        }
    }
}

/// GPU-aligned single point-light entry (`32` bytes, 16-byte stride).
#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct PointLight2dUniform {
    /// `xy` = world position (px), `z` = height above sprite plane (px), `w` = radius (px).
    pub pos_height_radius: Vec4,
    /// `rgb` = linear color * intensity, `w` = falloff exponent.
    pub color_falloff: Vec4,
}

impl PointLight2dUniform {
    /// Packs a [`PointLight2d`] and its world XY position into GPU uniform layout.
    pub fn from_light(world_pos: Vec2, light: &PointLight2d) -> Self {
        Self {
            pos_height_radius: Vec4::new(
                world_pos.x,
                world_pos.y,
                light.height.max(1.0),
                light.radius.max(1.0),
            ),
            color_falloff: Vec4::new(
                light.color.red * light.intensity,
                light.color.green * light.intensity,
                light.color.blue * light.intensity,
                light.falloff.max(0.5),
            ),
        }
    }
}

/// Uniform payload bound at `@group(2) @binding(0)` in `assets/shaders/sprite_lit.wgsl`.
#[derive(Debug, Clone, Copy, ShaderType)]
pub struct SpriteLitUniform {
    /// `xy` = top-left UV offset of active frame, `zw` = frame UV size `(1/cols, 1/rows)`.
    pub atlas_rect: Vec4,
    /// `rgb` = linear ambient color, `a` = ambient intensity.
    pub ambient: Vec4,
    /// `x` = flip_x (`+1.0` right / `-1.0` left), `y` = `cos(rot)`, `z` = `sin(rot)`, `w` = active light count.
    pub transform_params: Vec4,
    /// `x` = HDR emission strength, `y` = hit flash (`0..1`), `z` = cel steps, `w` = rim specular strength.
    pub fx_params: Vec4,
    /// `rgb` = tint multiplier, `a` = alpha multiplier.
    pub tint: Vec4,
    /// Up to 16 active dynamic 2D point lights.
    pub lights: [PointLight2dUniform; MAX_POINT_LIGHTS_2D],
}

impl Default for SpriteLitUniform {
    fn default() -> Self {
        Self {
            atlas_rect: Vec4::new(0.0, 0.0, 1.0, 1.0),
            ambient: Vec4::new(0.24, 0.30, 0.42, 0.36),
            transform_params: Vec4::new(1.0, 1.0, 0.0, 0.0),
            fx_params: Vec4::new(3.6, 0.0, 4.0, 0.28),
            tint: Vec4::ONE,
            lights: [PointLight2dUniform::default(); MAX_POINT_LIGHTS_2D],
        }
    }
}

/// Custom Bevy `Material2d` rendering synchronized `_diffuse`, `_normal`, and `_emission` sprite atlases.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct SpriteLitMaterial {
    #[uniform(0)]
    pub data: SpriteLitUniform,
    #[texture(1)]
    #[sampler(2)]
    pub diffuse: Handle<Image>,
    #[texture(3)]
    #[sampler(4)]
    pub normal: Handle<Image>,
    #[texture(5)]
    #[sampler(6)]
    pub emission: Handle<Image>,
}

impl Material2d for SpriteLitMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/sprite_lit.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

/// Packs up to [`MAX_POINT_LIGHTS_2D`] lights into a fixed uniform array + active count.
///
/// Pure helper usable in headless unit tests to verify light-cap clamping.
pub fn pack_point_lights(
    lights: impl IntoIterator<Item = (Vec2, PointLight2d)>,
) -> ([PointLight2dUniform; MAX_POINT_LIGHTS_2D], usize) {
    let mut packed = [PointLight2dUniform::default(); MAX_POINT_LIGHTS_2D];
    let mut count = 0usize;

    for (pos, light) in lights {
        if count >= MAX_POINT_LIGHTS_2D {
            break;
        }
        packed[count] = PointLight2dUniform::from_light(pos, &light);
        count += 1;
    }

    (packed, count)
}

/// Drives frame playback on a [`SpriteLitMaterial`] entity using a [`SpriteSheetManifest`].
#[derive(Component, Debug, Clone)]
pub struct SpriteSheetAnimator {
    pub manifest: SpriteSheetManifest,
    pub current_anim: String,
    pub current_frame: u32,
    pub frame_timer: f32,
    pub flip_x: bool,
    pub finished: bool,
}

impl SpriteSheetAnimator {
    /// Creates an animator initialized on `initial_anim` (falling back to the first available clip).
    pub fn new(manifest: SpriteSheetManifest, initial_anim: &str) -> Self {
        let anim_name = if manifest.animations.contains_key(initial_anim) {
            initial_anim.to_string()
        } else {
            manifest
                .animations
                .keys()
                .next()
                .cloned()
                .unwrap_or_default()
        };
        let start_frame = manifest
            .animations
            .get(&anim_name)
            .map(|a| a.start)
            .unwrap_or(0);

        Self {
            manifest,
            current_anim: anim_name,
            current_frame: start_frame,
            frame_timer: 0.0,
            flip_x: false,
            finished: false,
        }
    }

    /// Switches to `anim_name` if it exists in the manifest and differs from the active clip.
    pub fn play(&mut self, anim_name: &str) {
        if self.current_anim == anim_name {
            return;
        }
        if let Some(def) = self.manifest.animations.get(anim_name) {
            self.current_anim = anim_name.to_string();
            self.current_frame = def.start;
            self.frame_timer = 0.0;
            self.finished = false;
        }
    }

    /// Restarts `anim_name` from its first frame even if it is already playing.
    pub fn play_restart(&mut self, anim_name: &str) {
        if let Some(def) = self.manifest.animations.get(anim_name) {
            self.current_anim = anim_name.to_string();
            self.current_frame = def.start;
            self.frame_timer = 0.0;
            self.finished = false;
        }
    }

    /// Advances playback by `dt_secs` and returns the active global frame index.
    pub fn tick(&mut self, dt_secs: f32) -> u32 {
        let Some(def) = self.manifest.animations.get(&self.current_anim) else {
            return self.current_frame;
        };
        if def.fps <= 0.0 || dt_secs <= 0.0 {
            return self.current_frame;
        }

        let frame_duration = 1.0 / def.fps;
        self.frame_timer += dt_secs;

        while self.frame_timer >= frame_duration {
            self.frame_timer -= frame_duration;
            if self.current_frame < def.end {
                self.current_frame += 1;
            } else if def.looping {
                self.current_frame = def.start;
            } else {
                self.current_frame = def.end;
                self.finished = true;
                self.frame_timer = 0.0;
                break;
            }
        }

        self.current_frame
    }

    /// Returns the UV rect `(u_off, v_off, u_scale, v_scale)` for the current frame.
    pub fn current_uv_rect(&self) -> Vec4 {
        self.manifest.frame_uv_rect(self.current_frame)
    }
}

/// Syncs [`SpriteSheetAnimator`] frame UVs, sprite flip/world rotation, and up to 16 [`PointLight2d`]
/// components into every active [`SpriteLitMaterial`].
pub fn sync_sprite_lit_materials(
    lights_query: Query<(&GlobalTransform, &PointLight2d)>,
    sprites_query: Query<(
        &GlobalTransform,
        &SpriteSheetAnimator,
        &MeshMaterial2d<SpriteLitMaterial>,
    )>,
    mut materials: ResMut<Assets<SpriteLitMaterial>>,
) {
    let (packed_lights, light_count) = pack_point_lights(
        lights_query
            .iter()
            .map(|(gt, light)| (gt.translation().truncate(), *light)),
    );

    for (global_transform, animator, mat_handle) in &sprites_query {
        let Some(mut material) = materials.get_mut(&mat_handle.0) else {
            continue;
        };

        let (_, world_rot, _) = global_transform.to_scale_rotation_translation();
        let (_, _, rot_z) = world_rot.to_euler(EulerRot::XYZ);
        let (sin_r, cos_r) = rot_z.sin_cos();

        material.data.atlas_rect = animator.current_uv_rect();
        material.data.transform_params = Vec4::new(
            if animator.flip_x { -1.0 } else { 1.0 },
            cos_r,
            sin_r,
            light_count as f32,
        );
        material.data.lights = packed_lights;
    }
}
