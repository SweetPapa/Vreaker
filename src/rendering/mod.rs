//! Pixel-perfect 2D rendering pipeline (`640x360` internal render target with
//! integer upscaling, `ImageSampler::nearest()`, HDR, Bloom, custom 2D normal-mapped
//! sprite lighting via `assets/shaders/sprite_lit.wgsl`, Sonic cyan/amber speed
//! afterimage ghost trails, baked normal-mapped AI fighters, Smash KO flash beams,
//! and velocity look-ahead + Zone C arena framing camera).

pub mod sprite_lit;

use std::path::Path;

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{Hdr, RenderTarget, ScalingMode};
use bevy::image::{ImageLoaderSettings, ImageSampler};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::Msaa;
use bevy::sprite_render::{Material2dPlugin, MeshMaterial2d};
use bevy::transform::TransformSystems;
use bevy::window::{PrimaryWindow, WindowResized};

use crate::combat::{
    AttackKind, AttackState, BusterProjectile, BusterShotEvent, BusterState, CombatHitEvent,
    Hitstop, Hitstun, Invulnerable,
};
use crate::enemies::{
    BlastEdge, BlastZone, BlastZoneKoEvent, EnemyAiState, EnemyArchetype, EnemyFighter,
    TurretProjectile, WallTurret,
};
use crate::physics::{
    BoostPad, KinematicBody, LoopSpline, MovementState, MovingPlatform, OneWayPlatform,
    PhysicsConfig, SlopeSegment, SolidRect, SpikeHazard,
};
use crate::player::{Player, PlayerFeetPivot, PlayerSpawnPoint, PlayerSpriteVisual};
use crate::sprite_manifest::SpriteSheetManifest;
use crate::state::{AppState, StageRestartEvent};

pub use sprite_lit::{
    MAX_POINT_LIGHTS_2D, PointLight2d, SpriteLitMaterial, SpriteLitUniform, SpriteSheetAnimator,
};

/// Internal low-resolution pixel canvas width (16:9, exact integer scales to 720p/1080p/1440p).
pub const VIRTUAL_WIDTH: u32 = 640;
/// Internal low-resolution pixel canvas height.
pub const VIRTUAL_HEIGHT: u32 = 360;

/// Render layer for all pixel-art gameplay entities rendered onto the low-res target.
pub const WORLD_RENDER_LAYER: usize = 0;
/// Render layer for the outer display camera and upscaled pixel canvas quad.
pub const CANVAS_RENDER_LAYER: usize = 1;

/// Interval (`0.04s` per SPEC.MD Section 7) between translucent Sonic speed afterimages.
const AFTERIMAGE_INTERVAL_SECS: f32 = 0.04;

/// Pure integer scaling result for the pixel-perfect render target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntegerViewport {
    /// Strict integer upscale factor (`>= 1`).
    pub scale: u32,
    /// Upscaled logical display width in pixels (`virtual_width * scale`).
    pub display_width: u32,
    /// Upscaled logical display height in pixels (`virtual_height * scale`).
    pub display_height: u32,
}

/// Computes the largest exact integer scale that fits `(virtual_width, virtual_height)`
/// inside `(window_width, window_height)` with zero fractional pixel distortion.
pub fn compute_integer_viewport(
    window_width: f32,
    window_height: f32,
    virtual_width: u32,
    virtual_height: u32,
) -> IntegerViewport {
    let vw = virtual_width.max(1);
    let vh = virtual_height.max(1);
    let scale_x = (window_width.max(1.0) / vw as f32).floor() as u32;
    let scale_y = (window_height.max(1.0) / vh as f32).floor() as u32;
    let scale = scale_x.min(scale_y).max(1);

    IntegerViewport {
        scale,
        display_width: vw * scale,
        display_height: vh * scale,
    }
}

/// Resource storing the offscreen pixel-art render target handle.
#[derive(Resource, Debug, Clone)]
pub struct PixelCanvasImage(pub Handle<Image>);

/// Cached hero sprite handles reused by translucent Sonic speed afterimage ghosts.
#[derive(Resource, Debug, Clone)]
struct HeroSpriteCache {
    mesh: Handle<Mesh>,
    diffuse: Handle<Image>,
    normal: Handle<Image>,
    emission: Handle<Image>,
}

/// Timer & alternating color state for translucent cyan/amber Sonic afterimage spawning.
#[derive(Resource, Debug, Default)]
struct AfterimageSpawner {
    timer: f32,
    alternate: bool,
}

/// Fading Sonic speed afterimage ghost entity.
#[derive(Component, Debug)]
struct AfterimageGhost {
    remaining: f32,
    max_lifetime: f32,
    base_alpha: f32,
}

/// Directional camera trauma / shake state driven by heavy melee impacts, Charged Buster, and KOs.
#[derive(Resource, Debug, Default)]
pub struct CameraShake {
    pub trauma: f32,
    pub direction: Vec2,
    pub phase: f32,
}

impl CameraShake {
    pub fn add_impulse(&mut self, amount: f32, dir: Vec2) {
        self.trauma = (self.trauma + amount).min(1.0);
        if dir.length_squared() > 1e-4 {
            self.direction = dir.normalize();
        }
    }
}

/// Marker for the low-res `640x360` world camera (HDR + Bloom, `Msaa::Off`).
#[derive(Component, Debug)]
pub struct PixelWorldCamera {
    smooth_pos: Vec2,
    look_ahead: Vec2,
}

/// Marker for the outer native-resolution display camera (also hosts native Bevy UI).
#[derive(Component, Debug)]
pub struct OuterDisplayCamera;

/// Marker for the quad sprite presenting the upscaled pixel canvas.
#[derive(Component, Debug)]
pub struct PixelCanvasSprite;

/// Orbiting warm-amber 2D point light tracking near the hero for live normal-map visibility.
#[derive(Component, Debug)]
struct HeroCompanionLight {
    angle: f32,
    orbit_radius_x: f32,
    orbit_radius_y: f32,
}

/// Dynamic burst point light triggered on impacts, Charged Buster release, and KOs.
#[derive(Component, Debug)]
struct ActionBurstLight {
    decay: f32,
}

/// Glowing charge aura visual + dynamic light attached to the player during Buster charge.
#[derive(Component, Debug)]
struct BusterChargeAura {
    pulse: f32,
}

/// Child visual of an [`EnemyFighter`] driving baked Rogue/Barbarian sprites + hitstop jitter.
#[derive(Component, Debug)]
struct EnemySpriteVisual {
    owner: Entity,
    jitter_phase: f32,
}

/// Child visual marker for a [`WallTurret`] cannon & telegraph LED.
#[derive(Component, Debug)]
struct TurretTelegraphLed {
    owner: Entity,
}

/// Short-lived HDR hit-spark particle bursting outward on combat impact.
#[derive(Component, Debug)]
struct HitSpark {
    velocity: Vec2,
    lifetime: f32,
    max_lifetime: f32,
}

/// Dramatic Smash-style directional KO flash beam exploding inward from a [`BlastEdge`].
#[derive(Component, Debug)]
struct SmashKoFlashBeam {
    lifetime: f32,
    max_lifetime: f32,
}

/// Tracks the last started melee move on the visual animator so combo hits restart cleanly.
#[derive(Resource, Debug, Default)]
struct LastVisualAttack {
    last_move: Option<AttackKind>,
    last_active_timer: f32,
}

