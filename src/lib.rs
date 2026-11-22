//! *VELOCITY // BREAKER* — High-FPS 2D pixel-art action-platformer in Rust + Bevy.

pub mod audio;
pub mod combat;
pub mod enemies;
pub mod input;
pub mod level;
pub mod physics;
pub mod player;
pub mod rendering;
pub mod sprite_manifest;
pub mod state;
pub mod ui;

use bevy::prelude::*;
use bevy_ecs_ldtk::LdtkPlugin;

use audio::GameAudioPlugin;
use combat::CombatPlugin;
use enemies::EnemiesPlugin;
use input::GameInputPlugin;
use level::LevelPlugin;
use physics::PhysicsPlugin;
use player::PlayerPlugin;
use rendering::RenderingPlugin;
use state::GameStatePlugin;
use ui::GameUiPlugin;

/// Deterministic fixed simulation frequency (`60 Hz`) shared across physics and combat.
pub const FIXED_TIMESTEP_HZ: f64 = 60.0;

/// Headless-safe gameplay logic plugin compatible with `MinimalPlugins`.
///
/// Contains NO rendering, window, or texture dependencies so simulation unit tests
/// run deterministically without a GPU.
pub struct CoreLogicPlugin;

impl Plugin for CoreLogicPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_hz(FIXED_TIMESTEP_HZ))
            .add_plugins((
                GameStatePlugin,
                GameInputPlugin,
                PhysicsPlugin,
                CombatPlugin,
                PlayerPlugin,
                EnemiesPlugin,
            ));
    }
}

/// Top-level game plugin combining deterministic core logic, pixel-perfect rendering,
/// native Bevy UI, and LDtk level loading.
pub struct VelocityBreakerPlugin;

