//! Pixel-perfect 2D rendering pipeline (`640x360` internal render target with
//! integer upscaling, `ImageSampler::nearest()`, HDR, Bloom, custom 2D normal-mapped
//! sprite lighting via `assets/shaders/sprite_lit.wgsl`, Sonic cyan/amber speed
//! afterimage ghost trails, baked normal-mapped AI fighters, Smash KO flash beams,
//! and velocity look-ahead + Zone C arena framing camera).

pub mod sprite_lit;
pub mod vfx_energy;

use std::path::Path;

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{Hdr, RenderTarget, ScalingMode};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::image::{ImageLoaderSettings, ImageSampler};
use bevy::post_process::bloom::{Bloom, BloomCompositeMode};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::Msaa;
use bevy::sprite_render::{Material2dPlugin, MeshMaterial2d};
use bevy::transform::TransformSystems;
use bevy::window::{PrimaryWindow, WindowResized};

use crate::combat::{
    AirDodgeState, AttackKind, AttackState, BreakerWaveProjectile, BusterProjectile,
    BusterShotEvent, BusterState, CombatHitEvent, CombatVisualEvent, CounterStance, DamagePercent,
    GroundDodgeState, Hitstop, Hitstun, Invulnerable, MeteorDiveState, ShieldState,
    SmashChargeState,
};
use crate::enemies::{
    BlastEdge, BlastZone, BlastZoneKoEvent, BossState, EncounterStatus, EnemyAiState,
    EnemyArchetype, EnemyFighter, EnemyHealth, LockdownGate, TurretProjectile, WallTurret,
};
use crate::physics::{
    BoostPad, BreakableWall, CheckpointBeacon, CollectibleItem, CollectibleKind, HazardBeamPhase,
    JumpPad, KinematicBody, LoopSpline, MovementEvent, MovementFxKind, MovementState,
    MovingPlatform, OneWayPlatform, PhysicsConfig, SlopeSegment, SolidRect, SpikeHazard,
    TimedHazardBeam,
};
use crate::player::{
    Player, PlayerFeetPivot, PlayerP1Marker, PlayerRespawnState, PlayerSpawnPoint,
    PlayerSpriteVisual, RespawnAngelPlatformVisual,
};
use crate::sprite_manifest::SpriteSheetManifest;
use crate::state::{AppState, StageRestartEvent};