/// Plugin setting up the pixel-perfect camera rig, integer upscaler, custom 2D normal-mapped
/// sprite material, smooth look-ahead & Zone C arena framing camera, Sonic afterimages,
/// Smash KO flashes, and automatic visual decoration of LDtk gameplay entities.
pub struct RenderingPlugin;

impl Plugin for RenderingPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(Material2dPlugin::<SpriteLitMaterial>::default())
            .init_resource::<CameraShake>()
            .init_resource::<LastVisualAttack>()
            .init_resource::<AfterimageSpawner>()
            .add_systems(Startup, (setup_pixel_cameras, spawn_foundry_backdrop))
            .add_systems(
                Update,
                (
                    decorate_added_player,
                    decorate_added_solids,
                    decorate_added_slopes,
                    decorate_added_one_ways,
                    decorate_added_boost_pads,
                    decorate_added_loop_splines,
                    decorate_added_spike_hazards,
                    decorate_added_wall_turrets,
                    decorate_added_enemy_fighters,
                    decorate_added_buster_projectiles,
                    decorate_added_turret_projectiles,
                    decorate_added_blast_zones,
                ),
            )
            .add_systems(
                Update,
                (
                    sync_canvas_integer_scale,
                    drive_player_visuals_and_animations,
                    spawn_and_fade_speed_afterimages,
                    update_buster_charge_aura,
                    drive_enemy_fighter_visuals,
                    update_wall_turret_visuals,
                    handle_combat_hit_and_shot_fx,
                    handle_blast_zone_ko_flash,
                    update_hit_sparks_and_ko_beams,
                    update_follow_camera,
                    animate_hero_companion_light,
                ),
            )
            .add_systems(
                PostUpdate,
                sprite_lit::sync_sprite_lit_materials.after(TransformSystems::Propagate),
            );
    }
}

fn setup_pixel_cameras(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    primary_window: Query<&Window, With<PrimaryWindow>>,
) {
    let mut canvas_image = Image::new_target_texture(
        VIRTUAL_WIDTH,
        VIRTUAL_HEIGHT,
        TextureFormat::Rgba8UnormSrgb,
        None,
    );
    canvas_image.sampler = ImageSampler::nearest();
    let canvas_handle = images.add(canvas_image);
    commands.insert_resource(PixelCanvasImage(canvas_handle.clone()));

    let initial_viewport = primary_window
        .single()
        .map(|w| compute_integer_viewport(w.width(), w.height(), VIRTUAL_WIDTH, VIRTUAL_HEIGHT))
        .unwrap_or_else(|_| {
            compute_integer_viewport(
                (VIRTUAL_WIDTH * 2) as f32,
                (VIRTUAL_HEIGHT * 2) as f32,
                VIRTUAL_WIDTH,
                VIRTUAL_HEIGHT,
            )
        });

    let start_cam = Vec2::new(-180.0, 20.0);

    commands.spawn((
        Name::new("PixelWorldCamera"),
        PixelWorldCamera {
            smooth_pos: start_cam,
            look_ahead: Vec2::ZERO,
        },
        Camera2d,
        Camera {
            order: 0,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.03, 0.04, 0.08)),
            ..default()
        },
        Transform::from_xyz(start_cam.x, start_cam.y, 0.0),
        RenderTarget::Image(canvas_handle.clone().into()),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed {
                width: VIRTUAL_WIDTH as f32,
                height: VIRTUAL_HEIGHT as f32,
            },
            ..OrthographicProjection::default_2d()
        }),
        Hdr,
        Bloom {
            intensity: 0.28,
            ..Bloom::NATURAL
        },
        Msaa::Off,
        RenderLayers::layer(WORLD_RENDER_LAYER),
    ));

    commands.spawn((
        Name::new("OuterDisplayCamera"),
        OuterDisplayCamera,
        Camera2d,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
        IsDefaultUiCamera,
        Msaa::Off,
        RenderLayers::layer(CANVAS_RENDER_LAYER),
    ));

    commands.spawn((
        Name::new("PixelCanvasSprite"),
        PixelCanvasSprite,
        Sprite::from_image(canvas_handle),
        Transform::from_scale(Vec3::new(
            initial_viewport.scale as f32,
            initial_viewport.scale as f32,
            1.0,
        )),
        RenderLayers::layer(CANVAS_RENDER_LAYER),
    ));
}

fn sync_canvas_integer_scale(
    mut resize_events: MessageReader<WindowResized>,
    mut canvas_query: Query<&mut Transform, With<PixelCanvasSprite>>,
) {
    let Some(last_event) = resize_events.read().last() else {
        return;
    };

    let viewport = compute_integer_viewport(
        last_event.width,
        last_event.height,
        VIRTUAL_WIDTH,
        VIRTUAL_HEIGHT,
    );

    if let Ok(mut transform) = canvas_query.single_mut() {
        let scale = viewport.scale as f32;
        transform.scale = Vec3::new(scale, scale, 1.0);
    }
}

fn spawn_foundry_backdrop(mut commands: Commands) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    let grid_color = Color::srgb(0.06, 0.09, 0.14);

    for x in (-448..=1920).step_by(64) {
        commands.spawn((
            Sprite {
                color: grid_color,
                custom_size: Some(Vec2::new(1.0, 1080.0)),
                ..default()
            },
            Transform::from_xyz(x as f32, 160.0, -20.0),
            world_layer.clone(),
        ));
    }
    for y in (-256..=640).step_by(64) {
        commands.spawn((
            Sprite {
                color: grid_color,
                custom_size: Some(Vec2::new(2400.0, 1.0)),
                ..default()
            },
            Transform::from_xyz(720.0, y as f32, -20.0),
            world_layer.clone(),
        ));
    }

    commands.spawn((
        Name::new("HeroCompanionLight"),
        HeroCompanionLight {
            angle: 0.4,
            orbit_radius_x: 74.0,
            orbit_radius_y: 42.0,
        },
        PointLight2d {
            color: LinearRgba::new(1.0, 0.72, 0.30, 1.0),
            intensity: 2.5,
            radius: 220.0,
            height: 30.0,
            falloff: 1.4,
        },
        Sprite {
            color: Color::linear_rgb(4.2, 2.4, 0.45),
            custom_size: Some(Vec2::new(5.0, 5.0)),
            ..default()
        },
        Transform::from_xyz(-160.0, 40.0, 8.0),
        world_layer.clone(),
    ));

    commands.spawn((
        Name::new("ActionBurstLight"),
        ActionBurstLight { decay: 0.0 },
        PointLight2d {
            color: LinearRgba::new(0.2, 0.92, 1.0, 1.0),
            intensity: 0.0,
            radius: 190.0,
            height: 24.0,
            falloff: 1.4,
        },
        Transform::from_xyz(-200.0, 20.0, 6.0),
    ));

    // Overhead foundry zone lamps (Zone A ramp, Zone B shaft, Zone C Smash Arena)
    for (lx, ly, color, radius) in [
        (-160.0, 110.0, LinearRgba::new(0.22, 0.85, 1.0, 1.0), 270.0),
        (892.0, 150.0, LinearRgba::new(1.0, 0.56, 0.18, 1.0), 300.0),
        (1245.0, 390.0, LinearRgba::new(0.28, 0.88, 1.0, 1.0), 360.0),
        (1535.0, 390.0, LinearRgba::new(1.0, 0.62, 0.24, 1.0), 360.0),
    ] {
        commands.spawn((
            PointLight2d {
                color,
                intensity: 2.0,
                radius,
                height: 52.0,
                falloff: 1.35,
            },
            Sprite {
                color: Color::linear_rgb(color.red * 3.6, color.green * 3.6, color.blue * 3.6),
                custom_size: Some(Vec2::new(12.0, 4.0)),
                ..default()
            },
            Transform::from_xyz(lx, ly, -4.0),
            world_layer.clone(),
        ));
    }
}

