//! Application state machine (`Title -> Playing -> Paused`) and state-change commands.
//!
//! Decoupled from rendering so state transitions can be tested deterministically
//! with `MinimalPlugins`.

use bevy::app::AppExit;
use bevy::prelude::*;

/// High-level application state governing menus, simulation, and overlays.
#[derive(States, Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppState {
    #[default]
    Title,
    Playing,
    Paused,
}

/// Commands requesting a transition between [`AppState`] variants or app exit.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameStateCommand {
    StartGame,
    TogglePause,
    ResumeGame,
    RestartStage,
    ReturnToTitle,
    QuitGame,
}

/// Message emitted whenever the player requests a full stage restart from Pause/Title.
#[derive(Message, Debug, Clone, Copy, Default)]
pub struct StageRestartEvent;

/// System set for applying queued [`GameStateCommand`] messages.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct StateTransitionSet;

/// Plugin registering [`AppState`] and [`GameStateCommand`] handling.
pub struct GameStatePlugin;

impl Plugin for GameStatePlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<AppState>()
            .add_message::<GameStateCommand>()
            .add_message::<StageRestartEvent>()
            .add_systems(
                PreUpdate,
                apply_game_state_commands.in_set(StateTransitionSet),
            );
    }
}

/// Processes queued [`GameStateCommand`] messages and schedules `NextState` transitions.
pub fn apply_game_state_commands(
    mut commands_reader: MessageReader<GameStateCommand>,
    current_state: Res<State<AppState>>,
    mut next_state: ResMut<NextState<AppState>>,
    mut restart_writer: MessageWriter<StageRestartEvent>,
    mut exit_writer: MessageWriter<AppExit>,
) {
    let mut logical_state = *current_state.get();
    let mut changed = false;

    for command in commands_reader.read() {
        match command {
            GameStateCommand::StartGame => {
                logical_state = AppState::Playing;
                changed = true;
            }
            GameStateCommand::TogglePause => match logical_state {
                AppState::Playing => {
                    logical_state = AppState::Paused;
                    changed = true;
                }
                AppState::Paused => {
                    logical_state = AppState::Playing;
                    changed = true;
                }
                AppState::Title => {}
            },
            GameStateCommand::ResumeGame => {
                if logical_state == AppState::Paused {
                    logical_state = AppState::Playing;
                    changed = true;
                }
            }
            GameStateCommand::RestartStage => {
                restart_writer.write(StageRestartEvent);
                logical_state = AppState::Playing;
                changed = true;
            }
            GameStateCommand::ReturnToTitle => {
                restart_writer.write(StageRestartEvent);
                logical_state = AppState::Title;
                changed = true;
            }
            GameStateCommand::QuitGame => {
                exit_writer.write(AppExit::Success);
            }
        }
    }

    if changed {
        next_state.set(logical_state);
    }
}