impl Plugin for VelocityBreakerPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            CoreLogicPlugin,
            RenderingPlugin,
            GameAudioPlugin,
            GameUiPlugin,
            LdtkPlugin,
            LevelPlugin,
        ));
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::input::InputPlugin;
    use bevy::state::app::StatesPlugin;
    use leafwing_input_manager::prelude::{ActionState, Buttonlike};

    use super::*;
    use crate::input::{
        MenuAction, MenuInput, PlayerAction, default_menu_input_map, default_player_input_map,
    };
    use crate::rendering::{VIRTUAL_HEIGHT, VIRTUAL_WIDTH, compute_integer_viewport};
    use crate::state::{AppState, GameStateCommand};

    fn headless_test_app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin, InputPlugin, CoreLogicPlugin));
        app.finish();
        app.cleanup();
        app.update();
        app
    }

    #[test]
    fn test_state_transitions_title_playing_paused() {
        let mut app = headless_test_app();
        assert_eq!(*app.world().resource::<State<AppState>>().get(), AppState::Title);

        // Title -> Playing
        app.world_mut().write_message(GameStateCommand::StartGame);
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppState>>().get(),
            AppState::Playing
        );

        // Playing -> Paused
        app.world_mut().write_message(GameStateCommand::TogglePause);
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppState>>().get(),
            AppState::Paused
        );

        // Paused -> Playing
        app.world_mut().write_message(GameStateCommand::ResumeGame);
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppState>>().get(),
            AppState::Playing
        );

        // Playing -> Paused -> Title
        app.world_mut().write_message(GameStateCommand::TogglePause);
        app.update();
        app.world_mut()
            .write_message(GameStateCommand::ReturnToTitle);
        app.update();
        assert_eq!(*app.world().resource::<State<AppState>>().get(), AppState::Title);
    }

    #[test]
    fn test_pause_input_toggles_playing_and_paused() {
        let mut app = headless_test_app();

        // Pause press on Title must stay on Title (exercising full InputPlugin -> Leafwing InputMap path)
        KeyCode::Escape.press(app.world_mut());
        app.update();
        {
            let mut query = app
                .world_mut()
                .query_filtered::<&ActionState<MenuAction>, With<MenuInput>>();
            let state = query.single(app.world()).expect("MenuInput entity exists");
            assert!(state.just_pressed(&MenuAction::Pause));
        }
        assert_eq!(*app.world().resource::<State<AppState>>().get(), AppState::Title);

        // Release Escape and enter Playing
        KeyCode::Escape.release(app.world_mut());
        app.world_mut().write_message(GameStateCommand::StartGame);
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppState>>().get(),
            AppState::Playing
        );

        // Freshly press Escape (Pause) in Playing -> Paused
        KeyCode::Escape.press(app.world_mut());
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppState>>().get(),
            AppState::Paused
        );

        // Release then freshly press Escape (Pause / Cancel) in Paused -> Playing
        KeyCode::Escape.release(app.world_mut());
        app.update();
        KeyCode::Escape.press(app.world_mut());
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppState>>().get(),
            AppState::Playing
        );
    }

    #[test]
    fn test_input_maps_cover_all_spec_actions() {
        let player_map = default_player_input_map();
        assert!(player_map.get_dual_axislike(&PlayerAction::Move).is_some());
        for action in [
            PlayerAction::Jump,
            PlayerAction::MeleeAttack,
            PlayerAction::Special,
            PlayerAction::Buster,
            PlayerAction::SpinDash,
            PlayerAction::Pause,
        ] {
            let bindings = player_map
                .get_buttonlike(&action)
                .unwrap_or_else(|| panic!("Missing bindings for {:?}", action));
            assert!(
                bindings.len() >= 2,
                "Expected both gamepad and keyboard bindings for {:?}",
                action
            );
        }

        let menu_map = default_menu_input_map();
        for action in [
            MenuAction::Up,
            MenuAction::Down,
            MenuAction::Confirm,
            MenuAction::Cancel,
            MenuAction::Pause,
        ] {
            assert!(
                menu_map.get_buttonlike(&action).is_some(),
                "Missing bindings for {:?}",
                action
            );
        }
    }

    #[test]
    fn test_pixel_perfect_integer_viewport_scaling() {
        // Exact 1x, 2x, 3x 16:9 resolutions
        let vp1 = compute_integer_viewport(640.0, 360.0, VIRTUAL_WIDTH, VIRTUAL_HEIGHT);
        assert_eq!(vp1.scale, 1);
        assert_eq!((vp1.display_width, vp1.display_height), (640, 360));

        let vp2 = compute_integer_viewport(1280.0, 720.0, VIRTUAL_WIDTH, VIRTUAL_HEIGHT);
        assert_eq!(vp2.scale, 2);
        assert_eq!((vp2.display_width, vp2.display_height), (1280, 720));

        let vp3 = compute_integer_viewport(1920.0, 1080.0, VIRTUAL_WIDTH, VIRTUAL_HEIGHT);
        assert_eq!(vp3.scale, 3);
        assert_eq!((vp3.display_width, vp3.display_height), (1920, 1080));

        // Non-integer window size (1600x900 = 2.5x) must floor cleanly to 2x
        let vp_non_int = compute_integer_viewport(1600.0, 900.0, VIRTUAL_WIDTH, VIRTUAL_HEIGHT);
        assert_eq!(vp_non_int.scale, 2);
        assert_eq!((vp_non_int.display_width, vp_non_int.display_height), (1280, 720));

        // Sub-virtual dimensions must clamp safely to 1x (never zero)
        let vp_tiny = compute_integer_viewport(320.0, 200.0, VIRTUAL_WIDTH, VIRTUAL_HEIGHT);
        assert_eq!(vp_tiny.scale, 1);
    }

    #[test]
    fn test_fixed_update_timestep_is_60hz() {
        let app = headless_test_app();
        let fixed_time = app.world().resource::<Time<Fixed>>();
        let expected = Duration::from_secs_f64(1.0 / FIXED_TIMESTEP_HZ);
        let diff = fixed_time.timestep().abs_diff(expected);
        assert!(
            diff < Duration::from_micros(10),
            "Expected ~60Hz fixed timestep, got {:?}",
            fixed_time.timestep()
        );
    }

    #[test]
    fn test_spec_manifest_ron_roundtrip() {
        use crate::sprite_manifest::SpriteSheetManifest;

        let spec_ron = r#"
SpriteSheetManifest(
    frame_size: (96, 96),
    columns: 8,
    rows: 6,
    animations: {
        "run": AnimationDef(start: 0, end: 7, fps: 16.0, looping: true),
        "f_air": AnimationDef(
            start: 8, end: 13, fps: 18.0, looping: false,
            active_hitboxes: [
                FrameHitbox(frame: 10, offset: (22.0, 4.0), radius: 14.0, damage: 11.0, base_kb: 180.0, kb_scale: 2.8, angle_deg: 35.0),
                FrameHitbox(frame: 11, offset: (24.0, 2.0), radius: 14.0, damage: 11.0, base_kb: 180.0, kb_scale: 2.8, angle_deg: 35.0),
            ],
        ),
    },
)
"#;
        let manifest = SpriteSheetManifest::from_ron_str(spec_ron)
            .expect("SPEC.MD RON schema must deserialize cleanly");
        manifest.validate().expect("SPEC.MD manifest must validate");

        assert_eq!(manifest.frame_size, (96, 96));
        assert_eq!(manifest.columns, 8);
        assert_eq!(manifest.rows, 6);
        assert_eq!(manifest.animations["run"].frame_count(), 8);
        assert!(manifest.animations["run"].active_hitboxes.is_empty());

        let f_air = &manifest.animations["f_air"];
        assert_eq!(f_air.active_hitboxes.len(), 2);
        assert_eq!(f_air.hitboxes_for_frame(10).count(), 1);
        assert_eq!(f_air.hitboxes_for_frame(9).count(), 0);

        // Frame 10 on an 8x6 grid is col=2, row=1
        let uv = manifest.frame_uv_rect(10);
        assert!((uv.x - 2.0 / 8.0).abs() < 1e-5);
        assert!((uv.y - 1.0 / 6.0).abs() < 1e-5);
        assert!((uv.z - 1.0 / 8.0).abs() < 1e-5);
        assert!((uv.w - 1.0 / 6.0).abs() < 1e-5);
    }

    #[test]
    fn test_baked_hero_manifest_and_assets_exist() {
        use std::path::Path;
        use crate::sprite_manifest::SpriteSheetManifest;

        let dir = Path::new("assets/sprites/hero");
        for file in [
            "hero_diffuse.png",
            "hero_normal.png",
            "hero_emission.png",
            "hero.ron",
        ] {
            let path = dir.join(file);
            assert!(
                path.exists(),
                "Expected Phase 1 baked artifact at {}",
                path.display()
            );
            assert!(
                std::fs::metadata(&path).expect("metadata readable").len() > 0,
                "Baked artifact {} must be non-empty",
                path.display()
            );
        }

        let manifest = SpriteSheetManifest::load_from_file(dir.join("hero.ron"))
            .expect("Baked hero.ron must parse and validate");
        assert_eq!(manifest.frame_size, (96, 96));
        for required in [
            "idle",
            "run",
            "sprint",
            "jump",
            "jab",
            "dash_attack",
            "f_air",
            "d_air",
            "up_special",
            "buster",
        ] {
            assert!(
                manifest.animations.contains_key(required),
                "Missing expected baked hero animation '{}'",
                required
            );
        }
        assert!(
            !manifest.animations["f_air"].active_hitboxes.is_empty(),
            "Baked f_air clip must include bone-projected active_hitboxes"
        );
        assert!(
            !manifest.bone_sockets.is_empty(),
            "Baked hero manifest must include projected per-frame bone_sockets"
        );
    }

    #[test]
    fn test_sprite_animator_looping_and_oneshot() {
        use crate::rendering::SpriteSheetAnimator;
        use crate::sprite_manifest::SpriteSheetManifest;

        let manifest = SpriteSheetManifest::load_from_file("assets/sprites/hero/hero.ron")
            .expect("Baked hero.ron loads");
        let run_def = manifest.animations["run"].clone();
        let mut animator = SpriteSheetAnimator::new(manifest.clone(), "run");
        assert_eq!(animator.current_frame, run_def.start);

        // Step through a full loop cycle + 1 frame and verify clean wrap-around
        let dt_per_frame = 1.0 / run_def.fps;
        for _ in 0..run_def.frame_count() {
            animator.tick(dt_per_frame);
        }
        assert_eq!(animator.current_frame, run_def.start);
        assert!(!animator.finished);

        // Switch to non-looping f_air and verify it clamps at end frame with finished=true
        let fair_def = manifest.animations["f_air"].clone();
        animator.play("f_air");
        assert_eq!(animator.current_frame, fair_def.start);
        for _ in 0..(fair_def.frame_count() + 4) {
            animator.tick(1.0 / fair_def.fps);
        }
        assert_eq!(animator.current_frame, fair_def.end);
        assert!(animator.finished);
    }

    #[test]
    fn test_point_light_packing_clamps_to_16() {
        use crate::rendering::sprite_lit::{MAX_POINT_LIGHTS_2D, PointLight2d, pack_point_lights};

        let lights: Vec<(Vec2, PointLight2d)> = (0..24)
            .map(|i| {
                (
                    Vec2::new(i as f32 * 10.0, -5.0),
                    PointLight2d {
                        color: LinearRgba::new(0.5, 0.25, 1.0, 1.0),
                        intensity: 2.0,
                        radius: 140.0,
                        height: 24.0,
                        falloff: 1.5,
                    },
                )
            })
            .collect();

        let (packed, count) = pack_point_lights(lights);
        assert_eq!(count, MAX_POINT_LIGHTS_2D);
        assert_eq!(packed[0].pos_height_radius, Vec4::new(0.0, -5.0, 24.0, 140.0));
        assert_eq!(packed[0].color_falloff, Vec4::new(1.0, 0.5, 2.0, 1.5));
        assert_eq!(
            packed[15].pos_height_radius,
            Vec4::new(150.0, -5.0, 24.0, 140.0)
        );
    }

    // -------------------------------------------------------------------------
    // Phase 2 — Deterministic 60 Hz Headless Physics Verification Suite
    // -------------------------------------------------------------------------

    /// Helper advancing the deterministic 60 Hz `FixedUpdate` schedule by exact single ticks.
    fn step_fixed_ticks(app: &mut App, ticks: usize) {
        let dt = Duration::from_secs_f64(1.0 / FIXED_TIMESTEP_HZ);
        for _ in 0..ticks {
            app.world_mut()
                .resource_mut::<Time<Fixed>>()
                .advance_by(dt);
            app.world_mut().run_schedule(FixedUpdate);
        }
    }

    /// Creates a headless physics test app already in `AppState::Playing` with a
    /// manually-driven player (`ManualPlayerIntent`) positioned at `spawn_pos`.
    fn physics_test_app(spawn_pos: Vec2) -> (App, Entity) {
        use crate::physics::{KinematicBody, ManualPlayerIntent, PlayerIntent};
        use crate::player::Player;

        let mut app = headless_test_app();
        app.world_mut().write_message(GameStateCommand::StartGame);
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppState>>().get(),
            AppState::Playing
        );

        let player_entity = {
            let mut q = app.world_mut().query_filtered::<Entity, With<Player>>();
            q.single(app.world()).expect("Player entity exists")
        };

        app.world_mut()
            .entity_mut(player_entity)
            .insert((
                ManualPlayerIntent,
                PlayerIntent::default(),
                KinematicBody::at(spawn_pos),
                Transform::from_xyz(spawn_pos.x, spawn_pos.y, 0.0),
            ));

        (app, player_entity)
    }

    /// SPEC.MD Phase 2 Required Test 1:
    /// Entering a 360° loop below `min_speed` stays grounded; entering at/above `min_speed`
    /// locks to the spline, traverses 360°, and exits with velocity aligned to exit tangent.
    #[test]
    fn test_loop_entry_requires_min_speed() {
        use crate::physics::{KinematicBody, LoopSpline, MovementState, PlayerIntent, SolidRect};

        let run_case = |initial_speed: f32, boost_held: bool| -> (bool, Vec2, MovementState, Vec2) {
            let (mut app, player) = physics_test_app(Vec2::new(260.0, 18.0));

            // Flat runway at y = 0 spanning x = 0..=1200
            app.world_mut()
                .spawn(SolidRect::from_min_max(Vec2::new(0.0, -32.0), Vec2::new(1200.0, 0.0)));
            // 360° loop resting flush at y = 0, center=(420, 72), radius=72, entry=(376, 0)
            app.world_mut()
                .spawn(LoopSpline::circular_loop(1, Vec2::new(420.0, 72.0), 72.0, 44.0, 420.0));

            // Place player grounded approaching the entry trigger
            {
                let mut body = app
                    .world_mut()
                    .get_mut::<KinematicBody>(player)
                    .expect("player body");
                body.state = MovementState::Grounded;
                body.ground_speed = initial_speed;
                body.velocity = Vec2::new(initial_speed, 0.0);
            }
            {
                let mut intent = app
                    .world_mut()
                    .get_mut::<PlayerIntent>(player)
                    .expect("player intent");
                intent.move_axis = Vec2::new(1.0, 0.0);
                intent.boost_held = boost_held;
            }

            let mut ever_locked = false;
            let mut exit_vel = Vec2::ZERO;
            let mut prev_locked = false;

            for _ in 0..150 {
                step_fixed_ticks(&mut app, 1);
                let body = app.world().get::<KinematicBody>(player).unwrap();
                let is_locked = matches!(body.state, MovementState::LoopLocked { spline_id: 1, .. });
                if is_locked {
                    ever_locked = true;
                }
                if prev_locked && !is_locked {
                    exit_vel = body.velocity;
                    break;
                }
                prev_locked = is_locked;
            }

            let body = app.world().get::<KinematicBody>(player).unwrap();
            (ever_locked, exit_vel, body.state, body.position)
        };

        // Case A: Below min_speed (200 < 420) must NEVER enter LoopLocked and stays Grounded.
        let (locked_slow, _, state_slow, pos_slow) = run_case(200.0, false);
        assert!(
            !locked_slow,
            "Player moving below min_speed must not lock into LoopSpline"
        );
        assert_eq!(state_slow, MovementState::Grounded);
        assert!(
            pos_slow.x > 460.0,
            "Slow player should have walked straight past the loop entrance, got x={}",
            pos_slow.x
        );

        // Case B: Above min_speed (500 >= 420) must lock to spline and exit with exit tangent (+X) velocity.
        let (locked_fast, exit_vel, state_fast, pos_fast) = run_case(500.0, true);
        assert!(
            locked_fast,
            "Player moving >= min_speed must lock into LoopSpline"
        );
        assert_eq!(
            state_fast,
            MovementState::Grounded,
            "Horizontal loop exit should return smoothly to Grounded"
        );
        assert!(
            exit_vel.x > 350.0 && exit_vel.y.abs() < 40.0,
            "Exit velocity should align with +X exit tangent, got {:?}",
            exit_vel
        );
        assert!(
            pos_fast.x >= 455.0,
            "Player should exit past loop exit point, got x={}",
            pos_fast.x
        );
    }

    /// SPEC.MD Phase 2 Required Test 2:
    /// Pressing Down on a one-way platform disables one-way collision and re-enables after `0.15s`.
    #[test]
    fn test_drop_through_platform_timer() {
        use crate::physics::{KinematicBody, MovementState, OneWayPlatform, PlayerIntent};

        // Upper one-way at top y = 100; lower one-way at top y = -40 (140px drop -> ~0.4s fall > 0.15s)
        let (mut app, player) = physics_test_app(Vec2::new(0.0, 118.0));
        app.world_mut()
            .spawn(OneWayPlatform::new(Vec2::new(0.0, 97.0), 160.0, 6.0));
        app.world_mut()
            .spawn(OneWayPlatform::new(Vec2::new(0.0, -43.0), 200.0, 6.0));

        // Let player settle grounded on upper one-way platform
        step_fixed_ticks(&mut app, 4);
        {
            let body = app.world().get::<KinematicBody>(player).unwrap();
            assert_eq!(body.state, MovementState::Grounded);
            assert!(body.on_one_way);
            assert!((body.feet_y() - 100.0).abs() < 0.5);
        }

        // Tap Down for 1 frame -> starts 0.15s drop-through timer and detaches cleanly
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.move_axis = Vec2::new(0.0, -1.0);
            intent.down_just_pressed = true;
        }
        step_fixed_ticks(&mut app, 1);
        {
            let body = app.world().get::<KinematicBody>(player).unwrap();
            assert_eq!(body.state, MovementState::Airborne);
            assert!(
                (body.drop_through_timer - 0.15).abs() < 0.02,
                "Expected drop_through_timer ≈ 0.15s, got {}",
                body.drop_through_timer
            );
            assert!(
                body.feet_y() < 100.0,
                "Player should drop below upper platform surface"
            );
        }

        // Release Down and advance 9 frames (9/60 = 0.15s) -> timer must expire cleanly
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.move_axis = Vec2::ZERO;
            intent.down_just_pressed = false;
        }
        step_fixed_ticks(&mut app, 9);
        {
            let body = app.world().get::<KinematicBody>(player).unwrap();
            assert!(
                body.drop_through_timer <= 1e-4,
                "Drop-through timer must expire after 0.15s, remaining={}",
                body.drop_through_timer
            );
            assert!(
                body.feet_y() < 80.0,
                "Player should have cleared upper platform"
            );
        }

        // Continue falling -> one-way collision is re-enabled and catches player on lower platform (y = -40)
        step_fixed_ticks(&mut app, 45);
        {
            let body = app.world().get::<KinematicBody>(player).unwrap();
            assert_eq!(
                body.state,
                MovementState::Grounded,
                "One-way collision must re-enable after 0.15s and catch player on lower platform"
            );
            assert!(body.on_one_way);
            assert!(
                (body.feet_y() - (-40.0)).abs() < 1.0,
                "Expected landing on lower platform y=-40, got feet_y={}",
                body.feet_y()
            );
        }
    }

    /// SPEC.MD Phase 2 Required Test 3:
    /// Jump triggers cleanly within `0.09s` coyote time of leaving a ledge (and fails after expiry);
    /// releasing Jump early cuts upward velocity by 50%.
    #[test]
    fn test_coyote_time_and_variable_jump() {
        use crate::physics::{KinematicBody, MovementState, PhysicsConfig, PlayerIntent, SolidRect};

        // --- Part 1: Coyote jump succeeds within 0.09s of walking off ledge ---
        let (mut app, player) = physics_test_app(Vec2::new(40.0, 18.0));
        app.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-100.0, -32.0), Vec2::new(50.0, 0.0)));

        // Settle grounded near ledge edge, then move right until walking off ledge
        step_fixed_ticks(&mut app, 2);
        {
            let mut body = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
            body.ground_speed = 240.0;
            body.velocity.x = 240.0;
        }
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.move_axis = Vec2::new(1.0, 0.0);
        }

        // Step until player walks off the ledge into Airborne
        let mut left_ledge = false;
        for _ in 0..20 {
            step_fixed_ticks(&mut app, 1);
            if app.world().get::<KinematicBody>(player).unwrap().state == MovementState::Airborne {
                left_ledge = true;
                break;
            }
        }
        assert!(left_ledge, "Player must walk off ledge");

        // Wait 3 frames (~0.05s < 0.09s coyote window), then press Jump
        step_fixed_ticks(&mut app, 3);
        let coyote_left = app
            .world()
            .get::<KinematicBody>(player)
            .unwrap()
            .coyote_timer;
        assert!(
            coyote_left > 0.0,
            "Coyote timer must remain active within 0.09s, got {}",
            coyote_left
        );

        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.jump_just_pressed = true;
            intent.jump_held = true;
        }
        step_fixed_ticks(&mut app, 1);

        let expected_jump_vy = app.world().resource::<PhysicsConfig>().jump_velocity;
        let gravity = app.world().resource::<PhysicsConfig>().gravity;
        let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;
        let vy_after_jump = app.world().get::<KinematicBody>(player).unwrap().velocity.y;
        assert!(
            vy_after_jump > expected_jump_vy * 0.85,
            "Coyote jump must fire full jump velocity (~{}), got {}",
            expected_jump_vy - gravity * dt,
            vy_after_jump
        );

        // --- Part 2: Variable jump cut — releasing Jump early cuts upward velocity by 50% ---
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.jump_just_pressed = false;
            intent.jump_held = false;
            intent.jump_just_released = true;
        }
        step_fixed_ticks(&mut app, 1);
        let vy_after_cut = app.world().get::<KinematicBody>(player).unwrap().velocity.y;
        let expected_cut_vy = vy_after_jump * 0.5 - gravity * dt;
        assert!(
            (vy_after_cut - expected_cut_vy).abs() < 2.0,
            "Early jump release must cut upward velocity by 50%: expected ~{}, got {}",
            expected_cut_vy,
            vy_after_cut
        );

        // --- Part 3: After > 0.09s off ledge, coyote jump must NOT trigger ---
        let (mut app2, player2) = physics_test_app(Vec2::new(62.0, 18.0));
        {
            let mut body = app2.world_mut().get_mut::<KinematicBody>(player2).unwrap();
            body.state = MovementState::Airborne;
            body.coyote_timer = 0.09;
        }
        // Advance 7 frames (0.117s > 0.09s)
        step_fixed_ticks(&mut app2, 7);
        let vy_before = app2.world().get::<KinematicBody>(player2).unwrap().velocity.y;
        {
            let mut intent = app2.world_mut().get_mut::<PlayerIntent>(player2).unwrap();
            intent.jump_just_pressed = true;
            intent.jump_held = true;
        }
        step_fixed_ticks(&mut app2, 1);
        let vy_after = app2.world().get::<KinematicBody>(player2).unwrap().velocity.y;
        assert!(
            vy_after < vy_before,
            "Expired coyote time must NOT trigger an air jump (before={}, after={})",
            vy_before,
            vy_after
        );
    }

    #[test]
    fn test_slope_alignment_and_gravity() {
        use crate::physics::{KinematicBody, MovementState, SlopeSegment};

        // 30° downhill slope going down to the right
        let (mut app, player) = physics_test_app(Vec2::new(20.0, 18.0));
        let slope = SlopeSegment::new(Vec2::new(0.0, 0.0), Vec2::new(200.0, -115.47));
        let expected_normal = slope.normal();
        app.world_mut().spawn(slope);

        {
            let mut body = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
            body.state = MovementState::Grounded;
            body.ground_speed = 0.0;
        }

        // With zero input on a 30° downhill slope, slope gravity a = -g * sin(theta) must accelerate right
        step_fixed_ticks(&mut app, 15);
        let body = app.world().get::<KinematicBody>(player).unwrap();
        assert_eq!(body.state, MovementState::Grounded);
        assert!(
            (body.ground_normal - expected_normal).length() < 0.02,
            "Player must align ground_normal to slope ({:?} vs {:?})",
            body.ground_normal,
            expected_normal
        );
        assert!(
            body.ground_speed > 90.0,
            "Downhill slope gravity must accelerate player without input, got ground_speed={}",
            body.ground_speed
        );
        assert!(
            body.visual_angle < -0.4,
            "Visual angle must tilt downhill, got {}",
            body.visual_angle
        );
    }

    #[test]
    fn test_wall_slide_and_wall_jump() {
        use crate::physics::{KinematicBody, MovementState, PhysicsConfig, PlayerIntent, SolidRect};

        let (mut app, player) = physics_test_app(Vec2::new(38.0, 120.0));
        // Vertical wall on the right (inner face at x = 50.0)
        app.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(50.0, 0.0), Vec2::new(80.0, 260.0)));

        // Fall while pressing right into the wall -> enters WallSliding and clamps fall speed
        {
            let mut body = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
            body.velocity = Vec2::new(40.0, -300.0);
        }
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.move_axis = Vec2::new(1.0, 0.0);
        }
        step_fixed_ticks(&mut app, 4);

        let max_slide = app.world().resource::<PhysicsConfig>().wall_slide_max_fall;
        {
            let body = app.world().get::<KinematicBody>(player).unwrap();
            assert_eq!(body.state, MovementState::WallSliding { wall_dir: 1 });
            assert!(
                body.velocity.y >= -max_slide - 1.0,
                "Wall-slide must clamp fall speed to -{}, got {}",
                max_slide,
                body.velocity.y
            );
        }

        // Press Jump -> crisp wall-jump launching left (-X) and upward (+Y)
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.jump_just_pressed = true;
            intent.jump_held = true;
        }
        step_fixed_ticks(&mut app, 1);
        {
            let body = app.world().get::<KinematicBody>(player).unwrap();
            assert_eq!(body.state, MovementState::Airborne);
            assert!(
                body.velocity.x < -200.0 && body.velocity.y > 350.0,
                "Wall-jump must kick away and up, got {:?}",
                body.velocity
            );
        }
    }

    #[test]
    fn test_jump_buffer_fires_on_landing() {
        use crate::physics::{KinematicBody, MovementState, PlayerIntent, SolidRect};

        let (mut app, player) = physics_test_app(Vec2::new(0.0, 32.0));
        app.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-100.0, -32.0), Vec2::new(100.0, 0.0)));

        // Falling toward floor, press Jump 3 frames before touchdown (0.05s < 0.10s buffer)
        {
            let mut body = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
            body.velocity = Vec2::new(0.0, -240.0);
        }
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.jump_just_pressed = true;
            intent.jump_held = true;
        }
        step_fixed_ticks(&mut app, 1);
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.jump_just_pressed = false;
        }
        step_fixed_ticks(&mut app, 5);

        let body = app.world().get::<KinematicBody>(player).unwrap();
        assert_eq!(body.state, MovementState::Airborne);
        assert!(
            body.velocity.y > 300.0,
            "Buffered jump must trigger automatically on touchdown, got vy={}",
            body.velocity.y
        );
    }

    #[test]
    fn test_loop_stall_detaches_cleanly() {
        use crate::physics::{KinematicBody, LoopSpline, MovementState};

        let (mut app, player) = physics_test_app(Vec2::new(420.0, 140.0));
        app.world_mut()
            .spawn(LoopSpline::circular_loop(7, Vec2::new(420.0, 72.0), 72.0, 44.0, 420.0));

        // Force body into upper apex of the loop below stall speed (100 < 155)
        {
            let mut body = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
            body.state = MovementState::LoopLocked {
                spline_id: 7,
                progress: 0.50,
                speed: 100.0,
            };
        }

        step_fixed_ticks(&mut app, 1);
        let body = app.world().get::<KinematicBody>(player).unwrap();
        assert_eq!(
            body.state,
            MovementState::Airborne,
            "Stalling near loop apex must detach cleanly into Airborne"
        );
        assert!(
            body.loop_exit_cooldown > 0.0,
            "Stall detachment must set loop_exit_cooldown"
        );
    }

    #[test]
    fn test_single_wall_jump_climb() {
        use crate::physics::{KinematicBody, PlayerIntent, SolidRect};

        let (mut app, player) = physics_test_app(Vec2::new(39.0, 40.0));
        // Single tall vertical wall on the right (inner face x = 50.0)
        app.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(50.0, 0.0), Vec2::new(80.0, 400.0)));

        // Hold right into the single wall and chain 3 consecutive Mega Man X wall-jumps
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.move_axis = Vec2::new(1.0, 0.0);
        }

        let start_y: f32 = 40.0;
        let mut wall_jumps = 0usize;
        let mut peak_y: f32 = start_y;
        for _ in 0..110 {
            let can_wall_kick = {
                let b = app.world().get::<KinematicBody>(player).unwrap();
                b.wall_coyote_timer > 0.0 && b.velocity.y <= 140.0
            };
            {
                let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
                intent.jump_just_pressed = can_wall_kick && wall_jumps < 3;
                intent.jump_held = true;
            }
            if can_wall_kick && wall_jumps < 3 {
                wall_jumps += 1;
            }
            step_fixed_ticks(&mut app, 1);
            let cur_y = app.world().get::<KinematicBody>(player).unwrap().position.y;
            peak_y = peak_y.max(cur_y);
        }

        assert_eq!(
            wall_jumps, 3,
            "Player must cleanly return to the same wall for 3 chained wall-jumps"
        );
        assert!(
            peak_y > start_y + 180.0,
            "Single-wall jump climb must gain substantial height (start={}, peak={})",
            start_y,
            peak_y
        );
    }

    // -------------------------------------------------------------------------
    // Phase 3 — Deterministic 60 Hz Headless Combat & Mega Man Buster Tests
    // -------------------------------------------------------------------------

    /// Helper spawning a grounded enemy hurtbox target for combat simulation tests.
    fn spawn_test_target(app: &mut App, pos: Vec2, initial_percent: f32) -> Entity {
        use crate::combat::{DamagePercent, Hurtbox, Team};
        use crate::physics::{KinematicBody, MovementState};

        app.world_mut()
            .spawn((
                Team::Enemy,
                Hurtbox {
                    offset: Vec2::ZERO,
                    radius: 18.0,
                },
                DamagePercent(initial_percent),
                KinematicBody {
                    position: pos,
                    state: MovementState::Grounded,
                    ..default()
                },
                Transform::from_xyz(pos.x, pos.y, 0.0),
            ))
            .id()
    }

    /// SPEC.MD Phase 3 Required Test 1:
    /// Identical hit at sprint speed yields strictly higher knockback than stationary hit.
    #[test]
    fn test_momentum_scales_knockback() {
        use crate::combat::{DamagePercent, LastHitTelemetry, compute_smash_knockback};
        use crate::physics::{KinematicBody, MovementState, PlayerIntent, SolidRect};

        // Direct formula sanity check matching SPEC.MD Section 4.2
        let stationary_formula = compute_smash_knockback(180.0, 50.0, 2.8, 0.0, 0.62);
        let sprint_formula = compute_smash_knockback(180.0, 50.0, 2.8, 500.0, 0.62);
        assert!(
            sprint_formula > stationary_formula + 250.0,
            "Momentum formula must strictly boost sprint knockback ({} vs {})",
            sprint_formula,
            stationary_formula
        );

        // Full headless ECS verification: identical airborne F-Air hit at 0 px/s vs 500 px/s
        let run_f_air_hit = |attacker_vx: f32| -> (f32, f32) {
            let (mut app, player) = physics_test_app(Vec2::new(0.0, 60.0));
            app.world_mut()
                .spawn(SolidRect::from_min_max(Vec2::new(-200.0, -32.0), Vec2::new(400.0, 0.0)));
            let target = spawn_test_target(&mut app, Vec2::new(34.0, 60.0), 40.0);

            {
                let mut body = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
                body.state = MovementState::Airborne;
                body.velocity = Vec2::new(attacker_vx, 0.0);
                body.facing = 1.0;
            }
            {
                let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
                intent.melee_just_pressed = true;
            }

            step_fixed_ticks(&mut app, 1);

            let kb_reported = app.world().resource::<LastHitTelemetry>().last_knockback;
            let target_vel = app
                .world()
                .get::<KinematicBody>(target)
                .unwrap()
                .velocity
                .length();
            let pct_after = app.world().get::<DamagePercent>(target).unwrap().0;
            assert!(pct_after > 40.0, "Target must take F-Air damage");
            (kb_reported, target_vel)
        };

        let (kb_stationary, vel_stationary) = run_f_air_hit(0.0);
        let (kb_sprint, vel_sprint) = run_f_air_hit(500.0);

        assert!(
            kb_sprint > kb_stationary + 200.0,
            "Sprint F-Air knockback ({}) must strictly exceed stationary F-Air ({})",
            kb_sprint,
            kb_stationary
        );
        assert!(
            vel_sprint > vel_stationary + 200.0,
            "Sprint launch velocity ({}) must strictly exceed stationary launch velocity ({})",
            vel_sprint,
            vel_stationary
        );
    }

    /// SPEC.MD Phase 3 Required Test 2:
    /// Entities with active `Hitstop` do not integrate position until timer expires.
    #[test]
    fn test_hitstop_pauses_physics() {
        use crate::combat::Hitstop;
        use crate::physics::{KinematicBody, MovementState};

        let (mut app, player) = physics_test_app(Vec2::new(10.0, 120.0));
        {
            let mut body = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
            body.state = MovementState::Airborne;
            body.velocity = Vec2::new(360.0, 120.0);
        }
        // Apply 5 frames of impact freeze (5 / 60 s)
        app.world_mut()
            .entity_mut(player)
            .insert(Hitstop::from_frames(5));

        let start_pos = app.world().get::<KinematicBody>(player).unwrap().position;

        // Every single tick during the 5-frame Hitstop must leave position 100% frozen
        for tick in 1..=5 {
            step_fixed_ticks(&mut app, 1);
            let cur_pos = app.world().get::<KinematicBody>(player).unwrap().position;
            assert_eq!(
                cur_pos, start_pos,
                "Entity must NOT integrate position on Hitstop tick {}",
                tick
            );
        }

        // Hitstop component must now be removed and the very next tick must resume integration!
        assert!(
            app.world().get::<Hitstop>(player).is_none(),
            "Expired Hitstop should be cleared after 5 ticks"
        );
        step_fixed_ticks(&mut app, 1);
        let resumed_pos = app.world().get::<KinematicBody>(player).unwrap().position;
        assert!(
            resumed_pos.x > start_pos.x + 4.0,
            "Physics integration must resume immediately after Hitstop expires (start={:?}, resumed={:?})",
            start_pos,
            resumed_pos
        );
    }

    /// SPEC.MD Phase 3 Required Test 3:
    /// Max 3 uncharged pellets active simultaneously; holding `>= 0.7s` fires charged variant.
    #[test]
    fn test_buster_cap_and_charge() {
        use crate::combat::{
            BUSTER_CHARGE_TIME_SECS, BusterProjectile, MAX_UNCHARGED_BUSTER_PELLETS,
        };
        use crate::physics::{KinematicBody, MovementState, PlayerIntent, SolidRect};

        let (mut app, player) = physics_test_app(Vec2::new(0.0, 18.0));
        app.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-200.0, -32.0), Vec2::new(800.0, 0.0)));
        {
            let mut body = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
            body.state = MovementState::Grounded;
            body.facing = 1.0;
        }

        // Fire 5 rapid taps spaced past the 0.09s fire cooldown (6 ticks each) while pellets are still alive
        for _ in 0..5 {
            {
                let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
                intent.buster_just_pressed = true;
                intent.buster_held = false;
            }
            step_fixed_ticks(&mut app, 1);
            {
                let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
                intent.buster_just_pressed = false;
            }
            step_fixed_ticks(&mut app, 6);
        }

        let uncharged_count = {
            let mut q = app.world_mut().query::<&BusterProjectile>();
            q.iter(app.world()).filter(|p| !p.charged).count()
        };
        assert_eq!(
            uncharged_count, MAX_UNCHARGED_BUSTER_PELLETS,
            "Uncharged Buster pellets must cap strictly at 3 active on screen"
        );

        // Hold Buster for >= 0.70s (45 ticks = 0.75s) and release -> must spawn Charged variant!
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.buster_just_pressed = false;
            intent.buster_held = true;
        }
        let charge_ticks = (BUSTER_CHARGE_TIME_SECS * FIXED_TIMESTEP_HZ as f32).ceil() as usize + 3;
        step_fixed_ticks(&mut app, charge_ticks);

        // Release Buster button
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.buster_held = false;
        }
        step_fixed_ticks(&mut app, 1);

        let charged_count = {
            let mut q = app.world_mut().query::<&BusterProjectile>();
            q.iter(app.world()).filter(|p| p.charged).count()
        };
        assert_eq!(
            charged_count, 1,
            "Releasing Buster after >= 0.7s hold must fire a Charged Shot"
        );
    }

    #[test]
    fn test_up_special_air_brake_cancels_sonic_momentum() {
        use crate::physics::{KinematicBody, MovementState, PlayerIntent};

        let (mut app, player) = physics_test_app(Vec2::new(0.0, 100.0));
        {
            let mut body = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
            body.state = MovementState::Airborne;
            body.velocity = Vec2::new(500.0, -40.0);
        }
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.special_just_pressed = true;
        }
        step_fixed_ticks(&mut app, 1);

        let body = app.world().get::<KinematicBody>(player).unwrap();
        assert!(
            body.velocity.x.abs() < 120.0,
            "Up-Special must act as an instant air-brake canceling Sonic horizontal momentum, got vx={}",
            body.velocity.x
        );
        assert!(
            body.velocity.y > 400.0,
            "Up-Special must deliver a strong vertical recovery burst, got vy={}",
            body.velocity.y
        );
    }

    #[test]
    fn test_down_air_meteor_spike_and_recoil() {
        use crate::physics::{KinematicBody, MovementState, PlayerIntent};

        let (mut app, player) = physics_test_app(Vec2::new(0.0, 100.0));
        let target = spawn_test_target(&mut app, Vec2::new(6.0, 76.0), 30.0);
        {
            let mut body = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
            body.state = MovementState::Airborne;
            body.velocity = Vec2::new(0.0, -50.0);
            body.facing = 1.0;
        }
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.move_axis = Vec2::new(0.0, -1.0);
            intent.melee_just_pressed = true;
        }
        step_fixed_ticks(&mut app, 1);

        let target_vy = app.world().get::<KinematicBody>(target).unwrap().velocity.y;
        let attacker_vy = app.world().get::<KinematicBody>(player).unwrap().velocity.y;
        assert!(
            target_vy < -200.0,
            "D-Air must meteor-spike target sharply downward, got target_vy={}",
            target_vy
        );
        assert!(
            attacker_vy >= 240.0,
            "D-Air contact must grant attacker upward recoil bounce, got attacker_vy={}",
            attacker_vy
        );
    }

    #[test]
    fn test_charged_buster_pierces_multiple_targets() {
        use crate::combat::{BUSTER_CHARGE_TIME_SECS, DamagePercent};
        use crate::physics::{KinematicBody, MovementState, PlayerIntent, SolidRect};

        let (mut app, player) = physics_test_app(Vec2::new(0.0, 18.0));
        app.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-100.0, -32.0), Vec2::new(500.0, 0.0)));
        let target_a = spawn_test_target(&mut app, Vec2::new(60.0, 18.0), 0.0);
        let target_b = spawn_test_target(&mut app, Vec2::new(115.0, 18.0), 0.0);

        {
            let mut body = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
            body.state = MovementState::Grounded;
            body.facing = 1.0;
        }

        // Charge for >= 0.7s then release
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.buster_held = true;
        }
        let charge_ticks = (BUSTER_CHARGE_TIME_SECS * FIXED_TIMESTEP_HZ as f32).ceil() as usize + 2;
        step_fixed_ticks(&mut app, charge_ticks);
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.buster_held = false;
        }

        // Step enough ticks for Charged Shot to travel past both targets
        step_fixed_ticks(&mut app, 24);

        let pct_a = app.world().get::<DamagePercent>(target_a).unwrap().0;
        let pct_b = app.world().get::<DamagePercent>(target_b).unwrap().0;
        assert!(
            pct_a >= 15.0 && pct_b >= 15.0,
            "Piercing Charged Shot must damage both lined-up targets (pct_a={}, pct_b={})",
            pct_a,
            pct_b
        );
    }

    #[test]
    fn test_invulnerable_target_ignores_hitbox() {
        use crate::combat::{DamagePercent, Invulnerable};
        use crate::physics::{KinematicBody, MovementState, PlayerIntent};

        let (mut app, player) = physics_test_app(Vec2::new(0.0, 60.0));
        let target = spawn_test_target(&mut app, Vec2::new(26.0, 60.0), 25.0);
        app.world_mut()
            .entity_mut(target)
            .insert(Invulnerable::new(0.5));

        {
            let mut body = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
            body.state = MovementState::Grounded;
            body.facing = 1.0;
        }
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.melee_just_pressed = true;
        }
        step_fixed_ticks(&mut app, 2);

        let pct = app.world().get::<DamagePercent>(target).unwrap().0;
        assert_eq!(
            pct, 25.0,
            "Invulnerable target must take zero damage from overlapping hitboxes"
        );
    }

    // -------------------------------------------------------------------------
    // Phase 4 — "The Foundry Gym" LDtk Stage, Hazards, AI, KOs & Audio Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_demo_gym_ldtk_loads_all_zones() {
        use std::collections::HashMap;
        use bevy::asset::{AssetPlugin, LoadState};
        use bevy_ecs_ldtk::assets::{LdtkAssetPlugin, LdtkProject};
        use crate::level::ldtk_rect_to_bevy_min_max;

        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            LdtkAssetPlugin,
        ));
        app.finish();
        app.cleanup();

        let handle: Handle<LdtkProject> = app
            .world()
            .resource::<AssetServer>()
            .load("levels/demo_gym.ldtk");

        let mut loaded = false;
        for _ in 0..400 {
            app.update();
            match app
                .world()
                .resource::<AssetServer>()
                .get_load_state(&handle)
            {
                Some(LoadState::Loaded) => {
                    loaded = true;
                    break;
                }
                Some(LoadState::Failed(err)) => {
                    panic!("Failed to load assets/levels/demo_gym.ldtk via LdtkProjectLoader: {err:?}");
                }
                _ => std::thread::sleep(Duration::from_millis(5)),
            }
        }
        assert!(loaded, "Timed out waiting for assets/levels/demo_gym.ldtk to load");

        let project = app
            .world()
            .resource::<Assets<LdtkProject>>()
            .get(&handle)
            .expect("LdtkProject asset present");

        let level = project
            .json_data()
            .levels
            .first()
            .expect("Level index 0 (TheFoundryGym) must exist");
        assert_eq!(level.identifier, "TheFoundryGym");

        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut wall_left_min_y: Option<f32> = None;

        for layer in level
            .layer_instances
            .as_ref()
            .expect("Internal level must embed layerInstances")
        {
            for ent in &layer.entity_instances {
                *counts.entry(ent.identifier.clone()).or_insert(0) += 1;
                if ent.identifier == "SolidBlock" {
                    let (min, max) = ldtk_rect_to_bevy_min_max(ent);
                    if (min.x - 800.0).abs() < 2.0 && (max.x - 828.0).abs() < 2.0 {
                        wall_left_min_y = Some(min.y);
                    }
                }
            }
        }

        assert_eq!(counts.get("PlayerSpawn").copied().unwrap_or(0), 1);
        assert_eq!(counts.get("Slope").copied().unwrap_or(0), 2);
        assert_eq!(counts.get("BoostPad").copied().unwrap_or(0), 2);
        assert_eq!(counts.get("LoopSpline").copied().unwrap_or(0), 1);
        assert_eq!(counts.get("MovingPlatform").copied().unwrap_or(0), 2);
        assert_eq!(counts.get("SpikeHazard").copied().unwrap_or(0), 2);
        assert_eq!(counts.get("WallTurret").copied().unwrap_or(0), 2);
        assert!(counts.get("OneWayPlatform").copied().unwrap_or(0) >= 4);
        assert_eq!(counts.get("EnemySpawn").copied().unwrap_or(0), 2);
        assert_eq!(counts.get("BlastZone").copied().unwrap_or(0), 1);

        // Verify Zone B left shaft wall leaves an open doorway from Zone A launch ramp
        let doorway_y = wall_left_min_y.expect("Zone B left shaft wall found");
        assert!(
            doorway_y >= 20.0,
            "Zone B left wall must start above doorway headroom (>= 20.0), got {}",
            doorway_y
        );
    }

    #[test]
    fn test_baked_enemy_sprites_and_sfx_assets_exist() {
        use std::path::Path;
        use crate::sprite_manifest::SpriteSheetManifest;

        for pack in ["rogue", "barbarian"] {
            let dir = Path::new("assets/sprites").join(pack);
            for suffix in ["_diffuse.png", "_normal.png", "_emission.png", ".ron"] {
                let p = dir.join(format!("{pack}{suffix}"));
                assert!(p.exists(), "Expected baked enemy asset at {}", p.display());
                assert!(
                    std::fs::metadata(&p).expect("readable").len() > 0,
                    "Baked asset {} must be non-empty",
                    p.display()
                );
            }
            let manifest = SpriteSheetManifest::load_from_file(dir.join(format!("{pack}.ron")))
                .unwrap_or_else(|e| panic!("Baked {pack}.ron failed to load: {e}"));
            assert!(manifest.animations.contains_key("idle"));
            assert!(manifest.animations.contains_key("run"));
            assert!(manifest.animations.contains_key("jab"));
            assert!(manifest.animations.contains_key("hitstun"));
        }

        for sfx in [
            "hit_light.wav",
            "hit_heavy.wav",
            "buster_pellet.wav",
            "buster_charged.wav",
            "ko_blast.wav",
            "boost_pad.wav",
            "hazard_hit.wav",
            "jump.wav",
        ] {
            let p = Path::new("assets/audio/sfx").join(sfx);
            assert!(p.exists(), "Expected SFX asset at {}", p.display());
            assert!(
                std::fs::metadata(&p).expect("readable").len() > 44,
                "SFX WAV {} must contain audio frames",
                p.display()
            );
        }
    }

    #[test]
    fn test_moving_platform_carries_grounded_player() {
        use crate::physics::{KinematicBody, MovementState, MovingPlatform, OneWayPlatform};

        let (mut app, player) = physics_test_app(Vec2::new(100.0, 21.0));
        let plat_entity = app
            .world_mut()
            .spawn((
                OneWayPlatform::new(Vec2::new(100.0, 0.0), 64.0, 6.0),
                MovingPlatform::new(Vec2::new(100.0, 0.0), Vec2::new(40.0, 30.0), 2.5),
                Transform::from_xyz(100.0, 0.0, 0.0),
            ))
            .id();

        // Settle player grounded on top of moving platform
        step_fixed_ticks(&mut app, 4);
        let start_pos = app.world().get::<KinematicBody>(player).unwrap().position;
        assert_eq!(
            app.world().get::<KinematicBody>(player).unwrap().state,
            MovementState::Grounded
        );

        // Advance 30 ticks (~0.5s) -> platform moves substantially right & up, carrying player
        step_fixed_ticks(&mut app, 30);
        let end_pos = app.world().get::<KinematicBody>(player).unwrap().position;
        let plat_center = app.world().get::<OneWayPlatform>(plat_entity).unwrap().center;
        let plat_tf = app.world().get::<Transform>(plat_entity).unwrap().translation;

        assert!(
            end_pos.x > start_pos.x + 15.0 && end_pos.y > start_pos.y + 10.0,
            "MovingPlatform must carry grounded rider smoothly (start={:?}, end={:?})",
            start_pos,
            end_pos
        );
        assert!(
            (plat_tf.x - plat_center.x).abs() < 0.01 && (plat_tf.y - plat_center.y).abs() < 0.01,
            "MovingPlatform Transform must stay synced with collision center"
        );
    }

    #[test]
    fn test_spike_hazard_grants_iframes_and_knockback() {
        use crate::combat::{DamagePercent, Invulnerable};
        use crate::physics::{KinematicBody, SolidRect, SpikeHazard};

        let (mut app, player) = physics_test_app(Vec2::new(0.0, 18.0));
        app.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-100.0, -32.0), Vec2::new(100.0, 0.0)));
        app.world_mut()
            .spawn(SpikeHazard::new(Vec2::new(0.0, 6.0), Vec2::new(40.0, 12.0)));

        step_fixed_ticks(&mut app, 1);
        let pct_after = app.world().get::<DamagePercent>(player).unwrap().0;
        let vy_after = app.world().get::<KinematicBody>(player).unwrap().velocity.y;
        let has_invuln = app
            .world()
            .get::<Invulnerable>(player)
            .is_some_and(Invulnerable::is_active);

        assert!(pct_after > 0.0, "Spikes must deal non-lethal % damage");
        assert!(
            vy_after > 200.0,
            "Spikes must knock player upward out of hazard, got vy={}",
            vy_after
        );
        assert!(has_invuln, "Spikes must grant brief i-frames");

        // Stepping again during i-frames must NOT stack additional spike damage
        step_fixed_ticks(&mut app, 6);
        assert_eq!(
            app.world().get::<DamagePercent>(player).unwrap().0,
            pct_after,
            "Active i-frames must prevent repeat spike damage"
        );
    }

    #[test]
    fn test_wall_turret_fires_and_buster_destroys_it() {
        use crate::enemies::{ArenaScore, TurretProjectile, WallTurret, spawn_wall_turret};
        use crate::physics::{KinematicBody, MovementState, PlayerIntent, SolidRect};

        let (mut app, player) = physics_test_app(Vec2::new(0.0, 20.0));
        app.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-100.0, -32.0), Vec2::new(300.0, 0.0)));
        let turret = {
            let mut commands = app.world_mut().commands();
            let id = spawn_wall_turret(&mut commands, Vec2::new(130.0, 22.0), -1.0);
            app.world_mut().flush();
            id
        };

        // Fast-forward turret fire_timer near zero -> must spawn a TurretProjectile
        app.world_mut()
            .get_mut::<WallTurret>(turret)
            .unwrap()
            .fire_timer = 0.01;
        step_fixed_ticks(&mut app, 2);
        let orb_count = app
            .world_mut()
            .query::<&TurretProjectile>()
            .iter(app.world())
            .count();
        assert!(orb_count >= 1, "In-range WallTurret must fire plasma orb");

        // Shoot 2 rapid Buster pellets at the turret -> destroys/disables turret & increments score
        {
            let mut body = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
            body.state = MovementState::Grounded;
            body.facing = 1.0;
        }
        for _ in 0..2 {
            {
                let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
                intent.buster_just_pressed = true;
            }
            step_fixed_ticks(&mut app, 1);
            {
                let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
                intent.buster_just_pressed = false;
            }
            step_fixed_ticks(&mut app, 8);
        }
        step_fixed_ticks(&mut app, 20);

        let t = app.world().get::<WallTurret>(turret).unwrap();
        assert!(
            !t.is_active(),
            "Two Buster pellets (7% >= threshold) must disable WallTurret"
        );
        assert_eq!(app.world().resource::<ArenaScore>().turrets_destroyed, 1);
    }

    #[test]
    fn test_blast_zone_ko_and_3s_respawn() {
        use crate::combat::{DamagePercent, Invulnerable};
        use crate::enemies::{
            ArenaScore, BlastZone, ENEMY_KO_RESPAWN_SECS, EnemyArchetype, EnemyFighter,
            spawn_enemy_fighter,
        };
        use crate::physics::KinematicBody;

        let (mut app, _player) = physics_test_app(Vec2::new(1350.0, 240.0));
        app.world_mut().spawn(BlastZone {
            min: Vec2::new(1035.0, 40.0),
            max: Vec2::new(1765.0, 510.0),
            arena_center: Vec2::new(1390.0, 290.0),
        });

        let home = Vec2::new(1500.0, 240.0);
        let enemy = {
            let mut commands = app.world_mut().commands();
            let id = spawn_enemy_fighter(&mut commands, EnemyArchetype::LightStriker, home, false);
            app.world_mut().flush();
            id
        };

        // Launch enemy outside right blast zone boundary
        {
            let mut body = app.world_mut().get_mut::<KinematicBody>(enemy).unwrap();
            body.position = Vec2::new(1790.0, 280.0);
        }
        app.world_mut().get_mut::<DamagePercent>(enemy).unwrap().0 = 115.0;

        step_fixed_ticks(&mut app, 1);
        assert_eq!(
            app.world().resource::<ArenaScore>().ko_count,
            1,
            "Crossing BlastZone must increment KO count"
        );
        assert_eq!(
            app.world_mut()
                .query::<&EnemyFighter>()
                .iter(app.world())
                .count(),
            0,
            "KO'd enemy must despawn immediately"
        );

        // Advance just under 3.0s (170 ticks) -> must not respawn prematurely
        step_fixed_ticks(&mut app, 170);
        assert_eq!(
            app.world_mut()
                .query::<&EnemyFighter>()
                .iter(app.world())
                .count(),
            0
        );

        // Advance to exact 3.0s respawn tick (170 + 10 = 180 ticks = 3.00s) -> respawns right at home_pos!
        let remaining_ticks =
            (ENEMY_KO_RESPAWN_SECS * FIXED_TIMESTEP_HZ as f32).ceil() as usize - 170;
        step_fixed_ticks(&mut app, remaining_ticks);

        let mut q = app
            .world_mut()
            .query::<(&EnemyFighter, &KinematicBody, Option<&Invulnerable>)>();
        let (fighter, body, invuln) = q
            .single(app.world())
            .expect("Enemy must respawn cleanly after 3.0s");
        assert_eq!(fighter.archetype, EnemyArchetype::LightStriker);
        assert_eq!(fighter.home_pos, home);
        assert!(
            (body.position - home).length() < 1.0,
            "Enemy must respawn at home_pos ({:?}), got {:?}",
            home,
            body.position
        );
        assert!(invuln.is_some_and(Invulnerable::is_active));
    }

    #[test]
    fn test_heavy_brawler_resists_knockback_more_than_light_striker() {
        use crate::enemies::{EnemyArchetype, spawn_enemy_fighter};
        use crate::physics::{KinematicBody, MovementState, PlayerIntent};

        let measure_kb = |archetype: EnemyArchetype| -> f32 {
            let (mut app, player) = physics_test_app(Vec2::new(0.0, 60.0));
            let enemy = {
                let mut commands = app.world_mut().commands();
                let id = spawn_enemy_fighter(&mut commands, archetype, Vec2::new(30.0, 60.0), false);
                app.world_mut().flush();
                id
            };
            {
                let mut body = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
                body.state = MovementState::Airborne;
                body.velocity = Vec2::new(280.0, 0.0);
                body.facing = 1.0;
            }
            {
                let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
                intent.melee_just_pressed = true;
            }
            step_fixed_ticks(&mut app, 1);
            app.world().get::<KinematicBody>(enemy).unwrap().velocity.length()
        };

        let light_kb = measure_kb(EnemyArchetype::LightStriker);
        let heavy_kb = measure_kb(EnemyArchetype::HeavyBrawler);
        assert!(
            light_kb > heavy_kb * 1.35,
            "LightStriker ({}) must take significantly higher knockback than HeavyBrawler ({})",
            light_kb,
            heavy_kb
        );
    }

    #[test]
    fn test_stage_restart_and_procedural_wav_fallback() {
        use crate::audio::synthesize_pcm_wav;
        use crate::combat::DamagePercent;
        use crate::enemies::ArenaScore;
        use crate::physics::KinematicBody;
        use crate::player::DEFAULT_PLAYER_SPAWN;

        let (mut app, player) = physics_test_app(Vec2::new(1400.0, 260.0));
        app.world_mut().get_mut::<DamagePercent>(player).unwrap().0 = 88.0;
        app.world_mut().resource_mut::<ArenaScore>().ko_count = 4;

        // Pause -> RestartStage -> returns cleanly to Playing with reset player & score
        app.world_mut().write_message(GameStateCommand::TogglePause);
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppState>>().get(),
            AppState::Paused
        );

        app.world_mut().write_message(GameStateCommand::RestartStage);
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppState>>().get(),
            AppState::Playing
        );
        // One more Update tick delivers StageRestartEvent to Player & Enemies listeners
        app.update();

        let body = app.world().get::<KinematicBody>(player).unwrap();
        assert_eq!(body.position, DEFAULT_PLAYER_SPAWN);
        assert_eq!(app.world().get::<DamagePercent>(player).unwrap().0, 0.0);
        assert_eq!(app.world().resource::<ArenaScore>().ko_count, 0);

        // Verify procedural WAV synthesizer emits a valid RIFF/WAVE PCM header + audio data
        let wav = synthesize_pcm_wav(440.0, 220.0, 0.12, true);
        assert!(wav.len() > 44);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");
    }
}