struct BakedSpriteHandles {
    mesh: Handle<Mesh>,
    material: Handle<SpriteLitMaterial>,
    animator: SpriteSheetAnimator,
    diffuse: Handle<Image>,
    normal: Handle<Image>,
    emission: Handle<Image>,
}

fn load_baked_sprite_handles(
    asset_server: &AssetServer,
    meshes: &mut Assets<Mesh>,
    sprite_materials: &mut Assets<SpriteLitMaterial>,
    folder_and_name: &str,
    initial_anim: &str,
) -> Option<BakedSpriteHandles> {
    let ron_path = format!("assets/sprites/{}/{}.ron", folder_and_name, folder_and_name);
    let manifest = SpriteSheetManifest::load_from_file(Path::new(&ron_path)).ok()?;
    let (fw, fh) = manifest.frame_size;

    let diffuse: Handle<Image> = asset_server
        .load_builder()
        .with_settings(|s: &mut ImageLoaderSettings| {
            s.is_srgb = true;
            s.sampler = ImageSampler::nearest();
        })
        .load(format!(
            "sprites/{}/{}_diffuse.png",
            folder_and_name, folder_and_name
        ));
    let normal: Handle<Image> = asset_server
        .load_builder()
        .with_settings(|s: &mut ImageLoaderSettings| {
            s.is_srgb = false;
            s.sampler = ImageSampler::nearest();
        })
        .load(format!(
            "sprites/{}/{}_normal.png",
            folder_and_name, folder_and_name
        ));
    let emission: Handle<Image> = asset_server
        .load_builder()
        .with_settings(|s: &mut ImageLoaderSettings| {
            s.is_srgb = true;
            s.sampler = ImageSampler::nearest();
        })
        .load(format!(
            "sprites/{}/{}_emission.png",
            folder_and_name, folder_and_name
        ));

    let animator = SpriteSheetAnimator::new(manifest, initial_anim);
    let uniform = SpriteLitUniform {
        atlas_rect: animator.current_uv_rect(),
        ..default()
    };
    let material = sprite_materials.add(SpriteLitMaterial {
        data: uniform,
        diffuse: diffuse.clone(),
        normal: normal.clone(),
        emission: emission.clone(),
    });
    let mesh = meshes.add(Rectangle::new(fw as f32, fh as f32));

    Some(BakedSpriteHandles {
        mesh,
        material,
        animator,
        diffuse,
        normal,
        emission,
    })
}

fn decorate_added_player(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut sprite_materials: ResMut<Assets<SpriteLitMaterial>>,
    added_players: Query<(Entity, &KinematicBody), Added<Player>>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);

    for (player_entity, body) in &added_players {
        let foot_pivot_y = -body.half_extents.y;

        commands.entity(player_entity).with_children(|parent| {
            parent.spawn((
                Name::new("BusterChargeAura"),
                BusterChargeAura { pulse: 0.0 },
                PointLight2d {
                    color: LinearRgba::new(0.15, 0.92, 1.0, 1.0),
                    intensity: 0.0,
                    radius: 150.0,
                    height: 22.0,
                    falloff: 1.4,
                },
                Sprite {
                    color: Color::linear_rgba(0.2, 2.8, 4.2, 0.0),
                    custom_size: Some(Vec2::new(34.0, 44.0)),
                    ..default()
                },
                Transform::from_xyz(0.0, 2.0, -1.0),
                world_layer.clone(),
            ));
        });

        if let Some(baked) = load_baked_sprite_handles(
            &asset_server,
            &mut meshes,
            &mut sprite_materials,
            "hero",
            "idle",
        ) {
            commands.insert_resource(HeroSpriteCache {
                mesh: baked.mesh.clone(),
                diffuse: baked.diffuse,
                normal: baked.normal,
                emission: baked.emission,
            });

            commands.entity(player_entity).with_children(|parent| {
                parent
                    .spawn((
                        Name::new("PlayerFeetPivot"),
                        PlayerFeetPivot,
                        Transform::from_xyz(0.0, foot_pivot_y, 0.0),
                        Visibility::default(),
                    ))
                    .with_children(|pivot| {
                        pivot.spawn((
                            Name::new("PlayerSpriteVisual"),
                            PlayerSpriteVisual,
                            baked.animator,
                            Mesh2d(baked.mesh),
                            MeshMaterial2d(baked.material),
                            Transform::from_xyz(0.0, 36.0, 2.0),
                            world_layer.clone(),
                        ));
                    });
            });
        } else {
            warn!("Baked hero sprite missing; attaching fallback player silhouette.");
            commands.entity(player_entity).with_children(|parent| {
                parent.spawn((
                    Name::new("PlayerFallbackBox"),
                    PlayerFeetPivot,
                    Sprite {
                        color: Color::linear_rgb(0.35, 3.0, 4.4),
                        custom_size: Some(body.half_extents * 2.0),
                        ..default()
                    },
                    Transform::from_xyz(0.0, 0.0, 2.0),
                    world_layer.clone(),
                ));
            });
        }
    }
}

fn decorate_added_solids(mut commands: Commands, added: Query<(Entity, &SolidRect), Added<SolidRect>>) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    for (entity, rect) in &added {
        let size = rect.half_extents * 2.0;
        commands.entity(entity).insert((
            Sprite {
                color: Color::srgb(0.14, 0.18, 0.26),
                custom_size: Some(size),
                ..default()
            },
            Transform::from_xyz(rect.center.x, rect.center.y, -6.0),
            world_layer.clone(),
        ));
        let is_wall = size.y > size.x * 1.4;
        if is_wall {
            for side in [-1.0_f32, 1.0_f32] {
                commands.spawn((
                    Sprite {
                        color: Color::linear_rgb(2.6, 1.1, 0.2),
                        custom_size: Some(Vec2::new(2.0, size.y)),
                        ..default()
                    },
                    Transform::from_xyz(
                        rect.center.x + side * (rect.half_extents.x - 1.0),
                        rect.center.y,
                        -5.0,
                    ),
                    world_layer.clone(),
                ));
            }
        } else {
            commands.spawn((
                Sprite {
                    color: Color::linear_rgb(0.22, 1.85, 2.8),
                    custom_size: Some(Vec2::new(size.x, 2.0)),
                    ..default()
                },
                Transform::from_xyz(rect.center.x, rect.center.y + rect.half_extents.y - 1.0, -5.0),
                world_layer.clone(),
            ));
        }
    }
}

fn decorate_added_slopes(
    mut commands: Commands,
    added: Query<(Entity, &SlopeSegment), Added<SlopeSegment>>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    for (entity, slope) in &added {
        let delta = slope.end - slope.start;
        let len = delta.length().max(1.0);
        let mid = (slope.start + slope.end) * 0.5;
        let angle = slope.angle_rad();
        let normal = slope.normal();

        commands.entity(entity).insert((
            Sprite {
                color: Color::srgb(0.15, 0.20, 0.29),
                custom_size: Some(Vec2::new(len + 4.0, 18.0)),
                ..default()
            },
            Transform::from_xyz(mid.x - normal.x * 9.0, mid.y - normal.y * 9.0, -6.0)
                .with_rotation(Quat::from_rotation_z(angle)),
            world_layer.clone(),
        ));
        commands.spawn((
            Sprite {
                color: Color::linear_rgb(0.25, 2.4, 3.6),
                custom_size: Some(Vec2::new(len + 2.0, 2.5)),
                ..default()
            },
            Transform::from_xyz(mid.x - normal.x * 1.0, mid.y - normal.y * 1.0, -5.0)
                .with_rotation(Quat::from_rotation_z(angle)),
            world_layer.clone(),
        ));
    }
}

