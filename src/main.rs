//! Binary entrypoint for *VELOCITY // BREAKER*.

use bevy::prelude::*;
use bevy::window::WindowResizeConstraints;
use velocity_breaker::VelocityBreakerPlugin;
use velocity_breaker::rendering::{VIRTUAL_HEIGHT, VIRTUAL_WIDTH};

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
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
                }),
        )
        .add_plugins(VelocityBreakerPlugin)
        .run();
}
