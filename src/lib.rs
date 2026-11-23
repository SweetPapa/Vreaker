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
pub mod story;
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
use story::StoryPlugin;
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
                StoryPlugin,
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
                    if (min.x - 470.0).abs() < 2.0 && (max.x - 496.0).abs() < 2.0 {
                        wall_left_min_y = Some(min.y);
                    }
                }
            }
        }

        assert_eq!(counts.get("PlayerSpawn").copied().unwrap_or(0), 1);
        assert_eq!(
            counts.get("LoopSpline").copied().unwrap_or(0),
            0,
            "360° Sonic LoopSpline must be removed from demo_gym.ldtk"
        );
        assert!(counts.get("MovingPlatform").copied().unwrap_or(0) >= 2);
        assert!(counts.get("SpikeHazard").copied().unwrap_or(0) >= 2);
        assert!(counts.get("WallTurret").copied().unwrap_or(0) >= 4);
        assert!(counts.get("OneWayPlatform").copied().unwrap_or(0) >= 12);
        assert!(counts.get("EnemySpawn").copied().unwrap_or(0) >= 6);
        assert_eq!(counts.get("EncounterZone").copied().unwrap_or(0), 3);
        assert_eq!(counts.get("BossSpawn").copied().unwrap_or(0), 1);
        assert_eq!(counts.get("BlastZone").copied().unwrap_or(0), 1);

        // Verify Zone A vertical shaft left wall leaves an open bottom doorway above y = 28 floor
        let doorway_y = wall_left_min_y.expect("Zone A vertical shaft left wall found");
        assert!(
            doorway_y >= 110.0,
            "Shaft left wall must leave >= 82px doorway headroom above y=28 floor, got {}",
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

    // -------------------------------------------------------------------------
    // Milestone 2 — Spin Dash, Expanded Aerials, Shield, Air-Dodge, RON Data,
    //               Combo Tracker, Speedrun Timer & Foundry Art Assets
    // -------------------------------------------------------------------------

    #[test]
    fn test_spin_dash_charge_and_explosive_launch() {
        use crate::physics::{KinematicBody, MovementState, PlayerIntent, SolidRect};

        let (mut app, player) = physics_test_app(Vec2::new(0.0, 18.0));
        app.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-200.0, -32.0), Vec2::new(1200.0, 0.0)));
        step_fixed_ticks(&mut app, 3);
        assert_eq!(
            app.world().get::<KinematicBody>(player).unwrap().state,
            MovementState::Grounded
        );

        // Hold Down + SpinDash to charge Spin Dash in place
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.move_axis = Vec2::new(0.0, -1.0);
            intent.boost_just_pressed = true;
            intent.boost_held = true;
        }
        step_fixed_ticks(&mut app, 1);
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.boost_just_pressed = false;
        }
        step_fixed_ticks(&mut app, 25);

        let charge = match app.world().get::<KinematicBody>(player).unwrap().state {
            MovementState::SpinCharging { charge } => charge,
            other => panic!("Expected MovementState::SpinCharging, got {:?}", other),
        };
        assert!(
            charge > 0.70,
            "Holding Spin Dash for ~0.43s must build strong charge, got {}",
            charge
        );

        // Release Down & Boost -> explosive Sonic launch >= 550 px/s (clears 360° loop threshold!)
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.move_axis = Vec2::ZERO;
            intent.boost_held = false;
        }
        step_fixed_ticks(&mut app, 1);

        let body = app.world().get::<KinematicBody>(player).unwrap();
        assert_eq!(body.state, MovementState::Grounded);
        assert!(
            body.ground_speed >= 520.0,
            "Spin Dash release must launch cleanly above 360° loop threshold, got {}",
            body.ground_speed
        );
    }

    #[test]
    fn test_up_air_and_back_air_directional_launches() {
        use crate::combat::AttackState;
        use crate::physics::{KinematicBody, MovementState, PlayerIntent};

        // 1. Up-Air launches overhead target sharply upward
        let (mut app, player) = physics_test_app(Vec2::new(0.0, 80.0));
        let above = spawn_test_target(&mut app, Vec2::new(4.0, 108.0), 20.0);
        {
            let mut body = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
            body.state = MovementState::Airborne;
            body.facing = 1.0;
        }
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.move_axis = Vec2::new(0.0, 1.0);
            intent.melee_just_pressed = true;
        }
        step_fixed_ticks(&mut app, 1);
        assert_eq!(
            app.world().get::<AttackState>(player).unwrap().active_move,
            Some(crate::combat::AttackKind::UpAir)
        );
        let up_vel = app.world().get::<KinematicBody>(above).unwrap().velocity;
        assert!(
            up_vel.y > 190.0,
            "Up-Air must juggle target strongly upward, got {:?}",
            up_vel
        );

        // 2. Back-Air strikes target BEHIND hero while facing right and launches left (-X)
        let (mut app2, player2) = physics_test_app(Vec2::new(0.0, 80.0));
        let behind = spawn_test_target(&mut app2, Vec2::new(-28.0, 82.0), 30.0);
        {
            let mut body = app2.world_mut().get_mut::<KinematicBody>(player2).unwrap();
            body.state = MovementState::Airborne;
            body.facing = 1.0;
        }
        {
            let mut intent = app2.world_mut().get_mut::<PlayerIntent>(player2).unwrap();
            intent.move_axis = Vec2::new(-1.0, 0.0);
            intent.melee_just_pressed = true;
        }
        step_fixed_ticks(&mut app2, 1);
        assert_eq!(
            app2.world().get::<AttackState>(player2).unwrap().active_move,
            Some(crate::combat::AttackKind::BackAir)
        );
        let back_vel = app2.world().get::<KinematicBody>(behind).unwrap().velocity;
        assert!(
            back_vel.x < -200.0,
            "Back-Air must launch target behind player sharply left, got {:?}",
            back_vel
        );
    }

    #[test]
    fn test_grounded_shield_blocks_incoming_attack() {
        use crate::combat::{DamagePercent, Hitbox, ShieldState, Team};
        use crate::physics::{KinematicBody, MovementState, PlayerIntent, SolidRect};

        let (mut app, player) = physics_test_app(Vec2::new(0.0, 18.0));
        app.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-200.0, -32.0), Vec2::new(200.0, 0.0)));
        step_fixed_ticks(&mut app, 3);
        assert_eq!(
            app.world().get::<KinematicBody>(player).unwrap().state,
            MovementState::Grounded
        );

        // Hold Guard -> ShieldState becomes active
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.guard_held = true;
        }
        step_fixed_ticks(&mut app, 1);
        assert!(app.world().get::<ShieldState>(player).unwrap().active);
        let hp_before = app.world().get::<ShieldState>(player).unwrap().health;

        // Spawn an overlapping enemy hitbox directly onto the shielded player
        let attacker = spawn_test_target(&mut app, Vec2::new(30.0, 18.0), 0.0);
        app.world_mut().spawn((
            Hitbox {
                owner: attacker,
                team: Team::Enemy,
                offset: Vec2::ZERO,
                radius: 20.0,
                damage: 14.0,
                base_kb: 210.0,
                kb_scale: 2.5,
                momentum_mult: 0.0,
                angle_deg: 35.0,
                attacker_speed: 0.0,
                facing: -1.0,
                lifetime: Timer::from_seconds(0.2, TimerMode::Once),
                piercing: false,
                despawn_on_expire: true,
                recoil_bounce_vy: None,
                hit_entities: std::collections::HashSet::new(),
                is_charged: false,
                min_hitstop_frames: 3,
            },
            Transform::from_xyz(0.0, 18.0, 0.0),
        ));

        step_fixed_ticks(&mut app, 1);

        assert_eq!(
            app.world().get::<DamagePercent>(player).unwrap().0,
            0.0,
            "Active grounded Shield must block incoming hit with 0% damage"
        );
        let hp_after = app.world().get::<ShieldState>(player).unwrap().health;
        assert!(
            hp_after < hp_before - 20.0,
            "Blocking heavy hit must drain shield health ({} -> {})",
            hp_before,
            hp_after
        );
    }

    #[test]
    fn test_directional_air_dodge_grants_iframes_once_per_airtime() {
        use crate::combat::{AirDodgeState, Invulnerable};
        use crate::physics::{KinematicBody, MovementState, PlayerIntent, SolidRect};

        let (mut app, player) = physics_test_app(Vec2::new(0.0, 120.0));
        app.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-200.0, -32.0), Vec2::new(200.0, 0.0)));
        {
            let mut body = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
            body.state = MovementState::Airborne;
            body.velocity = Vec2::ZERO;
        }
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.move_axis = Vec2::new(1.0, 0.0);
            intent.guard_just_pressed = true;
        }

        step_fixed_ticks(&mut app, 1);

        let invuln = app
            .world()
            .get::<Invulnerable>(player)
            .is_some_and(Invulnerable::is_active);
        let vx = app.world().get::<KinematicBody>(player).unwrap().velocity.x;
        let dodge = app.world().get::<AirDodgeState>(player).unwrap();
        assert!(invuln, "Air-Dodge must grant invulnerability frames");
        assert!(vx > 240.0, "Directional Air-Dodge must burst right, got vx={}", vx);
        assert!(dodge.used_in_air);

        // Wait until air-dodge finishes while still airborne, try again -> must NOT trigger twice!
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.guard_just_pressed = false;
        }
        step_fixed_ticks(&mut app, 26);
        {
            let mut body = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
            body.position.y = 140.0;
            body.velocity = Vec2::ZERO;
        }
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.move_axis = Vec2::new(-1.0, 0.0);
            intent.guard_just_pressed = true;
        }
        step_fixed_ticks(&mut app, 1);
        assert!(
            app.world().get::<Invulnerable>(player).is_none(),
            "Air-Dodge must be limited to once per airtime until touching ground/wall"
        );
    }

    #[test]
    fn test_bone_projected_ron_hitboxes_and_combo_timer() {
        use crate::combat::{
            AttackKind, ComboTracker, FighterFrameData, StageRunTimer, resolve_move_spec,
        };
        use crate::physics::{KinematicBody, MovementState, PlayerIntent};

        let frame_data = FighterFrameData::load_default();
        assert!(
            frame_data.hero_manifest.is_some(),
            "FighterFrameData must load baked assets/sprites/hero/hero.ron headlessly"
        );
        let d_air = resolve_move_spec(AttackKind::DownAir, &frame_data);
        assert_eq!(d_air.angle_deg, -78.0);
        assert!(d_air.offset.y < -14.0);
        let muzzle = frame_data.buster_muzzle_offset();
        assert!(muzzle.x >= 18.0);

        // Verify ComboTracker increments across consecutive hits and StageRunTimer advances in Playing
        let (mut app, player) = physics_test_app(Vec2::new(0.0, 60.0));
        let _target = spawn_test_target(&mut app, Vec2::new(26.0, 60.0), 0.0);
        {
            let mut body = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
            body.state = MovementState::Grounded;
            body.facing = 1.0;
        }
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.melee_just_pressed = true;
        }
        step_fixed_ticks(&mut app, 1);
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.melee_just_pressed = false;
        }
        step_fixed_ticks(&mut app, 15);

        let combo = app.world().resource::<ComboTracker>();
        assert_eq!(combo.hits, 1);
        assert!(combo.damage > 0.0);

        let elapsed = app.world().resource::<StageRunTimer>().elapsed_secs;
        assert!(
            elapsed >= 0.24,
            "StageRunTimer must advance deterministically in FixedUpdate (got {})",
            elapsed
        );
    }

    #[test]
    fn test_milestone2_foundry_textures_and_prompt_icons_exist() {
        use std::path::Path;

        for tex in [
            "assets/textures/foundry/foundry_bg_far.png",
            "assets/textures/foundry/foundry_bg_mid.png",
            "assets/textures/foundry/foundry_tile_metal.png",
        ] {
            let p = Path::new(tex);
            assert!(p.exists(), "Expected Foundry art asset at {}", p.display());
            assert!(std::fs::metadata(p).expect("readable").len() > 500);
        }

        for icon in [
            "assets/ui/prompts/xbox_button_color_a.png",
            "assets/ui/prompts/xbox_button_color_x.png",
            "assets/ui/prompts/xbox_lb.png",
            "assets/ui/prompts/keyboard_space.png",
            "assets/ui/prompts/keyboard_q.png",
            "assets/ui/prompts/keyboard_escape.png",
        ] {
            let p = Path::new(icon);
            assert!(p.exists(), "Expected Kenney prompt icon at {}", p.display());
            assert!(std::fs::metadata(p).expect("readable").len() > 100);
        }

        let map = default_player_input_map();
        let guard_bindings = map
            .get_buttonlike(&PlayerAction::Guard)
            .expect("Guard action must have gamepad + keyboard bindings");
        assert!(guard_bindings.len() >= 4);
    }

    // -------------------------------------------------------------------------
    // Playtest Patch — Hit Deduplication, Ledge-Stop, Double-Jump, Angel Respawn & Pushbox
    // -------------------------------------------------------------------------

    /// Required regression test: a single active melee swing overlapping an enemy across
    /// multiple ticks hits that enemy AT MOST ONCE (HashSet deduplication — zero multi-ticking).
    #[test]
    fn test_hitbox_hits_each_target_once_per_swing() {
        use crate::combat::{ComboTracker, DamagePercent};
        use crate::physics::{KinematicBody, MovementState, PlayerIntent, SolidRect};

        let (mut app, player) = physics_test_app(Vec2::new(0.0, 18.0));
        app.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-200.0, -32.0), Vec2::new(400.0, 0.0)));
        let target = spawn_test_target(&mut app, Vec2::new(22.0, 18.0), 0.0);

        // Single clean melee swing while overlapping target
        {
            let mut body = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
            body.state = MovementState::Grounded;
            body.facing = 1.0;
        }
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.melee_just_pressed = true;
        }
        step_fixed_ticks(&mut app, 1);
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.melee_just_pressed = false;
        }

        // Keep target deliberately pinned right inside the active hitbox for the entire swing lifetime
        let damage_after_first = app.world().get::<DamagePercent>(target).unwrap().0;
        assert!(damage_after_first > 0.0, "Initial swing contact must land");
        for _ in 0..12 {
            {
                let mut tb = app.world_mut().get_mut::<KinematicBody>(target).unwrap();
                tb.position = Vec2::new(22.0, 18.0);
                tb.velocity = Vec2::ZERO;
            }
            step_fixed_ticks(&mut app, 1);
        }

        assert_eq!(
            app.world().get::<DamagePercent>(target).unwrap().0,
            damage_after_first,
            "A single swing must NEVER multi-tick the same target across active frames"
        );
        assert_eq!(
            app.world().resource::<ComboTracker>().hits,
            1,
            "Combo counter must register exactly 1 hit for 1 swing on 1 target"
        );
    }

    #[test]
    fn test_grounded_attack_ledge_stop_prevents_self_yeet() {
        use crate::physics::{KinematicBody, MovementState, PlayerIntent, SolidRect};

        // Platform ends sharply at x = 100.0; place player sprinting right near the edge (x = 84)
        let (mut app, player) = physics_test_app(Vec2::new(84.0, 18.0));
        app.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-100.0, -32.0), Vec2::new(100.0, 0.0)));
        step_fixed_ticks(&mut app, 2);

        {
            let mut body = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
            body.state = MovementState::Grounded;
            body.ground_speed = 240.0;
            body.velocity.x = 240.0;
            body.facing = 1.0;
        }
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.move_axis = Vec2::new(1.0, 0.0);
            intent.melee_just_pressed = true;
        }
        step_fixed_ticks(&mut app, 1);
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.melee_just_pressed = false;
        }
        step_fixed_ticks(&mut app, 10);

        let body = app.world().get::<KinematicBody>(player).unwrap();
        assert_eq!(
            body.state,
            MovementState::Grounded,
            "Grounded Dash Attack near a ledge must clamp safely at edge instead of yeeting off"
        );
        assert!(
            body.position.x <= 101.0,
            "Player x ({}) must stay clamped on the platform edge (<= 100)",
            body.position.x
        );
    }

    #[test]
    fn test_player_double_jump_and_controlled_up_b() {
        use crate::physics::{KinematicBody, MovementState, PlayerIntent, SolidRect};

        let (mut app, player) = physics_test_app(Vec2::new(0.0, 18.0));
        app.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-200.0, -32.0), Vec2::new(200.0, 0.0)));
        step_fixed_ticks(&mut app, 3);
        assert_eq!(
            app.world().get::<KinematicBody>(player).unwrap().air_jumps_remaining,
            1
        );

        // 1. Ground jump
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
        // Let apex pass so vy < 0, then trigger mid-air Double Jump!
        step_fixed_ticks(&mut app, 26);
        assert!(app.world().get::<KinematicBody>(player).unwrap().velocity.y < 0.0);

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
        let body_after_dj = app.world().get::<KinematicBody>(player).unwrap();
        assert_eq!(body_after_dj.state, MovementState::Airborne);
        assert_eq!(body_after_dj.air_jumps_remaining, 0);
        assert!(
            body_after_dj.velocity.y > 350.0,
            "Double jump must grant crisp vertical lift, got vy={}",
            body_after_dj.velocity.y
        );

        // 2. Follow up with controlled Up-B recovery
        step_fixed_ticks(&mut app, 20);
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.move_axis = Vec2::new(1.0, 1.0);
            intent.special_just_pressed = true;
        }
        step_fixed_ticks(&mut app, 1);
        let body_after_upb = app.world().get::<KinematicBody>(player).unwrap();
        assert!(
            body_after_upb.velocity.y > 380.0 && body_after_upb.velocity.x.abs() <= 160.0,
            "Up-B must deliver controlled recovery without screen-crossing zip, got {:?}",
            body_after_upb.velocity
        );
    }

    #[test]
    fn test_player_blast_zone_respawn_angel_platform() {
        use crate::combat::{DamagePercent, Invulnerable};
        use crate::enemies::BlastZone;
        use crate::physics::KinematicBody;
        use crate::player::{PlayerRespawnState, RESPAWN_ANGEL_PLATFORM_SECS};

        let (mut app, player) = physics_test_app(Vec2::new(1045.0, 260.0));
        app.world_mut().spawn(BlastZone {
            min: Vec2::new(635.0, 35.0),
            max: Vec2::new(1455.0, 515.0),
            arena_center: Vec2::new(1045.0, 292.0),
        });

        // Knock player below the bottom blast zone
        app.world_mut().get_mut::<DamagePercent>(player).unwrap().0 = 95.0;
        {
            let mut body = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
            body.position = Vec2::new(1045.0, 20.0);
        }

        step_fixed_ticks(&mut app, 1);

        let respawn = app.world().get::<PlayerRespawnState>(player).unwrap();
        assert!(respawn.active, "Player KO must activate the Angel Respawn Platform");
        assert!((respawn.timer - RESPAWN_ANGEL_PLATFORM_SECS).abs() < 0.05);
        assert!((respawn.platform_pos.x - 1045.0).abs() < 1.0);
        assert_eq!(app.world().get::<DamagePercent>(player).unwrap().0, 0.0);
        assert!(
            app.world()
                .get::<Invulnerable>(player)
                .is_some_and(Invulnerable::is_active),
            "Angel platform respawn must grant 2.0s blinking invulnerability"
        );

        // Hover safely for 1.5s without falling, then auto-release cleanly!
        let hover_y = app.world().get::<KinematicBody>(player).unwrap().position.y;
        step_fixed_ticks(&mut app, 30);
        assert!(
            (app.world().get::<KinematicBody>(player).unwrap().position.y - hover_y).abs() < 0.1,
            "Player must hover steadily on the angel platform while timer is active"
        );
        step_fixed_ticks(&mut app, 65);
        assert!(
            !app.world().get::<PlayerRespawnState>(player).unwrap().active,
            "Angel platform must release cleanly after 1.5s"
        );
    }

    #[test]
    fn test_grounded_fighter_pushbox_soft_separation() {
        use crate::physics::{KinematicBody, MovementState, SolidRect};

        let (mut app, player) = physics_test_app(Vec2::new(100.0, 18.0));
        app.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-100.0, -32.0), Vec2::new(300.0, 0.0)));
        let enemy = spawn_test_target(&mut app, Vec2::new(102.0, 18.0), 0.0);

        {
            let mut pb = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
            pb.state = MovementState::Grounded;
            let mut eb = app.world_mut().get_mut::<KinematicBody>(enemy).unwrap();
            eb.state = MovementState::Grounded;
        }

        let start_gap = (app.world().get::<KinematicBody>(player).unwrap().position.x
            - app.world().get::<KinematicBody>(enemy).unwrap().position.x)
            .abs();
        step_fixed_ticks(&mut app, 12);
        let end_gap = (app.world().get::<KinematicBody>(player).unwrap().position.x
            - app.world().get::<KinematicBody>(enemy).unwrap().position.x)
            .abs();

        assert!(
            end_gap > start_gap + 14.0,
            "Grounded pushbox must gently separate overlapping fighters (start_gap={}, end_gap={})",
            start_gap,
            end_gap
        );
    }

    #[test]
    fn test_neutral_stick_with_boost_held_never_shoots_forward() {
        use crate::physics::{KinematicBody, MovementState, PlayerIntent, SolidRect};

        let (mut app, player) = physics_test_app(Vec2::new(0.0, 18.0));
        app.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-300.0, -32.0), Vec2::new(300.0, 0.0)));
        step_fixed_ticks(&mut app, 3);
        let start_x = app.world().get::<KinematicBody>(player).unwrap().position.x;

        // Hold Boost/Dash with neutral stick -> must NOT auto-run left or right on its own!
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.move_axis = Vec2::ZERO;
            intent.boost_held = true;
        }
        step_fixed_ticks(&mut app, 20);
        let body = app.world().get::<KinematicBody>(player).unwrap();
        assert_eq!(body.state, MovementState::Grounded);
        assert!(
            body.velocity.x.abs() < 1.0 && (body.position.x - start_x).abs() < 1.0,
            "Neutral stick + boost_held must NEVER shoot character forward (pos_x={}, vx={})",
            body.position.x,
            body.velocity.x
        );

        // Even briefly dipping Down while boost_held was already held must NOT trigger Spin Dash launch!
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.move_axis = Vec2::new(0.0, -1.0);
            intent.boost_just_pressed = false;
            intent.boost_held = true;
        }
        step_fixed_ticks(&mut app, 4);
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.move_axis = Vec2::ZERO;
            intent.boost_held = false;
        }
        step_fixed_ticks(&mut app, 4);
        let body_after = app.world().get::<KinematicBody>(player).unwrap();
        assert!(
            body_after.velocity.x.abs() < 1.0,
            "Holding Boost while crouching without fresh press must NOT rocket player forward (vx={})",
            body_after.velocity.x
        );
    }

    #[test]
    fn test_sync_player_intent_from_leafwing_actions() {
        use crate::physics::PlayerIntent;
        use crate::player::Player;

        // Exercise the REAL Leafwing ActionState -> PlayerIntent sync system (without ManualPlayerIntent)
        let mut app = headless_test_app();
        app.world_mut().write_message(GameStateCommand::StartGame);
        app.update();

        KeyCode::KeyE.press(app.world_mut());
        KeyCode::KeyQ.press(app.world_mut());
        KeyCode::KeyJ.press(app.world_mut());
        app.update();
        step_fixed_ticks(&mut app, 1);

        let player = app
            .world_mut()
            .query_filtered::<Entity, With<Player>>()
            .single(app.world())
            .expect("Player exists");
        let intent = *app.world().get::<PlayerIntent>(player).unwrap();
        assert!(intent.boost_held, "Digital KeyE must activate boost_held");
        assert!(intent.guard_held, "Digital KeyQ must activate guard_held");
        assert!(
            intent.melee_just_pressed,
            "First FixedUpdate tick after press must fire melee_just_pressed"
        );

        // Second FixedUpdate tick within same render frame must NOT double-fire melee_just_pressed!
        step_fixed_ticks(&mut app, 1);
        let intent2 = *app.world().get::<PlayerIntent>(player).unwrap();
        assert!(
            !intent2.melee_just_pressed,
            "FixedUpdate edge latch must prevent double-firing melee_just_pressed on multi-tick frames"
        );
    }

    /// Regression test for the single-frame horizontal floor-ejection teleport bug:
    /// falling at terminal velocity onto any point of a 500px-wide solid floor must land
    /// straight down with ZERO horizontal teleporting.
    #[test]
    fn test_fast_fall_onto_wide_floor_never_teleports_horizontally() {
        use crate::combat::Hitstun;
        use crate::physics::{KinematicBody, MovementState, SolidRect};

        // 500px-wide Battlefield deck: x = 795..1295, top y = 220 (center x = 1045)
        let drop_x = 960.0;
        let (mut app, player) = physics_test_app(Vec2::new(drop_x, 240.0));
        app.world_mut().spawn(SolidRect::from_min_max(
            Vec2::new(795.0, 196.0),
            Vec2::new(1295.0, 220.0),
        ));

        // Drop player at terminal fall velocity (-620 px/s) -> crosses 10.3px in a single tick!
        {
            let mut body = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
            body.state = MovementState::Airborne;
            body.velocity = Vec2::new(0.0, -620.0);
        }
        step_fixed_ticks(&mut app, 4);

        let body = app.world().get::<KinematicBody>(player).unwrap();
        assert_eq!(body.state, MovementState::Grounded);
        assert!(
            (body.position.x - drop_x).abs() < 0.5,
            "Fast-falling onto a wide floor must NEVER eject horizontally (expected x={}, got x={})",
            drop_x,
            body.position.x
        );
        assert!(
            (body.feet_y() - 220.0).abs() < 0.5,
            "Expected clean landing at y=220, got feet_y={}",
            body.feet_y()
        );

        // Also verify a Hitstun enemy falling onto the same wide floor never teleports off-stage!
        let enemy = spawn_test_target(&mut app, Vec2::new(1120.0, 242.0), 60.0);
        {
            let mut eb = app.world_mut().get_mut::<KinematicBody>(enemy).unwrap();
            eb.state = MovementState::Airborne;
            eb.velocity = Vec2::new(0.0, -520.0);
        }
        app.world_mut().entity_mut(enemy).insert(Hitstun::new(0.5));
        step_fixed_ticks(&mut app, 4);

        let eb = app.world().get::<KinematicBody>(enemy).unwrap();
        assert!(
            (eb.position.x - 1120.0).abs() < 0.5,
            "Hitstun enemy falling onto wide floor must NEVER teleport horizontally (got x={})",
            eb.position.x
        );
    }

    // -------------------------------------------------------------------------
    // Expanded Smash Moveset, 4-Archetype Enemy Roster & Gamepad D-Pad Guard Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_down_tilt_and_forward_smash_launch_arcs() {
        use crate::combat::{AttackKind, AttackState, DamagePercent};
        use crate::physics::{KinematicBody, MovementState, PlayerIntent, SolidRect};

        // 1. Down-Tilt: low sweeping combo starter launching target sharply upward (~72°)
        let (mut app, player) = physics_test_app(Vec2::new(0.0, 18.0));
        app.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-200.0, -32.0), Vec2::new(400.0, 0.0)));
        let target = spawn_test_target(&mut app, Vec2::new(26.0, 14.0), 15.0);
        step_fixed_ticks(&mut app, 2);

        {
            let mut body = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
            body.facing = 1.0;
        }
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.move_axis = Vec2::new(0.0, -1.0);
            intent.melee_just_pressed = true;
        }
        step_fixed_ticks(&mut app, 1);
        assert_eq!(
            app.world().get::<AttackState>(player).unwrap().active_move,
            Some(AttackKind::DownTilt)
        );
        let dtilt_vel = app.world().get::<KinematicBody>(target).unwrap().velocity;
        assert!(
            dtilt_vel.y > 140.0,
            "Down-Tilt must pop enemy upward for aerial combo followups, got {:?}",
            dtilt_vel
        );

        // 2. Forward-Smash: heavy grounded finisher dealing >= 15% and strong horizontal launch
        let (mut app2, player2) = physics_test_app(Vec2::new(0.0, 18.0));
        app2.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-200.0, -32.0), Vec2::new(400.0, 0.0)));
        let target2 = spawn_test_target(&mut app2, Vec2::new(30.0, 18.0), 40.0);
        step_fixed_ticks(&mut app2, 2);
        {
            let mut body = app2.world_mut().get_mut::<KinematicBody>(player2).unwrap();
            body.state = MovementState::Grounded;
            body.ground_speed = 0.0;
            body.velocity = Vec2::ZERO;
            body.facing = 1.0;
        }
        {
            let mut intent = app2.world_mut().get_mut::<PlayerIntent>(player2).unwrap();
            intent.move_axis = Vec2::new(1.0, 0.0);
            intent.melee_just_pressed = true;
        }
        step_fixed_ticks(&mut app2, 1);
        assert_eq!(
            app2.world().get::<AttackState>(player2).unwrap().active_move,
            Some(AttackKind::ForwardSmash)
        );
        let pct2 = app2.world().get::<DamagePercent>(target2).unwrap().0;
        let vel2 = app2.world().get::<KinematicBody>(target2).unwrap().velocity;
        assert!(pct2 >= 55.0, "Forward-Smash must deal heavy damage, got pct={}", pct2);
        assert!(
            vel2.x > 250.0,
            "Forward-Smash must launch target powerfully forward, got {:?}",
            vel2
        );
    }

    #[test]
    fn test_four_way_specials_wave_blitz_counter_and_dive_slam() {
        use crate::combat::{
            AttackKind, AttackState, BreakerWaveProjectile, CounterStance, DamagePercent, Hitbox,
            MeteorDiveState, Team,
        };
        use crate::physics::{KinematicBody, MovementState, PlayerIntent, SolidRect};

        // 1. Grounded Neutral-B: Breaker Crescent Energy Wave spawns and hits enemy ahead
        let (mut app, player) = physics_test_app(Vec2::new(0.0, 18.0));
        app.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-200.0, -32.0), Vec2::new(400.0, 0.0)));
        let target = spawn_test_target(&mut app, Vec2::new(84.0, 18.0), 0.0);
        step_fixed_ticks(&mut app, 2);
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.move_axis = Vec2::ZERO;
            intent.special_just_pressed = true;
        }
        step_fixed_ticks(&mut app, 1);
        assert_eq!(
            app.world().get::<AttackState>(player).unwrap().active_move,
            Some(AttackKind::NeutralSpecial)
        );
        assert_eq!(
            app.world_mut()
                .query::<&BreakerWaveProjectile>()
                .iter(app.world())
                .count(),
            1
        );
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.special_just_pressed = false;
        }
        step_fixed_ticks(&mut app, 18);
        assert!(
            app.world().get::<DamagePercent>(target).unwrap().0 >= 10.0,
            "Neutral-B Breaker Crescent Wave must travel and strike enemies ahead"
        );

        // 2. Side-B: Plasma Blitz dashes forward cleanly (and ledge-clamps on platform!)
        let (mut app_side, p_side) = physics_test_app(Vec2::new(82.0, 18.0));
        app_side
            .world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-100.0, -32.0), Vec2::new(100.0, 0.0)));
        step_fixed_ticks(&mut app_side, 2);
        {
            let mut intent = app_side.world_mut().get_mut::<PlayerIntent>(p_side).unwrap();
            intent.move_axis = Vec2::new(1.0, 0.0);
            intent.special_just_pressed = true;
        }
        step_fixed_ticks(&mut app_side, 1);
        assert_eq!(
            app_side.world().get::<AttackState>(p_side).unwrap().active_move,
            Some(AttackKind::SideSpecial)
        );
        {
            let mut intent = app_side.world_mut().get_mut::<PlayerIntent>(p_side).unwrap();
            intent.special_just_pressed = false;
        }
        step_fixed_ticks(&mut app_side, 10);
        let side_body = app_side.world().get::<KinematicBody>(p_side).unwrap();
        assert_eq!(
            side_body.state,
            MovementState::Grounded,
            "Grounded Side-B Plasma Blitz near ledge must clamp safely without self-yeeting"
        );

        // 3. Grounded Down-B: Aegis Counter negates incoming hit (0% taken) and blasts enemy back!
        let (mut app_cnt, p_cnt) = physics_test_app(Vec2::new(0.0, 18.0));
        app_cnt
            .world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-200.0, -32.0), Vec2::new(200.0, 0.0)));
        let attacker = spawn_test_target(&mut app_cnt, Vec2::new(32.0, 18.0), 10.0);
        step_fixed_ticks(&mut app_cnt, 2);
        {
            let mut intent = app_cnt.world_mut().get_mut::<PlayerIntent>(p_cnt).unwrap();
            intent.move_axis = Vec2::new(0.0, -1.0);
            intent.special_just_pressed = true;
        }
        step_fixed_ticks(&mut app_cnt, 1);
        assert!(app_cnt.world().get::<CounterStance>(p_cnt).unwrap().is_active());
        {
            let mut intent = app_cnt.world_mut().get_mut::<PlayerIntent>(p_cnt).unwrap();
            intent.special_just_pressed = false;
        }
        app_cnt.world_mut().spawn((
            Hitbox {
                owner: attacker,
                team: Team::Enemy,
                offset: Vec2::ZERO,
                radius: 22.0,
                damage: 14.0,
                base_kb: 200.0,
                kb_scale: 2.0,
                momentum_mult: 0.0,
                angle_deg: 35.0,
                attacker_speed: 0.0,
                facing: -1.0,
                lifetime: Timer::from_seconds(0.2, TimerMode::Once),
                piercing: false,
                despawn_on_expire: true,
                recoil_bounce_vy: None,
                hit_entities: std::collections::HashSet::new(),
                is_charged: false,
                min_hitstop_frames: 4,
            },
            Transform::from_xyz(0.0, 18.0, 0.0),
        ));
        step_fixed_ticks(&mut app_cnt, 2);
        assert_eq!(
            app_cnt.world().get::<DamagePercent>(p_cnt).unwrap().0,
            0.0,
            "Active Down-B Aegis Counter must negate 100% incoming damage"
        );
        assert!(
            app_cnt.world().get::<DamagePercent>(attacker).unwrap().0 > 20.0,
            "Triggered Aegis Counter must blast attacker back with heavy counter damage"
        );

        // 4. Airborne Down-B: Meteor Breaker Dive slams floor and detonates shockwave hitting nearby foe!
        let (mut app_dive, p_dive) = physics_test_app(Vec2::new(0.0, 90.0));
        app_dive
            .world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-200.0, -32.0), Vec2::new(200.0, 0.0)));
        let victim = spawn_test_target(&mut app_dive, Vec2::new(42.0, 18.0), 0.0);
        {
            let mut b = app_dive.world_mut().get_mut::<KinematicBody>(p_dive).unwrap();
            b.state = MovementState::Airborne;
            b.velocity = Vec2::ZERO;
        }
        {
            let mut intent = app_dive.world_mut().get_mut::<PlayerIntent>(p_dive).unwrap();
            intent.move_axis = Vec2::new(0.0, -1.0);
            intent.special_just_pressed = true;
        }
        step_fixed_ticks(&mut app_dive, 1);
        assert!(app_dive.world().get::<MeteorDiveState>(p_dive).unwrap().active);
        {
            let mut intent = app_dive.world_mut().get_mut::<PlayerIntent>(p_dive).unwrap();
            intent.special_just_pressed = false;
        }
        step_fixed_ticks(&mut app_dive, 18);
        assert!(
            !app_dive.world().get::<MeteorDiveState>(p_dive).unwrap().active,
            "Meteor Dive must finish upon slamming the ground"
        );
        assert!(
            app_dive.world().get::<DamagePercent>(victim).unwrap().0 >= 12.0,
            "Meteor Dive touchdown shockwave must damage nearby enemies"
        );
    }

    #[test]
    fn test_four_enemy_archetypes_and_baked_mage_assassin_assets() {
        use std::path::Path;
        use crate::enemies::{
            EnemyArchetype, EnemyFighter, TurretProjectile, spawn_enemy_fighter,
        };
        use crate::physics::{KinematicBody, SolidRect};
        use crate::sprite_manifest::SpriteSheetManifest;

        // 1. Verify freshly baked 19-clip / 106-frame sprite packs for hero, mage, and assassin!
        for pack in ["hero", "mage", "assassin"] {
            let dir = Path::new("assets/sprites").join(pack);
            for suffix in ["_diffuse.png", "_normal.png", "_emission.png", ".ron"] {
                let p = dir.join(format!("{pack}{suffix}"));
                assert!(p.exists(), "Expected baked asset at {}", p.display());
                assert!(std::fs::metadata(&p).expect("readable").len() > 0);
            }
            let manifest = SpriteSheetManifest::load_from_file(dir.join(format!("{pack}.ron")))
                .unwrap_or_else(|e| panic!("Baked {pack}.ron failed to load: {e}"));
            for clip in [
                "idle",
                "run",
                "jab",
                "down_tilt",
                "up_tilt",
                "f_smash",
                "neutral_special",
                "side_special",
                "down_special",
                "guard",
            ] {
                assert!(
                    manifest.animations.contains_key(clip),
                    "Pack {pack} missing expanded clip '{clip}'"
                );
            }
        }

        // 2. Verify ArcaneZoner aims and fires an arcane plasma orb at mid-range!
        let (mut app, _player) = physics_test_app(Vec2::new(860.0, 240.0));
        app.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(795.0, 196.0), Vec2::new(1295.0, 220.0)));
        let mage = {
            let mut commands = app.world_mut().commands();
            let id = spawn_enemy_fighter(
                &mut commands,
                EnemyArchetype::ArcaneZoner,
                Vec2::new(1040.0, 240.0),
                false,
            );
            app.world_mut().flush();
            id
        };
        app.world_mut()
            .get_mut::<EnemyFighter>(mage)
            .unwrap()
            .special_cooldown = 0.0;
        step_fixed_ticks(&mut app, 45);
        let arcane_orbs = app
            .world_mut()
            .query::<&TurretProjectile>()
            .iter(app.world())
            .filter(|p| p.is_arcane)
            .count();
        assert!(
            arcane_orbs >= 1,
            "ArcaneZoner must telegraph and fire aimed arcane plasma orbs at mid-range"
        );

        // 3. Verify ShadowAssassin spawns cleanly with high move speed & unique archetype metadata
        let assassin = {
            let mut commands = app.world_mut().commands();
            let id = spawn_enemy_fighter(
                &mut commands,
                EnemyArchetype::ShadowAssassin,
                Vec2::new(1120.0, 240.0),
                false,
            );
            app.world_mut().flush();
            id
        };
        let a_fighter = app.world().get::<EnemyFighter>(assassin).unwrap();
        assert_eq!(a_fighter.archetype, EnemyArchetype::ShadowAssassin);
        assert!(a_fighter.archetype.move_speed() > EnemyArchetype::LightStriker.move_speed());
        assert!(app.world().get::<KinematicBody>(assassin).is_some());
    }

    #[test]
    fn test_gamepad_dpad_and_uncalibrated_trigger_guard() {
        use bevy::input::gamepad::{
            GamepadButtonChangedEvent, GamepadConnection, GamepadConnectionEvent,
            RawGamepadButtonChangedEvent, RawGamepadEvent,
        };
        use crate::physics::{GamepadTriggerCalibration, PlayerIntent};
        use crate::player::Player;

        let mut app = headless_test_app();
        app.world_mut().write_message(GameStateCommand::StartGame);
        app.update();

        // Spawn a simulated macOS Gamepad entity with uncalibrated LeftTrigger2 resting at 0.50!
        let gp = app.world_mut().spawn_empty().id();
        app.world_mut().write_message(GamepadConnectionEvent::new(
            gp,
            GamepadConnection::Connected {
                name: "TestMacGamepad".into(),
                vendor_id: Some(0x045e),
                product_id: Some(0x02ea),
            },
        ));
        app.update();

        // Simulate macOS resting-midpoint LT2 = 0.50 + pressing DPadRight -> MUST move right, NEVER Guard!
        app.world_mut()
            .write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                gp,
                GamepadButton::LeftTrigger2,
                0.50,
            )));
        app.world_mut()
            .write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                gp,
                GamepadButton::DPadRight,
                1.0,
            )));
        app.world_mut()
            .write_message(GamepadButtonChangedEvent::new(
                gp,
                GamepadButton::LeftTrigger2,
                bevy::input::ButtonState::Pressed,
                0.50,
            ));
        app.update();
        step_fixed_ticks(&mut app, 1);

        let player = app
            .world_mut()
            .query_filtered::<Entity, With<Player>>()
            .single(app.world())
            .expect("Player exists");
        let intent = *app.world().get::<PlayerIntent>(player).unwrap();
        assert!(
            !intent.guard_held,
            "Uncalibrated 0.50 resting LeftTrigger2 must NEVER activate Shield/Guard"
        );
        assert!(
            intent.move_axis.x > 0.5,
            "Direct Gamepad DPadRight must move right cleanly, got {:?}",
            intent.move_axis
        );
        assert!(
            !app.world()
                .resource::<GamepadTriggerCalibration>()
                .lt2_calibrated,
            "LT2 must remain uncalibrated until observed at < 0.15 rest"
        );

        // Now bring LT2 to rest (0.0) -> calibrates, then pull fully (0.95) -> activates Guard!
        app.world_mut()
            .write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                gp,
                GamepadButton::LeftTrigger2,
                0.0,
            )));
        app.world_mut()
            .write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                gp,
                GamepadButton::DPadRight,
                0.0,
            )));
        app.update();
        step_fixed_ticks(&mut app, 1);
        assert!(
            app.world()
                .resource::<GamepadTriggerCalibration>()
                .lt2_calibrated
        );

        app.world_mut()
            .write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                gp,
                GamepadButton::LeftTrigger2,
                0.95,
            )));
        app.update();
        step_fixed_ticks(&mut app, 1);
        let intent2 = *app.world().get::<PlayerIntent>(player).unwrap();
        assert!(
            intent2.guard_held,
            "Calibrated full LT2 press (0.95) must activate Guard cleanly"
        );
    }

    // -------------------------------------------------------------------------
    // Full Stage Progression, D-Pad Calibration, Lockdown Ambush & Boss Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_dpad_calibration_handles_buttons_axes_and_hat_switch() {
        use bevy::input::gamepad::{
            GamepadConnection, GamepadConnectionEvent, RawGamepadAxisChangedEvent,
            RawGamepadButtonChangedEvent, RawGamepadEvent,
        };
        use crate::input::{
            DpadBinding, DpadCalibration, SavedGamepadAxis, SavedGamepadButton,
        };
        use crate::physics::PlayerIntent;
        use crate::player::Player;

        let mut app = headless_test_app();
        app.world_mut().write_message(GameStateCommand::StartGame);
        app.update();

        let gp = app.world_mut().spawn_empty().id();
        app.world_mut().write_message(GamepadConnectionEvent::new(
            gp,
            GamepadConnection::Connected {
                name: "MacOsQuirkyPad".into(),
                vendor_id: Some(0x2dc8),
                product_id: Some(0x6001),
            },
        ));
        app.update();

        // Calibrate:
        // - LEFT -> Other(12) raw button
        // - RIGHT -> mis-mapped LeftTrigger (verifying Shield suppression!)
        // - UP / DOWN -> single-axis HID Hat Switch Other(7) with discrete values -1.0 and +0.14 (neutral 1.0)
        *app.world_mut().resource_mut::<DpadCalibration>() = DpadCalibration {
            left: Some(DpadBinding::Button(SavedGamepadButton::Other(12))),
            right: Some(DpadBinding::Button(SavedGamepadButton::LeftTrigger)),
            up: Some(DpadBinding::AxisTarget {
                axis: SavedGamepadAxis::Other(7),
                target: -1.0,
                neutral: 1.0,
                tolerance: 0.18,
            }),
            down: Some(DpadBinding::AxisTarget {
                axis: SavedGamepadAxis::Other(7),
                target: 0.14,
                neutral: 1.0,
                tolerance: 0.18,
            }),
        };

        let player = app
            .world_mut()
            .query_filtered::<Entity, With<Player>>()
            .single(app.world())
            .expect("Player exists");

        // 1. Press mis-mapped LeftTrigger (calibrated as RIGHT) -> moves RIGHT and suppresses Shield!
        app.world_mut()
            .write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                gp,
                GamepadButton::LeftTrigger,
                1.0,
            )));
        app.update();
        step_fixed_ticks(&mut app, 1);
        let intent_r = *app.world().get::<PlayerIntent>(player).unwrap();
        assert!(
            intent_r.move_axis.x > 0.5,
            "Calibrated Right must move right cleanly, got {:?}",
            intent_r.move_axis
        );
        assert!(
            !intent_r.guard_held,
            "Calibrated D-Pad button colliding with LeftTrigger must suppress Shield!"
        );

        // 2. Release LeftTrigger, push single-axis Hat Switch Other(7) to +0.14 (DOWN) -> moves DOWN!
        app.world_mut()
            .write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                gp,
                GamepadButton::LeftTrigger,
                0.0,
            )));
        app.world_mut()
            .write_message(RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(
                gp,
                GamepadAxis::Other(7),
                0.14,
            )));
        app.update();
        step_fixed_ticks(&mut app, 1);
        let intent_d = *app.world().get::<PlayerIntent>(player).unwrap();
        assert!(
            intent_d.move_axis.y < -0.5,
            "Calibrated single-axis HID hat switch DOWN must register cleanly, got {:?}",
            intent_d.move_axis
        );

        // 3. Return Hat Switch to neutral (1.0) + press raw Other(12) (LEFT) -> moves LEFT!
        app.world_mut()
            .write_message(RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(
                gp,
                GamepadAxis::Other(7),
                1.0,
            )));
        app.world_mut()
            .write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                gp,
                GamepadButton::Other(12),
                1.0,
            )));
        app.update();
        step_fixed_ticks(&mut app, 1);
        let intent_l = *app.world().get::<PlayerIntent>(player).unwrap();
        assert!(
            intent_l.move_axis.x < -0.5,
            "Calibrated raw Other(12) LEFT must register cleanly, got {:?}",
            intent_l.move_axis
        );
    }

    #[test]
    fn test_lockdown_ambush_gates_waves_and_clear_reward() {
        use crate::combat::DamagePercent;
        use crate::enemies::{
            ArenaScore, EncounterPhase, EncounterStatus, EnemyArchetype, EnemyFighter,
            LockdownEncounter, LockdownGate,
        };
        use crate::physics::{KinematicBody, SolidRect};

        let (mut app, player) = physics_test_app(Vec2::new(820.0, 240.0));
        app.world_mut().spawn(SolidRect::from_min_max(
            Vec2::new(780.0, 196.0),
            Vec2::new(1500.0, 220.0),
        ));
        app.world_mut().get_mut::<DamagePercent>(player).unwrap().0 = 50.0;

        let enc_entity = app
            .world_mut()
            .spawn(LockdownEncounter {
                id: 1,
                title: "TEST CAGE".to_string(),
                is_boss: false,
                trigger_x: 920.0,
                min_x: 840.0,
                max_x: 1460.0,
                floor_y: 220.0,
                arena_center: Vec2::new(1150.0, 312.0),
                spawn_right_gate: true,
                waves: vec![
                    vec![(EnemyArchetype::LightStriker, Vec2::new(1040.0, 250.0))],
                    vec![(EnemyArchetype::ArcaneZoner, Vec2::new(1240.0, 250.0))],
                ],
                current_wave: 0,
                phase: EncounterPhase::Idle,
                active_enemies: Vec::new(),
                gate_entities: Vec::new(),
            })
            .id();

        // Cross trigger_x -> lockdown activates, spawns 2 SolidRect LockdownGates & Wave 1 enemy!
        app.world_mut()
            .get_mut::<KinematicBody>(player)
            .unwrap()
            .position = Vec2::new(960.0, 240.0);
        step_fixed_ticks(&mut app, 2);

        assert_eq!(
            app.world().get::<LockdownEncounter>(enc_entity).unwrap().phase,
            EncounterPhase::Active
        );
        assert_eq!(
            app.world_mut()
                .query::<&LockdownGate>()
                .iter(app.world())
                .count(),
            2,
            "Ambush lockdown must slam shut both left & right laser gates"
        );
        assert!(
            app.world()
                .resource::<EncounterStatus>()
                .camera_lock_center
                .is_some()
        );
        assert_eq!(
            app.world_mut()
                .query::<&EnemyFighter>()
                .iter(app.world())
                .count(),
            1
        );

        // Defeat Wave 1 -> Wave 2 spawns automatically!
        let wave1: Vec<Entity> = app
            .world_mut()
            .query_filtered::<Entity, With<EnemyFighter>>()
            .iter(app.world())
            .collect();
        for e in wave1 {
            app.world_mut().entity_mut(e).despawn();
        }
        step_fixed_ticks(&mut app, 2);
        assert_eq!(
            app.world()
                .get::<LockdownEncounter>(enc_entity)
                .unwrap()
                .current_wave,
            1,
            "Clearing Wave 1 must immediately trigger Wave 2 reinforcements"
        );
        assert_eq!(
            app.world_mut()
                .query::<&EnemyFighter>()
                .iter(app.world())
                .count(),
            1
        );

        // Defeat Wave 2 -> Encounter clears, both gates shatter, camera unlocks, player heals!
        let wave2: Vec<Entity> = app
            .world_mut()
            .query_filtered::<Entity, With<EnemyFighter>>()
            .iter(app.world())
            .collect();
        for e in wave2 {
            app.world_mut().entity_mut(e).despawn();
        }
        step_fixed_ticks(&mut app, 2);
        assert_eq!(
            app.world().get::<LockdownEncounter>(enc_entity).unwrap().phase,
            EncounterPhase::Cleared
        );
        assert_eq!(
            app.world_mut()
                .query::<&LockdownGate>()
                .iter(app.world())
                .count(),
            0,
            "Clearing final wave must shatter all lockdown gates"
        );
        assert!(
            app.world()
                .resource::<EncounterStatus>()
                .camera_lock_center
                .is_none()
        );
        assert_eq!(app.world().resource::<ArenaScore>().encounters_cleared, 1);
        assert!(
            app.world().get::<DamagePercent>(player).unwrap().0 < 50.0,
            "Clearing ambush arena must heal player Smash %"
        );
    }

    #[test]
    fn test_boss_two_phases_baked_144px_assets_and_stage_clear_victory() {
        use std::path::Path;
        use crate::combat::CombatHitEvent;
        use crate::enemies::{
            BossState, EncounterPhase, EnemyArchetype, EnemyHealth, LockdownEncounter,
        };
        use crate::physics::{KinematicBody, SolidRect};
        use crate::sprite_manifest::SpriteSheetManifest;

        // 1. Verify freshly baked 144x144 19-clip Boss sprite pack exists & loads cleanly!
        let boss_dir = Path::new("assets/sprites/boss");
        for suffix in ["_diffuse.png", "_normal.png", "_emission.png", ".ron"] {
            let p = boss_dir.join(format!("boss{suffix}"));
            assert!(p.exists(), "Expected baked boss asset at {}", p.display());
            assert!(std::fs::metadata(&p).expect("readable").len() > 0);
        }
        let manifest =
            SpriteSheetManifest::load_from_file(boss_dir.join("boss.ron")).expect("boss.ron loads");
        assert_eq!(manifest.frame_size, (144, 144));
        for clip in ["idle", "f_smash", "down_special", "side_special", "neutral_special", "guard"]
        {
            assert!(manifest.animations.contains_key(clip), "boss.ron missing '{clip}'");
        }

        // 2. Trigger Boss Sanctum lockdown, drop Boss <= 50% -> Phase 2 Overdrive, then defeat -> Victory!
        let (mut app, player) = physics_test_app(Vec2::new(2940.0, 240.0));
        app.world_mut().spawn(SolidRect::from_min_max(
            Vec2::new(2800.0, 196.0),
            Vec2::new(3500.0, 220.0),
        ));
        app.world_mut().spawn(LockdownEncounter {
            id: 3,
            title: "IRON TITAN: VULCANUS".to_string(),
            is_boss: true,
            trigger_x: 2900.0,
            min_x: 2820.0,
            max_x: 3460.0,
            floor_y: 220.0,
            arena_center: Vec2::new(3140.0, 312.0),
            spawn_right_gate: false,
            waves: vec![vec![(EnemyArchetype::FoundryBoss, Vec2::new(3300.0, 254.0))]],
            current_wave: 0,
            phase: EncounterPhase::Idle,
            active_enemies: Vec::new(),
            gate_entities: Vec::new(),
        });

        step_fixed_ticks(&mut app, 2);
        let boss_entity = app
            .world_mut()
            .query_filtered::<Entity, With<BossState>>()
            .single(app.world())
            .expect("Boss spawns upon entering Sanctum");
        assert_eq!(app.world().get::<BossState>(boss_entity).unwrap().phase, 1);
        assert_eq!(app.world().get::<EnemyHealth>(boss_entity).unwrap().max, 240.0);

        // Deal 130 damage -> triggers Phase 2 Overdrive!
        app.world_mut().write_message(CombatHitEvent {
            attacker: player,
            target: boss_entity,
            hit_pos: Vec2::new(3300.0, 254.0),
            damage: 130.0,
            target_percent_after: 130.0,
            knockback_mag: 220.0,
            knockback_vec: Vec2::new(200.0, 90.0),
            hitstop_frames: 6,
            is_charged: true,
        });
        step_fixed_ticks(&mut app, 1);
        assert_eq!(
            app.world().get::<BossState>(boss_entity).unwrap().phase,
            2,
            "Dropping Boss <= 50% HP must trigger Phase 2 Overdrive"
        );

        // Deal finishing blow (150 damage) -> Boss despawns, encounter clears, Victory triggers!
        app.world_mut().write_message(CombatHitEvent {
            attacker: player,
            target: boss_entity,
            hit_pos: Vec2::new(3300.0, 254.0),
            damage: 150.0,
            target_percent_after: 280.0,
            knockback_mag: 360.0,
            knockback_vec: Vec2::new(320.0, 140.0),
            hitstop_frames: 8,
            is_charged: true,
        });
        step_fixed_ticks(&mut app, 1);
        assert!(
            app.world_mut()
                .query::<&BossState>()
                .iter(app.world())
                .count()
                == 0,
            "Defeating Boss must despawn Boss entity"
        );

        // Advance past the 1.15s dramatic victory celebration delay (~75 fixed ticks) + update state
        step_fixed_ticks(&mut app, 80);
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppState>>().get(),
            AppState::Victory,
            "Defeating End-of-Level Boss must transition cleanly to AppState::Victory!"
        );
        let _ = app.world().get::<KinematicBody>(player);
    }

    #[test]
    fn test_boss_phase2_laser_telegraph_emits_before_firing() {
        use crate::combat::CombatVisualEvent;
        use crate::enemies::{BossState, EnemyFighter, TurretProjectile, spawn_foundry_boss};
        use crate::physics::SolidRect;

        let (mut app, _player) = physics_test_app(Vec2::new(4580.0, 240.0));
        app.world_mut().spawn(SolidRect::from_min_max(
            Vec2::new(4440.0, 196.0),
            Vec2::new(5120.0, 220.0),
        ));
        let boss = {
            let mut commands = app.world_mut().commands();
            let id = spawn_foundry_boss(&mut commands, Vec2::new(4920.0, 250.0), 4460.0, 5100.0);
            app.world_mut().flush();
            id
        };
        // Settle boss grounded in Phase 2 right before Pattern C (pattern_step = 1 -> next is 2)
        step_fixed_ticks(&mut app, 4);
        {
            let mut bs = app.world_mut().get_mut::<BossState>(boss).unwrap();
            bs.phase = 2;
            bs.enraged_triggered = true;
            bs.pattern_step = 1;
            let mut ef = app.world_mut().get_mut::<EnemyFighter>(boss).unwrap();
            ef.special_cooldown = 0.0;
        }

        // Tick 1: Must emit BossLaserTelegraph BEFORE spawning any projectiles!
        step_fixed_ticks(&mut app, 1);
        let saw_telegraph = {
            let msgs = app.world().resource::<Messages<CombatVisualEvent>>();
            let mut cursor = msgs.get_cursor();
            cursor
                .read(msgs)
                .any(|ev| matches!(ev, CombatVisualEvent::BossLaserTelegraph { duration, .. } if *duration >= 0.45))
        };
        assert!(
            saw_telegraph,
            "Vulcanus Phase 2 Forge Volley must emit a >=0.45s BossLaserTelegraph before firing!"
        );
        assert_eq!(
            app.world_mut()
                .query::<&TurretProjectile>()
                .iter(app.world())
                .count(),
            0,
            "Projectiles must NOT fire before the 0.48s laser sightline telegraph finishes!"
        );

        // Advance past the 0.48s telegraph (~32 ticks) -> 3-way volley fires cleanly!
        step_fixed_ticks(&mut app, 32);
        let fired = app
            .world_mut()
            .query::<&TurretProjectile>()
            .iter(app.world())
            .filter(|p| p.is_magma_wave)
            .count();
        assert_eq!(
            fired, 3,
            "Vulcanus must fire the 3-way molten fan after the 0.48s laser telegraph completes"
        );
    }

    #[test]
    fn test_all_moves_and_projectiles_never_multi_tick_single_target() {
        use crate::combat::ComboTracker;
        use crate::physics::{KinematicBody, MovementState, PlayerIntent, SolidRect};

        // Audit Forward-Smash, Side-Special, and Neutral-Special Crescent Wave against a pinned enemy:
        // each single attack MUST register strictly 1 hit (zero multi-ticking).
        for (label, axis, is_special, target_x) in [
            ("ForwardSmash", Vec2::new(1.0, 0.0), false, 28.0),
            ("SideSpecial", Vec2::new(1.0, 0.0), true, 28.0),
            ("NeutralSpecialWave", Vec2::ZERO, true, 54.0),
        ] {
            let (mut app, player) = physics_test_app(Vec2::new(0.0, 18.0));
            app.world_mut().spawn(SolidRect::from_min_max(
                Vec2::new(-200.0, -32.0),
                Vec2::new(500.0, 0.0),
            ));
            let target = spawn_test_target(&mut app, Vec2::new(target_x, 18.0), 0.0);
            step_fixed_ticks(&mut app, 2);

            {
                let mut b = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
                b.state = MovementState::Grounded;
                b.facing = 1.0;
                b.ground_speed = 0.0;
                b.velocity = Vec2::ZERO;
            }
            {
                let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
                intent.move_axis = axis;
                intent.melee_just_pressed = !is_special;
                intent.special_just_pressed = is_special;
            }
            step_fixed_ticks(&mut app, 1);
            {
                let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
                intent.melee_just_pressed = false;
                intent.special_just_pressed = false;
            }
            // Pin target in place across the entire active duration of the move/wave!
            for _ in 0..20 {
                {
                    let mut tb = app.world_mut().get_mut::<KinematicBody>(target).unwrap();
                    tb.position = Vec2::new(target_x, 18.0);
                    tb.velocity = Vec2::ZERO;
                }
                step_fixed_ticks(&mut app, 1);
            }
            assert_eq!(
                app.world().resource::<ComboTracker>().hits,
                1,
                "{label} must hit a pinned target strictly ONCE (zero multi-ticking!)"
            );
        }
    }

    #[test]
    fn test_music_crossfade_resolver_and_audio_settings_clamping() {
        use std::path::Path;
        use crate::audio::music::{AudioSettings, MusicTrackId, resolve_desired_track};
        use crate::enemies::EncounterStatus;

        // 1. Verify all 4 converted Suno OGG tracks + README exist in assets/audio/music/
        for fname in [
            "stage1_theme.ogg",
            "boss_theme.ogg",
            "title_theme.ogg",
            "stage_clear.ogg",
            "README.md",
        ] {
            let p = Path::new("assets/audio/music").join(fname);
            assert!(p.exists(), "Expected music asset at {}", p.display());
            assert!(std::fs::metadata(&p).expect("readable").len() > 200);
        }

        // 2. Verify track state machine: Title -> Stage1 -> Boss -> KO fade (None) -> StageClear
        let mut enc = EncounterStatus::default();
        assert_eq!(
            resolve_desired_track(AppState::Title, &enc, None),
            Some(MusicTrackId::Title)
        );
        assert_eq!(
            resolve_desired_track(AppState::Playing, &enc, Some(MusicTrackId::Title)),
            Some(MusicTrackId::Stage1)
        );
        enc.active_title = Some("IRON TITAN: VULCANUS".into());
        enc.is_boss = true;
        assert_eq!(
            resolve_desired_track(AppState::Playing, &enc, Some(MusicTrackId::Stage1)),
            Some(MusicTrackId::Boss)
        );
        assert_eq!(
            resolve_desired_track(AppState::Paused, &enc, Some(MusicTrackId::Boss)),
            Some(MusicTrackId::Boss)
        );
        enc.victory_delay_timer = Some(0.9);
        assert_eq!(
            resolve_desired_track(AppState::Playing, &enc, Some(MusicTrackId::Boss)),
            None
        );
        assert_eq!(
            resolve_desired_track(AppState::Victory, &enc, None),
            Some(MusicTrackId::StageClear)
        );

        // 3. Verify AudioSettings live clamping
        let mut settings = AudioSettings::default();
        settings.adjust_music(1.0);
        assert_eq!(settings.music_volume, 1.0);
        settings.adjust_music(-2.0);
        assert_eq!(settings.music_volume, 0.0);
        settings.adjust_sfx(0.5);
        assert_eq!(settings.sfx_volume, 1.0);
    }

    // -------------------------------------------------------------------------
    // Smash Moveset Deepening, 360° Sword Spin, Chargeable Smashes, Dodges & Enemy Kits
    // -------------------------------------------------------------------------

    #[test]
    fn test_hurricane_sword_spin_and_down_smash_hit_both_sides_outward() {
        use std::path::Path;
        use crate::combat::{AttackKind, AttackState, DamagePercent};
        use crate::physics::{KinematicBody, MovementState, PlayerIntent, SolidRect};
        use crate::sprite_manifest::SpriteSheetManifest;

        // 1. Verify ALL 6 freshly baked 28-clip character sprite sheets have the expanded Smash clips!
        for pack in ["hero", "rogue", "barbarian", "mage", "assassin", "boss"] {
            let manifest = SpriteSheetManifest::load_from_file(
                Path::new("assets/sprites").join(pack).join(format!("{pack}.ron")),
            )
            .unwrap_or_else(|e| panic!("Baked {pack}.ron failed to load: {e}"));
            for clip in [
                "jab_2",
                "jab_3",
                "up_smash",
                "down_smash",
                "sword_spin",
                "dodge_fwd",
                "dodge_back",
                "spot_dodge",
                "taunt",
            ] {
                assert!(
                    manifest.animations.contains_key(clip),
                    "Sprite pack '{pack}' missing newly baked clip '{clip}'"
                );
            }
        }

        // 2. Grounded Up-B -> 360° Hurricane Sword Spin hits targets on BOTH left and right outward!
        let (mut app, player) = physics_test_app(Vec2::new(0.0, 18.0));
        app.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-250.0, -32.0), Vec2::new(250.0, 0.0)));
        let left_foe = spawn_test_target(&mut app, Vec2::new(-32.0, 18.0), 15.0);
        let right_foe = spawn_test_target(&mut app, Vec2::new(32.0, 18.0), 15.0);
        step_fixed_ticks(&mut app, 2);
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.move_axis = Vec2::new(0.0, 1.0);
            intent.special_just_pressed = true;
        }
        step_fixed_ticks(&mut app, 1);
        assert_eq!(
            app.world().get::<AttackState>(player).unwrap().active_move,
            Some(AttackKind::CycloneSpin)
        );
        let lv = app.world().get::<KinematicBody>(left_foe).unwrap().velocity;
        let rv = app.world().get::<KinematicBody>(right_foe).unwrap().velocity;
        assert!(
            lv.x < -120.0 && rv.x > 120.0,
            "360° Hurricane Sword Spin must launch left foe left and right foe right (lv={:?}, rv={:?})",
            lv,
            rv
        );

        // 3. Down-Smash (Boost + Down + Melee) sweeps BOTH sides outward with heavy damage!
        let (mut app2, p2) = physics_test_app(Vec2::new(0.0, 18.0));
        app2.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-250.0, -32.0), Vec2::new(250.0, 0.0)));
        let lf2 = spawn_test_target(&mut app2, Vec2::new(-30.0, 16.0), 0.0);
        let rf2 = spawn_test_target(&mut app2, Vec2::new(30.0, 16.0), 0.0);
        step_fixed_ticks(&mut app2, 2);
        {
            let mut b = app2.world_mut().get_mut::<KinematicBody>(p2).unwrap();
            b.state = MovementState::Grounded;
            b.ground_speed = 0.0;
        }
        {
            let mut intent = app2.world_mut().get_mut::<PlayerIntent>(p2).unwrap();
            intent.move_axis = Vec2::new(0.0, -1.0);
            intent.boost_held = true;
            intent.melee_just_pressed = true;
            intent.melee_held = false;
        }
        step_fixed_ticks(&mut app2, 1);
        assert_eq!(
            app2.world().get::<AttackState>(p2).unwrap().active_move,
            Some(AttackKind::DownSmash)
        );
        assert!(app2.world().get::<DamagePercent>(lf2).unwrap().0 >= 15.0);
        assert!(app2.world().get::<DamagePercent>(rf2).unwrap().0 >= 15.0);
        assert!(app2.world().get::<KinematicBody>(lf2).unwrap().velocity.x < -140.0);
        assert!(app2.world().get::<KinematicBody>(rf2).unwrap().velocity.x > 140.0);
    }

    #[test]
    fn test_chargeable_smash_scales_damage_and_knockback() {
        use crate::combat::{AttackKind, AttackState, DamagePercent, SmashChargeState};
        use crate::physics::{KinematicBody, MovementState, PlayerIntent, SolidRect};

        // Run A: Uncharged Forward-Smash
        let (mut app_un, p_un) = physics_test_app(Vec2::new(0.0, 18.0));
        app_un
            .world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-200.0, -32.0), Vec2::new(300.0, 0.0)));
        let t_un = spawn_test_target(&mut app_un, Vec2::new(28.0, 18.0), 20.0);
        step_fixed_ticks(&mut app_un, 2);
        {
            let mut intent = app_un.world_mut().get_mut::<PlayerIntent>(p_un).unwrap();
            intent.move_axis = Vec2::new(1.0, 0.0);
            intent.melee_just_pressed = true;
            intent.melee_held = false;
        }
        step_fixed_ticks(&mut app_un, 1);
        let uncharged_pct = app_un.world().get::<DamagePercent>(t_un).unwrap().0;
        let uncharged_vx = app_un.world().get::<KinematicBody>(t_un).unwrap().velocity.x;

        // Run B: Hold Melee for 0.65s (~39 ticks) -> Charged Forward-Smash!
        let (mut app_ch, p_ch) = physics_test_app(Vec2::new(0.0, 18.0));
        app_ch
            .world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-200.0, -32.0), Vec2::new(300.0, 0.0)));
        let t_ch = spawn_test_target(&mut app_ch, Vec2::new(28.0, 18.0), 20.0);
        step_fixed_ticks(&mut app_ch, 2);
        {
            let mut b = app_ch.world_mut().get_mut::<KinematicBody>(p_ch).unwrap();
            b.state = MovementState::Grounded;
            b.ground_speed = 0.0;
        }
        {
            let mut intent = app_ch.world_mut().get_mut::<PlayerIntent>(p_ch).unwrap();
            intent.move_axis = Vec2::new(1.0, 0.0);
            intent.melee_just_pressed = true;
            intent.melee_held = true;
        }
        step_fixed_ticks(&mut app_ch, 1);
        assert!(
            app_ch
                .world()
                .get::<SmashChargeState>(p_ch)
                .unwrap()
                .is_charging(),
            "Holding Melee on Forward-Smash must enter SmashChargeState"
        );
        {
            let mut intent = app_ch.world_mut().get_mut::<PlayerIntent>(p_ch).unwrap();
            intent.melee_just_pressed = false;
        }
        step_fixed_ticks(&mut app_ch, 38);
        // Release Melee -> unleashes Charged Forward-Smash!
        {
            let mut intent = app_ch.world_mut().get_mut::<PlayerIntent>(p_ch).unwrap();
            intent.melee_held = false;
        }
        step_fixed_ticks(&mut app_ch, 1);
        assert_eq!(
            app_ch.world().get::<AttackState>(p_ch).unwrap().active_move,
            Some(AttackKind::ForwardSmash)
        );
        let charged_pct = app_ch.world().get::<DamagePercent>(t_ch).unwrap().0;
        let charged_vx = app_ch.world().get::<KinematicBody>(t_ch).unwrap().velocity.x;
        assert!(
            charged_pct > uncharged_pct + 6.0 && charged_vx > uncharged_vx + 60.0,
            "Charged Smash must significantly out-damage & out-launch uncharged (pct {} vs {}, vx {} vs {})",
            charged_pct,
            uncharged_pct,
            charged_vx,
            uncharged_vx
        );
    }

    #[test]
    fn test_smash_roll_dodge_spot_dodge_and_fast_fall() {
        use crate::combat::{GroundDodgeKind, GroundDodgeState, Invulnerable, ShieldState};
        use crate::physics::{KinematicBody, MovementState, PlayerIntent, SolidRect};

        // 1. Shield + Right -> RollForward with i-frames & facing flip on finish!
        let (mut app, player) = physics_test_app(Vec2::new(0.0, 18.0));
        app.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-200.0, -32.0), Vec2::new(200.0, 0.0)));
        step_fixed_ticks(&mut app, 2);
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.guard_held = true;
        }
        step_fixed_ticks(&mut app, 1);
        assert!(app.world().get::<ShieldState>(player).unwrap().active);
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.right_just_pressed = true;
            intent.move_axis = Vec2::new(1.0, 0.0);
        }
        step_fixed_ticks(&mut app, 1);
        let gd = app.world().get::<GroundDodgeState>(player).unwrap();
        assert_eq!(gd.kind, GroundDodgeKind::RollForward);
        assert!(gd.is_active());
        assert!(
            app.world()
                .get::<Invulnerable>(player)
                .is_some_and(Invulnerable::is_active),
            "Roll-Dodge must grant invulnerability frames"
        );
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.right_just_pressed = false;
            intent.guard_held = false;
            intent.move_axis = Vec2::ZERO;
        }
        step_fixed_ticks(&mut app, 16);
        let body = app.world().get::<KinematicBody>(player).unwrap();
        assert!(body.position.x > 30.0, "Roll-Dodge must slide player forward, got x={}", body.position.x);
        assert!(body.facing < 0.0, "Smash Roll-Dodge must auto-face back toward origin at finish");

        // 2. Shield + Down -> Spot-Dodge with i-frames in place!
        let x_before_spot = body.position.x;
        step_fixed_ticks(&mut app, 6);
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.guard_held = true;
        }
        step_fixed_ticks(&mut app, 1);
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.down_just_pressed = true;
            intent.move_axis = Vec2::new(0.0, -1.0);
        }
        step_fixed_ticks(&mut app, 1);
        let gd2 = app.world().get::<GroundDodgeState>(player).unwrap();
        assert_eq!(gd2.kind, GroundDodgeKind::Spot);
        assert!(gd2.is_active());
        assert!((app.world().get::<KinematicBody>(player).unwrap().position.x - x_before_spot).abs() < 2.0);

        // 3. Airborne Down past jump apex -> Fast-Fall snaps velocity sharply downward!
        let (mut app_ff, p_ff) = physics_test_app(Vec2::new(0.0, 220.0));
        {
            let mut b = app_ff.world_mut().get_mut::<KinematicBody>(p_ff).unwrap();
            b.state = MovementState::Airborne;
            b.velocity = Vec2::new(0.0, -10.0);
        }
        {
            let mut intent = app_ff.world_mut().get_mut::<PlayerIntent>(p_ff).unwrap();
            intent.down_just_pressed = true;
            intent.move_axis = Vec2::new(0.0, -1.0);
        }
        step_fixed_ticks(&mut app_ff, 1);
        let vy = app_ff.world().get::<KinematicBody>(p_ff).unwrap().velocity.y;
        assert!(vy <= -450.0, "Fast-Fall must snap downward speed sharply, got vy={}", vy);
    }

    #[test]
    fn test_right_dpad_simultaneous_shield_lockout_and_control_config() {
        use bevy::input::gamepad::{
            GamepadConnection, GamepadConnectionEvent, RawGamepadButtonChangedEvent,
            RawGamepadEvent,
        };
        use crate::input::{ControlConfig, ShieldButtonMode};
        use crate::physics::PlayerIntent;
        use crate::player::Player;

        let mut app = headless_test_app();
        app.world_mut().write_message(GameStateCommand::StartGame);
        app.update();

        let gp = app.world_mut().spawn_empty().id();
        app.world_mut().write_message(GamepadConnectionEvent::new(
            gp,
            GamepadConnection::Connected {
                name: "QuirkyRightDpadPad".into(),
                vendor_id: None,
                product_id: None,
            },
        ));
        app.update();

        // 1. Simulate broken controller firing DPadRight + LeftTrigger simultaneously -> MUST move right, NEVER Shield!
        app.world_mut()
            .write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                gp,
                GamepadButton::DPadRight,
                1.0,
            )));
        app.world_mut()
            .write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                gp,
                GamepadButton::LeftTrigger,
                1.0,
            )));
        app.update();
        step_fixed_ticks(&mut app, 1);

        let player = app
            .world_mut()
            .query_filtered::<Entity, With<Player>>()
            .single(app.world())
            .expect("Player exists");
        let intent = *app.world().get::<PlayerIntent>(player).unwrap();
        assert!(intent.move_axis.x > 0.5, "Must move right cleanly, got {:?}", intent.move_axis);
        assert!(
            !intent.guard_held,
            "Simultaneous DPadRight + LeftTrigger glitch must NEVER activate Shield!"
        );

        // 2. Switch ControlConfig to LtOnly -> pressing LeftTrigger alone NEVER activates Shield!
        app.world_mut()
            .write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                gp,
                GamepadButton::DPadRight,
                0.0,
            )));
        app.world_mut()
            .write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                gp,
                GamepadButton::LeftTrigger,
                0.0,
            )));
        app.world_mut().resource_mut::<ControlConfig>().shield_mode = ShieldButtonMode::LtOnly;
        app.update();
        step_fixed_ticks(&mut app, 1);

        app.world_mut()
            .write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                gp,
                GamepadButton::LeftTrigger,
                1.0,
            )));
        app.update();
        step_fixed_ticks(&mut app, 1);
        assert!(
            !app.world().get::<PlayerIntent>(player).unwrap().guard_held,
            "ShieldButtonMode::LtOnly must completely ignore LeftTrigger (LB)"
        );
    }

    #[test]
    fn test_expanded_enemy_movesets_whirlwind_and_arcane_pillar() {
        use crate::combat::DualSidedHitbox;
        use crate::enemies::{
            EnemyArchetype, EnemyFighter, PendingArcanePillar, spawn_enemy_fighter,
        };
        use crate::physics::SolidRect;

        // 1. Iron Brawler on pattern_counter=1 unleashes 360° Whirlwind Greataxe Spin (`variant 6`)!
        let (mut app, _player) = physics_test_app(Vec2::new(1000.0, 240.0));
        app.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(800.0, 196.0), Vec2::new(1300.0, 220.0)));
        let brawler = {
            let mut commands = app.world_mut().commands();
            let id = spawn_enemy_fighter(
                &mut commands,
                EnemyArchetype::HeavyBrawler,
                Vec2::new(1055.0, 240.0),
                false,
            );
            app.world_mut().flush();
            id
        };
        {
            let mut f = app.world_mut().get_mut::<EnemyFighter>(brawler).unwrap();
            f.special_cooldown = 0.0;
            f.pattern_counter = 1; // Triggers Whirlwind Greataxe Spin!
        }
        step_fixed_ticks(&mut app, 45);
        assert_eq!(
            app.world().get::<EnemyFighter>(brawler).unwrap().attack_variant,
            6,
            "Iron Brawler must telegraph and unleash Whirlwind Greataxe Spin (variant 6)"
        );
        assert!(
            app.world_mut()
                .query::<&DualSidedHitbox>()
                .iter(app.world())
                .count()
                >= 1,
            "Brawler Whirlwind Spin must spawn a DualSidedHitbox"
        );

        // 2. Plasma Arcanist on pattern_counter=1 summons a delayed PendingArcanePillar under P1!
        let (mut app2, _p2) = physics_test_app(Vec2::new(880.0, 240.0));
        app2.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(780.0, 196.0), Vec2::new(1300.0, 220.0)));
        let mage = {
            let mut commands = app2.world_mut().commands();
            let id = spawn_enemy_fighter(
                &mut commands,
                EnemyArchetype::ArcaneZoner,
                Vec2::new(1060.0, 240.0),
                false,
            );
            app2.world_mut().flush();
            id
        };
        {
            let mut f = app2.world_mut().get_mut::<EnemyFighter>(mage).unwrap();
            f.special_cooldown = 0.0;
            f.pattern_counter = 1; // Triggers Arcane Pillar Eruption!
        }
        step_fixed_ticks(&mut app2, 42);
        assert!(
            app2.world_mut()
                .query::<&PendingArcanePillar>()
                .iter(app2.world())
                .count()
                >= 1,
            "Plasma Arcanist must summon a telegraphed PendingArcanePillar ground rune"
        );
    }

    // -------------------------------------------------------------------------
    // Full 8-Sector Stage 1 ("THE FOUNDRY CITADEL") Mechanics & Layout Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_full_stage1_foundry_citadel_features_and_counts() {
        use std::collections::HashMap;
        use bevy::asset::{AssetPlugin, LoadState};
        use bevy_ecs_ldtk::assets::{LdtkAssetPlugin, LdtkProject};
        use bevy_ecs_ldtk::prelude::LdtkFields;

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), LdtkAssetPlugin));
        app.finish();
        app.cleanup();

        let handle: Handle<LdtkProject> = app
            .world()
            .resource::<AssetServer>()
            .load("levels/demo_gym.ldtk");

        let mut loaded = false;
        for _ in 0..400 {
            app.update();
            if matches!(
                app.world().resource::<AssetServer>().get_load_state(&handle),
                Some(LoadState::Loaded)
            ) {
                loaded = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(loaded);

        let project = app
            .world()
            .resource::<Assets<LdtkProject>>()
            .get(&handle)
            .expect("LdtkProject loaded");
        let level = project.json_data().levels.first().unwrap();
        assert!(level.px_wid >= 9000, "Full 8-Sector Stage 1 must span >= 9,000px");

        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut cores = 0usize;
        let mut relics = 0usize;
        let mut heals = 0usize;

        for layer in level.layer_instances.as_ref().unwrap() {
            for ent in &layer.entity_instances {
                *counts.entry(ent.identifier.clone()).or_insert(0) += 1;
                if ent.identifier == "Collectible"
                    && let Ok(kind) = ent.get_string_field("kind")
                {
                    match kind.as_str() {
                        "relic" => relics += 1,
                        "heal" => heals += 1,
                        _ => cores += 1,
                    }
                }
            }
        }

        assert!(counts.get("JumpPad").copied().unwrap_or(0) >= 5, "Expected >= 5 JumpPads");
        assert_eq!(counts.get("CheckpointBeacon").copied().unwrap_or(0), 5, "Expected 5 Checkpoints");
        assert!(counts.get("BreakableWall").copied().unwrap_or(0) >= 3, "Expected >= 3 BreakableWalls");
        assert!(counts.get("TimedHazardBeam").copied().unwrap_or(0) >= 5, "Expected >= 5 TimedHazardBeams");
        assert!(counts.get("Slope").copied().unwrap_or(0) >= 3, "Expected >= 3 Slopes");
        assert!(counts.get("MovingPlatform").copied().unwrap_or(0) >= 4, "Expected >= 4 MovingPlatforms");
        assert!(counts.get("EnemySpawn").copied().unwrap_or(0) >= 12, "Expected >= 12 roaming enemies");
        assert_eq!(counts.get("EncounterZone").copied().unwrap_or(0), 3);
        assert!(cores >= 15, "Expected >= 15 Energy Cores, got {cores}");
        assert_eq!(relics, 3, "Expected exactly 3 hidden Secret Relics");
        assert!(heals >= 5, "Expected >= 5 Heal Canisters, got {heals}");
    }

    #[test]
    fn test_jump_pad_checkpoint_beacon_and_collectibles() {
        use crate::combat::DamagePercent;
        use crate::enemies::{ArenaScore, PlayerCheckpoint};
        use crate::physics::{
            CheckpointBeacon, CollectibleItem, CollectibleKind, JumpPad, KinematicBody,
            MovementState, SolidRect,
        };

        let (mut app, player) = physics_test_app(Vec2::new(0.0, 18.0));
        app.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-200.0, -32.0), Vec2::new(400.0, 0.0)));
        app.world_mut().get_mut::<DamagePercent>(player).unwrap().0 = 80.0;

        // 1. Activate Checkpoint Beacon -> saves respawn & heals -25%!
        let beacon = app
            .world_mut()
            .spawn(CheckpointBeacon::new(1, Vec2::new(10.0, 0.0)))
            .id();
        step_fixed_ticks(&mut app, 2);
        assert!(app.world().get::<CheckpointBeacon>(beacon).unwrap().activated);
        assert!((app.world().resource::<PlayerCheckpoint>().0.x - 10.0).abs() < 1.0);
        assert_eq!(app.world().get::<DamagePercent>(player).unwrap().0, 55.0);

        // 2. Collect EnergyCore (-5%), SecretRelic (-20%), and HealCanister (-40% -> clamps 0%)!
        app.world_mut().resource_mut::<ArenaScore>().cores_total = 10;
        app.world_mut().resource_mut::<ArenaScore>().relics_total = 3;
        app.world_mut()
            .spawn(CollectibleItem::new(CollectibleKind::EnergyCore, Vec2::new(2.0, 18.0)));
        app.world_mut()
            .spawn(CollectibleItem::new(CollectibleKind::SecretRelic, Vec2::new(4.0, 18.0)));
        app.world_mut()
            .spawn(CollectibleItem::new(CollectibleKind::HealCanister, Vec2::new(6.0, 18.0)));
        step_fixed_ticks(&mut app, 2);

        let score = *app.world().resource::<ArenaScore>();
        assert_eq!(score.cores_collected, 1);
        assert_eq!(score.relics_collected, 1);
        assert_eq!(app.world().get::<DamagePercent>(player).unwrap().0, 0.0);
        assert_eq!(
            app.world_mut().query::<&CollectibleItem>().iter(app.world()).count(),
            0
        );

        // 3. Step on JumpPad -> catapults sharply upward and refreshes double-jump!
        app.world_mut()
            .spawn(JumpPad::new(Vec2::new(0.0, 5.0), Vec2::new(40.0, 10.0), 600.0));
        app.world_mut()
            .get_mut::<KinematicBody>(player)
            .unwrap()
            .air_jumps_remaining = 0;
        step_fixed_ticks(&mut app, 1);
        let body = app.world().get::<KinematicBody>(player).unwrap();
        assert_eq!(body.state, MovementState::Airborne);
        assert!(body.velocity.y >= 550.0, "JumpPad must launch player sharply upward, got vy={}", body.velocity.y);
        assert_eq!(body.air_jumps_remaining, 1);

        // 4. Verify S-Rank calculation
        let s_score = ArenaScore {
            cores_collected: 16,
            cores_total: 18,
            relics_collected: 3,
            relics_total: 3,
            ..default()
        };
        assert_eq!(s_score.compute_stage_rank(150.0, 6), "S");
    }

    #[test]
    fn test_breakable_wall_shatters_and_timed_hazard_beam_cycles() {
        use crate::combat::DamagePercent;
        use crate::enemies::{ArenaScore, spawn_breakable_wall};
        use crate::physics::{
            BreakableWall, HazardBeamPhase, KinematicBody, MovementState, PlayerIntent, SolidRect,
            TimedHazardBeam,
        };

        // 1. BreakableWall shatters on melee attacks and clears its SolidRect!
        let (mut app, player) = physics_test_app(Vec2::new(0.0, 18.0));
        app.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-100.0, -32.0), Vec2::new(300.0, 0.0)));
        let wall = {
            let mut cmds = app.world_mut().commands();
            let id = spawn_breakable_wall(&mut cmds, Vec2::new(32.0, 30.0), Vec2::new(22.0, 60.0), 16.0);
            app.world_mut().flush();
            id
        };
        step_fixed_ticks(&mut app, 2);
        {
            let mut b = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
            b.state = MovementState::Grounded;
            b.facing = 1.0;
            b.ground_speed = 0.0;
        }
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.move_axis = Vec2::new(1.0, 0.0);
            intent.melee_just_pressed = true;
            intent.melee_held = false;
        }
        step_fixed_ticks(&mut app, 2);
        assert!(
            app.world().get::<BreakableWall>(wall).is_none(),
            "Forward-Smash (18 dmg) must shatter 16 HP BreakableWall cleanly!"
        );
        assert_eq!(app.world().resource::<ArenaScore>().walls_broken, 1);

        // 2. TimedHazardBeam cycles Off -> Telegraph -> Active, only hurting P1 while Active!
        let (mut app2, p2) = physics_test_app(Vec2::new(100.0, 18.0));
        app2.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(0.0, -32.0), Vec2::new(200.0, 0.0)));
        let beam = app2
            .world_mut()
            .spawn(TimedHazardBeam::new(Vec2::new(100.0, 45.0), Vec2::new(20.0, 90.0), 0.0))
            .id();
        assert_eq!(app2.world().get::<TimedHazardBeam>(beam).unwrap().phase(), HazardBeamPhase::Off);
        step_fixed_ticks(&mut app2, 30); // t = 0.50s -> still Off
        assert_eq!(app2.world().get::<DamagePercent>(p2).unwrap().0, 0.0);

        // Advance into Telegraph (t = 1.85s) -> still 0% damage!
        step_fixed_ticks(&mut app2, 81);
        assert_eq!(
            app2.world().get::<TimedHazardBeam>(beam).unwrap().phase(),
            HazardBeamPhase::Telegraph
        );
        assert_eq!(app2.world().get::<DamagePercent>(p2).unwrap().0, 0.0);

        // Advance into Active (t = 2.35s) -> triggers hazard damage & Smash launch!
        step_fixed_ticks(&mut app2, 30);
        assert_eq!(
            app2.world().get::<TimedHazardBeam>(beam).unwrap().phase(),
            HazardBeamPhase::Active
        );
        assert!(
            app2.world().get::<DamagePercent>(p2).unwrap().0 >= 10.0,
            "Active TimedHazardBeam must damage overlapping player"
        );
    }

    #[test]
    fn test_story_bible_comms_keepsakes_and_boss_dialogue() {
        use crate::enemies::{BlastEdge, BlastZoneKoEvent, EnemyArchetype};
        use crate::physics::{CollectibleItem, CollectibleKind, KinematicBody};
        use crate::story::{SpeakerId, StoryBeat, StoryCommsState};

        // Verify 2085 Story Bible cast & enemy roster display names
        assert_eq!(EnemyArchetype::LightStriker.display_name(), "STRIDER DRONE (MK-II)");
        assert_eq!(EnemyArchetype::HeavyBrawler.display_name(), "SLAG ENFORCER");
        assert_eq!(EnemyArchetype::ArcaneZoner.display_name(), "ARC WEAVER");
        assert_eq!(EnemyArchetype::ShadowAssassin.display_name(), "PHANTOM INTERCEPTOR");
        assert_eq!(EnemyArchetype::FoundryBoss.display_name(), "IRON TITAN: VULCANUS");
        let player_ko = BlastZoneKoEvent {
            is_player: true,
            archetype: None,
            ko_pos: Vec2::ZERO,
            edge: BlastEdge::Bottom,
            final_percent: 110.0,
        };
        assert_eq!(player_ko.display_name(), "P1 LEVY");

        // Entering Playing triggers Stage 1 Intro (Mina -> Barnaby -> Levy)
        let (mut app, player) = physics_test_app(Vec2::new(0.0, 20.0));
        {
            let comms = app.world().resource::<StoryCommsState>();
            assert!(comms.triggered.contains(&StoryBeat::Stage1Intro));
            assert_eq!(comms.current.unwrap().speaker, SpeakerId::Mina);
            assert_eq!(comms.queue.len(), 2);
        }

        // Advance through Mina -> Barnaby -> Levy -> empty
        for expected in [SpeakerId::Barnaby, SpeakerId::Levy] {
            let mut comms = app.world_mut().resource_mut::<StoryCommsState>();
            comms.advance_or_skip(); // finish typewriter
            comms.advance_or_skip(); // next line
            assert_eq!(comms.current.unwrap().speaker, expected);
        }

        // Collect all 3 Stage 1 keepsakes at Sector 2, Sector 4, and Sector 6 coordinates
        for relic_x in [720.0_f32, 2600.0_f32, 5700.0_f32] {
            app.world_mut()
                .spawn(CollectibleItem::new(CollectibleKind::SecretRelic, Vec2::new(relic_x, 20.0)));
            app.world_mut().get_mut::<KinematicBody>(player).unwrap().position =
                Vec2::new(relic_x, 20.0);
            step_fixed_ticks(&mut app, 2);
        }
        assert!(
            app.world().resource::<StoryCommsState>().all_stage1_relics_found(),
            "Collecting Sector 2, 4, and 6 relics must record all three 1700s keepsakes"
        );

        // Entering Sector 8 Sanctum threshold fires verbatim 4-line Vulcanus pre-boss confrontation
        app.world_mut().get_mut::<KinematicBody>(player).unwrap().position =
            Vec2::new(8380.0, 20.0);
        step_fixed_ticks(&mut app, 2);
        {
            let comms = app.world().resource::<StoryCommsState>();
            assert!(comms.triggered.contains(&StoryBeat::BossVulcanusIntro));
            let cur = comms.current.unwrap();
            assert_eq!(cur.speaker, SpeakerId::Vulcanus);
            assert!(cur.text.contains("UNREGISTERED ORGANIC DETECTED"));
            assert_eq!(comms.queue.len(), 3);
        }
    }

    #[test]
    fn test_instant_advancing_sword_combo_c_stick_heavy_smash_and_telegraph() {
        use std::path::Path;
        use crate::combat::{AttackKind, AttackState, SmashChargeState};
        use crate::enemies::{EnemyArchetype, EnemyFighter, spawn_enemy_fighter};
        use crate::physics::{KinematicBody, MovementState, PlayerIntent, SolidRect};
        use crate::sprite_manifest::SpriteSheetManifest;

        // 1. Walking forward + pressing Melee (even while melee_held=true!) fires instant
        //    3-hit Rift Saber Combo (Jab1 -> Jab2 -> Jab3) with 7-frame input buffering — NEVER freezes!
        let (mut app, player) = physics_test_app(Vec2::new(0.0, 18.0));
        app.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-300.0, -32.0), Vec2::new(600.0, 0.0)));
        step_fixed_ticks(&mut app, 2);
        {
            let mut body = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
            body.state = MovementState::Grounded;
            body.ground_speed = 110.0;
            body.velocity = Vec2::new(110.0, 0.0);
            body.facing = 1.0;
        }
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.move_axis = Vec2::new(1.0, 0.0);
            intent.melee_just_pressed = true;
            intent.melee_held = true;
        }
        step_fixed_ticks(&mut app, 1);
        assert!(
            !app.world().get::<SmashChargeState>(player).unwrap().is_charging(),
            "Walking forward + Melee must NOT stall in SmashChargeState!"
        );
        assert_eq!(
            app.world().get::<AttackState>(player).unwrap().active_move,
            Some(AttackKind::Jab1),
            "Walking forward + Melee must immediately swing Rift Slash I (Jab1)"
        );

        // Buffer 2nd press slightly before Jab1 cooldown expires -> chains cleanly into Jab2!
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.melee_just_pressed = false;
        }
        step_fixed_ticks(&mut app, 6);
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.melee_just_pressed = true;
        }
        step_fixed_ticks(&mut app, 1);
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.melee_just_pressed = false;
        }
        step_fixed_ticks(&mut app, 4);
        assert_eq!(
            app.world().get::<AttackState>(player).unwrap().active_move,
            Some(AttackKind::Jab2),
            "Buffered Melee press must chain seamlessly into Rising Cross II (Jab2)"
        );

        // Chain 3rd hit -> Breaker Cleave III (Jab3)!
        step_fixed_ticks(&mut app, 7);
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.melee_just_pressed = true;
        }
        step_fixed_ticks(&mut app, 1);
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.melee_just_pressed = false;
        }
        step_fixed_ticks(&mut app, 4);
        assert_eq!(
            app.world().get::<AttackState>(player).unwrap().active_move,
            Some(AttackKind::Jab3),
            "3rd combo hit must unleash Breaker Cleave III (Jab3)"
        );

        // 2. Up + Melee & Down + Melee with melee_held=true fire ZERO-LAG UpTilt & DownTilt on Frame 1!
        let (mut app_tilt, p_tilt) = physics_test_app(Vec2::new(0.0, 18.0));
        app_tilt
            .world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-200.0, -32.0), Vec2::new(200.0, 0.0)));
        step_fixed_ticks(&mut app_tilt, 2);
        {
            let mut intent = app_tilt.world_mut().get_mut::<PlayerIntent>(p_tilt).unwrap();
            intent.move_axis = Vec2::new(0.0, 1.0);
            intent.melee_just_pressed = true;
            intent.melee_held = true;
        }
        step_fixed_ticks(&mut app_tilt, 1);
        assert_eq!(
            app_tilt.world().get::<AttackState>(p_tilt).unwrap().active_move,
            Some(AttackKind::UpTilt),
            "Up + Melee must swing UpTilt immediately with zero charge-hold delay!"
        );

        // 3. Dedicated HeavySmash (`U`/`V`) holds to charge & releases UpSmash cleanly,
        //    and Right-Stick C-Stick triggers instant grounded DownSmash & airborne BackAir!
        let (mut app_heavy, p_heavy) = physics_test_app(Vec2::new(0.0, 18.0));
        app_heavy
            .world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-200.0, -32.0), Vec2::new(200.0, 0.0)));
        step_fixed_ticks(&mut app_heavy, 2);
        {
            let mut intent = app_heavy.world_mut().get_mut::<PlayerIntent>(p_heavy).unwrap();
            intent.move_axis = Vec2::new(0.0, 1.0);
            intent.heavy_just_pressed = true;
            intent.heavy_held = true;
        }
        step_fixed_ticks(&mut app_heavy, 1);
        assert!(app_heavy.world().get::<SmashChargeState>(p_heavy).unwrap().is_charging());
        {
            let mut intent = app_heavy.world_mut().get_mut::<PlayerIntent>(p_heavy).unwrap();
            intent.heavy_just_pressed = false;
        }
        step_fixed_ticks(&mut app_heavy, 12);
        {
            let mut intent = app_heavy.world_mut().get_mut::<PlayerIntent>(p_heavy).unwrap();
            intent.heavy_held = false;
        }
        step_fixed_ticks(&mut app_heavy, 1);
        assert_eq!(
            app_heavy.world().get::<AttackState>(p_heavy).unwrap().active_move,
            Some(AttackKind::UpSmash),
            "Dedicated HeavySmash + Up must charge and release UpSmash!"
        );

        // Right-Stick C-Stick airborne Back-Air test
        let (mut app_cs, p_cs) = physics_test_app(Vec2::new(0.0, 100.0));
        {
            let mut b = app_cs.world_mut().get_mut::<KinematicBody>(p_cs).unwrap();
            b.state = MovementState::Airborne;
            b.facing = 1.0;
        }
        {
            let mut intent = app_cs.world_mut().get_mut::<PlayerIntent>(p_cs).unwrap();
            intent.c_stick_axis = Vec2::new(-1.0, 0.0);
            intent.c_stick_just_flicked = true;
        }
        step_fixed_ticks(&mut app_cs, 1);
        assert_eq!(
            app_cs.world().get::<AttackState>(p_cs).unwrap().active_move,
            Some(AttackKind::BackAir),
            "Flicking Right-Stick behind Levy in midair must fire instant Back-Air!"
        );

        // 4. Patrolling enemies actively pace instead of freezing like statues!
        let (mut app_pat, _p_pat) = physics_test_app(Vec2::new(-900.0, 18.0));
        app_pat
            .world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-1000.0, -32.0), Vec2::new(500.0, 0.0)));
        let drone = {
            let mut cmds = app_pat.world_mut().commands();
            let id = spawn_enemy_fighter(&mut cmds, EnemyArchetype::LightStriker, Vec2::new(200.0, 22.0), false);
            app_pat.world_mut().flush();
            id
        };
        step_fixed_ticks(&mut app_pat, 12);
        assert!(
            app_pat.world().get::<KinematicBody>(drone).unwrap().speed() > 15.0,
            "Patrolling enemies must actively pace around their post instead of standing frozen!"
        );
        let _ = app_pat.world().get::<EnemyFighter>(drone);

        // 5. All 6 freshly baked 29-clip character sprite sheets contain the new `telegraph` windup clip!
        for pack in ["hero", "rogue", "barbarian", "mage", "assassin", "boss"] {
            let manifest = SpriteSheetManifest::load_from_file(
                Path::new("assets/sprites").join(pack).join(format!("{pack}.ron")),
            )
            .unwrap_or_else(|e| panic!("Baked {pack}.ron failed to load: {e}"));
            assert!(
                manifest.animations.contains_key("telegraph"),
                "Sprite pack '{pack}' must include newly baked 'telegraph' windup animation!"
            );
        }
    }

    #[test]
    fn test_enemy_never_walks_off_narrow_platform_and_foundry_textures_exist() {
        use std::path::Path;
        use crate::enemies::{EnemyArchetype, spawn_enemy_fighter};
        use crate::physics::{KinematicBody, MovementState, OneWayPlatform};

        // 1. Verify enemy AI stops cleanly on a narrow 100px platform during BOTH Patrol and Chase!
        //    (Platform spans x = 150..250, top y = 100; player waits far right at x = 420 across the chasm!)
        let (mut app, player) = physics_test_app(Vec2::new(420.0, 118.0));
        app.world_mut()
            .spawn(OneWayPlatform::new(Vec2::new(200.0, 96.0), 100.0, 8.0));
        app.world_mut()
            .spawn(OneWayPlatform::new(Vec2::new(420.0, 96.0), 100.0, 8.0));

        let drone = {
            let mut cmds = app.world_mut().commands();
            let id = spawn_enemy_fighter(&mut cmds, EnemyArchetype::LightStriker, Vec2::new(200.0, 120.0), false);
            app.world_mut().flush();
            id
        };

        // Run 120 ticks (2 full seconds) of active Chase toward player across the chasm!
        step_fixed_ticks(&mut app, 120);
        let eb = app.world().get::<KinematicBody>(drone).unwrap();
        assert_eq!(
            eb.state,
            MovementState::Grounded,
            "Enemy chasing across a gap must NEVER walk off its platform edge (pos={:?})",
            eb.position
        );
        assert!(
            eb.position.x >= 150.0 && eb.position.x <= 250.0,
            "Enemy must remain safely on its 150..250 platform deck, got x={}",
            eb.position.x
        );

        // Move player far away so enemy enters Patrol -> run 180 ticks (3 full seconds) of pacing!
        app.world_mut().get_mut::<KinematicBody>(player).unwrap().position =
            Vec2::new(1400.0, 118.0);
        step_fixed_ticks(&mut app, 180);
        let eb2 = app.world().get::<KinematicBody>(drone).unwrap();
        assert_eq!(
            eb2.state,
            MovementState::Grounded,
            "Patrolling enemy must reverse cleanly at ledges and never walk off (pos={:?})",
            eb2.position
        );
        assert!(
            eb2.position.x >= 150.0 && eb2.position.x <= 250.0,
            "Patrolling enemy must stay on its 150..250 platform, got x={}",
            eb2.position.x
        );

        // 2. Verify all 19 Stage 1 Iron Foundry pixel-art textures & parallax assets exist!
        for tex in [
            "foundry_bg_far.png",
            "foundry_bg_mid.png",
            "foundry_bg_near.png",
            "foundry_tile_metal.png",
            "foundry_tile_trim_top.png",
            "foundry_tile_wall.png",
            "foundry_platform_oneway.png",
            "foundry_platform_moving.png",
            "foundry_breakable_wall.png",
            "foundry_boost_pad.png",
            "foundry_jump_pad.png",
            "foundry_spikes.png",
            "foundry_turret.png",
            "foundry_checkpoint.png",
            "foundry_core.png",
            "foundry_relic.png",
            "foundry_heal.png",
            "foundry_lava_strip.png",
            "foundry_molten_cascade.png",
            "foundry_lamp_fixture.png",
        ] {
            let p = Path::new("assets/textures/foundry").join(tex);
            assert!(p.exists(), "Expected Stage 1 Foundry texture at {}", p.display());
            assert!(
                std::fs::metadata(&p).expect("readable").len() > 80,
                "Stage 1 Foundry texture {} must be non-empty",
                p.display()
            );
        }
    }

    #[test]
    fn test_stage1_polish_comms_read_time_sector5_ceiling_and_top_blast_safety() {
        use bevy::asset::{AssetPlugin, LoadState};
        use bevy_ecs_ldtk::assets::{LdtkAssetPlugin, LdtkProject};
        use crate::combat::Hitstun;
        use crate::enemies::BlastZone;
        use crate::level::ldtk_rect_to_bevy_min_max;
        use crate::physics::KinematicBody;
        use crate::player::PlayerRespawnState;
        use crate::story::{StoryBeat, script_for_beat};

        // 1. Keepsake pickups & comms banners give generous reading time mid-platforming!
        for relic_beat in [
            StoryBeat::Relic1Compass,
            StoryBeat::Relic2Portrait,
            StoryBeat::Relic3Locket,
        ] {
            for line in script_for_beat(relic_beat) {
                assert!(
                    line.duration_secs >= 5.5,
                    "Keepsake dialogue {:?} must last >= 5.5s for comfortable reading, got {}",
                    relic_beat,
                    line.duration_secs
                );
            }
        }
        for beat in [
            StoryBeat::Stage1Intro,
            StoryBeat::BossVulcanusIntro,
            StoryBeat::BossVulcanusDefeated,
        ] {
            for line in script_for_beat(beat) {
                assert!(
                    line.duration_secs >= 4.5,
                    "Comms line in {:?} must last >= 4.5s, got {}",
                    beat,
                    line.duration_secs
                );
            }
        }

        // 2. LDtk Stage 1 BlastZone top >= 900.0 and Sector 5 upper vault ceiling leaves >= 180px above deck!
        let mut ldtk_app = App::new();
        ldtk_app.add_plugins((MinimalPlugins, AssetPlugin::default(), LdtkAssetPlugin));
        ldtk_app.finish();
        ldtk_app.cleanup();
        let handle: Handle<LdtkProject> = ldtk_app
            .world()
            .resource::<AssetServer>()
            .load("levels/demo_gym.ldtk");
        let mut loaded = false;
        for _ in 0..400 {
            ldtk_app.update();
            if matches!(
                ldtk_app.world().resource::<AssetServer>().get_load_state(&handle),
                Some(LoadState::Loaded)
            ) {
                loaded = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(loaded, "demo_gym.ldtk must load cleanly after Sector 5 headroom raise");

        let project = ldtk_app
            .world()
            .resource::<Assets<LdtkProject>>()
            .get(&handle)
            .unwrap();
        let level = project.json_data().levels.first().unwrap();
        let mut blast_max_y: Option<f32> = None;
        let mut sector5_ceiling_min_y: Option<f32> = None;
        for layer in level.layer_instances.as_ref().unwrap() {
            for ent in &layer.entity_instances {
                let (min, max) = ldtk_rect_to_bevy_min_max(ent);
                if ent.identifier == "BlastZone" {
                    blast_max_y = Some(max.y);
                } else if ent.identifier == "SolidBlock"
                    && (min.x - 5040.0).abs() < 4.0
                    && (max.x - 5280.0).abs() < 4.0
                {
                    sector5_ceiling_min_y = Some(min.y);
                }
            }
        }
        let top_y = blast_max_y.expect("BlastZone entity found");
        assert!(
            top_y >= 900.0,
            "Upper BlastZone must be raised >= 900.0 for generous Sector 5/6 headroom, got {}",
            top_y
        );
        let ceil_y = sector5_ceiling_min_y.expect("Sector 5 upper vault SolidBlock found");
        assert!(
            ceil_y >= 520.0,
            "Sector 5 ceiling bottom must sit >= 520.0 (>= 180px above y=340 deck), got {}",
            ceil_y
        );

        // 3. Voluntary high jump / Up-B past top BlastZone without Hitstun must NOT ring-out P1,
        //    whereas being launched past the top while in Hitstun DOES Star-KO!
        let (mut app, player) = physics_test_app(Vec2::new(4950.0, 500.0));
        app.world_mut().spawn(BlastZone {
            min: Vec2::new(-360.0, -180.0),
            max: Vec2::new(9340.0, 960.0),
            arena_center: Vec2::new(1420.0, 312.0),
        });
        app.world_mut().get_mut::<KinematicBody>(player).unwrap().position =
            Vec2::new(4950.0, 990.0);
        step_fixed_ticks(&mut app, 1);
        assert!(
            !app.world().get::<PlayerRespawnState>(player).unwrap().active,
            "Voluntary jump / Up-B above top blast line without Hitstun must NEVER Star-KO P1!"
        );

        // Now apply active Hitstun above the top blast line -> authentic Smash Star-KO triggers!
        app.world_mut()
            .entity_mut(player)
            .insert(Hitstun::new(0.40));
        app.world_mut().get_mut::<KinematicBody>(player).unwrap().position =
            Vec2::new(4950.0, 990.0);
        step_fixed_ticks(&mut app, 1);
        assert!(
            app.world().get::<PlayerRespawnState>(player).unwrap().active,
            "Being launched past top BlastZone in active Hitstun must still trigger Smash Star-KO!"
        );
    }

    #[test]
    fn test_meteor_dive_onto_jump_pad_cancels_dive_and_restores_air_control() {
        use crate::combat::MeteorDiveState;
        use crate::physics::{JumpPad, KinematicBody, MovementState, PlayerIntent, SolidRect};

        // Repro: player performs airborne Down-B (Meteor Breaker Dive) straight onto a JumpPad.
        // Previously, JumpPad bounced Levy upward while Airborne, leaving MeteorDiveState.active=true
        // forever -> locking out air movement and causing an infinite up-and-down JumpPad loop!
        let (mut app, player) = physics_test_app(Vec2::new(100.0, 90.0));
        app.world_mut()
            .spawn(SolidRect::from_min_max(Vec2::new(-200.0, -32.0), Vec2::new(400.0, 0.0)));
        app.world_mut()
            .spawn(JumpPad::new(Vec2::new(100.0, 5.0), Vec2::new(42.0, 10.0), 615.0));

        {
            let mut body = app.world_mut().get_mut::<KinematicBody>(player).unwrap();
            body.state = MovementState::Airborne;
            body.velocity = Vec2::ZERO;
        }
        // Trigger airborne Down-B (Meteor Breaker Dive) directly above the JumpPad
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.move_axis = Vec2::new(0.0, -1.0);
            intent.special_just_pressed = true;
        }
        step_fixed_ticks(&mut app, 1);
        assert!(
            app.world().get::<MeteorDiveState>(player).unwrap().active,
            "Airborne Down-B must enter MeteorDiveState"
        );

        // Release Down-B and hold Right to steer off the JumpPad once launched
        {
            let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
            intent.move_axis = Vec2::new(1.0, 0.0);
            intent.special_just_pressed = false;
        }

        // Step enough ticks to dive onto the JumpPad, get launched upward, and steer right!
        step_fixed_ticks(&mut app, 25);

        let dive = app.world().get::<MeteorDiveState>(player).unwrap();
        let body = app.world().get::<KinematicBody>(player).unwrap();
        assert!(
            !dive.active,
            "Hitting a JumpPad during MeteorDiveState must cancel dive immediately (no infinite bounce lock!)"
        );
        assert!(
            body.position.x > 135.0,
            "Player must retain full horizontal air control after JumpPad launch (got x={})",
            body.position.x
        );
    }

    #[test]
    fn test_vfx_textures_energy_shader_and_all_28_sfx_assets_exist() {
        use std::path::Path;

        // 1. Custom GPU energy/plasma/shield/crescent WGSL shader exists
        let shader = Path::new("assets/shaders/vfx_energy.wgsl");
        assert!(shader.exists(), "Expected GPU energy shader at {}", shader.display());
        assert!(std::fs::metadata(shader).unwrap().len() > 2000);

        // 2. All 12 sculpted 32-bit pixel-art VFX textures exist & are non-empty
        for tex in [
            "vfx_buster_pellet.png",
            "vfx_buster_charged.png",
            "vfx_turret_bolt.png",
            "vfx_arcane_orb.png",
            "vfx_magma_wave.png",
            "vfx_emitter_nozzle.png",
            "vfx_respawn_platform.png",
            "vfx_beacon_crystal.png",
            "vfx_shadow_oval.png",
            "vfx_smoke_puff.png",
            "vfx_star_spark.png",
            "vfx_p1_marker.png",
        ] {
            let p = Path::new("assets/textures/vfx").join(tex);
            assert!(p.exists(), "Expected pixel-art VFX texture at {}", p.display());
            assert!(
                std::fs::metadata(&p).expect("readable").len() > 80,
                "VFX texture {} must be non-empty",
                p.display()
            );
        }

        // 3. All 28 gameplay SFX WAV files exist on disk (zero raw 1-oscillator fallback beeps)
        for sfx in [
            "hit_light.wav",
            "hit_heavy.wav",
            "buster_pellet.wav",
            "buster_charged.wav",
            "ko_blast.wav",
            "hazard_hit.wav",
            "jump.wav",
            "boost_pad.wav",
            "land.wav",
            "spin_rev.wav",
            "shield_block.wav",
            "air_dodge.wav",
            "slash_light.wav",
            "slash_heavy.wav",
            "counter_boom.wav",
            "ground_slam.wav",
            "side_blitz.wav",
            "enemy_blink.wav",
            "boss_laser_charge.wav",
            "cyclone_spin.wav",
            "smash_release.wav",
            "fast_fall.wav",
            "arcane_erupt.wav",
            "jump_pad.wav",
            "checkpoint.wav",
            "collect_core.wav",
            "collect_relic.wav",
            "wall_break.wav",
        ] {
            let p = Path::new("assets/audio/sfx").join(sfx);
            assert!(p.exists(), "Expected layered SFX WAV at {}", p.display());
            assert!(
                std::fs::metadata(&p).expect("readable").len() > 2000,
                "SFX WAV {} must contain rich audio samples",
                p.display()
            );
        }
    }

    #[test]
    fn test_stage2_cryo_conduit_ldtk_assets_and_borealis_sprites_exist() {
        use std::path::Path;
        use crate::sprite_manifest::SpriteSheetManifest;

        // 1. Stage 2 LDtk level file exists & contains Borealis boss + icy decks + 3 keepsake relics
        let ldtk_raw = std::fs::read_to_string("assets/levels/cryo_conduit.ldtk")
            .expect("assets/levels/cryo_conduit.ldtk must exist");
        assert!(ldtk_raw.contains("borealis_boss"), "Stage 2 LDtk must contain borealis_boss encounter");
        assert!(ldtk_raw.contains("cryo_server_farm"), "Stage 2 LDtk must contain cryo_server_farm ambush");
        assert!(ldtk_raw.contains("cryo_vault"), "Stage 2 LDtk must contain cryo_vault ambush");
        assert!(ldtk_raw.contains("\"__identifier\": \"icy\""), "Stage 2 LDtk must declare icy surface fields");
        assert!(ldtk_raw.contains("SUB-ZERO WARDEN: BOREALIS"));

        // 2. All 18 Cryo Conduit environment textures exist on disk
        for tex in [
            "cryo_bg_far.png",
            "cryo_bg_mid.png",
            "cryo_bg_near.png",
            "cryo_tile_metal.png",
            "cryo_tile_trim_top.png",
            "cryo_tile_ice_top.png",
            "cryo_tile_wall.png",
            "cryo_platform_oneway.png",
            "cryo_platform_moving.png",
            "cryo_breakable_wall.png",
            "cryo_boost_pad.png",
            "cryo_jump_pad.png",
            "cryo_spikes.png",
            "cryo_turret.png",
            "cryo_checkpoint.png",
            "cryo_lava_strip.png",
            "cryo_molten_cascade.png",
            "cryo_lamp_fixture.png",
        ] {
            let p = Path::new("assets/textures/cryo").join(tex);
            assert!(p.exists(), "Expected Stage 2 Cryo texture at {}", p.display());
            assert!(std::fs::metadata(&p).unwrap().len() > 80);
        }

        // 3. Borealis HUD portrait + 144x144 3-pass baked sprite sheet & manifest
        assert!(Path::new("assets/ui/portraits/borealis.png").exists());
        for suffix in ["borealis_diffuse.png", "borealis_normal.png", "borealis_emission.png"] {
            let p = Path::new("assets/sprites/borealis").join(suffix);
            assert!(p.exists(), "Expected Borealis baked sprite {}", p.display());
        }
        let manifest = SpriteSheetManifest::load_from_file("assets/sprites/borealis/borealis.ron")
            .expect("Borealis sprite manifest must load");
        assert_eq!(manifest.frame_size, (144, 144));
        assert!(manifest.animations.len() >= 25, "Borealis must contain full combat animation roster");

        // 4. Stage 2 & Borealis Boss OGG music tracks exist
        for track in ["stage2_theme.ogg", "boss_borealis.ogg"] {
            let p = Path::new("assets/audio/music").join(track);
            assert!(p.exists(), "Expected Stage 2 BGM at {}", p.display());
            assert!(std::fs::metadata(&p).unwrap().len() > 10_000);
        }
    }

    #[test]
    fn test_ice_surface_low_friction_slide_and_higher_sprint_speed() {
        use crate::physics::{IceSurface, KinematicBody, MovementState, PlayerIntent, SolidRect};

        let run_slide = |icy: bool| -> (bool, f32, f32) {
            let (mut app, player) = physics_test_app(Vec2::new(0.0, 20.0));
            let mut floor = app.world_mut().spawn(SolidRect::from_min_max(
                Vec2::new(-100.0, -32.0),
                Vec2::new(1400.0, 0.0),
            ));
            if icy {
                floor.insert(IceSurface);
            }
            // Settle onto ground
            step_fixed_ticks(&mut app, 6);
            // Run right for 30 ticks
            {
                let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
                intent.move_axis = Vec2::new(1.0, 0.0);
                intent.boost_held = true;
            }
            step_fixed_ticks(&mut app, 35);
            let peak_speed = app.world().get::<KinematicBody>(player).unwrap().speed();
            let on_ice = app.world().get::<KinematicBody>(player).unwrap().on_ice;
            let release_x = app.world().get::<KinematicBody>(player).unwrap().position.x;

            // Release stick and measure glide distance over 20 ticks
            {
                let mut intent = app.world_mut().get_mut::<PlayerIntent>(player).unwrap();
                intent.move_axis = Vec2::ZERO;
                intent.boost_held = false;
            }
            step_fixed_ticks(&mut app, 20);
            let body = app.world().get::<KinematicBody>(player).unwrap();
            assert_eq!(body.state, MovementState::Grounded);
            (on_ice, peak_speed, body.position.x - release_x)
        };

        let (norm_ice, _norm_peak, norm_glide) = run_slide(false);
        let (icy_flag, _icy_peak, icy_glide) = run_slide(true);
        assert!(!norm_ice, "Regular iron deck must report on_ice=false");
        assert!(icy_flag, "Mirror-Ice deck must report on_ice=true");
        assert!(
            icy_glide > norm_glide * 1.45,
            "Mirror-Ice must produce noticeably longer low-friction slides (ice={icy_glide:.1}px vs normal={norm_glide:.1}px)"
        );
    }

    #[test]
    fn test_aspect_diamond_unlock_cycling_and_forge_thermal_melt_2x() {
        use crate::combat::CombatHitEvent;
        use crate::enemies::spawn_breakable_wall;
        use crate::physics::{BreakableWall, PlayerIntent};
        use crate::state::{AspectDiamond, CampaignState, StageId};

        let mut app = headless_test_app();
        app.world_mut().write_message(GameStateCommand::StartGame);
        app.update();

        // Fresh Stage 1 starts with Rift only
        {
            let camp = app.world().resource::<CampaignState>();
            assert_eq!(camp.active_stage, StageId::Stage1Foundry);
            assert_eq!(camp.active_diamond, AspectDiamond::Rift);
            assert!(camp.is_diamond_unlocked(AspectDiamond::Rift));
            assert!(!camp.is_diamond_unlocked(AspectDiamond::Forge));
            assert!(!camp.is_diamond_unlocked(AspectDiamond::Stasis));
        }

        // Clearing Stage 1 unlocks Forge Diamond!
        app.world_mut().write_message(GameStateCommand::StageCleared);
        app.update();
        assert!(
            app.world()
                .resource::<CampaignState>()
                .is_diamond_unlocked(AspectDiamond::Forge)
        );

        // Enter Playing and press CycleDiamond -> swaps Rift -> Forge!
        app.world_mut().write_message(GameStateCommand::StartGame);
        app.update();
        let player = app
            .world_mut()
            .query_filtered::<Entity, With<crate::player::Player>>()
            .single(app.world())
            .unwrap();
        app.world_mut()
            .entity_mut(player)
            .insert(crate::physics::ManualPlayerIntent);
        app.world_mut()
            .get_mut::<PlayerIntent>(player)
            .unwrap()
            .cycle_diamond_just_pressed = true;
        step_fixed_ticks(&mut app, 1);
        app.world_mut()
            .get_mut::<PlayerIntent>(player)
            .unwrap()
            .cycle_diamond_just_pressed = false;
        assert_eq!(
            app.world().resource::<CampaignState>().active_diamond,
            AspectDiamond::Forge
        );

        // Verify Forge Diamond deals 2.0x thermal shatter damage to BreakableWall!
        let wall = {
            let mut cmds = app.world_mut().commands();
            spawn_breakable_wall(&mut cmds, Vec2::new(300.0, 20.0), Vec2::new(28.0, 72.0), 40.0)
        };
        app.update();
        app.world_mut().write_message(CombatHitEvent {
            attacker: player,
            target: wall,
            hit_pos: Vec2::new(300.0, 20.0),
            damage: 10.0,
            target_percent_after: 10.0,
            knockback_mag: 150.0,
            knockback_vec: Vec2::new(150.0, 0.0),
            hitstop_frames: 4,
            is_charged: false,
        });
        step_fixed_ticks(&mut app, 1);
        let remaining_hp = app.world().get::<BreakableWall>(wall).unwrap().hp;
        assert!(
            (remaining_hp - 20.0).abs() < 0.01,
            "Forge Diamond must deal 2.0x damage (20 dmg) to 40 HP bulkhead, leaving 20 HP (got {remaining_hp})"
        );
    }

    #[test]
    fn test_stage2_select_borealis_boss_story_and_music() {
        use crate::audio::music::{MusicTrackId, resolve_desired_track_for_stage};
        use crate::combat::CombatHitEvent;
        use crate::enemies::{BossState, EncounterStatus, EnemyArchetype, EnemyHealth, spawn_cryo_boss};
        use crate::state::{AspectDiamond, CampaignState, StageId, StageSwitchEvent};
        use crate::story::{SpeakerId, StoryBeat, StoryCommsState, script_for_beat, stage_relic_names};

        let mut app = headless_test_app();

        // Select Stage 2 directly from Title -> enters Playing, switches stage, auto-equips Forge!
        app.world_mut()
            .write_message(GameStateCommand::SelectStageAndPlay(StageId::Stage2Cryo));
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppState>>().get(),
            AppState::Playing
        );
        {
            let camp = app.world().resource::<CampaignState>();
            assert_eq!(camp.active_stage, StageId::Stage2Cryo);
            assert_eq!(camp.active_diamond, AspectDiamond::Forge);
            assert!(camp.is_diamond_unlocked(AspectDiamond::Forge));
        }

        // Story Bible Stage 2 intro & keepsakes
        assert_eq!(stage_relic_names(StageId::Stage2Cryo).len(), 3);
        assert!(
            script_for_beat(StoryBeat::BossBorealisIntro)
                .iter()
                .any(|l| l.speaker == SpeakerId::Borealis)
        );
        let comms = app.world().resource::<StoryCommsState>();
        assert!(
            comms.triggered.contains(&StoryBeat::Stage2Intro),
            "Entering Stage 2 must automatically queue Stage2Intro comms!"
        );

        // Stage 2 & Borealis Boss music resolution
        let mut enc = EncounterStatus::default();
        assert_eq!(
            resolve_desired_track_for_stage(AppState::Playing, StageId::Stage2Cryo, &enc, None),
            Some(MusicTrackId::Stage2)
        );
        enc.is_boss = true;
        enc.active_title = Some("SUB-ZERO WARDEN: BOREALIS".to_string());
        assert_eq!(
            resolve_desired_track_for_stage(AppState::Playing, StageId::Stage2Cryo, &enc, None),
            Some(MusicTrackId::BossBorealis)
        );

        // Spawn Borealis Boss and verify Phase 2 Absolute Zero Overdrive triggers at <= 55% HP!
        let borealis = {
            let mut cmds = app.world_mut().commands();
            spawn_cryo_boss(&mut cmds, Vec2::new(9000.0, 260.0), 8540.0, 9220.0)
        };
        app.update();
        assert_eq!(
            app.world().get::<EnemyHealth>(borealis).unwrap().max,
            EnemyArchetype::CryoBoss.max_hp()
        );
        let player = app
            .world_mut()
            .query_filtered::<Entity, With<crate::player::Player>>()
            .single(app.world())
            .unwrap();
        app.world_mut().write_message(CombatHitEvent {
            attacker: player,
            target: borealis,
            hit_pos: Vec2::new(9000.0, 260.0),
            damage: 140.0, // drops 280 HP -> 140 HP (50% <= 55%)
            target_percent_after: 140.0,
            knockback_mag: 220.0,
            knockback_vec: Vec2::new(220.0, 60.0),
            hitstop_frames: 6,
            is_charged: true,
        });
        step_fixed_ticks(&mut app, 1);
        assert_eq!(
            app.world().get::<BossState>(borealis).unwrap().phase,
            2,
            "Borealis must enter Phase 2 Absolute Zero Overdrive at <= 55% HP"
        );

        // Clearing Stage 2 unlocks the Stasis Diamond!
        app.world_mut().write_message(GameStateCommand::StageCleared);
        app.update();
        assert!(
            app.world()
                .resource::<CampaignState>()
                .is_diamond_unlocked(AspectDiamond::Stasis),
            "Clearing Stage 2 must unlock the Stasis Diamond!"
        );
        let _ = StageSwitchEvent(StageId::Stage1Foundry);
    }
}