fn decorate_added_one_ways(
    mut commands: Commands,
    added: Query<(Entity, &OneWayPlatform, Option<&MovingPlatform>), Added<OneWayPlatform>>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    for (entity, plat, maybe_moving) in &added {
        let width = plat.half_width * 2.0;
        let is_moving = maybe_moving.is_some();
        let body_color = if is_moving {
            Color::srgb(0.16, 0.28, 0.34)
        } else {
            Color::srgb(0.20, 0.24, 0.34)
        };
        let top_glow = if is_moving {
            Color::linear_rgb(0.25, 3.2, 3.6)
        } else {
            Color::linear_rgb(3.4, 1.45, 0.20)
        };

        commands
            .entity(entity)
            .insert((
                Sprite {
                    color: body_color,
                    custom_size: Some(Vec2::new(width, plat.thickness)),
                    ..default()
                },
                Transform::from_xyz(plat.center.x, plat.center.y, -5.0),
                world_layer.clone(),
            ))
            .with_children(|parent| {
                parent.spawn((
                    Sprite {
                        color: top_glow,
                        custom_size: Some(Vec2::new(width, 2.0)),
                        ..default()
                    },
                    Transform::from_xyz(0.0, plat.thickness * 0.5 - 0.5, 0.5),
                    world_layer.clone(),
                ));
            });
    }
}

fn decorate_added_boost_pads(
    mut commands: Commands,
    added: Query<(Entity, &BoostPad), Added<BoostPad>>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    for (entity, pad) in &added {
        let size = pad.half_extents * 2.0;
        commands.entity(entity).insert((
            Sprite {
                color: Color::linear_rgb(0.25, 3.4, 4.6),
                custom_size: Some(Vec2::new(size.x, 4.0)),
                ..default()
            },
            PointLight2d {
                color: LinearRgba::new(0.18, 0.95, 1.0, 1.0),
                intensity: 2.2,
                radius: 170.0,
                height: 24.0,
                falloff: 1.5,
            },
            Transform::from_xyz(pad.center.x, pad.center.y - pad.half_extents.y + 2.0, -3.0),
            world_layer.clone(),
        ));
    }
}

fn decorate_added_loop_splines(
    mut commands: Commands,
    added: Query<&LoopSpline, Added<LoopSpline>>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    for spline in &added {
        let pts = spline.polyline_points(48);
        for window in pts.windows(2) {
            let a = window[0];
            let b = window[1];
            let delta = b - a;
            let seg_len = delta.length().max(1.0);
            let mid = (a + b) * 0.5;
            let angle = delta.y.atan2(delta.x);
            let tangent = delta / seg_len;
            let inward_n = Vec2::new(-tangent.y, tangent.x);

            commands.spawn((
                Sprite {
                    color: Color::srgb(0.16, 0.21, 0.32),
                    custom_size: Some(Vec2::new(seg_len + 2.5, 10.0)),
                    ..default()
                },
                Transform::from_xyz(mid.x - inward_n.x * 5.0, mid.y - inward_n.y * 5.0, -8.0)
                    .with_rotation(Quat::from_rotation_z(angle)),
                world_layer.clone(),
            ));
            commands.spawn((
                Sprite {
                    color: Color::linear_rgb(0.22, 2.35, 3.8),
                    custom_size: Some(Vec2::new(seg_len + 1.5, 2.0)),
                    ..default()
                },
                Transform::from_xyz(mid.x - inward_n.x * 1.0, mid.y - inward_n.y * 1.0, -7.0)
                    .with_rotation(Quat::from_rotation_z(angle)),
                world_layer.clone(),
            ));
        }

        let apex = spline.sample(0.5);
        commands.spawn((
            PointLight2d {
                color: LinearRgba::new(0.25, 0.88, 1.0, 1.0),
                intensity: 2.6,
                radius: 230.0,
                height: 36.0,
                falloff: 1.35,
            },
            Sprite {
                color: Color::linear_rgb(0.35, 3.2, 4.5),
                custom_size: Some(Vec2::new(10.0, 10.0)),
                ..default()
            },
            Transform::from_xyz(apex.position.x, apex.position.y - 26.0, -5.0),
            world_layer.clone(),
        ));
    }
}

fn decorate_added_spike_hazards(
    mut commands: Commands,
    added: Query<(Entity, &SpikeHazard), Added<SpikeHazard>>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    for (entity, spike) in &added {
        let size = spike.half_extents * 2.0;
        commands
            .entity(entity)
            .insert((
                Sprite {
                    color: Color::srgb(0.30, 0.10, 0.14),
                    custom_size: Some(Vec2::new(size.x, 4.0)),
                    ..default()
                },
                Transform::from_xyz(spike.center.x, spike.center.y - spike.half_extents.y + 2.0, -4.0),
                world_layer.clone(),
            ))
            .with_children(|parent| {
                let teeth = ((size.x / 8.0).floor() as i32).max(2);
                let step = size.x / teeth as f32;
                for i in 0..teeth {
                    let tx = -spike.half_extents.x + step * (i as f32 + 0.5);
                    parent.spawn((
                        Sprite {
                            color: Color::linear_rgb(4.0, 0.35, 0.45),
                            custom_size: Some(Vec2::new(4.0, size.y - 2.0)),
                            ..default()
                        },
                        Transform::from_xyz(tx, size.y * 0.45, 0.2),
                        world_layer.clone(),
                    ));
                }
            });
    }
}

fn decorate_added_wall_turrets(
    mut commands: Commands,
    added: Query<(Entity, &WallTurret), Added<WallTurret>>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    for (entity, turret) in &added {
        commands.entity(entity).with_children(|parent| {
            // Armored turret housing
            parent.spawn((
                Sprite {
                    color: Color::srgb(0.24, 0.27, 0.36),
                    custom_size: Some(Vec2::new(18.0, 18.0)),
                    ..default()
                },
                Transform::from_xyz(0.0, 0.0, 1.0),
                world_layer.clone(),
            ));
            // Cannon barrel pointing into the shaft
            parent.spawn((
                Sprite {
                    color: Color::srgb(0.38, 0.42, 0.52),
                    custom_size: Some(Vec2::new(12.0, 6.0)),
                    ..default()
                },
                Transform::from_xyz(turret.facing * 9.0, 0.0, 1.2),
                world_layer.clone(),
            ));
            // Glowing telegraph core LED
            parent.spawn((
                TurretTelegraphLed { owner: entity },
                Sprite {
                    color: Color::linear_rgb(3.6, 0.35, 0.45),
                    custom_size: Some(Vec2::new(6.0, 6.0)),
                    ..default()
                },
                Transform::from_xyz(turret.facing * 4.0, 2.0, 1.4),
                world_layer.clone(),
            ));
        });
    }
}

