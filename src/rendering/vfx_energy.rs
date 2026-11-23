//! GPU-accelerated procedural 2D energy/VFX `Material2d` (`assets/shaders/vfx_energy.wgsl`)
//! powering hexagonal holographic shields, high-voltage plasma lasers, swirling Buster charge
//! auras, radial shockwave rings, sculpted curved crescent saber smears, and volumetric lamp cones.

use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, MeshMaterial2d};

use crate::state::AppState;

/// Procedural mode indices matching `assets/shaders/vfx_energy.wgsl`.
pub mod vfx_mode {
    pub const HEX_SHIELD: f32 = 0.0;
    pub const PLASMA_BEAM: f32 = 1.0;
    pub const CHARGE_VORTEX: f32 = 2.0;
    pub const SHOCKWAVE_RING: f32 = 3.0;
    pub const CRESCENT_SLASH: f32 = 4.0;
    pub const LAMP_LIGHT_CONE: f32 = 5.0;
}

/// Uniform payload bound at `@group(2) @binding(0)` in `assets/shaders/vfx_energy.wgsl`.
#[derive(Debug, Clone, Copy, ShaderType)]
pub struct VfxEnergyUniform {
    /// `x` = mode (`0..5`), `y` = time (`secs`), `z` = progress/health (`0..1`), `w` = intensity (`0..1.5`).
    pub params0: Vec4,
    /// `x` = aspect (`h/w`), `y` = sweep/extra, `z` = telegraph flag (`0`/`1`), `w` = flip_x (`+1`/`-1`).
    pub params1: Vec4,
    /// Primary linear HDR color (`rgba`).
    pub color_a: Vec4,
    /// Core highlight linear HDR color (`rgba`).
    pub color_b: Vec4,
}

impl Default for VfxEnergyUniform {
    fn default() -> Self {
        Self {
            params0: Vec4::new(vfx_mode::HEX_SHIELD, 0.0, 1.0, 1.0),
            params1: Vec4::new(1.0, 1.2, 0.0, 1.0),
            color_a: Vec4::new(0.25, 2.8, 4.4, 0.85),
            color_b: Vec4::new(3.8, 4.8, 5.4, 1.0),
        }
    }
}

/// Custom Bevy `Material2d` rendering animated procedural HDR plasma, shields, and light cones.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct VfxEnergyMaterial {
    #[uniform(0)]
    pub data: VfxEnergyUniform,
}

impl Material2d for VfxEnergyMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/vfx_energy.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

/// Marker for persistent GPU shader VFX meshes whose `params0.y` (`time`) should animate continuously.
#[derive(Component, Debug, Clone, Copy)]
pub struct AnimatedVfxEnergy {
    pub phase_offset: f32,
}

/// Transient GPU shader VFX quad (sculpted crescent saber smear, shockwave ring, arcane pillar, KO beam)
/// that smoothly animates `params0.z` (`progress: 0 -> 1`) and despawns cleanly on completion.
#[derive(Component, Debug, Clone, Copy)]
pub struct TransientShaderVfx {
    pub velocity: Vec2,
    pub lifetime: f32,
    pub max_lifetime: f32,
    pub start_scale: Vec2,
    pub end_scale: Vec2,
    pub base_intensity: f32,
}

/// Cached unit-quad mesh (`1×1`) reused by all scaled `VfxEnergyMaterial` quads for minimal GPU overhead.
#[derive(Resource, Debug, Clone)]
pub struct SharedVfxQuadMesh(pub Handle<Mesh>);

/// Advances `time` on persistent [`VfxEnergyMaterial`] entities and animates/despawns
/// [`TransientShaderVfx`] quads deterministically.
#[allow(clippy::type_complexity)]
pub fn tick_vfx_energy_materials(
    mut commands: Commands,
    time: Res<Time>,
    state: Res<State<AppState>>,
    mut materials: ResMut<Assets<VfxEnergyMaterial>>,
    persistent: Query<
        (&AnimatedVfxEnergy, &MeshMaterial2d<VfxEnergyMaterial>),
        Without<TransientShaderVfx>,
    >,
    mut transients: Query<(
        Entity,
        &mut TransientShaderVfx,
        &mut Transform,
        &MeshMaterial2d<VfxEnergyMaterial>,
    )>,
) {
    if *state.get() == AppState::Paused {
        return;
    }
    let elapsed = time.elapsed_secs();
    let dt = time.delta_secs();

    for (anim, mat_handle) in &persistent {
        if let Some(mut mat) = materials.get_mut(&mat_handle.0) {
            mat.data.params0.y = elapsed + anim.phase_offset;
        }
    }

    for (entity, mut vfx, mut tf, mat_handle) in &mut transients {
        vfx.lifetime -= dt;
        if vfx.lifetime <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        let frac = (vfx.lifetime / vfx.max_lifetime.max(0.001)).clamp(0.0, 1.0);
        let prog = 1.0 - frac;
        tf.translation.x += vfx.velocity.x * dt;
        tf.translation.y += vfx.velocity.y * dt;
        let cur_scale = vfx.start_scale.lerp(vfx.end_scale, prog);
        tf.scale.x = cur_scale.x;
        tf.scale.y = cur_scale.y;

        if let Some(mut mat) = materials.get_mut(&mat_handle.0) {
            mat.data.params0.y = elapsed;
            mat.data.params0.z = prog;
            mat.data.params0.w = vfx.base_intensity * (0.25 + 0.75 * frac);
        }
    }
}
