//! Binary entrypoint for *VELOCITY // BREAKER*.
//!
//! On macOS, uses Apple's official native `GameController.framework` via `bevy_gamepad`
//! (disabling `GilrsPlugin` to avoid duplicate devices and macOS IOHIDManager D-Pad quirks).

#[cfg(target_os = "macos")]
use bevy::gilrs::GilrsPlugin;
use bevy::prelude::*;
use bevy::window::WindowResizeConstraints;
use velocity_breaker::VelocityBreakerPlugin;
use velocity_breaker::rendering::{VIRTUAL_HEIGHT, VIRTUAL_WIDTH};

fn main() {
    let mut app = App::new();

    let default_plugins = DefaultPlugins
        .set(ImagePlugin::default_nearest())
        .set(WindowPlugin {
            primary_window: Some(Window {
                title: "VELOCITY // BREAKER".to_string(),
                resolution: (VIRTUAL_WIDTH * 2, VIRTUAL_HEIGHT * 2).into(),
                resize_constraints: WindowResizeConstraints {
                    min_width: VIRTUAL_WIDTH as f32,
                    min_height: VIRTUAL_HEIGHT as f32,
                    ..default()
                },
                ..default()
            }),
            ..default()
        });

    #[cfg(target_os = "macos")]
    {
        app.add_plugins((
            default_plugins.disable::<GilrsPlugin>(),
            bevy_gamepad::GamepadPlugin,
        ));
    }

    #[cfg(not(target_os = "macos"))]
    {
        app.add_plugins(default_plugins);
    }

    app.add_plugins(VelocityBreakerPlugin).run();
}