fn decorate_added_enemy_fighters(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut sprite_materials: ResMut<Assets<SpriteLitMaterial>>,
    added: Query<(Entity, &EnemyFighter, &KinematicBody), Added<EnemyFighter>>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);

    for (entity, fighter, body) in &added {
        let pack = match fighter.archetype {
            EnemyArchetype::LightStriker => "rogue",
            EnemyArchetype::HeavyBrawler => "barbarian",
        };

        if let Some(baked) = load_baked_sprite_handles(
            &asset_server,
            &mut meshes,
            &mut sprite_materials,
            pack,
            "idle",
        ) {
            let foot_offset_y = 36.0 - body.half_extents.y;
            commands.entity(entity).with_children(|parent| {
                parent.spawn((
                    Name::new(format!("EnemyVisual_{}", pack)),
                    EnemySpriteVisual {
                        owner: entity,
                        jitter_phase: 0.0,
                    },
                    baked.animator,
                    Mesh2d(baked.mesh),
                    MeshMaterial2d(baked.material),
                    Transform::from_xyz(0.0, foot_offset_y, 1.5),
                    world_layer.clone(),
                ));
            });
        } else {
            let fallback_color = if fighter.archetype == EnemyArchetype::HeavyBrawler {
                Color::srgb(0.65, 0.22, 0.25)
            } else {
                Color::srgb(0.22, 0.55, 0.36)
            };
            commands.entity(entity).with_children(|parent| {
                parent.spawn((
                    Sprite {
                        color: fallback_color,
                        custom_size: Some(body.half_extents * 2.0),
                        ..default()
                    },
                    Transform::from_xyz(0.0, 0.0, 1.5),
                    world_layer.clone(),
                ));
            });
        }
    }
}

fn decorate_added_buster_projectiles(
    mut commands: Commands,
    added: Query<(Entity, &BusterProjectile), Added<BusterProjectile>>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    for (entity, proj) in &added {
        if proj.charged {
            commands.entity(entity).insert((
                Sprite {
                    color: Color::linear_rgb(0.35, 3.6, 5.2),
                    custom_size: Some(Vec2::new(26.0, 15.0)),
                    ..default()
                },
                PointLight2d {
                    color: LinearRgba::new(0.22, 0.95, 1.0, 1.0),
                    intensity: 3.4,
                    radius: 195.0,
                    height: 22.0,
                    falloff: 1.35,
                },
                world_layer.clone(),
            ));
            commands.entity(entity).with_children(|parent| {
                parent.spawn((
                    Sprite {
                        color: Color::linear_rgb(4.6, 4.2, 2.2),
                        custom_size: Some(Vec2::new(14.0, 7.0)),
                        ..default()
                    },
                    Transform::from_xyz(3.0 * proj.velocity.x.signum(), 0.0, 0.2),
                    world_layer.clone(),
                ));
            });
        } else {
            commands.entity(entity).insert((
                Sprite {
                    color: Color::linear_rgb(0.6, 3.8, 4.8),
                    custom_size: Some(Vec2::new(10.0, 5.0)),
                    ..default()
                },
                PointLight2d {
                    color: LinearRgba::new(0.25, 0.9, 1.0, 1.0),
                    intensity: 1.6,
                    radius: 110.0,
                    height: 18.0,
                    falloff: 1.6,
                },
                world_layer.clone(),
            ));
        }
    }
}

fn decorate_added_turret_projectiles(
    mut commands: Commands,
    added: Query<Entity, Added<TurretProjectile>>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    for entity in &added {
        commands.entity(entity).insert((
            Sprite {
                color: Color::linear_rgb(4.6, 0.45, 0.65),
                custom_size: Some(Vec2::new(9.0, 9.0)),
                ..default()
            },
            PointLight2d {
                color: LinearRgba::new(1.0, 0.2, 0.35, 1.0),
                intensity: 1.8,
                radius: 105.0,
                height: 18.0,
                falloff: 1.6,
            },
            world_layer.clone(),
        ));
    }
}