pub use sprite_lit::{
    MAX_POINT_LIGHTS_2D, PointLight2d, SpriteLitMaterial, SpriteLitUniform, SpriteSheetAnimator,
};
pub use vfx_energy::{
    AnimatedVfxEnergy, SharedVfxQuadMesh, TransientShaderVfx, VfxEnergyMaterial, VfxEnergyUniform,
    tick_vfx_energy_materials, vfx_mode,
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

/// Cached Foundry pixel-art environment textures loaded gracefully from `assets/textures/foundry/`.
#[derive(Resource, Debug, Default, Clone)]
struct FoundryArtAssets {
    bg_far: Option<Handle<Image>>,
    bg_mid: Option<Handle<Image>>,
    bg_near: Option<Handle<Image>>,
    tile_metal: Option<Handle<Image>>,
    tile_trim_top: Option<Handle<Image>>,
    tile_wall: Option<Handle<Image>>,
    platform_oneway: Option<Handle<Image>>,
    platform_moving: Option<Handle<Image>>,
    breakable_wall: Option<Handle<Image>>,
    boost_pad: Option<Handle<Image>>,
    jump_pad: Option<Handle<Image>>,
    spikes: Option<Handle<Image>>,
    turret: Option<Handle<Image>>,
    checkpoint: Option<Handle<Image>>,
    core: Option<Handle<Image>>,
    relic: Option<Handle<Image>>,
    heal: Option<Handle<Image>>,
    lava_strip: Option<Handle<Image>>,
    molten_cascade: Option<Handle<Image>>,
    lamp_fixture: Option<Handle<Image>>,
}

/// Sculpted 32-bit pixel-art VFX textures loaded gracefully from `assets/textures/vfx/`.
#[derive(Resource, Debug, Default, Clone)]
struct VfxArtAssets {
    buster_pellet: Option<Handle<Image>>,
    buster_charged: Option<Handle<Image>>,
    turret_bolt: Option<Handle<Image>>,
    arcane_orb: Option<Handle<Image>>,
    magma_wave: Option<Handle<Image>>,
    emitter_nozzle: Option<Handle<Image>>,
    respawn_platform: Option<Handle<Image>>,
    beacon_crystal: Option<Handle<Image>>,
    shadow_oval: Option<Handle<Image>>,
    smoke_puff: Option<Handle<Image>>,
    star_spark: Option<Handle<Image>>,
    p1_marker: Option<Handle<Image>>,
}

/// Animated pulsing molten lava & pouring liquid-metal furnace backdrop props.
#[derive(Component, Debug, Clone, Copy)]
struct MoltenFlowVisual {
    base_pos: Vec2,
    phase: f32,
    is_lava_horizon: bool,
}

/// Subtle ground contact shadow oval anchoring grounded fighters visually to platforms.
#[derive(Component, Debug, Clone, Copy)]
struct GroundContactShadow {
    owner: Entity,
    base_width: f32,
}

/// Seamless horizontal scrolling parallax layer tile anchored to `PixelWorldCamera`.
#[derive(Component, Debug, Clone, Copy)]
struct ParallaxLayer {
    factor_x: f32,
    factor_y: f32,
    tile_index: i32,
    tile_width: f32,
    z: f32,
}

/// Cadence timer for directional Smash knockback smoke trails behind launched fighters.
#[derive(Resource, Debug, Default)]
struct SmokeTrailTimer(f32);

/// Visual Smash energy shield bubble child attached to the player.
#[derive(Component, Debug)]
struct ShieldBubbleVisual;

/// Orbiting electric stun halo displayed above Levy's head during active `Hitstun` / Shield Break.
#[derive(Component, Debug)]
struct PlayerStunHalo;

/// Subtle constant cyan Rift-Diamond tracking aura behind Levy so he is always effortless to spot.
#[derive(Component, Debug)]
struct HeroTrackingGlow;

/// Individual orbiting electric stun star/bolt child inside [`PlayerStunHalo`].
#[derive(Component, Debug, Clone, Copy)]
struct PlayerStunStar(f32);

/// Cadence timer for crackling electric stun sparks around Levy's head while stunned.
#[derive(Resource, Debug, Default)]
struct StunSparkTimer(f32);

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

/// Child visual of an [`EnemyFighter`] driving baked sprites, state-change restarts,
/// anticipation lean, landing squash/stretch, and hitstop jitter.
#[derive(Component, Debug)]
struct EnemySpriteVisual {
    owner: Entity,
    jitter_phase: f32,
    base_y: f32,
    /// Exact pixel distance from sprite center down to boot sole (anchors squash/stretch to floor!).
    foot_dist: f32,
    /// Tracks last state category (`0`=idle/run, `1`=telegraph, `2`=attack, `3`=hitstun, `4`=air)
    /// so every attack & telegraph calls `play_restart(...)` cleanly from frame 0!
    last_state_tag: u8,
    last_variant: u8,
    was_grounded: bool,
    squash: Vec2,
    lean_rad: f32,
}

/// Miniature pixel-art overhead HP fill bar attached to regular stage & ambush enemies.
#[derive(Component, Debug)]
struct EnemyHealthBarFill {
    owner: Entity,
    max_width: f32,
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

/// Curved neon pixel-art blade trail segment emitted by melee & special swings.
#[derive(Component, Debug)]
struct SlashArcSeg {
    velocity: Vec2,
    lifetime: f32,
    max_lifetime: f32,
    base_len: f32,
}

/// Expanding octagonal neon shockwave ring segment for counters, slams, stomps, and warps.
#[derive(Component, Debug)]
struct ShockwaveRingSeg {
    center: Vec2,
    angle: f32,
    max_radius: f32,
    lifetime: f32,
    max_lifetime: f32,
}

/// Pulsing amber-gold aiming warning sightline shown during Vulcanus's 0.48s Phase 2 laser telegraph!
#[derive(Component, Debug)]
struct BossSightlineFx {
    lifetime: f32,
    max_lifetime: f32,
    length: f32,
}

/// Child visual orb on a [`CheckpointBeacon`] pylon (amber idle -> blazing cyan active!).
#[derive(Component, Debug)]
struct CheckpointBeaconVisual {
    owner: Entity,
}

/// Child visual beam column on a [`TimedHazardBeam`] magma/laser emitter.
#[derive(Component, Debug)]
struct TimedBeamColumnVisual {
    owner: Entity,
    size: Vec2,
}

/// Child visual glowing crack overlay on a [`BreakableWall`].
#[derive(Component, Debug)]
struct BreakableWallCrackVisual {
    owner: Entity,
}

/// Ambient rising Foundry ember spawner timer for living atmosphere across all 8 sectors.
#[derive(Resource, Debug, Default)]
struct AmbientEmberTimer {
    timer: f32,
    seed: u32,
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
        app.add_plugins((
            Material2dPlugin::<SpriteLitMaterial>::default(),
            Material2dPlugin::<VfxEnergyMaterial>::default(),
        ))
            .init_resource::<FoundryArtAssets>()
            .init_resource::<VfxArtAssets>()
            .init_resource::<CameraShake>()
            .init_resource::<LastVisualAttack>()
            .init_resource::<AfterimageSpawner>()
            .init_resource::<SmokeTrailTimer>()
            .init_resource::<StunSparkTimer>()
            .init_resource::<AmbientEmberTimer>()
            .add_systems(
                Startup,
                (
                    load_foundry_art_assets,
                    setup_pixel_cameras,
                    spawn_foundry_backdrop,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    decorate_added_player,
                    decorate_added_solids,
                    decorate_added_lockdown_gates,
                    decorate_added_slopes,
                    decorate_added_one_ways,
                    decorate_added_boost_pads,
                    decorate_added_jump_pads,
                    decorate_added_checkpoint_beacons,
                    decorate_added_collectibles,
                    decorate_added_breakable_walls,
                    decorate_added_timed_hazard_beams,
                    decorate_added_loop_splines,
                    decorate_added_spike_hazards,
                    decorate_added_wall_turrets,
                    decorate_added_enemy_fighters,
                    decorate_added_buster_projectiles,
                    decorate_added_breaker_waves,
                    decorate_added_turret_projectiles,
                    decorate_added_blast_zones,
                ),
            )
            .add_systems(
                Update,
                (
                    (
                        sync_canvas_integer_scale,
                        drive_player_visuals_and_animations,
                        spawn_and_fade_speed_afterimages,
                        update_buster_charge_aura,
                        update_shield_bubble_visual,
                        update_player_stun_fx,
                        update_respawn_angel_platform_and_p1_marker,
                        drive_enemy_fighter_visuals,
                        update_enemy_health_bars,
                        update_wall_turret_visuals,
                        update_stage_feature_visuals,
                        update_ground_contact_shadows_and_molten_fx,
                        spawn_ambient_foundry_embers,
                    )
                        .chain(),
                    (
                        handle_combat_hit_and_shot_fx,
                        handle_combat_visual_events,
                        spawn_movement_particles,
                        spawn_knockback_smoke_trails,
                        handle_blast_zone_ko_flash,
                        update_hit_sparks_and_ko_beams,
                        update_slash_and_shockwave_fx,
                        tick_vfx_energy_materials,
                        update_follow_camera,
                        update_parallax_backdrop,
                        animate_hero_companion_light,
                    )
                        .chain(),
                )
                    .chain(),
            )
            .add_systems(
                PostUpdate,
                sprite_lit::sync_sprite_lit_materials.after(TransformSystems::Propagate),
            );
    }
}

fn load_nearest_texture_if_exists(
    asset_server: &AssetServer,
    rel_path: &str,
) -> Option<Handle<Image>> {
    if Path::new("assets").join(rel_path).exists() {
        Some(
            asset_server
                .load_builder()
                .with_settings(|s: &mut ImageLoaderSettings| {
                    s.is_srgb = true;
                    s.sampler = ImageSampler::nearest();
                })
                .load(rel_path.to_string()),
        )
    } else {
        warn!(
            "Optional Foundry texture missing at assets/{}; using fallback visuals.",
            rel_path
        );
        None
    }
}

fn load_foundry_art_assets(
    asset_server: Res<AssetServer>,
    mut foundry: ResMut<FoundryArtAssets>,
    mut vfx: ResMut<VfxArtAssets>,
) {
    foundry.bg_far =
        load_nearest_texture_if_exists(&asset_server, "textures/foundry/foundry_bg_far.png");
    foundry.bg_mid =
        load_nearest_texture_if_exists(&asset_server, "textures/foundry/foundry_bg_mid.png");
    foundry.bg_near =
        load_nearest_texture_if_exists(&asset_server, "textures/foundry/foundry_bg_near.png");
    foundry.tile_metal =
        load_nearest_texture_if_exists(&asset_server, "textures/foundry/foundry_tile_metal.png");
    foundry.tile_trim_top =
        load_nearest_texture_if_exists(&asset_server, "textures/foundry/foundry_tile_trim_top.png");
    foundry.tile_wall =
        load_nearest_texture_if_exists(&asset_server, "textures/foundry/foundry_tile_wall.png");
    foundry.platform_oneway = load_nearest_texture_if_exists(
        &asset_server,
        "textures/foundry/foundry_platform_oneway.png",
    );
    foundry.platform_moving = load_nearest_texture_if_exists(
        &asset_server,
        "textures/foundry/foundry_platform_moving.png",
    );
    foundry.breakable_wall = load_nearest_texture_if_exists(
        &asset_server,
        "textures/foundry/foundry_breakable_wall.png",
    );
    foundry.boost_pad =
        load_nearest_texture_if_exists(&asset_server, "textures/foundry/foundry_boost_pad.png");
    foundry.jump_pad =
        load_nearest_texture_if_exists(&asset_server, "textures/foundry/foundry_jump_pad.png");
    foundry.spikes =
        load_nearest_texture_if_exists(&asset_server, "textures/foundry/foundry_spikes.png");
    foundry.turret =
        load_nearest_texture_if_exists(&asset_server, "textures/foundry/foundry_turret.png");
    foundry.checkpoint =
        load_nearest_texture_if_exists(&asset_server, "textures/foundry/foundry_checkpoint.png");
    foundry.core =
        load_nearest_texture_if_exists(&asset_server, "textures/foundry/foundry_core.png");
    foundry.relic =
        load_nearest_texture_if_exists(&asset_server, "textures/foundry/foundry_relic.png");
    foundry.heal =
        load_nearest_texture_if_exists(&asset_server, "textures/foundry/foundry_heal.png");
    foundry.lava_strip =
        load_nearest_texture_if_exists(&asset_server, "textures/foundry/foundry_lava_strip.png");
    foundry.molten_cascade = load_nearest_texture_if_exists(
        &asset_server,
        "textures/foundry/foundry_molten_cascade.png",
    );
    foundry.lamp_fixture =
        load_nearest_texture_if_exists(&asset_server, "textures/foundry/foundry_lamp_fixture.png");

    // Sculpted 32-bit pixel-art VFX textures
    vfx.buster_pellet =
        load_nearest_texture_if_exists(&asset_server, "textures/vfx/vfx_buster_pellet.png");
    vfx.buster_charged =
        load_nearest_texture_if_exists(&asset_server, "textures/vfx/vfx_buster_charged.png");
    vfx.turret_bolt =
        load_nearest_texture_if_exists(&asset_server, "textures/vfx/vfx_turret_bolt.png");
    vfx.arcane_orb =
        load_nearest_texture_if_exists(&asset_server, "textures/vfx/vfx_arcane_orb.png");
    vfx.magma_wave =
        load_nearest_texture_if_exists(&asset_server, "textures/vfx/vfx_magma_wave.png");
    vfx.emitter_nozzle =
        load_nearest_texture_if_exists(&asset_server, "textures/vfx/vfx_emitter_nozzle.png");
    vfx.respawn_platform =
        load_nearest_texture_if_exists(&asset_server, "textures/vfx/vfx_respawn_platform.png");
    vfx.beacon_crystal =
        load_nearest_texture_if_exists(&asset_server, "textures/vfx/vfx_beacon_crystal.png");
    vfx.shadow_oval =
        load_nearest_texture_if_exists(&asset_server, "textures/vfx/vfx_shadow_oval.png");
    vfx.smoke_puff =
        load_nearest_texture_if_exists(&asset_server, "textures/vfx/vfx_smoke_puff.png");
    vfx.star_spark =
        load_nearest_texture_if_exists(&asset_server, "textures/vfx/vfx_star_spark.png");
    vfx.p1_marker =
        load_nearest_texture_if_exists(&asset_server, "textures/vfx/vfx_p1_marker.png");
}

fn setup_pixel_cameras(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    primary_window: Query<&Window, With<PrimaryWindow>>,
) {
    commands.insert_resource(SharedVfxQuadMesh(meshes.add(Rectangle::new(1.0, 1.0))));
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
        Tonemapping::TonyMcMapface,
        DebandDither::Enabled,
        Bloom {
            intensity: 0.24,
            low_frequency_boost: 0.52,
            low_frequency_boost_curvature: 0.85,
            high_pass_frequency: 0.92,
            composite_mode: BloomCompositeMode::Additive,
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

fn spawn_foundry_backdrop(
    mut commands: Commands,
    foundry: Res<FoundryArtAssets>,
    vfx: Res<VfxArtAssets>,
    shared_quad: Res<SharedVfxQuadMesh>,
    mut vfx_materials: ResMut<Assets<VfxEnergyMaterial>>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);

    // Spawn seamless 3-tile parallax Foundry cavern layers when local WARLOCK textures are present;
    // otherwise fall back gracefully to the crisp geometric blueprint grid.
    if let Some(ref bg_far) = foundry.bg_far {
        for tile_index in -1..=1 {
            commands.spawn((
                Name::new(format!("FoundryParallaxFar_{tile_index}")),
                ParallaxLayer {
                    factor_x: 0.18,
                    factor_y: 0.08,
                    tile_index,
                    tile_width: VIRTUAL_WIDTH as f32,
                    z: -26.0,
                },
                Sprite {
                    image: bg_far.clone(),
                    color: Color::srgba(0.72, 0.76, 0.88, 1.0),
                    custom_size: Some(Vec2::new(VIRTUAL_WIDTH as f32, VIRTUAL_HEIGHT as f32)),
                    ..default()
                },
                Transform::from_xyz(tile_index as f32 * VIRTUAL_WIDTH as f32, 20.0, -26.0),
                world_layer.clone(),
            ));
        }
    } else {
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
    }

    if let Some(ref bg_mid) = foundry.bg_mid {
        for tile_index in -1..=1 {
            commands.spawn((
                Name::new(format!("FoundryParallaxMid_{tile_index}")),
                ParallaxLayer {
                    factor_x: 0.42,
                    factor_y: 0.18,
                    tile_index,
                    tile_width: VIRTUAL_WIDTH as f32,
                    z: -16.0,
                },
                Sprite {
                    image: bg_mid.clone(),
                    color: Color::srgba(0.74, 0.78, 0.88, 0.92),
                    custom_size: Some(Vec2::new(VIRTUAL_WIDTH as f32, VIRTUAL_HEIGHT as f32)),
                    ..default()
                },
                Transform::from_xyz(tile_index as f32 * VIRTUAL_WIDTH as f32, 20.0, -16.0),
                world_layer.clone(),
            ));
        }
    }

    if let Some(ref bg_near) = foundry.bg_near {
        for tile_index in -1..=1 {
            commands.spawn((
                Name::new(format!("FoundryParallaxNear_{tile_index}")),
                ParallaxLayer {
                    factor_x: 0.68,
                    factor_y: 0.26,
                    tile_index,
                    tile_width: VIRTUAL_WIDTH as f32,
                    z: -10.5,
                },
                Sprite {
                    image: bg_near.clone(),
                    color: Color::srgba(0.60, 0.65, 0.78, 0.74),
                    custom_size: Some(Vec2::new(VIRTUAL_WIDTH as f32, VIRTUAL_HEIGHT as f32)),
                    ..default()
                },
                Transform::from_xyz(tile_index as f32 * VIRTUAL_WIDTH as f32, 20.0, -10.5),
                world_layer.clone(),
            ));
        }
    }

    // Dimmed, desaturated background boiling molten slag horizon stretching across the 10,000px abyss
    // (kept strictly below HDR bloom threshold so foreground platforms & hazards pop cleanly!).
    if let Some(ref lava_img) = foundry.lava_strip {
        commands.spawn((
            Name::new("FoundryMoltenLavaHorizon"),
            MoltenFlowVisual {
                base_pos: Vec2::new(4500.0, -108.0),
                phase: 0.0,
                is_lava_horizon: true,
            },
            Sprite {
                image: lava_img.clone(),
                color: Color::srgba(0.66, 0.48, 0.35, 0.90),
                custom_size: Some(Vec2::new(10400.0, 28.0)),
                image_mode: SpriteImageMode::Tiled {
                    tile_x: true,
                    tile_y: false,
                    stretch_value: 1.0,
                },
                ..default()
            },
            Transform::from_xyz(4500.0, -108.0, -7.8),
            world_layer.clone(),
        ));
        // Deep smoky molten under-glow pool beneath the wave crest
        commands.spawn((
            Name::new("FoundryMoltenAbyssPool"),
            Sprite {
                color: Color::srgb(0.44, 0.18, 0.08),
                custom_size: Some(Vec2::new(10400.0, 120.0)),
                ..default()
            },
            Transform::from_xyz(4500.0, -178.0, -8.2),
            world_layer.clone(),
        ));
    }

    // Full-height background liquid-metal smelter pours running cleanly from above the top
    // camera ceiling (+920) all the way down into the molten slag abyss (-130), dimmed &
    // desaturated so they read clearly as atmospheric background scenery!
    if let Some(ref cas_img) = foundry.molten_cascade {
        for (idx, (cx, splash_lava)) in [
            // Sector 1-2 opening chasms pouring right into the molten slag lake
            (26.0_f32, true),
            (340.0, true),
            // Sector 3: The Smelter Cage
            (1260.0, false),
            (1580.0, false),
            // Sector 4: Slag-Slope Chasms
            (2435.0, true),
            (3310.0, true),
            // Sector 5: Sky-Gondola Abyss
            (3785.0, false),
            (4135.0, false),
            // Sector 6: Conveyor Tower
            (5620.0, false),
            // Sector 7: Reactor Crucible
            (6940.0, false),
            (7280.0, false),
            // Sector 8: Citadel Approach Chasm
            (7975.0, true),
            // Final Sanctum: Iron Titan Vulcanus
            (8660.0, false),
            (9080.0, false),
        ]
        .into_iter()
        .enumerate()
        {
            let cy = 395.0_f32;
            let h = 1050.0_f32;
            commands.spawn((
                Name::new(format!("FoundryMoltenCascade_{idx}")),
                MoltenFlowVisual {
                    base_pos: Vec2::new(cx, cy),
                    phase: idx as f32 * 0.9,
                    is_lava_horizon: false,
                },
                Sprite {
                    image: cas_img.clone(),
                    color: Color::srgba(0.62, 0.46, 0.34, 0.58),
                    custom_size: Some(Vec2::new(26.0, h)),
                    image_mode: SpriteImageMode::Tiled {
                        tile_x: false,
                        tile_y: true,
                        stretch_value: 1.0,
                    },
                    ..default()
                },
                PointLight2d {
                    color: LinearRgba::new(0.95, 0.46, 0.16, 1.0),
                    intensity: 0.95,
                    radius: 210.0,
                    height: 44.0,
                    falloff: 1.4,
                },
                Transform::from_xyz(cx, cy, -9.2),
                world_layer.clone(),
            ));
            if splash_lava {
                // Soft warm mist crown where the pour meets the background lava lake
                commands.spawn((
                    Sprite {
                        color: Color::srgba(0.84, 0.52, 0.28, 0.48),
                        custom_size: Some(Vec2::new(40.0, 9.0)),
                        ..default()
                    },
                    Transform::from_xyz(cx, -96.0, -7.6),
                    world_layer.clone(),
                ));
            }
        }
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
            intensity: 1.35,
            radius: 185.0,
            height: 30.0,
            falloff: 1.5,
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

    // Full 8-Sector industrial overhead foundry lamps spanning ALL 9,700px of Stage 1 (-240 .. 9180)!
    // Color temperature evolves naturally across sectors: Cyan Intake -> Amber Smelter ->
    // Molten Chasm -> Electric Cyan-Violet Reactor Crucible -> Crimson Citadel -> Molten Gold Sanctum!
    for (lx, ly, color, radius) in [
        // Sector 1-2: Intake Runway & Maintenance Shaft
        (-220.0, 130.0, LinearRgba::new(0.24, 0.88, 1.0, 1.0), 290.0),
        (140.0, 165.0, LinearRgba::new(0.32, 0.84, 1.0, 1.0), 300.0),
        (540.0, 240.0, LinearRgba::new(1.0, 0.56, 0.18, 1.0), 320.0),
        (820.0, 395.0, LinearRgba::new(0.28, 0.88, 1.0, 1.0), 340.0),
        // Sector 3: The Smelter Cage
        (1240.0, 415.0, LinearRgba::new(1.0, 0.54, 0.18, 1.0), 380.0),
        (1600.0, 415.0, LinearRgba::new(1.0, 0.62, 0.22, 1.0), 380.0),
        // Sector 4: Molten Chasm & Sky Bridges
        (2060.0, 405.0, LinearRgba::new(0.26, 0.90, 1.0, 1.0), 360.0),
        (2520.0, 415.0, LinearRgba::new(1.0, 0.50, 0.18, 1.0), 360.0),
        (2980.0, 425.0, LinearRgba::new(1.0, 0.58, 0.20, 1.0), 360.0),
        (3480.0, 415.0, LinearRgba::new(0.26, 0.88, 1.0, 1.0), 370.0),
        // Sector 5: Vertical Refinery & Mag-Lev Gauntlet
        (3980.0, 420.0, LinearRgba::new(1.0, 0.52, 0.18, 1.0), 370.0),
        (4480.0, 425.0, LinearRgba::new(0.30, 0.86, 1.0, 1.0), 370.0),
        (4980.0, 425.0, LinearRgba::new(1.0, 0.50, 0.18, 1.0), 380.0),
        (5480.0, 430.0, LinearRgba::new(0.32, 0.88, 1.0, 1.0), 380.0),
        (5980.0, 425.0, LinearRgba::new(1.0, 0.46, 0.22, 1.0), 380.0),
        (6480.0, 415.0, LinearRgba::new(0.28, 0.90, 1.0, 1.0), 370.0),
        // Sector 6: The Reactor Crucible (High-Voltage Cyan & Violet!)
        (6920.0, 425.0, LinearRgba::new(0.28, 0.92, 1.0, 1.0), 390.0),
        (7300.0, 425.0, LinearRgba::new(0.68, 0.36, 1.0, 1.0), 390.0),
        // Sector 7: Citadel Approach (Crimson-Amber Boss Warning!)
        (7780.0, 410.0, LinearRgba::new(1.0, 0.34, 0.22, 1.0), 370.0),
        (8240.0, 410.0, LinearRgba::new(1.0, 0.46, 0.18, 1.0), 380.0),
        // Sector 8: Iron Titan Vulcanus Furnace Sanctum!
        (8680.0, 430.0, LinearRgba::new(1.0, 0.44, 0.14, 1.0), 420.0),
        (9060.0, 430.0, LinearRgba::new(1.0, 0.62, 0.18, 1.0), 430.0),
    ] {
        let mut lamp = commands.spawn((
            PointLight2d {
                color,
                intensity: 2.15,
                radius,
                height: 52.0,
                falloff: 1.35,
            },
            Transform::from_xyz(lx, ly, -4.0),
            Visibility::default(),
        ));
        let cone_mat = vfx_materials.add(VfxEnergyMaterial {
            data: VfxEnergyUniform {
                params0: Vec4::new(vfx_mode::LAMP_LIGHT_CONE, 0.0, 0.0, 1.0),
                params1: Vec4::new(1.0, 1.0, 0.0, 1.0),
                color_a: Vec4::new(color.red * 0.95, color.green * 0.95, color.blue * 0.95, 0.16),
                color_b: Vec4::new(
                    (color.red + 0.35).min(1.4),
                    (color.green + 0.35).min(1.4),
                    (color.blue + 0.35).min(1.4),
                    1.0,
                ),
            },
        });
        lamp.with_children(|parent| {
            // Soft GPU-shader volumetric light cone descending into the industrial haze!
            parent.spawn((
                AnimatedVfxEnergy {
                    phase_offset: lx * 0.013,
                },
                Mesh2d(shared_quad.0.clone()),
                MeshMaterial2d(cone_mat),
                Transform::from_xyz(0.0, -60.0, -0.3).with_scale(Vec3::new(86.0, 118.0, 1.0)),
                world_layer.clone(),
            ));
            // Industrial hanging iron lamp hood + glowing bulb!
            if let Some(ref hood) = foundry.lamp_fixture {
                parent.spawn((
                    Sprite {
                        image: hood.clone(),
                        custom_size: Some(Vec2::new(28.0, 14.0)),
                        ..default()
                    },
                    Transform::from_xyz(0.0, 4.0, 0.1),
                    world_layer.clone(),
                ));
            }
            parent.spawn((
                Sprite {
                    color: Color::linear_rgb(color.red * 3.8, color.green * 3.8, color.blue * 3.8),
                    custom_size: Some(Vec2::new(14.0, 3.0)),
                    ..default()
                },
                Transform::from_xyz(0.0, -2.0, 0.2),
                world_layer.clone(),
            ));
        });
    }

    // Smash-style sculpted winged holographic Angel Respawn Platform (hidden until PlayerRespawnState.active)
    let respawn_sprite = if let Some(ref img) = vfx.respawn_platform {
        Sprite {
            image: img.clone(),
            color: Color::linear_rgb(1.45, 1.85, 2.2),
            custom_size: Some(Vec2::new(48.0, 20.0)),
            ..default()
        }
    } else {
        Sprite {
            color: Color::linear_rgb(0.4, 3.6, 5.0),
            custom_size: Some(Vec2::new(44.0, 6.0)),
            ..default()
        }
    };
    commands.spawn((
        Name::new("RespawnAngelPlatform"),
        RespawnAngelPlatformVisual,
        respawn_sprite,
        PointLight2d {
            color: LinearRgba::new(0.25, 0.92, 1.0, 1.0),
            intensity: 1.4,
            radius: 95.0,
            height: 20.0,
            falloff: 1.5,
        },
        Transform::from_xyz(0.0, 0.0, 5.0),
        Visibility::Hidden,
        world_layer.clone(),
    ));
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

fn create_soft_radial_glow_image(images: &mut Assets<Image>) -> Handle<Image> {
    let size = 32u32;
    let mut data = vec![0u8; (size * size * 4) as usize];
    let center = (size as f32 - 1.0) * 0.5;
    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 - center) / center;
            let dy = (y as f32 - center) / center;
            let dist = (dx * dx + dy * dy).sqrt();
            let falloff = (1.0 - dist).clamp(0.0, 1.0);
            let alpha = (falloff * falloff * 255.0).round().clamp(0.0, 255.0) as u8;
            let idx = ((y * size + x) * 4) as usize;
            data[idx] = 175;
            data[idx + 1] = 242;
            data[idx + 2] = 255;
            data[idx + 3] = alpha;
        }
    }
    let mut img = Image::new(
        bevy::render::render_resource::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        default(),
    );
    img.sampler = ImageSampler::nearest();
    images.add(img)
}

#[allow(clippy::too_many_arguments)]
fn decorate_added_player(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    vfx: Res<VfxArtAssets>,
    shared_quad: Res<SharedVfxQuadMesh>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut sprite_materials: ResMut<Assets<SpriteLitMaterial>>,
    mut vfx_materials: ResMut<Assets<VfxEnergyMaterial>>,
    added_players: Query<(Entity, &KinematicBody), Added<Player>>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);

    for (player_entity, body) in &added_players {
        let foot_pivot_y = -body.half_extents.y;
        let glow_tex = create_soft_radial_glow_image(&mut images);
        let charge_mat = vfx_materials.add(VfxEnergyMaterial {
            data: VfxEnergyUniform {
                params0: Vec4::new(vfx_mode::CHARGE_VORTEX, 0.0, 0.0, 0.0),
                params1: Vec4::new(1.0, 1.0, 0.0, 1.0),
                color_a: Vec4::new(0.22, 2.6, 4.2, 0.85),
                color_b: Vec4::new(4.4, 5.0, 5.6, 1.0),
            },
        });
        let shield_mat = vfx_materials.add(VfxEnergyMaterial {
            data: VfxEnergyUniform {
                params0: Vec4::new(vfx_mode::HEX_SHIELD, 0.0, 1.0, 0.0),
                params1: Vec4::new(1.0, 1.0, 0.0, 1.0),
                color_a: Vec4::new(0.22, 2.5, 4.2, 0.90),
                color_b: Vec4::new(3.6, 5.0, 5.6, 1.0),
            },
        });

        commands.entity(player_entity).with_children(|parent| {
            // Very subtle warm-cyan silhouette halo behind Levy so he stays easy to track without blue glow wash
            parent.spawn((
                Name::new("HeroTrackingGlow"),
                HeroTrackingGlow,
                Sprite {
                    image: glow_tex,
                    color: Color::linear_rgba(0.28, 0.95, 1.35, 0.11),
                    custom_size: Some(Vec2::new(30.0, 38.0)),
                    ..default()
                },
                Transform::from_xyz(0.0, 4.0, 1.35),
                world_layer.clone(),
            ));
            // GPU-shader swirling Rift Buster Charge Vortex (no blocky box!)
            parent.spawn((
                Name::new("BusterChargeAura"),
                BusterChargeAura { pulse: 0.0 },
                AnimatedVfxEnergy { phase_offset: 0.0 },
                PointLight2d {
                    color: LinearRgba::new(0.15, 0.92, 1.0, 1.0),
                    intensity: 0.0,
                    radius: 150.0,
                    height: 22.0,
                    falloff: 1.4,
                },
                Mesh2d(shared_quad.0.clone()),
                MeshMaterial2d(charge_mat),
                Transform::from_xyz(0.0, 4.0, 1.6).with_scale(Vec3::new(46.0, 46.0, 1.0)),
                world_layer.clone(),
            ));
            // GPU-shader Hexagonal Holographic Smash Shield Dome (Fresnel honeycomb sphere!)
            parent.spawn((
                Name::new("ShieldBubbleVisual"),
                ShieldBubbleVisual,
                AnimatedVfxEnergy { phase_offset: 0.7 },
                Mesh2d(shared_quad.0.clone()),
                MeshMaterial2d(shield_mat),
                Transform::from_xyz(0.0, 4.0, 4.5).with_scale(Vec3::new(54.0, 54.0, 1.0)),
                world_layer.clone(),
            ));
            // Subtle balanced personal key light so Levy reads cleanly with minimal blue spill
            parent.spawn((
                Name::new("HeroCyanRimLight"),
                PointLight2d {
                    color: LinearRgba::new(0.48, 0.84, 0.96, 1.0),
                    intensity: 0.50,
                    radius: 68.0,
                    height: 28.0,
                    falloff: 1.7,
                },
                Transform::from_xyz(0.0, 10.0, 4.0),
            ));
            // Orbiting crisp 4-point electric-yellow stun stars above Levy's head (visible during Hitstun)
            parent
                .spawn((
                    Name::new("PlayerStunHalo"),
                    PlayerStunHalo,
                    Transform::from_xyz(0.0, 32.0, 8.6),
                    Visibility::Hidden,
                ))
                .with_children(|halo| {
                    for i in 0..3 {
                        let phase = (i as f32 / 3.0) * std::f32::consts::TAU;
                        let star_sp = if let Some(ref img) = vfx.star_spark {
                            Sprite {
                                image: img.clone(),
                                color: if i % 2 == 0 {
                                    Color::linear_rgb(5.4, 4.4, 0.55)
                                } else {
                                    Color::linear_rgb(5.2, 5.2, 2.8)
                                },
                                custom_size: Some(Vec2::new(9.0, 9.0)),
                                ..default()
                            }
                        } else {
                            Sprite {
                                color: Color::linear_rgb(5.4, 4.4, 0.55),
                                custom_size: Some(Vec2::new(5.0, 5.0)),
                                ..default()
                            }
                        };
                        halo.spawn((
                            PlayerStunStar(phase),
                            star_sp,
                            Transform::from_xyz(phase.cos() * 14.0, phase.sin() * 4.0, 0.1),
                            world_layer.clone(),
                        ));
                    }
                });
            // Smooth pixel-dithered elliptical contact shadow anchoring Levy's boots onto metal decks
            let shadow_sp = if let Some(ref img) = vfx.shadow_oval {
                Sprite {
                    image: img.clone(),
                    color: Color::srgba(1.0, 1.0, 1.0, 0.72),
                    custom_size: Some(Vec2::new(26.0, 7.0)),
                    ..default()
                }
            } else {
                Sprite {
                    color: Color::srgba(0.02, 0.03, 0.06, 0.58),
                    custom_size: Some(Vec2::new(22.0, 4.0)),
                    ..default()
                }
            };
            parent.spawn((
                Name::new("PlayerContactShadow"),
                GroundContactShadow {
                    owner: player_entity,
                    base_width: 26.0,
                },
                shadow_sp,
                Transform::from_xyz(0.0, foot_pivot_y + 1.0, 1.1),
                world_layer.clone(),
            ));
            // Crisp bordered pixel-art P1 downward chevron indicator cleanly above Levy's head
            let marker_sp = if let Some(ref img) = vfx.p1_marker {
                Sprite {
                    image: img.clone(),
                    color: Color::linear_rgb(1.4, 1.8, 2.1),
                    custom_size: Some(Vec2::new(14.0, 14.0)),
                    ..default()
                }
            } else {
                Sprite {
                    color: Color::linear_rgb(0.35, 3.8, 5.2),
                    custom_size: Some(Vec2::new(8.0, 6.0)),
                    ..default()
                }
            };
            parent.spawn((
                Name::new("PlayerP1Marker"),
                PlayerP1Marker,
                marker_sp,
                Transform::from_xyz(0.0, 48.0, 8.5),
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
                            // Measured exact boot sole row=76 in 96x96 frame -> center sits +28px above feet!
                            Transform::from_xyz(0.0, 28.0, 2.0),
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

#[allow(clippy::type_complexity)]
fn decorate_added_solids(
    mut commands: Commands,
    foundry: Res<FoundryArtAssets>,
    added: Query<
        (Entity, &SolidRect),
        (Added<SolidRect>, Without<LockdownGate>, Without<BreakableWall>),
    >,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    for (entity, rect) in &added {
        let size = rect.half_extents * 2.0;
        let is_wall = size.y > size.x * 1.35;
        let sprite = if is_wall {
            if let Some(ref wall_img) = foundry.tile_wall {
                Sprite {
                    image: wall_img.clone(),
                    color: Color::srgb(0.95, 0.96, 1.0),
                    custom_size: Some(size),
                    image_mode: SpriteImageMode::Tiled {
                        tile_x: true,
                        tile_y: true,
                        stretch_value: 1.0,
                    },
                    ..default()
                }
            } else {
                Sprite {
                    color: Color::srgb(0.16, 0.21, 0.30),
                    custom_size: Some(size),
                    ..default()
                }
            }
        } else if let Some(ref tile_img) = foundry.tile_metal {
            Sprite {
                image: tile_img.clone(),
                color: Color::srgb(0.92, 0.95, 1.0),
                custom_size: Some(size),
                image_mode: SpriteImageMode::Tiled {
                    tile_x: true,
                    tile_y: true,
                    stretch_value: 1.0,
                },
                ..default()
            }
        } else {
            Sprite {
                color: Color::srgb(0.14, 0.18, 0.26),
                custom_size: Some(size),
                ..default()
            }
        };
        commands.entity(entity).insert((
            sprite,
            Transform::from_xyz(rect.center.x, rect.center.y, -6.0),
            world_layer.clone(),
        ));
        if is_wall {
            // Crisp amber-lit wall-jump edge strips
            for side in [-1.0_f32, 1.0_f32] {
                commands.spawn((
                    Sprite {
                        color: Color::linear_rgb(2.2, 1.0, 0.22),
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
            // Beveled steel walkway top cap with amber-black hazard chevrons + cyan conduit rail!
            let trim_h = size.y.min(12.0);
            if let Some(ref trim_img) = foundry.tile_trim_top {
                commands.spawn((
                    Sprite {
                        image: trim_img.clone(),
                        custom_size: Some(Vec2::new(size.x, trim_h)),
                        image_mode: SpriteImageMode::Tiled {
                            tile_x: true,
                            tile_y: false,
                            stretch_value: 1.0,
                        },
                        ..default()
                    },
                    Transform::from_xyz(
                        rect.center.x,
                        rect.center.y + rect.half_extents.y - trim_h * 0.5,
                        -4.8,
                    ),
                    world_layer.clone(),
                ));
            }
            // Subtle HDR cyan bloom rail accent along the deck lip
            commands.spawn((
                Sprite {
                    color: Color::linear_rgba(0.25, 2.6, 3.8, 0.65),
                    custom_size: Some(Vec2::new(size.x, 1.5)),
                    ..default()
                },
                Transform::from_xyz(
                    rect.center.x,
                    rect.center.y + rect.half_extents.y - 3.5,
                    -4.6,
                ),
                world_layer.clone(),
            ));
            // Dark recessed shadow lip underneath floating decks
            commands.spawn((
                Sprite {
                    color: Color::srgba(0.02, 0.03, 0.05, 0.85),
                    custom_size: Some(Vec2::new(size.x, 3.0)),
                    ..default()
                },
                Transform::from_xyz(
                    rect.center.x,
                    rect.center.y - rect.half_extents.y + 1.5,
                    -4.8,
                ),
                world_layer.clone(),
            ));

            // Heavy recessed industrial foundry support columns anchoring wide decks into the depths!
            if size.x >= 96.0 {
                let pillar_h = 260.0_f32;
                let pillar_y = rect.center.y - rect.half_extents.y - pillar_h * 0.5;
                let cols: Vec<f32> = if size.x >= 420.0 {
                    vec![
                        rect.center.x - rect.half_extents.x + 28.0,
                        rect.center.x,
                        rect.center.x + rect.half_extents.x - 28.0,
                    ]
                } else {
                    vec![
                        rect.center.x - rect.half_extents.x + 22.0,
                        rect.center.x + rect.half_extents.x - 22.0,
                    ]
                };
                for px in cols {
                    let pillar_sp = if let Some(ref wall_img) = foundry.tile_wall {
                        Sprite {
                            image: wall_img.clone(),
                            color: Color::srgba(0.46, 0.50, 0.62, 0.88),
                            custom_size: Some(Vec2::new(22.0, pillar_h)),
                            image_mode: SpriteImageMode::Tiled {
                                tile_x: false,
                                tile_y: true,
                                stretch_value: 1.0,
                            },
                            ..default()
                        }
                    } else {
                        Sprite {
                            color: Color::srgba(0.08, 0.11, 0.17, 0.85),
                            custom_size: Some(Vec2::new(22.0, pillar_h)),
                            ..default()
                        }
                    };
                    commands.spawn((
                        pillar_sp,
                        Transform::from_xyz(px, pillar_y, -7.2),
                        world_layer.clone(),
                    ));
                }
            }
        }
    }
}

/// Decorates active [`LockdownGate`] laser barriers with GPU-shader high-voltage plasma beams
/// anchored cleanly between heavy industrial emitter housings!
fn decorate_added_lockdown_gates(
    mut commands: Commands,
    vfx: Res<VfxArtAssets>,
    shared_quad: Res<SharedVfxQuadMesh>,
    mut vfx_materials: ResMut<Assets<VfxEnergyMaterial>>,
    added: Query<(Entity, &LockdownGate, &SolidRect), Added<LockdownGate>>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    for (entity, gate, rect) in &added {
        let size = rect.half_extents * 2.0;
        let (col_a, col_b, light_col) = if gate.is_boss {
            (
                Vec4::new(4.8, 1.35, 0.28, 0.96),
                Vec4::new(5.6, 4.6, 2.2, 1.0),
                LinearRgba::new(1.0, 0.45, 0.12, 1.0),
            )
        } else {
            (
                Vec4::new(4.6, 0.38, 0.65, 0.96),
                Vec4::new(5.6, 3.8, 4.4, 1.0),
                LinearRgba::new(1.0, 0.22, 0.35, 1.0),
            )
        };
        let beam_mat = vfx_materials.add(VfxEnergyMaterial {
            data: VfxEnergyUniform {
                params0: Vec4::new(vfx_mode::PLASMA_BEAM, 0.0, 1.0, 1.15),
                params1: Vec4::new((size.y / size.x.max(12.0)).max(2.0), 1.0, 0.0, 1.0),
                color_a: col_a,
                color_b: col_b,
            },
        });

        commands
            .entity(entity)
            .insert((
                PointLight2d {
                    color: light_col,
                    intensity: 2.6,
                    radius: 175.0,
                    height: 28.0,
                    falloff: 1.4,
                },
                Transform::from_xyz(rect.center.x, rect.center.y, 4.2),
                Visibility::default(),
            ))
            .with_children(|parent| {
                // Animated GPU plasma energy barrier with crackling electric tendrils!
                parent.spawn((
                    AnimatedVfxEnergy {
                        phase_offset: rect.center.x * 0.01,
                    },
                    Mesh2d(shared_quad.0.clone()),
                    MeshMaterial2d(beam_mat),
                    Transform::from_xyz(0.0, 0.0, 0.0)
                        .with_scale(Vec3::new((size.x * 1.65).max(26.0), size.y, 1.0)),
                    world_layer.clone(),
                ));
                // Heavy armored emitter caps anchoring bottom & top of the lockdown laser!
                if let Some(ref nozzle) = vfx.emitter_nozzle {
                    parent.spawn((
                        Sprite {
                            image: nozzle.clone(),
                            custom_size: Some(Vec2::new((size.x + 12.0).max(32.0), 12.0)),
                            ..default()
                        },
                        Transform::from_xyz(0.0, -rect.half_extents.y + 6.0, 0.3),
                        world_layer.clone(),
                    ));
                    parent.spawn((
                        Sprite {
                            image: nozzle.clone(),
                            flip_y: true,
                            custom_size: Some(Vec2::new((size.x + 12.0).max(32.0), 12.0)),
                            ..default()
                        },
                        Transform::from_xyz(0.0, rect.half_extents.y - 6.0, 0.3),
                        world_layer.clone(),
                    ));
                }
            });
    }
}

fn decorate_added_slopes(
    mut commands: Commands,
    foundry: Res<FoundryArtAssets>,
    added: Query<(Entity, &SlopeSegment), Added<SlopeSegment>>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    for (entity, slope) in &added {
        let delta = slope.end - slope.start;
        let len = delta.length().max(1.0);
        let mid = (slope.start + slope.end) * 0.5;
        let angle = slope.angle_rad();
        let normal = slope.normal();

        let body_sprite = if let Some(ref trim_img) = foundry.tile_trim_top {
            Sprite {
                image: trim_img.clone(),
                custom_size: Some(Vec2::new(len + 4.0, 14.0)),
                image_mode: SpriteImageMode::Tiled {
                    tile_x: true,
                    tile_y: false,
                    stretch_value: 1.0,
                },
                ..default()
            }
        } else {
            Sprite {
                color: Color::srgb(0.15, 0.20, 0.29),
                custom_size: Some(Vec2::new(len + 4.0, 14.0)),
                ..default()
            }
        };

        commands.entity(entity).insert((
            body_sprite,
            Transform::from_xyz(mid.x - normal.x * 7.0, mid.y - normal.y * 7.0, -5.6)
                .with_rotation(Quat::from_rotation_z(angle)),
            world_layer.clone(),
        ));
        commands.spawn((
            Sprite {
                color: Color::linear_rgba(0.25, 2.6, 3.8, 0.72),
                custom_size: Some(Vec2::new(len + 2.0, 2.0)),
                ..default()
            },
            Transform::from_xyz(mid.x - normal.x * 2.0, mid.y - normal.y * 2.0, -4.8)
                .with_rotation(Quat::from_rotation_z(angle)),
            world_layer.clone(),
        ));
    }
}

fn decorate_added_one_ways(
    mut commands: Commands,
    foundry: Res<FoundryArtAssets>,
    added: Query<(Entity, &OneWayPlatform, Option<&MovingPlatform>), Added<OneWayPlatform>>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    for (entity, plat, maybe_moving) in &added {
        let width = plat.half_width * 2.0;
        let is_moving = maybe_moving.is_some();
        let (vis_h, y_shift, sprite) = if is_moving {
            let sp = if let Some(ref img) = foundry.platform_moving {
                Sprite {
                    image: img.clone(),
                    custom_size: Some(Vec2::new(width, 16.0)),
                    image_mode: SpriteImageMode::Tiled {
                        tile_x: true,
                        tile_y: false,
                        stretch_value: 1.0,
                    },
                    ..default()
                }
            } else {
                Sprite {
                    color: Color::srgb(0.16, 0.28, 0.34),
                    custom_size: Some(Vec2::new(width, plat.thickness)),
                    ..default()
                }
            };
            (16.0, (plat.thickness - 16.0) * 0.5, sp)
        } else {
            let sp = if let Some(ref img) = foundry.platform_oneway {
                Sprite {
                    image: img.clone(),
                    custom_size: Some(Vec2::new(width, 12.0)),
                    image_mode: SpriteImageMode::Tiled {
                        tile_x: true,
                        tile_y: false,
                        stretch_value: 1.0,
                    },
                    ..default()
                }
            } else {
                Sprite {
                    color: Color::srgb(0.20, 0.24, 0.34),
                    custom_size: Some(Vec2::new(width, plat.thickness)),
                    ..default()
                }
            };
            (12.0, (plat.thickness - 12.0) * 0.5, sp)
        };
        let top_glow = if is_moving {
            Color::linear_rgba(0.28, 3.2, 4.4, 0.72)
        } else {
            Color::linear_rgba(3.4, 1.55, 0.22, 0.65)
        };

        commands
            .entity(entity)
            .insert((
                Transform::from_xyz(plat.center.x, plat.center.y, -4.8),
                Visibility::default(),
            ))
            .with_children(|parent| {
                // Visual catwalk/shuttle deck anchored flush to top collision surface!
                parent.spawn((
                    sprite,
                    Transform::from_xyz(0.0, y_shift, 0.0),
                    world_layer.clone(),
                ));
                parent.spawn((
                    Sprite {
                        color: top_glow,
                        custom_size: Some(Vec2::new(width, 1.5)),
                        ..default()
                    },
                    Transform::from_xyz(0.0, plat.thickness * 0.5 - 0.5, 0.4),
                    world_layer.clone(),
                ));
                // Bolted iron end-caps anchoring catwalk silhouettes cleanly!
                for side in [-1.0_f32, 1.0_f32] {
                    parent.spawn((
                        Sprite {
                            color: Color::srgb(0.28, 0.33, 0.44),
                            custom_size: Some(Vec2::new(4.0, vis_h * 0.72)),
                            ..default()
                        },
                        Transform::from_xyz(side * (plat.half_width - 2.0), y_shift + 1.0, 0.3),
                        world_layer.clone(),
                    ));
                }
                if is_moving {
                    parent.spawn((
                        PointLight2d {
                            color: LinearRgba::new(0.20, 0.90, 1.0, 1.0),
                            intensity: 1.35,
                            radius: 105.0,
                            height: 18.0,
                            falloff: 1.6,
                        },
                        Transform::from_xyz(0.0, -8.0, 0.5),
                    ));
                }
            });
    }
}

fn decorate_added_boost_pads(
    mut commands: Commands,
    foundry: Res<FoundryArtAssets>,
    added: Query<(Entity, &BoostPad), Added<BoostPad>>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    for (entity, pad) in &added {
        let size = pad.half_extents * 2.0;
        let sp = if let Some(ref img) = foundry.boost_pad {
            Sprite {
                image: img.clone(),
                flip_x: pad.direction.x < 0.0,
                color: Color::linear_rgb(1.4, 1.7, 1.9),
                custom_size: Some(Vec2::new(size.x, 12.0)),
                image_mode: SpriteImageMode::Tiled {
                    tile_x: true,
                    tile_y: false,
                    stretch_value: 1.0,
                },
                ..default()
            }
        } else {
            Sprite {
                color: Color::linear_rgb(0.25, 3.4, 4.6),
                custom_size: Some(Vec2::new(size.x, 4.0)),
                ..default()
            }
        };
        commands.entity(entity).insert((
            sp,
            PointLight2d {
                color: LinearRgba::new(0.18, 0.95, 1.0, 1.0),
                intensity: 2.3,
                radius: 175.0,
                height: 24.0,
                falloff: 1.5,
            },
            Transform::from_xyz(pad.center.x, pad.center.y - pad.half_extents.y + 4.0, -2.5),
            world_layer.clone(),
        ));
    }
}

fn decorate_added_jump_pads(
    mut commands: Commands,
    foundry: Res<FoundryArtAssets>,
    added: Query<(Entity, &JumpPad), Added<JumpPad>>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    for (entity, pad) in &added {
        let size = pad.half_extents * 2.0;
        let sp = if let Some(ref img) = foundry.jump_pad {
            Sprite {
                image: img.clone(),
                color: Color::linear_rgb(1.35, 1.30, 1.15),
                custom_size: Some(Vec2::new(size.x.max(40.0), 14.0)),
                ..default()
            }
        } else {
            Sprite {
                color: Color::srgb(0.22, 0.25, 0.34),
                custom_size: Some(Vec2::new(size.x, size.y.max(8.0))),
                ..default()
            }
        };
        commands
            .entity(entity)
            .insert((
                sp,
                PointLight2d {
                    color: LinearRgba::new(1.0, 0.72, 0.16, 1.0),
                    intensity: 2.2,
                    radius: 150.0,
                    height: 22.0,
                    falloff: 1.5,
                },
                Transform::from_xyz(pad.center.x, pad.center.y - pad.half_extents.y + 7.0, -2.5),
                world_layer.clone(),
            ))
            .with_children(|parent| {
                // Upward neon chevron beacon hover indicator
                parent.spawn((
                    Sprite {
                        color: Color::linear_rgb(0.35, 3.8, 5.2),
                        custom_size: Some(Vec2::new(12.0, 2.5)),
                        ..default()
                    },
                    Transform::from_xyz(0.0, 10.0, 0.3),
                    world_layer.clone(),
                ));
            });
    }
}

fn decorate_added_checkpoint_beacons(
    mut commands: Commands,
    foundry: Res<FoundryArtAssets>,
    vfx: Res<VfxArtAssets>,
    added: Query<(Entity, &CheckpointBeacon), Added<CheckpointBeacon>>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    for (entity, _beacon) in &added {
        commands.entity(entity).with_children(|parent| {
            // Armored steel resonance pylon pedestal
            if let Some(ref cp_img) = foundry.checkpoint {
                parent.spawn((
                    Sprite {
                        image: cp_img.clone(),
                        custom_size: Some(Vec2::new(20.0, 44.0)),
                        ..default()
                    },
                    Transform::from_xyz(0.0, 22.0, 0.0),
                    world_layer.clone(),
                ));
            } else {
                parent.spawn((
                    Sprite {
                        color: Color::srgb(0.22, 0.27, 0.36),
                        custom_size: Some(Vec2::new(12.0, 32.0)),
                        ..default()
                    },
                    Transform::from_xyz(0.0, 16.0, 0.0),
                    world_layer.clone(),
                ));
            }
            // Faceted luminous diamond resonance crystal (amber idle -> electric cyan activated!)
            let crystal_sp = if let Some(ref img) = vfx.beacon_crystal {
                Sprite {
                    image: img.clone(),
                    color: Color::linear_rgb(2.2, 1.1, 0.3),
                    custom_size: Some(Vec2::new(14.0, 20.0)),
                    ..default()
                }
            } else {
                Sprite {
                    color: Color::linear_rgb(2.2, 1.1, 0.3),
                    custom_size: Some(Vec2::new(9.0, 11.0)),
                    ..default()
                }
            };
            parent.spawn((
                CheckpointBeaconVisual { owner: entity },
                crystal_sp,
                PointLight2d {
                    color: LinearRgba::new(1.0, 0.55, 0.18, 1.0),
                    intensity: 1.5,
                    radius: 140.0,
                    height: 24.0,
                    falloff: 1.5,
                },
                Transform::from_xyz(0.0, 40.0, 0.4),
                world_layer.clone(),
            ));
        });
    }
}

fn decorate_added_collectibles(
    mut commands: Commands,
    foundry: Res<FoundryArtAssets>,
    added: Query<(Entity, &CollectibleItem), Added<CollectibleItem>>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    for (entity, item) in &added {
        match item.kind {
            CollectibleKind::EnergyCore => {
                let sp = if let Some(ref img) = foundry.core {
                    Sprite {
                        image: img.clone(),
                        color: Color::linear_rgb(1.4, 2.2, 2.6),
                        custom_size: Some(Vec2::new(14.0, 14.0)),
                        ..default()
                    }
                } else {
                    Sprite {
                        color: Color::linear_rgb(0.35, 3.8, 5.2),
                        custom_size: Some(Vec2::new(9.0, 9.0)),
                        ..default()
                    }
                };
                commands.entity(entity).insert((
                    sp,
                    Transform::from_xyz(item.pos.x, item.pos.y, 1.2),
                    world_layer.clone(),
                ));
            }
            CollectibleKind::SecretRelic => {
                let sp = if let Some(ref img) = foundry.relic {
                    Sprite {
                        image: img.clone(),
                        color: Color::linear_rgb(1.85, 1.65, 1.2),
                        custom_size: Some(Vec2::new(20.0, 20.0)),
                        ..default()
                    }
                } else {
                    Sprite {
                        color: Color::linear_rgb(5.2, 3.6, 0.55),
                        custom_size: Some(Vec2::new(15.0, 15.0)),
                        ..default()
                    }
                };
                commands.entity(entity).insert((
                    sp,
                    PointLight2d {
                        color: LinearRgba::new(1.0, 0.78, 0.22, 1.0),
                        intensity: 2.8,
                        radius: 160.0,
                        height: 24.0,
                        falloff: 1.4,
                    },
                    Transform::from_xyz(item.pos.x, item.pos.y, 1.5),
                    world_layer.clone(),
                ));
            }
            CollectibleKind::HealCanister => {
                let sp = if let Some(ref img) = foundry.heal {
                    Sprite {
                        image: img.clone(),
                        color: Color::linear_rgb(1.3, 1.8, 1.4),
                        custom_size: Some(Vec2::new(16.0, 20.0)),
                        ..default()
                    }
                } else {
                    Sprite {
                        color: Color::linear_rgb(0.35, 4.4, 1.8),
                        custom_size: Some(Vec2::new(12.0, 16.0)),
                        ..default()
                    }
                };
                commands.entity(entity).insert((
                    sp,
                    PointLight2d {
                        color: LinearRgba::new(0.25, 1.0, 0.50, 1.0),
                        intensity: 2.1,
                        radius: 135.0,
                        height: 20.0,
                        falloff: 1.5,
                    },
                    Transform::from_xyz(item.pos.x, item.pos.y, 1.3),
                    world_layer.clone(),
                ));
            }
        }
    }
}

fn decorate_added_breakable_walls(
    mut commands: Commands,
    foundry: Res<FoundryArtAssets>,
    added: Query<(Entity, &BreakableWall), Added<BreakableWall>>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    for (entity, wall) in &added {
        let sp = if let Some(ref img) = foundry.breakable_wall {
            Sprite {
                image: img.clone(),
                color: Color::linear_rgb(1.25, 1.15, 1.05),
                custom_size: Some(wall.size),
                image_mode: SpriteImageMode::Tiled {
                    tile_x: true,
                    tile_y: true,
                    stretch_value: 1.0,
                },
                ..default()
            }
        } else {
            Sprite {
                color: Color::srgb(0.32, 0.20, 0.16),
                custom_size: Some(wall.size),
                ..default()
            }
        };
        commands
            .entity(entity)
            .insert((
                sp,
                PointLight2d {
                    color: LinearRgba::new(1.0, 0.45, 0.14, 1.0),
                    intensity: 1.8,
                    radius: 130.0,
                    height: 20.0,
                    falloff: 1.5,
                },
                world_layer.clone(),
            ))
            .with_children(|parent| {
                // Pulsing HDR molten seam highlights signaling SMASHABLE bulkhead!
                for (y_off, rot) in [(-14.0, 0.45_f32), (0.0, -0.48), (14.0, 0.40)] {
                    parent.spawn((
                        BreakableWallCrackVisual { owner: entity },
                        Sprite {
                            color: Color::linear_rgb(4.6, 1.9, 0.32),
                            custom_size: Some(Vec2::new(wall.size.x * 0.88, 2.5)),
                            ..default()
                        },
                        Transform::from_xyz(0.0, y_off, 0.3)
                            .with_rotation(Quat::from_rotation_z(rot)),
                        world_layer.clone(),
                    ));
                }
            });
    }
}

fn decorate_added_timed_hazard_beams(
    mut commands: Commands,
    vfx: Res<VfxArtAssets>,
    shared_quad: Res<SharedVfxQuadMesh>,
    mut vfx_materials: ResMut<Assets<VfxEnergyMaterial>>,
    added: Query<(Entity, &TimedHazardBeam), Added<TimedHazardBeam>>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    for (entity, beam) in &added {
        let size = beam.half_extents * 2.0;
        let beam_mat = vfx_materials.add(VfxEnergyMaterial {
            data: VfxEnergyUniform {
                params0: Vec4::new(vfx_mode::PLASMA_BEAM, 0.0, 0.0, 0.0),
                params1: Vec4::new((size.y / size.x.max(12.0)).max(2.0), 1.0, 0.0, 1.0),
                color_a: Vec4::new(5.2, 1.2, 0.32, 0.96),
                color_b: Vec4::new(5.8, 4.8, 2.4, 1.0),
            },
        });
        commands.entity(entity).with_children(|parent| {
            // Heavy armored industrial plasma vent emitter anchored flush at the floor!
            let nozzle_sp = if let Some(ref img) = vfx.emitter_nozzle {
                Sprite {
                    image: img.clone(),
                    custom_size: Some(Vec2::new((size.x + 12.0).max(32.0), 12.0)),
                    ..default()
                }
            } else {
                Sprite {
                    color: Color::srgb(0.32, 0.24, 0.30),
                    custom_size: Some(Vec2::new(size.x + 8.0, 8.0)),
                    ..default()
                }
            };
            parent.spawn((
                nozzle_sp,
                Transform::from_xyz(0.0, -beam.half_extents.y + 6.0, 0.4),
                world_layer.clone(),
            ));
            // GPU-shader plasma laser column (Off -> Flickering Dashed Targeting Sightline -> Crackling Active Beam!)
            parent.spawn((
                TimedBeamColumnVisual {
                    owner: entity,
                    size,
                },
                AnimatedVfxEnergy {
                    phase_offset: beam.center.x * 0.017,
                },
                Mesh2d(shared_quad.0.clone()),
                MeshMaterial2d(beam_mat),
                PointLight2d {
                    color: LinearRgba::new(1.0, 0.28, 0.18, 1.0),
                    intensity: 0.0,
                    radius: 150.0,
                    height: 22.0,
                    falloff: 1.4,
                },
                Transform::from_xyz(0.0, 0.0, 0.2)
                    .with_scale(Vec3::new((size.x * 1.75).max(28.0), size.y, 1.0)),
                world_layer.clone(),
            ));
        });
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
    foundry: Res<FoundryArtAssets>,
    added: Query<(Entity, &SpikeHazard), Added<SpikeHazard>>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    for (entity, spike) in &added {
        let size = spike.half_extents * 2.0;
        if let Some(ref img) = foundry.spikes {
            commands.entity(entity).insert((
                Sprite {
                    image: img.clone(),
                    color: Color::linear_rgb(1.45, 1.05, 1.05),
                    custom_size: Some(Vec2::new(size.x, size.y.max(14.0))),
                    image_mode: SpriteImageMode::Tiled {
                        tile_x: true,
                        tile_y: false,
                        stretch_value: 1.0,
                    },
                    ..default()
                },
                Transform::from_xyz(
                    spike.center.x,
                    spike.center.y - spike.half_extents.y + size.y.max(14.0) * 0.5,
                    -3.2,
                ),
                world_layer.clone(),
            ));
        } else {
            commands
                .entity(entity)
                .insert((
                    Sprite {
                        color: Color::srgb(0.30, 0.10, 0.14),
                        custom_size: Some(Vec2::new(size.x, 4.0)),
                        ..default()
                    },
                    Transform::from_xyz(
                        spike.center.x,
                        spike.center.y - spike.half_extents.y + 2.0,
                        -4.0,
                    ),
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
}

fn decorate_added_wall_turrets(
    mut commands: Commands,
    foundry: Res<FoundryArtAssets>,
    added: Query<(Entity, &WallTurret), Added<WallTurret>>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    for (entity, turret) in &added {
        commands.entity(entity).with_children(|parent| {
            if let Some(ref img) = foundry.turret {
                parent.spawn((
                    Sprite {
                        image: img.clone(),
                        flip_x: turret.facing < 0.0,
                        custom_size: Some(Vec2::new(24.0, 24.0)),
                        ..default()
                    },
                    Transform::from_xyz(0.0, 0.0, 1.0),
                    world_layer.clone(),
                ));
            } else {
                parent.spawn((
                    Sprite {
                        color: Color::srgb(0.24, 0.27, 0.36),
                        custom_size: Some(Vec2::new(18.0, 18.0)),
                        ..default()
                    },
                    Transform::from_xyz(0.0, 0.0, 1.0),
                    world_layer.clone(),
                ));
                parent.spawn((
                    Sprite {
                        color: Color::srgb(0.38, 0.42, 0.52),
                        custom_size: Some(Vec2::new(12.0, 6.0)),
                        ..default()
                    },
                    Transform::from_xyz(turret.facing * 9.0, 0.0, 1.2),
                    world_layer.clone(),
                ));
            }
            // Glowing telegraph sensor LED right over optical eye
            parent.spawn((
                TurretTelegraphLed { owner: entity },
                Sprite {
                    color: Color::linear_rgb(3.6, 0.35, 0.45),
                    custom_size: Some(Vec2::new(5.0, 4.0)),
                    ..default()
                },
                Transform::from_xyz(turret.facing * 2.0, 1.5, 1.4),
                world_layer.clone(),
            ));
        });
    }
}

fn decorate_added_enemy_fighters(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    vfx: Res<VfxArtAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut sprite_materials: ResMut<Assets<SpriteLitMaterial>>,
    added: Query<(Entity, &EnemyFighter, &KinematicBody), Added<EnemyFighter>>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);

    for (entity, fighter, body) in &added {
        let is_boss = fighter.archetype == EnemyArchetype::FoundryBoss;
        // Exact measured boot-sole height from each baked sprite sheet so EVERY enemy plants
        // flush on solid decks, slopes, and one-way catwalks with ZERO hovering!
        let (pack, foot_dist) = match fighter.archetype {
            EnemyArchetype::LightStriker => ("rogue", 34.5_f32),
            EnemyArchetype::HeavyBrawler => ("barbarian", 26.0),
            EnemyArchetype::ArcaneZoner => ("mage", 26.5),
            EnemyArchetype::ShadowAssassin => ("assassin", 35.0),
            EnemyArchetype::FoundryBoss => ("boss", 33.5),
        };
        let foot_offset_y = foot_dist - body.half_extents.y;

        if let Some(baked) = load_baked_sprite_handles(
            &asset_server,
            &mut meshes,
            &mut sprite_materials,
            pack,
            "idle",
        ) {
            commands.entity(entity).with_children(|parent| {
                // Ground contact shadow anchoring enemy boots firmly onto every platform!
                let shadow_w = if is_boss {
                    52.0
                } else if fighter.archetype == EnemyArchetype::HeavyBrawler {
                    32.0
                } else {
                    26.0
                };
                let shadow_h = if is_boss { 10.0 } else { 7.0 };
                let enemy_shadow_sp = if let Some(ref img) = vfx.shadow_oval {
                    Sprite {
                        image: img.clone(),
                        color: Color::srgba(1.0, 1.0, 1.0, 0.70),
                        custom_size: Some(Vec2::new(shadow_w, shadow_h)),
                        ..default()
                    }
                } else {
                    Sprite {
                        color: Color::srgba(0.02, 0.03, 0.06, 0.56),
                        custom_size: Some(Vec2::new(shadow_w, if is_boss { 6.0 } else { 4.0 })),
                        ..default()
                    }
                };
                parent.spawn((
                    Name::new(format!("EnemyContactShadow_{}", pack)),
                    GroundContactShadow {
                        owner: entity,
                        base_width: shadow_w,
                    },
                    enemy_shadow_sp,
                    Transform::from_xyz(0.0, -body.half_extents.y + 1.0, 1.0),
                    world_layer.clone(),
                ));
                parent.spawn((
                    Name::new(format!("EnemyVisual_{}", pack)),
                    EnemySpriteVisual {
                        owner: entity,
                        jitter_phase: 0.0,
                        base_y: foot_offset_y,
                        foot_dist,
                        last_state_tag: 0,
                        last_variant: 255,
                        was_grounded: true,
                        squash: Vec2::ONE,
                        lean_rad: 0.0,
                    },
                    baked.animator,
                    Mesh2d(baked.mesh),
                    MeshMaterial2d(baked.material),
                    Transform::from_xyz(0.0, foot_offset_y, if is_boss { 1.8 } else { 1.5 }),
                    world_layer.clone(),
                ));
                if is_boss {
                    // Dramatic fiery rim light making Vulcanus tower with molten presence!
                    parent.spawn((
                        Name::new("BossMoltenRimLight"),
                        PointLight2d {
                            color: LinearRgba::new(1.0, 0.44, 0.14, 1.0),
                            intensity: 2.6,
                            radius: 175.0,
                            height: 36.0,
                            falloff: 1.4,
                        },
                        Transform::from_xyz(0.0, 18.0, 4.0),
                    ));
                }
            });
        } else {
            let fallback_color = match fighter.archetype {
                EnemyArchetype::HeavyBrawler => Color::srgb(0.65, 0.22, 0.25),
                EnemyArchetype::LightStriker => Color::srgb(0.75, 0.34, 0.24),
                EnemyArchetype::ArcaneZoner => Color::srgb(0.58, 0.20, 0.78),
                EnemyArchetype::ShadowAssassin => Color::srgb(0.72, 0.16, 0.48),
                EnemyArchetype::FoundryBoss => Color::linear_rgb(4.0, 1.2, 0.3),
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

        // Attach crisp miniature overhead pixel-art HP bar cleanly above enemy head
        if !is_boss {
            let bar_w = if fighter.archetype == EnemyArchetype::HeavyBrawler {
                32.0
            } else {
                24.0
            };
            let bar_y = body.half_extents.y + 22.0;
            commands.entity(entity).with_children(|parent| {
                parent.spawn((
                    Sprite {
                        color: Color::srgba(0.04, 0.05, 0.08, 0.85),
                        custom_size: Some(Vec2::new(bar_w + 2.0, 5.0)),
                        ..default()
                    },
                    Transform::from_xyz(0.0, bar_y, 7.0),
                    world_layer.clone(),
                ));
                parent.spawn((
                    EnemyHealthBarFill {
                        owner: entity,
                        max_width: bar_w,
                    },
                    Sprite {
                        color: Color::srgb(0.95, 0.26, 0.28),
                        custom_size: Some(Vec2::new(bar_w, 3.0)),
                        ..default()
                    },
                    Transform::from_xyz(0.0, bar_y, 7.1),
                    world_layer.clone(),
                ));
            });
        }
    }
}

fn decorate_added_buster_projectiles(
    mut commands: Commands,
    vfx: Res<VfxArtAssets>,
    added: Query<(Entity, &BusterProjectile), Added<BusterProjectile>>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    for (entity, proj) in &added {
        let flip_x = proj.velocity.x < 0.0;
        if proj.charged {
            let sp = if let Some(ref img) = vfx.buster_charged {
                Sprite {
                    image: img.clone(),
                    flip_x,
                    color: Color::linear_rgb(1.45, 2.2, 2.8),
                    custom_size: Some(Vec2::new(36.0, 22.0)),
                    ..default()
                }
            } else {
                Sprite {
                    color: Color::linear_rgb(0.35, 3.6, 5.2),
                    custom_size: Some(Vec2::new(26.0, 15.0)),
                    ..default()
                }
            };
            commands.entity(entity).insert((
                sp,
                PointLight2d {
                    color: LinearRgba::new(0.22, 0.95, 1.0, 1.0),
                    intensity: 3.2,
                    radius: 195.0,
                    height: 22.0,
                    falloff: 1.35,
                },
                world_layer.clone(),
            ));
        } else {
            let sp = if let Some(ref img) = vfx.buster_pellet {
                Sprite {
                    image: img.clone(),
                    flip_x,
                    color: Color::linear_rgb(1.4, 2.1, 2.6),
                    custom_size: Some(Vec2::new(18.0, 10.0)),
                    ..default()
                }
            } else {
                Sprite {
                    color: Color::linear_rgb(0.6, 3.8, 4.8),
                    custom_size: Some(Vec2::new(10.0, 5.0)),
                    ..default()
                }
            };
            commands.entity(entity).insert((
                sp,
                PointLight2d {
                    color: LinearRgba::new(0.25, 0.9, 1.0, 1.0),
                    intensity: 1.5,
                    radius: 110.0,
                    height: 18.0,
                    falloff: 1.6,
                },
                world_layer.clone(),
            ));
        }
    }
}

fn decorate_added_breaker_waves(
    mut commands: Commands,
    shared_quad: Res<SharedVfxQuadMesh>,
    mut vfx_materials: ResMut<Assets<VfxEnergyMaterial>>,
    added: Query<(Entity, &BreakerWaveProjectile), Added<BreakerWaveProjectile>>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    for (entity, wave) in &added {
        let dir = wave.facing.signum();
        let wave_mat = vfx_materials.add(VfxEnergyMaterial {
            data: VfxEnergyUniform {
                params0: Vec4::new(vfx_mode::CRESCENT_SLASH, 0.0, 0.15, 1.2),
                params1: Vec4::new(1.0, 1.48, 0.0, dir),
                color_a: Vec4::new(0.24, 3.4, 5.0, 0.98),
                color_b: Vec4::new(4.6, 5.4, 5.8, 1.0),
            },
        });
        commands
            .entity(entity)
            .insert((
                PointLight2d {
                    color: LinearRgba::new(0.20, 0.95, 1.0, 1.0),
                    intensity: 3.0,
                    radius: 175.0,
                    height: 22.0,
                    falloff: 1.4,
                },
                Visibility::default(),
            ))
            .with_children(|parent| {
                parent.spawn((
                    AnimatedVfxEnergy { phase_offset: 0.0 },
                    Mesh2d(shared_quad.0.clone()),
                    MeshMaterial2d(wave_mat),
                    Transform::from_xyz(-dir * 8.0, 0.0, 0.1)
                        .with_scale(Vec3::new(48.0, 48.0, 1.0)),
                    world_layer.clone(),
                ));
            });
    }
}

fn decorate_added_turret_projectiles(
    mut commands: Commands,
    vfx: Res<VfxArtAssets>,
    added: Query<(Entity, &TurretProjectile), Added<TurretProjectile>>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    for (entity, proj) in &added {
        let flip_x = proj.velocity.x < 0.0;
        if proj.is_magma_wave {
            let sp = if let Some(ref img) = vfx.magma_wave {
                Sprite {
                    image: img.clone(),
                    flip_x,
                    color: Color::linear_rgb(2.0, 1.5, 1.0),
                    custom_size: Some(Vec2::new(30.0, 22.0)),
                    ..default()
                }
            } else {
                Sprite {
                    color: Color::linear_rgb(5.2, 2.0, 0.32),
                    custom_size: Some(Vec2::new(18.0, 16.0)),
                    ..default()
                }
            };
            commands.entity(entity).insert((
                sp,
                PointLight2d {
                    color: LinearRgba::new(1.0, 0.50, 0.12, 1.0),
                    intensity: 2.6,
                    radius: 155.0,
                    height: 20.0,
                    falloff: 1.4,
                },
                world_layer.clone(),
            ));
        } else if proj.is_arcane {
            let sp = if let Some(ref img) = vfx.arcane_orb {
                Sprite {
                    image: img.clone(),
                    color: Color::linear_rgb(1.9, 1.3, 2.4),
                    custom_size: Some(Vec2::new(18.0, 18.0)),
                    ..default()
                }
            } else {
                Sprite {
                    color: Color::linear_rgb(3.8, 0.55, 5.2),
                    custom_size: Some(Vec2::new(12.0, 12.0)),
                    ..default()
                }
            };
            commands.entity(entity).insert((
                sp,
                PointLight2d {
                    color: LinearRgba::new(0.82, 0.24, 1.0, 1.0),
                    intensity: 2.2,
                    radius: 135.0,
                    height: 20.0,
                    falloff: 1.5,
                },
                world_layer.clone(),
            ));
        } else {
            let sp = if let Some(ref img) = vfx.turret_bolt {
                Sprite {
                    image: img.clone(),
                    color: Color::linear_rgb(2.1, 1.2, 1.4),
                    custom_size: Some(Vec2::new(14.0, 14.0)),
                    ..default()
                }
            } else {
                Sprite {
                    color: Color::linear_rgb(4.6, 0.45, 0.65),
                    custom_size: Some(Vec2::new(9.0, 9.0)),
                    ..default()
                }
            };
            commands.entity(entity).insert((
                sp,
                PointLight2d {
                    color: LinearRgba::new(1.0, 0.2, 0.35, 1.0),
                    intensity: 1.7,
                    radius: 105.0,
                    height: 18.0,
                    falloff: 1.6,
                },
                world_layer.clone(),
            ));
        }
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
            Option<&ShieldState>,
            Option<&CounterStance>,
            Option<&MeteorDiveState>,
            Option<&AirDodgeState>,
            Option<&GroundDodgeState>,
            Option<&SmashChargeState>,
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
    let Ok((
        body,
        attack,
        shield,
        counter,
        dive,
        air_dodge,
        ground_dodge,
        smash_charge,
        hitstop,
        hitstun,
        invuln,
    )) = players.single()
    else {
        return;
    };

    let dt = time.delta_secs();
    let in_hitstop = hitstop.is_some_and(Hitstop::is_active);
    let in_hitstun = hitstun.is_some_and(Hitstun::is_active);
    let in_air_dodge = air_dodge.is_some_and(|a| a.active_timer > 0.0);
    let in_ground_dodge = ground_dodge.is_some_and(GroundDodgeState::is_active);
    let smash_charge_ratio = smash_charge
        .filter(|s| s.is_visibly_charging())
        .map(SmashChargeState::charge_ratio);
    let in_shield = shield.is_some_and(|s| s.active);
    let in_counter = counter.is_some_and(CounterStance::is_active);
    let in_dive = dive.is_some_and(|d| d.active);
    let spin_charge = match body.state {
        MovementState::SpinCharging { charge } => Some(charge),
        _ => None,
    };

    let blink_alpha = if in_air_dodge || in_ground_dodge {
        0.54
    } else if invuln.is_some_and(Invulnerable::is_active) {
        if ((time.elapsed_secs() * 24.0) as u32).is_multiple_of(2) {
            0.35
        } else {
            1.0
        }
    } else {
        1.0
    };

    for mut pivot_tf in &mut pivots {
        if in_hitstun {
            // Unmistakable electric dazed rattle while stunned/hitstunned!
            let rattle = (time.elapsed_secs() * 68.0).sin() * 0.085;
            pivot_tf.rotation = Quat::from_rotation_z(body.visual_angle + rattle);
            pivot_tf.scale = Vec3::new(1.04, 0.96, 1.0);
        } else if let Some(charge) = spin_charge {
            let vib = ((time.elapsed_secs() * (45.0 + charge * 55.0)).sin()) * 0.14;
            pivot_tf.rotation = Quat::from_rotation_z(body.visual_angle - body.facing * 0.28 + vib);
            pivot_tf.scale = Vec3::new(1.0 + charge * 0.12, 0.80 - charge * 0.12, 1.0);
        } else if let Some(ratio) = smash_charge_ratio {
            // Iconic Smash Bros. weapon charge squash + rapid rumble vibration!
            let vib = ((time.elapsed_secs() * (52.0 + ratio * 48.0)).sin()) * (0.04 + ratio * 0.08);
            pivot_tf.rotation = Quat::from_rotation_z(body.visual_angle + vib);
            pivot_tf.scale = Vec3::new(1.0 + ratio * 0.10, 0.96 - ratio * 0.08, 1.0);
        } else {
            pivot_tf.rotation = Quat::from_rotation_z(body.visual_angle);
            pivot_tf.scale = Vec3::ONE;
        }
    }

    for (mut animator, mat_handle) in &mut visuals {
        if let Some(mut mat) = materials.get_mut(&mat_handle.0) {
            if in_hitstun {
                // High-visibility electric yellow-amber / white pulse so Hitstun & Shield Break
                // stun are 100% unmistakable and never feel like dropped inputs!
                let pulse = 0.5 + 0.5 * (time.elapsed_secs() * 34.0).sin();
                mat.data.tint = Vec4::new(
                    2.25 + pulse * 1.35,
                    1.85 + pulse * 1.15,
                    0.45 + pulse * 1.10,
                    1.0,
                );
            } else if let Some(ratio) = smash_charge_ratio {
                // Pulsing golden-amber Smash Charge aura!
                let pulse = 0.5 + 0.5 * (time.elapsed_secs() * (24.0 + ratio * 22.0)).sin();
                mat.data.tint = Vec4::new(
                    1.6 + ratio * 1.4 + pulse * 0.6,
                    1.3 + ratio * 1.0 + pulse * 0.4,
                    0.55 + (1.0 - ratio) * 0.6,
                    1.0,
                );
            } else if in_counter {
                // Radiant Aegis gold-white counter stance aura!
                let pulse = 0.5 + 0.5 * (time.elapsed_secs() * 32.0).sin();
                mat.data.tint = Vec4::new(2.3 + pulse * 0.9, 1.9 + pulse * 0.6, 0.65, 1.0);
            } else if in_dive {
                // Blazing molten meteor-dive tint!
                mat.data.tint = Vec4::new(2.5, 1.35, 0.45, 1.0);
            } else if in_air_dodge || in_ground_dodge {
                mat.data.tint = Vec4::new(0.45, 1.85, 2.8, blink_alpha);
            } else if let Some(charge) = spin_charge {
                let pulse = 0.5 + 0.5 * (time.elapsed_secs() * 34.0).sin();
                mat.data.tint = Vec4::new(
                    1.0 + charge * 1.4 * pulse,
                    1.0 + charge * 1.1,
                    1.2 + charge * 1.8 * (1.0 - pulse),
                    1.0,
                );
            } else {
                // Balanced true-color hero lighting preserving Levy's 1700s steel-navy coat
                // and leather boots without washing them out.
                mat.data.tint = Vec4::new(0.86, 0.88, 0.92, blink_alpha);
            }
            mat.data.fx_params.y = if in_hitstop && in_hitstun {
                0.90
            } else if in_hitstun {
                let pulse = 0.5 + 0.5 * (time.elapsed_secs() * 34.0).sin();
                0.28 + pulse * 0.34
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

        if let Some(gd) = ground_dodge
            && gd.is_active()
        {
            animator.play(gd.animation_name());
        } else if let Some(sc) = smash_charge
            && sc.is_visibly_charging()
            && sc.kind.is_some()
        {
            // Hold Levy's dramatic sky-high raised-saber coiled windup (`telegraph`) during Smash Charge!
            if last_attack.last_move != sc.kind {
                animator.play_restart("telegraph");
                last_attack.last_move = sc.kind;
            } else {
                animator.play("telegraph");
            }
            animator.tick(dt * 0.85);
            continue;
        } else if in_counter || in_dive {
            animator.play("down_special");
        } else if let Some(move_kind) = attack.active_move {
            let flip_override = if move_kind == AttackKind::BackAir {
                !animator.flip_x
            } else {
                animator.flip_x
            };
            animator.flip_x = flip_override;
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

            if in_shield {
                animator.play("guard");
            } else if attack.buster_anim_timer > 0.0 {
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
                    MovementState::SpinCharging { .. } => {
                        animator.play("sprint");
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

#[allow(clippy::type_complexity)]
fn update_buster_charge_aura(
    time: Res<Time>,
    state: Res<State<AppState>>,
    players: Query<&BusterState, With<Player>>,
    mut vfx_materials: ResMut<Assets<VfxEnergyMaterial>>,
    mut auras: Query<(
        &mut BusterChargeAura,
        &MeshMaterial2d<VfxEnergyMaterial>,
        &mut Transform,
        &mut PointLight2d,
    )>,
    mut tracking_glows: Query<&mut Sprite, With<HeroTrackingGlow>>,
) {
    if *state.get() == AppState::Paused {
        return;
    }
    let pulse = 0.5 + 0.5 * (time.elapsed_secs() * 3.2).sin();
    for mut glow in &mut tracking_glows {
        // Subtle, restrained tracking aura so Levy stands out cleanly without blue wash
        glow.color = Color::linear_rgba(0.28, 0.95, 1.35, 0.09 + 0.04 * pulse);
        glow.custom_size = Some(Vec2::new(30.0 + 2.0 * pulse, 38.0 + 2.0 * pulse));
    }
    let Ok(buster) = players.single() else {
        return;
    };

    for (mut aura, mat_handle, mut tf, mut light) in &mut auras {
        aura.pulse += time.delta_secs() * 18.0;
        let ratio = buster.charge_ratio();
        let Some(mut mat) = vfx_materials.get_mut(&mat_handle.0) else {
            continue;
        };
        if buster.charge_secs < 0.18 {
            mat.data.params0.w = 0.0;
            light.intensity = 0.0;
        } else if buster.is_fully_charged() {
            let flick = 0.65 + 0.35 * aura.pulse.sin();
            let sz = 46.0 + 6.0 * flick;
            tf.scale = Vec3::new(sz, sz, 1.0);
            mat.data.params0.z = 1.0;
            mat.data.params0.w = 1.05;
            mat.data.color_a = Vec4::new(0.4 + 2.2 * flick, 3.2, 4.8, 0.88);
            mat.data.color_b = Vec4::new(5.2, 4.8, 2.4, 1.0);
            light.color = LinearRgba::new(0.25 + 0.55 * flick, 0.92, 1.0, 1.0);
            light.intensity = 2.4 + 0.8 * flick;
        } else {
            let flick = 0.5 + 0.5 * aura.pulse.sin();
            let sz = 32.0 + 14.0 * ratio;
            tf.scale = Vec3::new(sz, sz, 1.0);
            mat.data.params0.z = ratio;
            mat.data.params0.w = 0.85 * ratio;
            mat.data.color_a = Vec4::new(0.18, 2.5, 4.2, 0.82);
            mat.data.color_b = Vec4::new(3.8, 5.0, 5.6, 1.0);
            light.color = LinearRgba::new(0.18, 0.85, 1.0, 1.0);
            light.intensity = 1.3 * ratio * (0.7 + 0.3 * flick);
        }
    }
}

fn update_shield_bubble_visual(
    time: Res<Time>,
    players: Query<&ShieldState, With<Player>>,
    mut vfx_materials: ResMut<Assets<VfxEnergyMaterial>>,
    mut bubbles: Query<(&MeshMaterial2d<VfxEnergyMaterial>, &mut Transform), With<ShieldBubbleVisual>>,
) {
    let Ok(shield) = players.single() else {
        return;
    };
    let pulse = 0.90 + 0.10 * (time.elapsed_secs() * 18.0).sin();
    for (mat_handle, mut tf) in &mut bubbles {
        let Some(mut mat) = vfx_materials.get_mut(&mat_handle.0) else {
            continue;
        };
        if shield.active {
            let ratio = shield.health_ratio();
            let size = 30.0 + 26.0 * ratio;
            tf.scale = Vec3::new(size, size * 1.06, 1.0);
            mat.data.params0.z = ratio;
            mat.data.params0.w = 1.05 * pulse;
            if ratio > 0.55 {
                mat.data.color_a = Vec4::new(0.20, 2.4, 4.2, 0.90);
                mat.data.color_b = Vec4::new(3.8, 5.0, 5.6, 1.0);
            } else if ratio > 0.25 {
                mat.data.color_a = Vec4::new(3.6, 1.9, 0.25, 0.92);
                mat.data.color_b = Vec4::new(5.4, 4.4, 1.6, 1.0);
            } else {
                mat.data.color_a = Vec4::new(4.6, 0.42, 0.48, 0.96);
                mat.data.color_b = Vec4::new(5.6, 3.2, 2.4, 1.0);
            }
        } else {
            mat.data.params0.w = 0.0;
        }
    }
}

/// Animates Levy's orbiting electric stun halo and crackling head sparks during `Hitstun` / Shield Break.
#[allow(clippy::type_complexity)]
fn update_player_stun_fx(
    mut commands: Commands,
    time: Res<Time>,
    state: Res<State<AppState>>,
    mut spark_timer: ResMut<StunSparkTimer>,
    players: Query<(&KinematicBody, Option<&Hitstun>), With<Player>>,
    mut halos: Query<&mut Visibility, With<PlayerStunHalo>>,
    mut stars: Query<(&PlayerStunStar, &mut Transform)>,
) {
    if *state.get() == AppState::Paused {
        return;
    }
    let Ok((body, hitstun)) = players.single() else {
        return;
    };
    let stunned = hitstun.is_some_and(Hitstun::is_active);
    let t = time.elapsed_secs();

    for mut vis in &mut halos {
        *vis = if stunned {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }

    if !stunned {
        spark_timer.0 = 0.0;
        return;
    }

    for (star, mut tf) in &mut stars {
        let a = t * 9.5 + star.0;
        tf.translation.x = (a.cos() * 14.0).round();
        tf.translation.y = (a.sin() * 4.5).round();
        tf.rotation = Quat::from_rotation_z(a * 1.8);
    }

    spark_timer.0 -= time.delta_secs();
    if spark_timer.0 <= 0.0 {
        spark_timer.0 = 0.055;
        let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
        let head = body.position + Vec2::new(0.0, 20.0);
        for i in 0..3 {
            let a = t * 17.0 + i as f32 * 2.1;
            let dir = Vec2::new(a.cos(), a.sin());
            commands.spawn((
                HitSpark {
                    velocity: dir * (78.0 + (i as f32) * 24.0) + Vec2::new(0.0, 22.0),
                    lifetime: 0.14,
                    max_lifetime: 0.14,
                },
                Sprite {
                    color: if i % 2 == 0 {
                        Color::linear_rgb(5.5, 4.6, 0.55)
                    } else {
                        Color::linear_rgb(5.2, 5.2, 4.2)
                    },
                    custom_size: Some(Vec2::new(6.0, 2.0)),
                    ..default()
                },
                Transform::from_xyz(head.x + dir.x * 8.0, head.y + dir.y * 5.0, 8.7)
                    .with_rotation(Quat::from_rotation_z(a)),
                world_layer.clone(),
            ));
        }
    }
}

#[allow(clippy::type_complexity)]
fn update_respawn_angel_platform_and_p1_marker(
    time: Res<Time>,
    players: Query<&PlayerRespawnState, With<Player>>,
    mut markers: Query<&mut Transform, (With<PlayerP1Marker>, Without<RespawnAngelPlatformVisual>)>,
    mut platforms: Query<
        (&mut Transform, &mut Visibility),
        (With<RespawnAngelPlatformVisual>, Without<PlayerP1Marker>),
    >,
) {
    let bob = (time.elapsed_secs() * 5.2).sin() * 2.0;
    for mut m_tf in &mut markers {
        m_tf.translation.y = (46.0 + bob).round();
    }

    let Ok(respawn) = players.single() else {
        return;
    };
    for (mut p_tf, mut vis) in &mut platforms {
        if respawn.active {
            *vis = Visibility::Visible;
            p_tf.translation.x = respawn.platform_pos.x.round();
            p_tf.translation.y = respawn.platform_pos.y.round();
        } else {
            *vis = Visibility::Hidden;
        }
    }
}

/// Emits directional Smash Bros. launch smoke trails behind high-speed fighters in `Hitstun`.
#[allow(clippy::type_complexity)]
fn spawn_knockback_smoke_trails(
    mut commands: Commands,
    time: Res<Time>,
    state: Res<State<AppState>>,
    vfx: Res<VfxArtAssets>,
    mut timer: ResMut<SmokeTrailTimer>,
    launched: Query<
        (&KinematicBody, &DamagePercent),
        (With<Hitstun>, Without<Hitstop>),
    >,
) {
    if *state.get() != AppState::Playing {
        return;
    }
    timer.0 -= time.delta_secs();
    if timer.0 > 0.0 {
        return;
    }
    timer.0 = 0.032;

    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);
    for (body, percent) in &launched {
        let spd = body.velocity.length();
        if spd < 210.0 {
            continue;
        }
        let high_pct = percent.0 >= 70.0 || spd >= 340.0;
        let back_dir = -body.velocity.normalize_or(Vec2::X);
        let puff_color = if high_pct {
            Color::linear_rgba(3.4, 1.7, 0.55, 0.82)
        } else {
            Color::srgba(0.84, 0.90, 0.98, 0.72)
        };
        let puff_size = if high_pct {
            Vec2::new(14.0, 14.0)
        } else {
            Vec2::new(11.0, 11.0)
        };
        let sp = if let Some(ref img) = vfx.smoke_puff {
            Sprite {
                image: img.clone(),
                color: puff_color,
                custom_size: Some(puff_size),
                ..default()
            }
        } else {
            Sprite {
                color: puff_color,
                custom_size: Some(puff_size * 0.6),
                ..default()
            }
        };
        commands.spawn((
            HitSpark {
                velocity: back_dir * 35.0 + Vec2::new(0.0, 18.0),
                lifetime: 0.30,
                max_lifetime: 0.30,
            },
            sp,
            Transform::from_xyz(body.position.x.round(), body.position.y.round(), 5.8),
            world_layer.clone(),
        ));
    }
}

/// Spawns sculpted pixel-art dust clouds, speed streaks, rev sparks, and shield/dodge starbursts.
fn spawn_movement_particles(
    mut commands: Commands,
    vfx: Res<VfxArtAssets>,
    mut move_events: MessageReader<MovementEvent>,
    mut shake: ResMut<CameraShake>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);

    for ev in move_events.read() {
        match ev.kind {
            MovementFxKind::Jump | MovementFxKind::Land => {
                let count = if ev.kind == MovementFxKind::Land { 6 } else { 5 };
                for i in 0..count {
                    let frac = (i as f32 / (count - 1).max(1) as f32) * 2.0 - 1.0;
                    let vel = Vec2::new(frac * 92.0, 22.0 + (1.0 - frac.abs()) * 38.0);
                    let sp = if let Some(ref img) = vfx.smoke_puff {
                        Sprite {
                            image: img.clone(),
                            color: Color::srgba(0.80, 0.88, 0.96, 0.74),
                            custom_size: Some(Vec2::new(9.0, 9.0)),
                            ..default()
                        }
                    } else {
                        Sprite {
                            color: Color::srgba(0.75, 0.85, 0.95, 0.72),
                            custom_size: Some(Vec2::new(4.0, 4.0)),
                            ..default()
                        }
                    };
                    commands.spawn((
                        HitSpark {
                            velocity: vel,
                            lifetime: 0.22,
                            max_lifetime: 0.22,
                        },
                        sp,
                        Transform::from_xyz(ev.pos.x + frac * 6.0, ev.pos.y + 3.0, 6.2),
                        world_layer.clone(),
                    ));
                }
            }
            MovementFxKind::WallJump => {
                let base_angle = ev.dir.y.atan2(ev.dir.x);
                for i in 0..6 {
                    let spread = ((i as f32 / 5.0) - 0.5) * 1.3;
                    let a = base_angle + spread;
                    commands.spawn((
                        HitSpark {
                            velocity: Vec2::new(a.cos(), a.sin()) * (110.0 + (i % 3) as f32 * 35.0),
                            lifetime: 0.22,
                            max_lifetime: 0.22,
                        },
                        Sprite {
                            color: Color::linear_rgb(3.6, 1.8, 0.4),
                            custom_size: Some(Vec2::new(4.0, 3.0)),
                            ..default()
                        },
                        Transform::from_xyz(ev.pos.x, ev.pos.y, 6.4)
                            .with_rotation(Quat::from_rotation_z(a)),
                        world_layer.clone(),
                    ));
                }
            }
            MovementFxKind::BoostPad | MovementFxKind::SpinDashRelease => {
                shake.add_impulse(0.26, ev.dir);
                for i in 0..10 {
                    let spread = ((i as f32 / 9.0) - 0.5) * 0.9;
                    let back = (-ev.dir).y.atan2((-ev.dir).x) + spread;
                    let color = if i % 2 == 0 {
                        Color::linear_rgb(0.3, 3.8, 5.0)
                    } else {
                        Color::linear_rgb(4.6, 2.2, 0.35)
                    };
                    commands.spawn((
                        HitSpark {
                            velocity: Vec2::new(back.cos(), back.sin()) * (190.0 + (i % 3) as f32 * 55.0),
                            lifetime: 0.24,
                            max_lifetime: 0.24,
                        },
                        Sprite {
                            color,
                            custom_size: Some(Vec2::new(8.0, 2.0)),
                            ..default()
                        },
                        Transform::from_xyz(ev.pos.x, ev.pos.y, 6.6)
                            .with_rotation(Quat::from_rotation_z(back)),
                        world_layer.clone(),
                    ));
                }
            }
            MovementFxKind::JumpPad => {
                shake.add_impulse(0.24, Vec2::Y);
                for i in 0..10 {
                    let spread = ((i as f32 / 9.0) - 0.5) * 1.1;
                    let a = std::f32::consts::FRAC_PI_2 + spread;
                    commands.spawn((
                        HitSpark {
                            velocity: Vec2::new(a.cos(), a.sin()) * (170.0 + (i % 3) as f32 * 45.0),
                            lifetime: 0.24,
                            max_lifetime: 0.24,
                        },
                        Sprite {
                            color: if i % 2 == 0 {
                                Color::linear_rgb(5.0, 3.4, 0.5)
                            } else {
                                Color::linear_rgb(0.35, 4.0, 5.2)
                            },
                            custom_size: Some(Vec2::new(7.0, 3.0)),
                            ..default()
                        },
                        Transform::from_xyz(ev.pos.x, ev.pos.y, 6.8)
                            .with_rotation(Quat::from_rotation_z(a)),
                        world_layer.clone(),
                    ));
                }
            }
            MovementFxKind::Checkpoint => {
                for i in 0..12 {
                    let a = (i as f32 / 12.0) * std::f32::consts::TAU;
                    commands.spawn((
                        HitSpark {
                            velocity: Vec2::new(a.cos() * 110.0, 55.0 + a.sin() * 110.0),
                            lifetime: 0.34,
                            max_lifetime: 0.34,
                        },
                        Sprite {
                            color: Color::linear_rgb(0.35, 4.4, 5.5),
                            custom_size: Some(Vec2::new(5.0, 5.0)),
                            ..default()
                        },
                        Transform::from_xyz(ev.pos.x, ev.pos.y, 7.2),
                        world_layer.clone(),
                    ));
                }
            }
            MovementFxKind::CollectCore => {
                for i in 0..6 {
                    let a = (i as f32 / 6.0) * std::f32::consts::TAU;
                    commands.spawn((
                        HitSpark {
                            velocity: Vec2::new(a.cos(), a.sin()) * 95.0,
                            lifetime: 0.20,
                            max_lifetime: 0.20,
                        },
                        Sprite {
                            color: Color::linear_rgb(0.45, 4.2, 5.4),
                            custom_size: Some(Vec2::new(4.0, 4.0)),
                            ..default()
                        },
                        Transform::from_xyz(ev.pos.x, ev.pos.y, 7.0),
                        world_layer.clone(),
                    ));
                }
            }
            MovementFxKind::CollectRelic | MovementFxKind::CollectHeal => {
                let is_heal = ev.kind == MovementFxKind::CollectHeal;
                for i in 0..14 {
                    let a = (i as f32 / 14.0) * std::f32::consts::TAU;
                    let col = if is_heal {
                        Color::linear_rgb(0.4, 4.8, 2.2)
                    } else {
                        Color::linear_rgb(5.4, 4.2, 0.75)
                    };
                    commands.spawn((
                        HitSpark {
                            velocity: Vec2::new(a.cos(), a.sin()) * (140.0 + (i % 3) as f32 * 45.0),
                            lifetime: 0.36,
                            max_lifetime: 0.36,
                        },
                        Sprite {
                            color: col,
                            custom_size: Some(Vec2::new(6.0, 4.0)),
                            ..default()
                        },
                        Transform::from_xyz(ev.pos.x, ev.pos.y, 7.4)
                            .with_rotation(Quat::from_rotation_z(a)),
                        world_layer.clone(),
                    ));
                }
            }
            MovementFxKind::WallBreak => {
                shake.add_impulse(0.65, Vec2::X);
                for i in 0..16 {
                    let a = (i as f32 / 16.0) * std::f32::consts::TAU;
                    let spd = 120.0 + (i % 4) as f32 * 65.0;
                    commands.spawn((
                        HitSpark {
                            velocity: Vec2::new(a.cos() * spd, a.sin() * spd + 65.0),
                            lifetime: 0.38,
                            max_lifetime: 0.38,
                        },
                        Sprite {
                            color: if i % 2 == 0 {
                                Color::linear_rgb(5.2, 2.2, 0.35)
                            } else {
                                Color::srgb(0.42, 0.36, 0.32)
                            },
                            custom_size: Some(Vec2::new(6.0, 6.0)),
                            ..default()
                        },
                        Transform::from_xyz(ev.pos.x, ev.pos.y, 7.6),
                        world_layer.clone(),
                    ));
                }
            }
            MovementFxKind::SpinDashRev => {
                for i in 0..4 {
                    let back_x = ev.dir.x.signum();
                    let vel = Vec2::new(
                        back_x * (120.0 + i as f32 * 32.0),
                        35.0 + (i % 2) as f32 * 48.0,
                    );
                    commands.spawn((
                        HitSpark {
                            velocity: vel,
                            lifetime: 0.16,
                            max_lifetime: 0.16,
                        },
                        Sprite {
                            color: if i % 2 == 0 {
                                Color::linear_rgb(0.35, 3.6, 5.0)
                            } else {
                                Color::linear_rgb(4.8, 2.6, 0.4)
                            },
                            custom_size: Some(Vec2::new(5.0, 2.0)),
                            ..default()
                        },
                        Transform::from_xyz(ev.pos.x, ev.pos.y + 3.0, 6.5),
                        world_layer.clone(),
                    ));
                }
            }
            MovementFxKind::ShieldBlock => {
                shake.add_impulse(0.22, ev.dir);
                for i in 0..8 {
                    let a = (i as f32 / 8.0) * std::f32::consts::TAU;
                    commands.spawn((
                        HitSpark {
                            velocity: Vec2::new(a.cos(), a.sin()) * 160.0,
                            lifetime: 0.18,
                            max_lifetime: 0.18,
                        },
                        Sprite {
                            color: Color::linear_rgb(0.4, 3.8, 5.2),
                            custom_size: Some(Vec2::new(4.0, 4.0)),
                            ..default()
                        },
                        Transform::from_xyz(ev.pos.x, ev.pos.y, 7.2),
                        world_layer.clone(),
                    ));
                }
            }
            MovementFxKind::AirDodge | MovementFxKind::RollDodge | MovementFxKind::SpotDodge => {
                for i in 0..8 {
                    let a = (i as f32 / 8.0) * std::f32::consts::TAU;
                    commands.spawn((
                        HitSpark {
                            velocity: Vec2::new(a.cos(), a.sin()) * 105.0,
                            lifetime: 0.22,
                            max_lifetime: 0.22,
                        },
                        Sprite {
                            color: Color::linear_rgba(0.35, 3.2, 5.0, 0.85),
                            custom_size: Some(Vec2::new(4.0, 4.0)),
                            ..default()
                        },
                        Transform::from_xyz(ev.pos.x, ev.pos.y, 6.5),
                        world_layer.clone(),
                    ));
                }
            }
            MovementFxKind::FastFall => {
                // Iconic Smash Bros. white-cyan sparkle star flash on Fast-Fall!
                for i in 0..6 {
                    let a = (i as f32 / 6.0) * std::f32::consts::TAU;
                    commands.spawn((
                        HitSpark {
                            velocity: Vec2::new(a.cos(), a.sin()) * 85.0,
                            lifetime: 0.14,
                            max_lifetime: 0.14,
                        },
                        Sprite {
                            color: Color::linear_rgb(4.6, 5.0, 5.4),
                            custom_size: Some(Vec2::new(6.0, 2.0)),
                            ..default()
                        },
                        Transform::from_xyz(ev.pos.x, ev.pos.y + 8.0, 7.0)
                            .with_rotation(Quat::from_rotation_z(a)),
                        world_layer.clone(),
                    ));
                }
            }
            MovementFxKind::Taunt => {
                for i in 0..8 {
                    let a = (i as f32 / 8.0) * std::f32::consts::TAU;
                    commands.spawn((
                        HitSpark {
                            velocity: Vec2::new(a.cos() * 90.0, 45.0 + a.sin().abs() * 80.0),
                            lifetime: 0.30,
                            max_lifetime: 0.30,
                        },
                        Sprite {
                            color: if i % 2 == 0 {
                                Color::linear_rgb(0.4, 4.2, 5.4)
                            } else {
                                Color::linear_rgb(5.2, 3.8, 0.6)
                            },
                            custom_size: Some(Vec2::new(4.0, 4.0)),
                            ..default()
                        },
                        Transform::from_xyz(ev.pos.x, ev.pos.y + 10.0, 7.2),
                        world_layer.clone(),
                    ));
                }
            }
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
        Option<&BossState>,
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
        let Ok((fighter, body, boss, hitstop, hitstun, invuln)) = enemies.get(vis.owner) else {
            continue;
        };

        let in_hitstop = hitstop.is_some_and(Hitstop::is_active);
        let in_hitstun = hitstun.is_some_and(Hitstun::is_active);
        let telegraph = fighter.is_telegraphing();
        let grounded = body.state == MovementState::Grounded;
        let base_y = vis.base_y;

        // Landing impact squash-and-stretch impulse!
        if grounded && !vis.was_grounded {
            vis.squash = Vec2::new(1.18, 0.82);
        } else if !grounded && vis.was_grounded && body.velocity.y > 120.0 {
            vis.squash = Vec2::new(0.86, 1.16);
        }
        vis.was_grounded = grounded;

        // Target procedural squash & directional body lean so enemies feel weighty, aggressive, and alive!
        let (target_squash, target_lean) = if in_hitstun {
            let tumble = (-body.velocity.x * 0.0008).clamp(-0.32, 0.32);
            (Vec2::new(0.92, 1.08), tumble)
        } else if telegraph {
            // Pull-back anticipation crouch & slight tremble before striking!
            let vib = (time.elapsed_secs() * 52.0).sin() * 0.03;
            (Vec2::new(1.08, 0.90), -body.facing * 0.14 + vib)
        } else if matches!(fighter.state, EnemyAiState::Attacking { .. }) {
            // Forward attack lunge stretch!
            (Vec2::new(1.08, 0.94), body.facing * 0.13)
        } else if !grounded {
            let stretch = (body.velocity.y.abs() * 0.00028).clamp(0.0, 0.10);
            (Vec2::new(1.0 - stretch * 0.7, 1.0 + stretch), body.facing * 0.06)
        } else if body.speed() > 25.0 {
            // Forward running lean into pursuit/patrol!
            (Vec2::ONE, body.facing * 0.08)
        } else {
            (Vec2::ONE, 0.0)
        };

        let blend = (dt * 16.0).min(1.0);
        vis.squash = vis.squash.lerp(target_squash, blend);
        vis.lean_rad += (target_lean - vis.lean_rad) * blend;

        if in_hitstop {
            vis.jitter_phase += dt * 115.0;
            tf.translation.x = (vis.jitter_phase.sin() * 3.0).round();
            tf.translation.y = base_y + ((vis.jitter_phase * 1.6).cos() * 2.0).round();
        } else {
            // Anchor squash cleanly at exact boot-sole pivot so feet never hover or sink!
            let foot_comp = (vis.squash.y - 1.0) * vis.foot_dist;
            tf.translation.x = 0.0;
            tf.translation.y = base_y + foot_comp;
            tf.scale = Vec3::new(vis.squash.x, vis.squash.y, 1.0);
            tf.rotation = Quat::from_rotation_z(vis.lean_rad);
        }

        if let Some(mut mat) = materials.get_mut(&mat_handle.0) {
            let phase2 = boss.is_some_and(|b| b.phase >= 2);
            // Preserve crisp true-color 2085 baked textures (white Strider, yellow Enforcer,
            // chrome-violet Arc Weaver, charcoal-magenta Phantom, gunmetal-molten Vulcanus)!
            let base_enemy_tint = match fighter.archetype {
                EnemyArchetype::FoundryBoss if phase2 => {
                    let rage = 0.5 + 0.5 * (time.elapsed_secs() * 14.0).sin();
                    Vec4::new(1.38 + rage * 0.45, 1.06 + rage * 0.18, 0.88, 1.0)
                }
                _ => Vec4::new(0.86, 0.86, 0.90, 1.0),
            };
            mat.data.fx_params.y = if in_hitstop {
                1.0
            } else if in_hitstun {
                let pulse = 0.5 + 0.5 * (time.elapsed_secs() * 30.0).sin();
                0.20 + pulse * 0.24
            } else {
                0.0
            };
            mat.data.tint = if in_hitstun {
                let pulse = 0.5 + 0.5 * (time.elapsed_secs() * 30.0).sin();
                Vec4::new(2.0 + pulse * 0.8, 1.7 + pulse * 0.7, 0.65 + pulse * 0.6, 1.0)
            } else if telegraph {
                let pulse = 0.5 + 0.5 * (time.elapsed_secs() * 30.0).sin();
                match fighter.archetype {
                    EnemyArchetype::ArcaneZoner => Vec4::new(1.45 + pulse * 0.55, 1.05, 1.75 + pulse * 0.65, 1.0),
                    EnemyArchetype::ShadowAssassin => Vec4::new(1.70 + pulse * 0.55, 0.95, 1.45 + pulse * 0.45, 1.0),
                    EnemyArchetype::FoundryBoss => Vec4::new(1.90 + pulse * 0.75, 1.25 + pulse * 0.35, 0.72, 1.0),
                    _ => Vec4::new(1.75 + pulse * 0.60, 1.18 + pulse * 0.28, 0.78, 1.0),
                }
            } else if invuln.is_some_and(Invulnerable::is_active) {
                let blink = if ((time.elapsed_secs() * 22.0) as u32).is_multiple_of(2) {
                    0.45
                } else {
                    1.0
                };
                Vec4::new(base_enemy_tint.x, base_enemy_tint.y, base_enemy_tint.z, blink)
            } else {
                base_enemy_tint
            };
        }

        if in_hitstop {
            continue;
        }

        animator.flip_x = body.facing < 0.0;
        let state_tag: u8 = if in_hitstun {
            3
        } else {
            match fighter.state {
                EnemyAiState::Attacking { .. } => 2,
                EnemyAiState::Telegraph { .. } => 1,
                _ if !grounded => 4,
                _ => 0,
            }
        };
        let state_changed = state_tag != vis.last_state_tag
            || (state_tag == 2 && fighter.attack_variant != vis.last_variant);
        vis.last_state_tag = state_tag;
        vis.last_variant = fighter.attack_variant;

        if in_hitstun {
            if state_changed {
                animator.play_restart("hitstun");
            } else {
                animator.play("hitstun");
            }
        } else {
            match fighter.state {
                EnemyAiState::Attacking { .. } => {
                    let clip = if fighter.archetype == EnemyArchetype::FoundryBoss {
                        match fighter.attack_variant {
                            0 => "f_smash",
                            1 => "down_special",
                            2 => "neutral_special",
                            6 => "sword_spin",
                            _ => "side_special",
                        }
                    } else {
                        match fighter.attack_variant {
                            1 => "down_special",    // Slag Enforcer seismic stomp
                            2 => "neutral_special", // Arc Weaver plasma orb cast
                            3 => "side_special",    // Phantom Interceptor dash-through blitz
                            4 => "jab_2",           // Strider Drone 2nd rising cross-slash
                            5 | 8 => "dodge_back",  // Strider evade / Phantom smoke backflip
                            6 => "sword_spin",      // Slag Enforcer 360° whirlwind hammer spin
                            7 => "up_tilt",         // Arc Weaver Arcane Pillar cast
                            _ => {
                                if fighter.archetype == EnemyArchetype::HeavyBrawler {
                                    "f_smash"
                                } else if fighter.archetype == EnemyArchetype::ShadowAssassin {
                                    "jab_2"
                                } else {
                                    "jab"
                                }
                            }
                        }
                    };
                    if state_changed {
                        animator.play_restart(clip);
                    } else {
                        animator.play(clip);
                    }
                }
                EnemyAiState::Telegraph { .. } => {
                    if state_changed {
                        animator.play_restart("telegraph");
                    } else {
                        animator.play("telegraph");
                    }
                }
                _ if !grounded => {
                    if body.velocity.y > 20.0 {
                        animator.play("jump");
                    } else {
                        animator.play("fall");
                    }
                }
                EnemyAiState::Patrol | EnemyAiState::Chase if body.speed() > 10.0 => {
                    if body.speed() > 165.0 {
                        animator.play("sprint");
                    } else {
                        animator.play("run");
                    }
                }
                _ => animator.play("idle"),
            }
        }
        animator.tick(dt);
    }
}

fn update_enemy_health_bars(
    enemies: Query<&EnemyHealth>,
    mut bars: Query<(&EnemyHealthBarFill, &mut Sprite, &mut Transform)>,
) {
    for (bar, mut sprite, mut tf) in &mut bars {
        let Ok(hp) = enemies.get(bar.owner) else {
            continue;
        };
        let ratio = hp.ratio();
        let w = (bar.max_width * ratio).max(0.0);
        sprite.custom_size = Some(Vec2::new(w, 3.0));
        tf.translation.x = -0.5 * (bar.max_width - w);
        sprite.color = if ratio > 0.55 {
            Color::srgb(0.95, 0.30, 0.32)
        } else if ratio > 0.25 {
            Color::srgb(1.0, 0.68, 0.20)
        } else {
            Color::linear_rgb(4.2, 1.4, 0.35)
        };
    }
}

/// Drives live visual feedback for Checkpoints, Collectibles, Breakable Walls, and GPU Plasma Hazard Beams!
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn update_stage_feature_visuals(
    time: Res<Time>,
    state: Res<State<AppState>>,
    beacons: Query<&CheckpointBeacon>,
    beams: Query<&TimedHazardBeam>,
    walls: Query<&BreakableWall>,
    mut vfx_materials: ResMut<Assets<VfxEnergyMaterial>>,
    mut beacon_vis: Query<
        (&CheckpointBeaconVisual, &mut Sprite, &mut PointLight2d),
        (Without<TimedBeamColumnVisual>, Without<BreakableWallCrackVisual>),
    >,
    mut beam_vis: Query<
        (
            &TimedBeamColumnVisual,
            &MeshMaterial2d<VfxEnergyMaterial>,
            &mut Transform,
            &mut PointLight2d,
        ),
        (Without<CheckpointBeaconVisual>, Without<BreakableWallCrackVisual>),
    >,
    mut wall_cracks: Query<
        (&BreakableWallCrackVisual, &mut Sprite),
        (Without<CheckpointBeaconVisual>, Without<TimedBeamColumnVisual>),
    >,
    mut collectibles: Query<(&CollectibleItem, &mut Transform), Without<TimedBeamColumnVisual>>,
) {
    if *state.get() == AppState::Paused {
        return;
    }
    let t = time.elapsed_secs();

    // 1. Checkpoint Beacons: warm amber idle -> blazing pulsing electric cyan when activated!
    for (vis, mut sprite, mut light) in &mut beacon_vis {
        let Ok(beacon) = beacons.get(vis.owner) else {
            continue;
        };
        if beacon.activated {
            let pulse = 0.8 + 0.2 * (t * 8.0).sin();
            sprite.color = Color::linear_rgb(0.35 * pulse, 4.2 * pulse, 5.4 * pulse);
            light.color = LinearRgba::new(0.20, 0.92, 1.0, 1.0);
            light.intensity = 2.6 * pulse;
        } else {
            sprite.color = Color::linear_rgb(2.0, 0.95, 0.25);
            light.color = LinearRgba::new(1.0, 0.50, 0.16, 1.0);
            light.intensity = 1.2;
        }
    }

    // 2. Collectibles: silky floating bob & subtle sway
    for (item, mut tf) in &mut collectibles {
        let bob = (t * 4.2 + item.bob_phase).sin() * 3.5;
        tf.translation.x = item.pos.x.round();
        tf.translation.y = (item.pos.y + bob).round();
        if item.kind != CollectibleKind::HealCanister {
            tf.rotation = Quat::from_rotation_z((t * 2.2 + item.bob_phase).sin() * 0.16);
        }
    }

    // 3. Timed Hazard Beams: GPU plasma laser shader (Off -> Dashed Targeting Sightline -> Crackling Active Column!)
    let flash = 0.45 + 0.55 * (t * 34.0).sin();
    for (vis, mat_handle, mut tf, mut light) in &mut beam_vis {
        let Ok(beam) = beams.get(vis.owner) else {
            continue;
        };
        let Some(mut mat) = vfx_materials.get_mut(&mat_handle.0) else {
            continue;
        };
        match beam.phase() {
            HazardBeamPhase::Off => {
                mat.data.params0.w = 0.0;
                light.intensity = 0.0;
            }
            HazardBeamPhase::Telegraph => {
                tf.scale = Vec3::new((vis.size.x * 0.9).max(16.0), vis.size.y, 1.0);
                mat.data.params0.w = 0.95;
                mat.data.params1.z = 1.0; // Telegraph dashed sightline mode!
                mat.data.color_a = Vec4::new(5.4, 3.2, 0.45, 0.92);
                mat.data.color_b = Vec4::new(5.8, 5.2, 3.0, 1.0);
                light.color = LinearRgba::new(1.0, 0.65, 0.18, 1.0);
                light.intensity = 1.4 * flash;
            }
            HazardBeamPhase::Active => {
                let w = (vis.size.x * 1.75).max(28.0) * (0.94 + 0.10 * flash);
                tf.scale = Vec3::new(w, vis.size.y, 1.0);
                mat.data.params0.w = 1.25;
                mat.data.params1.z = 0.0; // Full roaring plasma beam!
                mat.data.color_a = Vec4::new(5.2, 1.15 + 0.4 * flash, 0.30, 0.98);
                mat.data.color_b = Vec4::new(5.8, 5.0, 2.8, 1.0);
                light.color = LinearRgba::new(1.0, 0.30, 0.15, 1.0);
                light.intensity = 2.9;
            }
        }
    }

    // 4. Breakable Walls: fracture cracks glow hotter as HP drops!
    let pulse = 0.7 + 0.3 * (t * 14.0).sin();
    for (vis, mut sprite) in &mut wall_cracks {
        let Ok(wall) = walls.get(vis.owner) else {
            continue;
        };
        let dmg = 1.0 - wall.hp_ratio();
        sprite.color = Color::linear_rgb(
            (4.4 + dmg * 1.4) * pulse,
            (1.6 + dmg * 2.2) * pulse,
            0.3 + dmg * 0.9,
        );
    }
}

/// Animates grounded fighter contact shadows (visible when boots touch deck, hidden airborne)
/// and shimmering molten slag lava horizon + pouring smelter cascades!
#[allow(clippy::type_complexity)]
fn update_ground_contact_shadows_and_molten_fx(
    time: Res<Time>,
    state: Res<State<AppState>>,
    bodies: Query<&KinematicBody>,
    mut shadows: Query<(&GroundContactShadow, &mut Sprite), Without<MoltenFlowVisual>>,
    mut flows: Query<(&MoltenFlowVisual, &mut Transform, &mut Sprite), Without<GroundContactShadow>>,
) {
    if *state.get() == AppState::Paused {
        return;
    }
    let t = time.elapsed_secs();

    for (shadow, mut sprite) in &mut shadows {
        let Ok(body) = bodies.get(shadow.owner) else {
            continue;
        };
        if body.state == MovementState::Grounded {
            let speed_stretch = (body.speed() * 0.018).min(5.0);
            sprite.color.set_alpha(0.72);
            if let Some(ref mut sz) = sprite.custom_size {
                sz.x = shadow.base_width + speed_stretch;
            }
        } else {
            sprite.color.set_alpha(0.0);
        }
    }

    for (flow, mut tf, mut sprite) in &mut flows {
        if flow.is_lava_horizon {
            // Subtle dimmed boiling wave sway along the background abyss (non-blooming)
            let wave_x = (t * 1.4).sin() * 14.0;
            let wave_y = (t * 2.6).cos() * 1.5;
            let glow = 0.92 + 0.08 * (t * 3.8).sin();
            tf.translation.x = (flow.base_pos.x + wave_x).round();
            tf.translation.y = (flow.base_pos.y + wave_y).round();
            sprite.color = Color::srgba(0.66 * glow, 0.48 * glow, 0.35 * glow, 0.90);
        } else {
            // Subtle atmospheric background liquid-metal waterfall shimmer (non-blooming)
            let shimmer = 0.90 + 0.10 * (t * 6.5 + flow.phase).sin();
            let sway = (t * 2.2 + flow.phase).sin() * 1.0;
            tf.translation.x = (flow.base_pos.x + sway).round();
            sprite.color = Color::srgba(0.62 * shimmer, 0.46 * shimmer, 0.34 * shimmer, 0.58);
        }
    }
}

/// Spawns subtle rising molten Foundry ember motes near the active camera for rich atmospheric depth!
fn spawn_ambient_foundry_embers(
    mut commands: Commands,
    time: Res<Time>,
    state: Res<State<AppState>>,
    mut embers: ResMut<AmbientEmberTimer>,
    cameras: Query<&Transform, With<PixelWorldCamera>>,
) {
    if *state.get() != AppState::Playing {
        return;
    }
    embers.timer -= time.delta_secs();
    if embers.timer > 0.0 {
        return;
    }
    embers.timer = 0.18;
    embers.seed = embers.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);

    let Ok(cam_tf) = cameras.single() else {
        return;
    };
    let rx = ((embers.seed & 0xFFFF) as f32 / 65535.0) * 600.0 - 300.0;
    let ry = (((embers.seed >> 16) & 0xFFFF) as f32 / 65535.0) * 90.0 - 175.0;
    let pos = Vec2::new(cam_tf.translation.x + rx, cam_tf.translation.y + ry);
    let color = if embers.seed.is_multiple_of(3) {
        Color::linear_rgba(0.4, 3.2, 4.8, 0.75)
    } else {
        Color::linear_rgba(4.8, 1.9, 0.35, 0.80)
    };
    commands.spawn((
        HitSpark {
            velocity: Vec2::new((rx * 0.08).clamp(-18.0, 18.0), 34.0 + (embers.seed % 26) as f32),
            lifetime: 1.45,
            max_lifetime: 1.45,
        },
        Sprite {
            color,
            custom_size: Some(Vec2::new(2.0, 2.0)),
            ..default()
        },
        Transform::from_xyz(pos.x, pos.y, -2.5),
        RenderLayers::layer(WORLD_RENDER_LAYER),
    ));
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

/// Spawns a dramatic Smash-style GPU plasma KO explosion beam + heavy camera trauma on blast-zone KO!
fn handle_blast_zone_ko_flash(
    mut commands: Commands,
    vfx: Res<VfxArtAssets>,
    shared_quad: Res<SharedVfxQuadMesh>,
    mut vfx_materials: ResMut<Assets<VfxEnergyMaterial>>,
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

        // GPU plasma KO blast column erupting inward from the blast boundary (zero flat rectangles!)
        let beam_origin = ko.ko_pos + inward * 105.0;
        let ko_mat = vfx_materials.add(VfxEnergyMaterial {
            data: VfxEnergyUniform {
                params0: Vec4::new(vfx_mode::PLASMA_BEAM, 0.0, 0.0, 1.4),
                params1: Vec4::new(3.6, 1.0, 0.0, 1.0),
                color_a: Vec4::new(0.45, 3.8, 5.4, 0.98),
                color_b: Vec4::new(5.6, 4.8, 2.2, 1.0),
            },
        });
        commands.spawn((
            TransientShaderVfx {
                velocity: Vec2::ZERO,
                lifetime: 0.45,
                max_lifetime: 0.45,
                start_scale: Vec2::new(82.0, 270.0),
                end_scale: Vec2::new(16.0, 285.0),
                base_intensity: 1.4,
            },
            Mesh2d(shared_quad.0.clone()),
            MeshMaterial2d(ko_mat),
            Transform::from_xyz(beam_origin.x, beam_origin.y, 9.0)
                .with_rotation(Quat::from_rotation_z(angle - std::f32::consts::FRAC_PI_2))
                .with_scale(Vec3::new(82.0, 270.0, 1.0)),
            world_layer.clone(),
        ));

        // Burst of 18 crisp 4-point KO starbursts spraying inward
        for i in 0..18 {
            let spread = ((i as f32 / 17.0) - 0.5) * 1.4;
            let a = angle + spread;
            let spd = 220.0 + (i % 4) as f32 * 75.0;
            let col = if i % 2 == 0 {
                Color::linear_rgb(0.4, 4.2, 5.4)
            } else {
                Color::linear_rgb(5.0, 2.8, 0.5)
            };
            let sp = if let Some(ref img) = vfx.star_spark {
                Sprite {
                    image: img.clone(),
                    color: col,
                    custom_size: Some(Vec2::new(11.0, 11.0)),
                    ..default()
                }
            } else {
                Sprite {
                    color: col,
                    custom_size: Some(Vec2::new(8.0, 4.0)),
                    ..default()
                }
            };
            commands.spawn((
                HitSpark {
                    velocity: Vec2::new(a.cos(), a.sin()) * spd,
                    lifetime: 0.42,
                    max_lifetime: 0.42,
                },
                sp,
                Transform::from_xyz(ko.ko_pos.x, ko.ko_pos.y, 9.4)
                    .with_rotation(Quat::from_rotation_z(a)),
                world_layer.clone(),
            ));
        }
    }
}

/// Spawns GPU-shader curved saber crescents, silky radial shockwave rings, high-voltage plasma
/// eruptions, golden Aegis Counter starbursts, and pixel-art spark bursts!
fn handle_combat_visual_events(
    mut commands: Commands,
    vfx: Res<VfxArtAssets>,
    shared_quad: Res<SharedVfxQuadMesh>,
    mut vfx_materials: ResMut<Assets<VfxEnergyMaterial>>,
    mut vis_events: MessageReader<CombatVisualEvent>,
    mut shake: ResMut<CameraShake>,
) {
    let world_layer = RenderLayers::layer(WORLD_RENDER_LAYER);

    let spawn_ring = |commands: &mut Commands,
                      vfx_materials: &mut Assets<VfxEnergyMaterial>,
                      center: Vec2,
                      max_radius: f32,
                      rgb: [f32; 3]| {
        let diam = (max_radius * 2.15).max(28.0);
        let ring_mat = vfx_materials.add(VfxEnergyMaterial {
            data: VfxEnergyUniform {
                params0: Vec4::new(vfx_mode::SHOCKWAVE_RING, 0.0, 0.0, 1.25),
                params1: Vec4::new(1.0, 1.0, 0.0, 1.0),
                color_a: Vec4::new(rgb[0], rgb[1], rgb[2], 0.96),
                color_b: Vec4::new(
                    (rgb[0] + 2.4).min(5.8),
                    (rgb[1] + 2.4).min(5.8),
                    (rgb[2] + 2.4).min(5.8),
                    1.0,
                ),
            },
        });
        commands.spawn((
            TransientShaderVfx {
                velocity: Vec2::ZERO,
                lifetime: 0.24,
                max_lifetime: 0.24,
                start_scale: Vec2::splat(diam * 0.55),
                end_scale: Vec2::splat(diam),
                base_intensity: 1.25,
            },
            Mesh2d(shared_quad.0.clone()),
            MeshMaterial2d(ring_mat),
            Transform::from_xyz(center.x, center.y, 7.6)
                .with_scale(Vec3::new(diam * 0.55, diam * 0.55, 1.0)),
            world_layer.clone(),
        ));
    };

    for ev in vis_events.read() {
        match *ev {
            CombatVisualEvent::SlashArc {
                pos,
                facing,
                angle_rad,
                radius,
                heavy,
                color_rgb,
            } => {
                // Single silky GPU-shader curved crescent blade smear (zero jagged rectangular bars!)
                let base_dir_angle = if facing < 0.0 {
                    std::f32::consts::PI - angle_rad
                } else {
                    angle_rad
                };
                let lifetime = if heavy { 0.19 } else { 0.14 };
                let diam = (radius * if heavy { 2.35 } else { 2.05 }).max(34.0);
                let sweep_half = if heavy { 1.52 } else { 1.28 };
                let core_rgb = [
                    (color_rgb[0] + 2.8).min(5.8),
                    (color_rgb[1] + 2.8).min(5.8),
                    (color_rgb[2] + 2.8).min(5.8),
                ];
                let slash_mat = vfx_materials.add(VfxEnergyMaterial {
                    data: VfxEnergyUniform {
                        params0: Vec4::new(vfx_mode::CRESCENT_SLASH, 0.0, 0.0, 1.3),
                        params1: Vec4::new(1.0, sweep_half, 0.0, 1.0),
                        color_a: Vec4::new(color_rgb[0], color_rgb[1], color_rgb[2], 0.98),
                        color_b: Vec4::new(core_rgb[0], core_rgb[1], core_rgb[2], 1.0),
                    },
                });
                let dir_vec = Vec2::new(base_dir_angle.cos(), base_dir_angle.sin());
                commands.spawn((
                    TransientShaderVfx {
                        velocity: dir_vec * (if heavy { 52.0 } else { 36.0 }),
                        lifetime,
                        max_lifetime: lifetime,
                        start_scale: Vec2::splat(diam * 0.82),
                        end_scale: Vec2::splat(diam * 1.12),
                        base_intensity: 1.3,
                    },
                    Mesh2d(shared_quad.0.clone()),
                    MeshMaterial2d(slash_mat),
                    Transform::from_xyz(pos.x, pos.y, 7.45)
                        .with_rotation(Quat::from_rotation_z(base_dir_angle))
                        .with_scale(Vec3::new(diam * 0.82, diam * 0.82, 1.0)),
                    world_layer.clone(),
                ));

                // Crisp 4-point starburst sparks peeling off the saber tip!
                let outer_color = Color::linear_rgb(color_rgb[0], color_rgb[1], color_rgb[2]);
                let core_color = Color::linear_rgb(core_rgb[0], core_rgb[1], core_rgb[2]);
                let spark_n = if heavy { 6 } else { 3 };
                for s in 0..spark_n {
                    let frac = (s as f32 / (spark_n - 1).max(1) as f32) * 2.0 - 1.0;
                    let a = base_dir_angle + frac * 0.95;
                    let dir = Vec2::new(a.cos(), a.sin());
                    let p = pos + dir * (radius * 0.65);
                    let col = if s % 2 == 0 { core_color } else { outer_color };
                    let sp = if let Some(ref img) = vfx.star_spark {
                        Sprite {
                            image: img.clone(),
                            color: col,
                            custom_size: Some(Vec2::splat(if heavy { 9.0 } else { 7.0 })),
                            ..default()
                        }
                    } else {
                        Sprite {
                            color: col,
                            custom_size: Some(Vec2::new(if heavy { 6.0 } else { 4.0 }, 2.0)),
                            ..default()
                        }
                    };
                    commands.spawn((
                        HitSpark {
                            velocity: dir * (if heavy { 190.0 } else { 135.0 }),
                            lifetime: 0.16,
                            max_lifetime: 0.16,
                        },
                        sp,
                        Transform::from_xyz(p.x, p.y, 7.6)
                            .with_rotation(Quat::from_rotation_z(a)),
                        world_layer.clone(),
                    ));
                }
            }
            CombatVisualEvent::ShockwaveRing {
                pos,
                max_radius,
                color_rgb,
            } => {
                spawn_ring(&mut commands, &mut vfx_materials, pos, max_radius, color_rgb);
            }
            CombatVisualEvent::CounterTrigger { pos } => {
                // Iconic Smash Bros. golden Aegis Counter shockwave + crisp starbursts!
                shake.add_impulse(0.75, Vec2::Y);
                spawn_ring(&mut commands, &mut vfx_materials, pos, 76.0, [5.2, 4.2, 0.9]);
                for i in 0..12 {
                    let a = (i as f32 / 12.0) * std::f32::consts::TAU;
                    let spd = if i % 2 == 0 { 280.0 } else { 185.0 };
                    let col = if i % 2 == 0 {
                        Color::linear_rgb(5.4, 4.5, 1.2)
                    } else {
                        Color::linear_rgb(0.5, 4.2, 5.4)
                    };
                    let sp = if let Some(ref img) = vfx.star_spark {
                        Sprite {
                            image: img.clone(),
                            color: col,
                            custom_size: Some(Vec2::new(11.0, 11.0)),
                            ..default()
                        }
                    } else {
                        Sprite {
                            color: col,
                            custom_size: Some(Vec2::new(14.0, 3.0)),
                            ..default()
                        }
                    };
                    commands.spawn((
                        HitSpark {
                            velocity: Vec2::new(a.cos(), a.sin()) * spd,
                            lifetime: 0.28,
                            max_lifetime: 0.28,
                        },
                        sp,
                        Transform::from_xyz(pos.x, pos.y, 8.2)
                            .with_rotation(Quat::from_rotation_z(a)),
                        world_layer.clone(),
                    ));
                }
            }
            CombatVisualEvent::GroundSlam { pos } => {
                // Heavy dual-sided seismic impact plumes + sculpted debris clouds & sparks!
                shake.add_impulse(0.72, -Vec2::Y);
                for side in [-1.0_f32, 1.0_f32] {
                    for i in 0..6 {
                        let lift = 0.25 + (i as f32 / 5.0) * 1.05;
                        let vx = side * (120.0 + i as f32 * 36.0);
                        let vy = 70.0 + lift * 140.0;
                        let col = if i % 2 == 0 {
                            Color::linear_rgb(5.0, 2.3, 0.40)
                        } else {
                            Color::linear_rgb(0.45, 3.8, 5.2)
                        };
                        let img = if i % 2 == 0 {
                            vfx.star_spark.as_ref()
                        } else {
                            vfx.smoke_puff.as_ref()
                        };
                        let sp = if let Some(tex) = img {
                            Sprite {
                                image: tex.clone(),
                                color: col,
                                custom_size: Some(Vec2::splat(if i % 2 == 0 { 9.0 } else { 11.0 })),
                                ..default()
                            }
                        } else {
                            Sprite {
                                color: col,
                                custom_size: Some(Vec2::new(7.0, 4.0)),
                                ..default()
                            }
                        };
                        commands.spawn((
                            HitSpark {
                                velocity: Vec2::new(vx, vy),
                                lifetime: 0.30,
                                max_lifetime: 0.30,
                            },
                            sp,
                            Transform::from_xyz(pos.x + side * 8.0, pos.y, 7.8)
                                .with_rotation(Quat::from_rotation_z(vy.atan2(vx))),
                            world_layer.clone(),
                        ));
                    }
                }
            }
            CombatVisualEvent::SideBlitz { pos, facing } => {
                // Electric horizontal Plasma Blitz lightning streaks!
                shake.add_impulse(0.32, Vec2::new(facing, 0.0));
                for i in 0..8 {
                    let y_off = ((i as f32 / 7.0) - 0.5) * 26.0;
                    let col = if i % 2 == 0 {
                        Color::linear_rgb(0.35, 4.2, 5.4)
                    } else {
                        Color::linear_rgb(4.8, 4.2, 1.2)
                    };
                    let sp = if let Some(ref img) = vfx.star_spark {
                        Sprite {
                            image: img.clone(),
                            color: col,
                            custom_size: Some(Vec2::new(14.0, 6.0)),
                            ..default()
                        }
                    } else {
                        Sprite {
                            color: col,
                            custom_size: Some(Vec2::new(14.0, 2.5)),
                            ..default()
                        }
                    };
                    commands.spawn((
                        HitSpark {
                            velocity: Vec2::new(-facing * (150.0 + (i % 3) as f32 * 55.0), y_off * 1.4),
                            lifetime: 0.20,
                            max_lifetime: 0.20,
                        },
                        sp,
                        Transform::from_xyz(pos.x, pos.y + y_off, 7.5),
                        world_layer.clone(),
                    ));
                }
            }
            CombatVisualEvent::EnemyBlink { from, to } => {
                // Arcane phase-warp departure & arrival bursts!
                spawn_ring(&mut commands, &mut vfx_materials, from, 30.0, [3.8, 0.55, 5.4]);
                spawn_ring(&mut commands, &mut vfx_materials, to, 36.0, [4.6, 1.2, 5.6]);
                for anchor in [from, to] {
                    for i in 0..6 {
                        let a = (i as f32 / 6.0) * std::f32::consts::TAU;
                        let sp = if let Some(ref img) = vfx.star_spark {
                            Sprite {
                                image: img.clone(),
                                color: Color::linear_rgb(4.2, 0.85, 5.4),
                                custom_size: Some(Vec2::new(9.0, 9.0)),
                                ..default()
                            }
                        } else {
                            Sprite {
                                color: Color::linear_rgb(4.2, 0.85, 5.4),
                                custom_size: Some(Vec2::new(5.0, 3.0)),
                                ..default()
                            }
                        };
                        commands.spawn((
                            HitSpark {
                                velocity: Vec2::new(a.cos(), a.sin()) * 115.0,
                                lifetime: 0.22,
                                max_lifetime: 0.22,
                            },
                            sp,
                            Transform::from_xyz(anchor.x, anchor.y, 7.7)
                                .with_rotation(Quat::from_rotation_z(a)),
                            world_layer.clone(),
                        ));
                    }
                }
            }
            CombatVisualEvent::BossLaserTelegraph {
                origin,
                target_dir,
                duration,
            } => {
                // GPU-shader dashed amber-gold aiming sightlines + swirling weapon charge vortex before Vulcanus fires!
                let base_angle = target_dir.y.atan2(target_dir.x);
                let beam_len = 290.0;
                for spread in [-0.30_f32, 0.0, 0.30] {
                    let a = base_angle + spread;
                    let dir = Vec2::new(a.cos(), a.sin());
                    let mid = origin + dir * (beam_len * 0.5);
                    let sight_mat = vfx_materials.add(VfxEnergyMaterial {
                        data: VfxEnergyUniform {
                            params0: Vec4::new(vfx_mode::PLASMA_BEAM, 0.0, 0.0, 1.1),
                            params1: Vec4::new(14.0, 1.0, 1.0, 1.0), // Telegraph sightline mode!
                            color_a: Vec4::new(5.4, 3.2, 0.45, 0.95),
                            color_b: Vec4::new(5.8, 5.2, 3.0, 1.0),
                        },
                    });
                    commands.spawn((
                        TransientShaderVfx {
                            velocity: Vec2::ZERO,
                            lifetime: duration,
                            max_lifetime: duration,
                            start_scale: Vec2::new(10.0, beam_len),
                            end_scale: Vec2::new(6.0, beam_len),
                            base_intensity: 1.15,
                        },
                        Mesh2d(shared_quad.0.clone()),
                        MeshMaterial2d(sight_mat),
                        Transform::from_xyz(mid.x, mid.y, 7.9)
                            .with_rotation(Quat::from_rotation_z(a - std::f32::consts::FRAC_PI_2))
                            .with_scale(Vec3::new(10.0, beam_len, 1.0)),
                        world_layer.clone(),
                    ));
                }
                let orb_mat = vfx_materials.add(VfxEnergyMaterial {
                    data: VfxEnergyUniform {
                        params0: Vec4::new(vfx_mode::CHARGE_VORTEX, 0.0, 1.0, 1.3),
                        params1: Vec4::new(1.0, 1.0, 0.0, 1.0),
                        color_a: Vec4::new(5.4, 2.8, 0.45, 0.95),
                        color_b: Vec4::new(5.8, 5.4, 3.4, 1.0),
                    },
                });
                commands.spawn((
                    TransientShaderVfx {
                        velocity: Vec2::ZERO,
                        lifetime: duration,
                        max_lifetime: duration,
                        start_scale: Vec2::splat(18.0),
                        end_scale: Vec2::splat(38.0),
                        base_intensity: 1.35,
                    },
                    Mesh2d(shared_quad.0.clone()),
                    MeshMaterial2d(orb_mat),
                    Transform::from_xyz(origin.x, origin.y, 8.1).with_scale(Vec3::new(18.0, 18.0, 1.0)),
                    world_layer.clone(),
                ));
            }
            CombatVisualEvent::CycloneSpin {
                pos,
                radius,
                color_rgb,
            } => {
                // Iconic 360° Hurricane Sword Spin / Whirlwind Axe multi-ring GPU tornado VFX!
                shake.add_impulse(0.32, Vec2::Y);
                spawn_ring(&mut commands, &mut vfx_materials, pos, radius, color_rgb);
                spawn_ring(
                    &mut commands,
                    &mut vfx_materials,
                    pos + Vec2::new(0.0, 8.0),
                    radius * 0.72,
                    [
                        (color_rgb[0] + 1.8).min(5.6),
                        (color_rgb[1] + 1.8).min(5.6),
                        (color_rgb[2] + 1.8).min(5.6),
                    ],
                );
                // Twin opposed curved crescent spin arcs!
                for side in [0.0_f32, std::f32::consts::PI] {
                    let diam = (radius * 2.1).max(46.0);
                    let mat = vfx_materials.add(VfxEnergyMaterial {
                        data: VfxEnergyUniform {
                            params0: Vec4::new(vfx_mode::CRESCENT_SLASH, 0.0, 0.0, 1.2),
                            params1: Vec4::new(1.0, 1.75, 0.0, 1.0),
                            color_a: Vec4::new(color_rgb[0], color_rgb[1], color_rgb[2], 0.94),
                            color_b: Vec4::new(5.4, 4.8, 2.6, 1.0),
                        },
                    });
                    commands.spawn((
                        TransientShaderVfx {
                            velocity: Vec2::new(0.0, 24.0),
                            lifetime: 0.22,
                            max_lifetime: 0.22,
                            start_scale: Vec2::splat(diam * 0.85),
                            end_scale: Vec2::splat(diam * 1.15),
                            base_intensity: 1.2,
                        },
                        Mesh2d(shared_quad.0.clone()),
                        MeshMaterial2d(mat),
                        Transform::from_xyz(pos.x, pos.y + 4.0, 7.8)
                            .with_rotation(Quat::from_rotation_z(side))
                            .with_scale(Vec3::new(diam * 0.85, diam * 0.85, 1.0)),
                        world_layer.clone(),
                    ));
                }
            }
            CombatVisualEvent::SmashChargeRelease {
                pos,
                facing,
                charge_ratio,
            } => {
                // Golden-white charged Smash detonation burst!
                shake.add_impulse(0.35 + charge_ratio * 0.45, Vec2::new(facing, 0.25));
                spawn_ring(&mut commands, &mut vfx_materials, pos, 36.0 + charge_ratio * 28.0, [5.4, 3.8, 0.7]);
                for i in 0..10 {
                    let a = (i as f32 / 10.0) * std::f32::consts::TAU;
                    let col = if i % 2 == 0 {
                        Color::linear_rgb(5.5, 4.4, 1.0)
                    } else {
                        Color::linear_rgb(0.5, 4.4, 5.5)
                    };
                    let sp = if let Some(ref img) = vfx.star_spark {
                        Sprite {
                            image: img.clone(),
                            color: col,
                            custom_size: Some(Vec2::new(11.0, 11.0)),
                            ..default()
                        }
                    } else {
                        Sprite {
                            color: col,
                            custom_size: Some(Vec2::new(10.0, 3.0)),
                            ..default()
                        }
                    };
                    commands.spawn((
                        HitSpark {
                            velocity: Vec2::new(a.cos(), a.sin()) * (160.0 + charge_ratio * 140.0),
                            lifetime: 0.24,
                            max_lifetime: 0.24,
                        },
                        sp,
                        Transform::from_xyz(pos.x, pos.y, 8.2)
                            .with_rotation(Quat::from_rotation_z(a)),
                        world_layer.clone(),
                    ));
                }
            }
            CombatVisualEvent::ArcanePillarTelegraph { pos, duration } => {
                // Pulsing violet ground rune + GPU dashed vertical targeting sightline before eruption!
                spawn_ring(&mut commands, &mut vfx_materials, pos + Vec2::new(0.0, 4.0), 36.0, [4.2, 0.65, 5.4]);
                let sight_mat = vfx_materials.add(VfxEnergyMaterial {
                    data: VfxEnergyUniform {
                        params0: Vec4::new(vfx_mode::PLASMA_BEAM, 0.0, 0.0, 1.05),
                        params1: Vec4::new(8.0, 1.0, 1.0, 1.0),
                        color_a: Vec4::new(4.2, 0.65, 5.6, 0.90),
                        color_b: Vec4::new(5.6, 3.8, 5.8, 1.0),
                    },
                });
                commands.spawn((
                    TransientShaderVfx {
                        velocity: Vec2::ZERO,
                        lifetime: duration,
                        max_lifetime: duration,
                        start_scale: Vec2::new(16.0, 128.0),
                        end_scale: Vec2::new(10.0, 128.0),
                        base_intensity: 1.05,
                    },
                    Mesh2d(shared_quad.0.clone()),
                    MeshMaterial2d(sight_mat),
                    Transform::from_xyz(pos.x, pos.y + 64.0, 7.7)
                        .with_scale(Vec3::new(16.0, 128.0, 1.0)),
                    world_layer.clone(),
                ));
            }
            CombatVisualEvent::ArcanePillarErupt { pos } => {
                // Tall blazing GPU plasma violet arcane column erupting skyward!
                shake.add_impulse(0.45, Vec2::Y);
                let col_mat = vfx_materials.add(VfxEnergyMaterial {
                    data: VfxEnergyUniform {
                        params0: Vec4::new(vfx_mode::PLASMA_BEAM, 0.0, 0.0, 1.4),
                        params1: Vec4::new(4.0, 1.0, 0.0, 1.0),
                        color_a: Vec4::new(4.4, 0.68, 5.6, 0.98),
                        color_b: Vec4::new(5.6, 4.4, 5.8, 1.0),
                    },
                });
                commands.spawn((
                    TransientShaderVfx {
                        velocity: Vec2::ZERO,
                        lifetime: 0.30,
                        max_lifetime: 0.30,
                        start_scale: Vec2::new(46.0, 136.0),
                        end_scale: Vec2::new(12.0, 144.0),
                        base_intensity: 1.4,
                    },
                    Mesh2d(shared_quad.0.clone()),
                    MeshMaterial2d(col_mat),
                    Transform::from_xyz(pos.x, pos.y + 66.0, 8.3)
                        .with_scale(Vec3::new(46.0, 136.0, 1.0)),
                    world_layer.clone(),
                ));
                for i in 0..10 {
                    let spread_x = ((i as f32 / 9.0) - 0.5) * 80.0;
                    let sp = if let Some(ref img) = vfx.star_spark {
                        Sprite {
                            image: img.clone(),
                            color: Color::linear_rgb(4.6, 1.2, 5.6),
                            custom_size: Some(Vec2::new(9.0, 9.0)),
                            ..default()
                        }
                    } else {
                        Sprite {
                            color: Color::linear_rgb(4.6, 1.2, 5.6),
                            custom_size: Some(Vec2::new(5.0, 5.0)),
                            ..default()
                        }
                    };
                    commands.spawn((
                        HitSpark {
                            velocity: Vec2::new(spread_x, 190.0 + (i % 3) as f32 * 70.0),
                            lifetime: 0.28,
                            max_lifetime: 0.28,
                        },
                        sp,
                        Transform::from_xyz(pos.x + spread_x * 0.2, pos.y + 12.0, 8.6),
                        world_layer.clone(),
                    ));
                }
            }
        }
    }
}

#[allow(clippy::type_complexity)]
fn update_slash_and_shockwave_fx(
    mut commands: Commands,
    time: Res<Time>,
    state: Res<State<AppState>>,
    mut slashes: Query<
        (Entity, &mut SlashArcSeg, &mut Transform, &mut Sprite),
        (Without<ShockwaveRingSeg>, Without<BossSightlineFx>),
    >,
    mut rings: Query<
        (Entity, &mut ShockwaveRingSeg, &mut Transform, &mut Sprite),
        (Without<SlashArcSeg>, Without<BossSightlineFx>),
    >,
    mut sightlines: Query<
        (Entity, &mut BossSightlineFx, &mut Sprite),
        (Without<SlashArcSeg>, Without<ShockwaveRingSeg>),
    >,
) {
    if *state.get() == AppState::Paused {
        return;
    }
    let dt = time.delta_secs();

    for (entity, mut seg, mut tf, mut sprite) in &mut slashes {
        seg.lifetime -= dt;
        if seg.lifetime <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        let frac = (seg.lifetime / seg.max_lifetime).clamp(0.0, 1.0);
        tf.translation.x += seg.velocity.x * dt;
        tf.translation.y += seg.velocity.y * dt;
        let len_scale = 0.75 + (1.0 - frac) * 0.55;
        sprite.custom_size = Some(Vec2::new(seg.base_len * len_scale, (4.5 * frac).max(1.5)));
        sprite.color.set_alpha(frac);
    }

    for (entity, mut ring, mut tf, mut sprite) in &mut rings {
        ring.lifetime -= dt;
        if ring.lifetime <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        let frac = (ring.lifetime / ring.max_lifetime).clamp(0.0, 1.0);
        let progress = 1.0 - frac;
        let radius = ring.max_radius * (0.22 + progress * 0.78);
        let p = ring.center + Vec2::new(ring.angle.cos(), ring.angle.sin()) * radius;
        tf.translation.x = p.x;
        tf.translation.y = p.y;
        sprite.custom_size = Some(Vec2::new((radius * 0.85).max(4.0), (3.5 * frac).max(1.2)));
        sprite.color.set_alpha(frac);
    }

    let blink = 0.55 + 0.45 * (time.elapsed_secs() * 44.0).sin();
    for (entity, mut beam, mut sprite) in &mut sightlines {
        beam.lifetime -= dt;
        if beam.lifetime <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        let progress = 1.0 - (beam.lifetime / beam.max_lifetime).clamp(0.0, 1.0);
        if beam.length > 40.0 {
            // Aiming beam tightens & intensifies right before firing!
            let thick = (4.0 - progress * 2.2).max(1.4);
            sprite.custom_size = Some(Vec2::new(beam.length, thick));
            sprite.color = Color::linear_rgba(5.6, 3.0 + progress * 2.0, 0.55 + progress * 1.4, blink);
        } else {
            let sz = 12.0 + progress * 16.0;
            sprite.custom_size = Some(Vec2::new(sz, sz));
            sprite.color.set_alpha(0.5 + 0.5 * blink);
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

/// Smooth look-ahead camera that tracks progression across all 5 Acts and smoothly locks onto
/// active Lockdown Ambush Arenas and the End-of-Level Boss Chamber.
#[allow(clippy::too_many_arguments)]
fn update_follow_camera(
    time: Res<Time>,
    state: Res<State<AppState>>,
    spawn_point: Res<PlayerSpawnPoint>,
    enc_status: Res<EncounterStatus>,
    mut restarts: MessageReader<StageRestartEvent>,
    mut shake: ResMut<CameraShake>,
    players: Query<&KinematicBody, With<Player>>,
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

    for (mut cam, mut transform) in &mut cameras {
        let target = if let Some(arena_center) = enc_status.camera_lock_center {
            // Frame the locked Ambush Arena / Boss Sanctum tightly so both gates and foes stay visible!
            let player_bias = body.position + Vec2::new(0.0, 22.0);
            arena_center.lerp(player_bias, 0.22)
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

/// Scrolls multi-layer pixel-art Foundry parallax tiles seamlessly around `PixelWorldCamera`.
fn update_parallax_backdrop(
    cameras: Query<&Transform, (With<PixelWorldCamera>, Without<ParallaxLayer>)>,
    mut layers: Query<(&ParallaxLayer, &mut Transform)>,
) {
    let Ok(cam_tf) = cameras.single() else {
        return;
    };
    let cam_x = cam_tf.translation.x;
    let cam_y = cam_tf.translation.y;

    for (layer, mut tf) in &mut layers {
        let scroll_x = (cam_x * layer.factor_x).rem_euclid(layer.tile_width);
        let tile_x = cam_x - scroll_x + (layer.tile_index as f32) * layer.tile_width;
        let tile_y = cam_y - (cam_y * layer.factor_y).clamp(-48.0, 48.0);
        tf.translation.x = tile_x.round();
        tf.translation.y = tile_y.round();
        tf.translation.z = layer.z;
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