/// Renders subtle neon danger border rails outlining the Zone C Smash Blast Zone.
fn decorate_added_blast_zones(mut commands: Commands, added: Query<&BlastZone, Added<BlastZone>>) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    let border_color = Color::srgba(0.65, 0.20, 0.90, 0.35);

    for zone in &added {
        let w = zone.max.x - zone.min.x;
        let h = zone.max.y - zone.min.y;
        let cx = (zone.min.x + zone.max.x) * 0.5;
        let cy = (zone.min.y + zone.max.y) * 0.5;

        for (pos, size) in [
            (Vec2::new(zone.max.x, cy), Vec2::new(2.0, h)),
            (Vec2::new(cx, zone.max.y), Vec2::new(w, 2.0)),
            (Vec2::new(cx, zone.min.y), Vec2::new(w, 2.0)),
        ] {
            commands.spawn((
                Sprite {
                    color: border_color,
                    custom_size: Some(size),
                    ..default()
                },
                Transform::from_xyz(pos.x, pos.y, -12.0),
                world_layer.clone(),
            ));
        }
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn drive_player_visuals_and_animations(
    time: Res<Time>,
    state: Res<State<AppState>>,
    config: Res<PhysicsConfig>,
    mut last_attack: ResMut<LastVisualAttack>,
    players: Query<
        (
            &KinematicBody,
            &AttackState,
            Option<&Hitstop>,
            Option<&Hitstun>,
            Option<&Invulnerable>,
        ),
        With<Player>,
    >,
    mut pivots: Query<&mut Transform, With<PlayerFeetPivot>>,
    mut visuals: Query<
        (&mut SpriteSheetAnimator, &MeshMaterial2d<SpriteLitMaterial>),
        With<PlayerSpriteVisual>,
    >,
    mut materials: ResMut<Assets<SpriteLitMaterial>>,
) {
    if *state.get() == AppState::Paused {
        return;
    }
    let Ok((body, attack, hitstop, hitstun, invuln)) = players.single() else {
        return;
    };

    let dt = time.delta_secs();
    let in_hitstop = hitstop.is_some_and(Hitstop::is_active);
    let blink_alpha = if invuln.is_some_and(Invulnerable::is_active) {
        if ((time.elapsed_secs() * 24.0) as u32).is_multiple_of(2) {
            0.35
        } else {
            1.0
        }
    } else {
        1.0
    };

    for mut pivot_tf in &mut pivots {
        pivot_tf.rotation = Quat::from_rotation_z(body.visual_angle);
    }

    for (mut animator, mat_handle) in &mut visuals {
        if let Some(mut mat) = materials.get_mut(&mat_handle.0) {
            mat.data.tint.w = blink_alpha;
            mat.data.fx_params.y = if in_hitstop && hitstun.is_some_and(Hitstun::is_active) {
                0.85
            } else {
                0.0
            };
        }

        if *state.get() == AppState::Title {
            animator.flip_x = false;
            animator.play("idle");
            animator.tick(dt);
            continue;
        }

        if in_hitstop {
            continue;
        }

        if matches!(body.state, MovementState::LoopLocked { .. }) {
            animator.flip_x = false;
            animator.play("sprint");
            animator.tick(dt * 1.25);
            continue;
        }

        animator.flip_x = body.facing < 0.0;

        if hitstun.is_some_and(Hitstun::is_active) {
            animator.play("hitstun");
            animator.tick(dt);
            continue;
        }

        if let Some(move_kind) = attack.active_move {
            let restarted = last_attack.last_move != Some(move_kind)
                || attack.active_timer > last_attack.last_active_timer + 0.005;
            if restarted {
                animator.play_restart(move_kind.animation_name());
            } else {
                animator.play(move_kind.animation_name());
            }
            last_attack.last_move = Some(move_kind);
            last_attack.last_active_timer = attack.active_timer;
        } else {
            last_attack.last_move = None;
            last_attack.last_active_timer = 0.0;

            if attack.buster_anim_timer > 0.0 {
                animator.play("buster");
            } else {
                match body.state {
                    MovementState::Grounded => {
                        let spd = body.speed();
                        if spd >= config.run_max_speed * 1.15 {
                            animator.play("sprint");
                        } else if spd >= 18.0 {
                            animator.play("run");
                        } else {
                            animator.play("idle");
                        }
                    }
                    MovementState::Airborne | MovementState::WallSliding { .. } => {
                        if body.velocity.y > 20.0 {
                            animator.play("jump");
                        } else {
                            animator.play("fall");
                        }
                    }
                    MovementState::LoopLocked { .. } => {}
                }
            }
        }

        animator.tick(dt);
    }
}

/// Spawns translucent cyan/amber Sonic afterimage ghost trails every `0.04s` whenever
/// player speed exceeds `80%` of `sprint_max_speed` (or while traversing a 360° loop).
#[allow(clippy::too_many_arguments)]
fn spawn_and_fade_speed_afterimages(
    mut commands: Commands,
    time: Res<Time>,
    state: Res<State<AppState>>,
    config: Res<PhysicsConfig>,
    cache: Option<Res<HeroSpriteCache>>,
    mut spawner: ResMut<AfterimageSpawner>,
    players: Query<&KinematicBody, With<Player>>,
    hero_visuals: Query<(&GlobalTransform, &SpriteSheetAnimator), With<PlayerSpriteVisual>>,
    mut sprite_materials: ResMut<Assets<SpriteLitMaterial>>,
    mut ghosts: Query<(Entity, &mut AfterimageGhost, &MeshMaterial2d<SpriteLitMaterial>)>,
) {
    if *state.get() == AppState::Paused {
        return;
    }
    let dt = time.delta_secs();

    // Fade existing afterimages smoothly
    for (entity, mut ghost, mat_handle) in &mut ghosts {
        ghost.remaining -= dt;
        if ghost.remaining <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        if let Some(mut mat) = sprite_materials.get_mut(&mat_handle.0) {
            let frac = (ghost.remaining / ghost.max_lifetime).clamp(0.0, 1.0);
            mat.data.tint.w = ghost.base_alpha * frac;
        }
    }

    if *state.get() != AppState::Playing {
        return;
    }

    let Some(cache) = cache else {
        return;
    };
    let Ok(body) = players.single() else {
        return;
    };
    let Ok((global_tf, animator)) = hero_visuals.single() else {
        return;
    };

    let speed_threshold = config.sprint_max_speed * 0.80;
    let fast_enough =
        body.speed() >= speed_threshold || matches!(body.state, MovementState::LoopLocked { .. });

    if !fast_enough {
        spawner.timer = 0.0;
        return;
    }

    spawner.timer -= dt;
    if spawner.timer > 0.0 {
        return;
    }
    spawner.timer = AFTERIMAGE_INTERVAL_SECS;
    spawner.alternate = !spawner.alternate;

    let (_, rot, trans) = global_tf.to_scale_rotation_translation();
    let (_, _, rot_z) = rot.to_euler(EulerRot::XYZ);
    let (sin_r, cos_r) = rot_z.sin_cos();

    let tint = if spawner.alternate {
        Vec4::new(0.25, 2.2, 3.6, 0.56) // electric cyan ghost
    } else {
        Vec4::new(3.4, 1.5, 0.25, 0.50) // molten amber ghost
    };

    let uniform = SpriteLitUniform {
        atlas_rect: animator.current_uv_rect(),
        ambient: Vec4::new(1.0, 1.0, 1.0, 1.0),
        transform_params: Vec4::new(
            if animator.flip_x { -1.0 } else { 1.0 },
            cos_r,
            sin_r,
            0.0,
        ),
        fx_params: Vec4::new(2.2, 0.0, 0.0, 0.0),
        tint,
        ..default()
    };

    let mat_handle = sprite_materials.add(SpriteLitMaterial {
        data: uniform,
        diffuse: cache.diffuse.clone(),
        normal: cache.normal.clone(),
        emission: cache.emission.clone(),
    });

    commands.spawn((
        Name::new("SonicAfterimage"),
        AfterimageGhost {
            remaining: 0.22,
            max_lifetime: 0.22,
            base_alpha: tint.w,
        },
        Mesh2d(cache.mesh.clone()),
        MeshMaterial2d(mat_handle),
        Transform::from_xyz(trans.x.round(), trans.y.round(), 1.2)
            .with_rotation(Quat::from_rotation_z(rot_z)),
        RenderLayers::layer(WORLD_RENDER_LAYER),
    ));
}

fn update_buster_charge_aura(
    time: Res<Time>,
    state: Res<State<AppState>>,
    players: Query<&BusterState, With<Player>>,
    mut auras: Query<(&mut BusterChargeAura, &mut Sprite, &mut PointLight2d)>,
) {
    if *state.get() == AppState::Paused {
        return;
    }
    let Ok(buster) = players.single() else {
        return;
    };

    for (mut aura, mut sprite, mut light) in &mut auras {
        aura.pulse += time.delta_secs() * 18.0;
        let ratio = buster.charge_ratio();
        if buster.charge_secs < 0.18 {
            sprite.color = Color::linear_rgba(0.0, 0.0, 0.0, 0.0);
            light.intensity = 0.0;
        } else if buster.is_fully_charged() {
            let flick = 0.65 + 0.35 * aura.pulse.sin();
            sprite.color = Color::linear_rgba(0.6 + 2.4 * flick, 3.4, 4.8, 0.42);
            sprite.custom_size = Some(Vec2::new(38.0 + 4.0 * flick, 46.0 + 4.0 * flick));
            light.color = LinearRgba::new(0.25 + 0.55 * flick, 0.92, 1.0, 1.0);
            light.intensity = 2.6 + 0.9 * flick;
        } else {
            let flick = 0.5 + 0.5 * aura.pulse.sin();
            sprite.color = Color::linear_rgba(0.15, 2.2 * ratio, 3.6 * ratio, 0.25 * ratio);
            sprite.custom_size = Some(Vec2::new(26.0 + 10.0 * ratio, 34.0 + 10.0 * ratio));
            light.color = LinearRgba::new(0.18, 0.85, 1.0, 1.0);
            light.intensity = 1.4 * ratio * (0.7 + 0.3 * flick);
        }
    }
}

#[allow(clippy::type_complexity)]
fn drive_enemy_fighter_visuals(
    time: Res<Time>,
    state: Res<State<AppState>>,
    enemies: Query<(
        &EnemyFighter,
        &KinematicBody,
        Option<&Hitstop>,
        Option<&Hitstun>,
        Option<&Invulnerable>,
    )>,
    mut visuals: Query<(
        &mut EnemySpriteVisual,
        &mut SpriteSheetAnimator,
        &mut Transform,
        &MeshMaterial2d<SpriteLitMaterial>,
    )>,
    mut materials: ResMut<Assets<SpriteLitMaterial>>,
) {
    if *state.get() == AppState::Paused {
        return;
    }
    let dt = time.delta_secs();

    for (mut vis, mut animator, mut tf, mat_handle) in &mut visuals {
        let Ok((fighter, body, hitstop, hitstun, invuln)) = enemies.get(vis.owner) else {
            continue;
        };

        let in_hitstop = hitstop.is_some_and(Hitstop::is_active);
        let in_hitstun = hitstun.is_some_and(Hitstun::is_active);
        let telegraph = fighter.is_telegraphing();
        let base_y = 36.0 - body.half_extents.y;

        if in_hitstop {
            vis.jitter_phase += dt * 115.0;
            tf.translation.x = (vis.jitter_phase.sin() * 3.0).round();
            tf.translation.y = base_y + ((vis.jitter_phase * 1.6).cos() * 2.0).round();
        } else {
            tf.translation.x = 0.0;
            tf.translation.y = base_y;
        }

        if let Some(mut mat) = materials.get_mut(&mat_handle.0) {
            mat.data.fx_params.y = if in_hitstop { 1.0 } else { 0.0 };
            mat.data.tint = if telegraph {
                // Bright pulsing amber-red attack telegraph warning!
                let pulse = 0.6 + 0.4 * (time.elapsed_secs() * 28.0).sin();
                Vec4::new(1.8 + pulse, 0.65, 0.35, 1.0)
            } else if invuln.is_some_and(Invulnerable::is_active) {
                let blink = if ((time.elapsed_secs() * 22.0) as u32).is_multiple_of(2) {
                    0.45
                } else {
                    1.0
                };
                Vec4::new(1.0, 1.0, 1.0, blink)
            } else {
                Vec4::ONE
            };
        }

        if in_hitstop {
            continue;
        }

        animator.flip_x = body.facing < 0.0;
        if in_hitstun {
            animator.play("hitstun");
        } else {
            match fighter.state {
                EnemyAiState::Attacking { .. } => animator.play("jab"),
                EnemyAiState::Telegraph { .. } => animator.play("idle"),
                EnemyAiState::Chase if body.speed() > 18.0 => animator.play("run"),
                _ => animator.play("idle"),
            }
        }
        animator.tick(dt);
    }
}

fn update_wall_turret_visuals(
    time: Res<Time>,
    turrets: Query<&WallTurret>,
    mut leds: Query<(&TurretTelegraphLed, &mut Sprite)>,
) {
    let pulse = 0.5 + 0.5 * (time.elapsed_secs() * 26.0).sin();
    for (led, mut sprite) in &mut leds {
        let Ok(turret) = turrets.get(led.owner) else {
            continue;
        };
        if !turret.is_active() {
            sprite.color = Color::srgb(0.12, 0.14, 0.18);
        } else if turret.is_telegraphing() {
            sprite.color = Color::linear_rgb(5.0, 1.8 * pulse, 0.4);
        } else {
            sprite.color = Color::linear_rgb(2.6, 0.28, 0.40);
        }
    }
}

fn handle_combat_hit_and_shot_fx(
    mut commands: Commands,
    time: Res<Time>,
    mut hit_events: MessageReader<CombatHitEvent>,
    mut shot_events: MessageReader<BusterShotEvent>,
    mut shake: ResMut<CameraShake>,
    mut burst_lights: Query<(&mut ActionBurstLight, &mut PointLight2d, &mut Transform)>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    let dt = time.delta_secs();
    let mut latest_flash: Option<(Vec2, LinearRgba, f32)> = None;

    for ev in hit_events.read() {
        let heavy = ev.is_charged || ev.knockback_mag >= 300.0;
        let trauma = ((ev.knockback_mag - 80.0) / 520.0).clamp(0.18, 0.95);
        shake.add_impulse(trauma, ev.knockback_vec);

        let spark_count = if heavy { 14 } else { 8 };
        let base_angle = ev.knockback_vec.y.atan2(ev.knockback_vec.x);
        for i in 0..spark_count {
            let spread = ((i as f32 / spark_count as f32) - 0.5) * 2.4;
            let angle = base_angle + spread;
            let speed = if i % 2 == 0 { 240.0 } else { 150.0 } + ev.knockback_mag * 0.25;
            let vel = Vec2::new(angle.cos(), angle.sin()) * speed;
            let color = if ev.is_charged || i % 3 == 0 {
                Color::linear_rgb(0.4, 3.8, 5.2)
            } else {
                Color::linear_rgb(4.8, 2.5, 0.45)
            };
            commands.spawn((
                HitSpark {
                    velocity: vel,
                    lifetime: 0.22,
                    max_lifetime: 0.22,
                },
                Sprite {
                    color,
                    custom_size: Some(if heavy {
                        Vec2::new(5.0, 3.0)
                    } else {
                        Vec2::new(3.0, 3.0)
                    }),
                    ..default()
                },
                Transform::from_xyz(ev.hit_pos.x, ev.hit_pos.y, 7.0)
                    .with_rotation(Quat::from_rotation_z(angle)),
                world_layer.clone(),
            ));
        }

        let flash_color = if ev.is_charged {
            LinearRgba::new(0.2, 0.95, 1.0, 1.0)
        } else {
            LinearRgba::new(1.0, 0.55, 0.18, 1.0)
        };
        latest_flash = Some((ev.hit_pos, flash_color, if heavy { 4.4 } else { 3.0 }));
    }

    for shot in shot_events.read() {
        if shot.charged {
            shake.add_impulse(0.45, Vec2::new(-shot.direction, 0.15));
            latest_flash = Some((shot.origin, LinearRgba::new(0.2, 0.95, 1.0, 1.0), 4.0));
            for i in 0..6 {
                let spread = ((i as f32 / 5.0) - 0.5) * 1.1;
                let base = if shot.direction < 0.0 {
                    std::f32::consts::PI
                } else {
                    0.0
                };
                let angle = base + spread;
                commands.spawn((
                    HitSpark {
                        velocity: Vec2::new(angle.cos(), angle.sin()) * 210.0,
                        lifetime: 0.16,
                        max_lifetime: 0.16,
                    },
                    Sprite {
                        color: Color::linear_rgb(0.35, 3.8, 5.2),
                        custom_size: Some(Vec2::new(4.0, 2.0)),
                        ..default()
                    },
                    Transform::from_xyz(shot.origin.x, shot.origin.y, 7.0)
                        .with_rotation(Quat::from_rotation_z(angle)),
                    world_layer.clone(),
                ));
            }
        } else {
            latest_flash = Some((shot.origin, LinearRgba::new(0.22, 0.92, 1.0, 1.0), 2.2));
        }
    }

    for (mut burst, mut light, mut tf) in &mut burst_lights {
        if let Some((pos, color, intensity)) = latest_flash {
            tf.translation.x = pos.x.round();
            tf.translation.y = pos.y.round();
            light.color = color;
            burst.decay = 1.0;
            light.intensity = intensity;
        } else {
            burst.decay = (burst.decay - dt * 6.0).max(0.0);
            light.intensity *= (1.0 - dt * 8.0).max(0.0);
            if burst.decay <= 0.0 {
                light.intensity = 0.0;
            }
        }
    }
}

/// Spawns a dramatic Smash-style HDR KO explosion beam + heavy camera trauma on blast-zone KO!
fn handle_blast_zone_ko_flash(
    mut commands: Commands,
    mut ko_events: MessageReader<BlastZoneKoEvent>,
    mut shake: ResMut<CameraShake>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);

    for ko in ko_events.read() {
        let inward = match ko.edge {
            BlastEdge::Right => Vec2::new(-1.0, 0.0),
            BlastEdge::Left => Vec2::new(1.0, 0.0),
            BlastEdge::Top => Vec2::new(0.0, -1.0),
            BlastEdge::Bottom => Vec2::new(0.0, 1.0),
        };
        let angle = inward.y.atan2(inward.x);
        shake.add_impulse(1.0, inward);

        // Primary blazing Smash KO beam bursting inward from the blast boundary
        let beam_origin = ko.ko_pos + inward * 95.0;
        commands.spawn((
            SmashKoFlashBeam {
                lifetime: 0.45,
                max_lifetime: 0.45,
            },
            Sprite {
                color: Color::linear_rgb(0.65, 4.0, 5.2),
                custom_size: Some(Vec2::new(260.0, 72.0)),
                ..default()
            },
            Transform::from_xyz(beam_origin.x, beam_origin.y, 9.0)
                .with_rotation(Quat::from_rotation_z(angle)),
            world_layer.clone(),
        ));
        commands.spawn((
            SmashKoFlashBeam {
                lifetime: 0.34,
                max_lifetime: 0.34,
            },
            Sprite {
                color: Color::linear_rgb(5.2, 4.4, 2.2),
                custom_size: Some(Vec2::new(220.0, 26.0)),
                ..default()
            },
            Transform::from_xyz(beam_origin.x, beam_origin.y, 9.2)
                .with_rotation(Quat::from_rotation_z(angle)),
            world_layer.clone(),
        ));

        // Burst of 18 KO sparks spraying inward
        for i in 0..18 {
            let spread = ((i as f32 / 17.0) - 0.5) * 1.4;
            let a = angle + spread;
            let spd = 220.0 + (i % 4) as f32 * 75.0;
            commands.spawn((
                HitSpark {
                    velocity: Vec2::new(a.cos(), a.sin()) * spd,
                    lifetime: 0.42,
                    max_lifetime: 0.42,
                },
                Sprite {
                    color: if i % 2 == 0 {
                        Color::linear_rgb(0.4, 4.2, 5.4)
                    } else {
                        Color::linear_rgb(5.0, 2.8, 0.5)
                    },
                    custom_size: Some(Vec2::new(8.0, 4.0)),
                    ..default()
                },
                Transform::from_xyz(ko.ko_pos.x, ko.ko_pos.y, 9.4)
                    .with_rotation(Quat::from_rotation_z(a)),
                world_layer.clone(),
            ));
        }
    }
}

fn update_hit_sparks_and_ko_beams(
    mut commands: Commands,
    time: Res<Time>,
    state: Res<State<AppState>>,
    mut sparks: Query<(Entity, &mut HitSpark, &mut Transform, &mut Sprite), Without<SmashKoFlashBeam>>,
    mut beams: Query<(Entity, &mut SmashKoFlashBeam, &mut Transform, &mut Sprite)>,
) {
    if *state.get() == AppState::Paused {
        return;
    }
    let dt = time.delta_secs();

    for (entity, mut spark, mut tf, mut sprite) in &mut sparks {
        spark.lifetime -= dt;
        if spark.lifetime <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        tf.translation.x += spark.velocity.x * dt;
        tf.translation.y += spark.velocity.y * dt;
        spark.velocity *= (1.0 - dt * 5.0).max(0.0);
        let alpha = (spark.lifetime / spark.max_lifetime).clamp(0.0, 1.0);
        sprite.color.set_alpha(alpha);
    }

    for (entity, mut beam, mut tf, mut sprite) in &mut beams {
        beam.lifetime -= dt;
        if beam.lifetime <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        let frac = (beam.lifetime / beam.max_lifetime).clamp(0.0, 1.0);
        tf.scale.y = frac;
        sprite.color.set_alpha(frac);
    }
}

/// Smooth look-ahead camera that leads high-speed Sonic runs in Zones A/B and frames
/// the Smash Battlefield arena cleanly in Zone C, strictly integer-snapped to virtual pixels.
#[allow(clippy::too_many_arguments)]
fn update_follow_camera(
    time: Res<Time>,
    state: Res<State<AppState>>,
    spawn_point: Res<PlayerSpawnPoint>,
    mut restarts: MessageReader<StageRestartEvent>,
    mut shake: ResMut<CameraShake>,
    players: Query<&KinematicBody, With<Player>>,
    blast_zones: Query<&BlastZone>,
    mut cameras: Query<(&mut PixelWorldCamera, &mut Transform)>,
) {
    if restarts.read().next().is_some() {
        let snap = spawn_point.0 + Vec2::new(50.0, 26.0);
        shake.trauma = 0.0;
        for (mut cam, mut transform) in &mut cameras {
            cam.smooth_pos = snap;
            cam.look_ahead = Vec2::ZERO;
            transform.translation.x = snap.x.round();
            transform.translation.y = snap.y.round();
        }
        return;
    }
    if *state.get() == AppState::Paused {
        return;
    }
    let Ok(body) = players.single() else {
        return;
    };
    let dt = time.delta_secs();

    shake.trauma = (shake.trauma - dt * 3.2).max(0.0);
    shake.phase += dt * 68.0;
    let shake_mag = shake.trauma * shake.trauma * 10.0;
    let shake_offset = if shake_mag > 0.05 {
        let dir = shake.direction.normalize_or(Vec2::X);
        let perp = Vec2::new(-dir.y, dir.x);
        dir * (shake.phase.sin() * shake_mag) + perp * ((shake.phase * 1.43).cos() * shake_mag * 0.45)
    } else {
        Vec2::ZERO
    };

    let arena_frame = blast_zones
        .iter()
        .next()
        .filter(|z| body.position.x >= z.min.x + 10.0 && body.position.y >= z.min.y)
        .map(|z| z.arena_center);

    for (mut cam, mut transform) in &mut cameras {
        let target = if let Some(arena_center) = arena_frame {
            // Blend smoothly toward the Zone C Smash Arena center while still following player action
            let player_bias = body.position + Vec2::new(0.0, 26.0);
            arena_center.lerp(player_bias, 0.32)
        } else {
            let target_ahead = Vec2::new(
                (body.velocity.x * 0.22).clamp(-96.0, 118.0),
                (body.velocity.y * 0.08).clamp(-32.0, 48.0),
            );
            let ahead_blend = (dt * 4.5).min(1.0);
            cam.look_ahead = cam.look_ahead.lerp(target_ahead, ahead_blend);
            body.position + Vec2::new(0.0, 26.0) + cam.look_ahead
        };

        let follow_blend = (dt * 7.5).min(1.0);
        cam.smooth_pos = cam.smooth_pos.lerp(target, follow_blend);

        transform.translation.x = (cam.smooth_pos.x + shake_offset.x).round();
        transform.translation.y = (cam.smooth_pos.y + shake_offset.y).round();
    }
}

fn animate_hero_companion_light(
    time: Res<Time>,
    state: Res<State<AppState>>,
    players: Query<&KinematicBody, With<Player>>,
    mut lights: Query<(&mut HeroCompanionLight, &mut Transform)>,
) {
    if *state.get() == AppState::Paused {
        return;
    }
    let anchor = players
        .single()
        .map(|b| b.position + Vec2::new(0.0, 12.0))
        .unwrap_or(Vec2::ZERO);

    for (mut orbit, mut transform) in &mut lights {
        orbit.angle += time.delta_secs() * 1.45;
        let x = anchor.x + orbit.angle.cos() * orbit.orbit_radius_x;
        let y = anchor.y + orbit.angle.sin() * orbit.orbit_radius_y;
        transform.translation.x = x.round();
        transform.translation.y = y.round();
    }
}
