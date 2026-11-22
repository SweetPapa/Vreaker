//! Gamepad-first input bindings with full keyboard fallback via `leafwing-input-manager`.
//!
//! Per Bevy 0.20 / Leafwing 0.22 rules, `ActionState` and `InputMap` are attached
//! strictly as components on dedicated entities (never resources).

use bevy::prelude::*;
use leafwing_input_manager::plugin::InputManagerSystem;
use leafwing_input_manager::prelude::*;

use crate::state::{AppState, GameStateCommand, StateTransitionSet};

/// Gameplay actions matching `SPEC.md` Section 5.
#[derive(Actionlike, PartialEq, Eq, Clone, Copy, Hash, Debug, Reflect)]
pub enum PlayerAction {
    #[actionlike(DualAxis)]
    Move,
    Jump,
    MeleeAttack,
    Special,
    Buster,
    SpinDash,
    Pause,
}

/// Menu navigation and pause actions attached to the dedicated [`MenuInput`] entity.
#[derive(Actionlike, PartialEq, Eq, Clone, Copy, Hash, Debug, Reflect)]
pub enum MenuAction {
    Up,
    Down,
    Confirm,
    Cancel,
    Pause,
}

/// Marker component for the dedicated entity holding menu [`InputMap`] + [`ActionState`].
#[derive(Component, Debug, Default)]
pub struct MenuInput;

/// System set in which input actions are translated into [`GameStateCommand`]s.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct InputHandlingSet;

/// Builds the default gamepad + keyboard [`InputMap`] for [`PlayerAction`].
pub fn default_player_input_map() -> InputMap<PlayerAction> {
    InputMap::default()
        // Move / Aim / Fast-Fall: Left Stick / D-Pad | WASD / Arrow Keys
        .with_dual_axis(PlayerAction::Move, GamepadStick::LEFT)
        .with_dual_axis(PlayerAction::Move, VirtualDPad::dpad())
        .with_dual_axis(PlayerAction::Move, VirtualDPad::wasd())
        .with_dual_axis(PlayerAction::Move, VirtualDPad::arrow_keys())
        // Jump: A (South) | Space / Z
        .with(PlayerAction::Jump, GamepadButton::South)
        .with(PlayerAction::Jump, KeyCode::Space)
        .with(PlayerAction::Jump, KeyCode::KeyZ)
        // Melee Attack (Directional): X (West) | J / X
        .with(PlayerAction::MeleeAttack, GamepadButton::West)
        .with(PlayerAction::MeleeAttack, KeyCode::KeyJ)
        .with(PlayerAction::MeleeAttack, KeyCode::KeyX)
        // Special / Recovery (Directional): B (East) | K / C
        .with(PlayerAction::Special, GamepadButton::East)
        .with(PlayerAction::Special, KeyCode::KeyK)
        .with(PlayerAction::Special, KeyCode::KeyC)
        // Mega Man Buster (Tap / Hold): RB/R1 or Y (North) | L / Shift
        .with(PlayerAction::Buster, GamepadButton::RightTrigger)
        .with(PlayerAction::Buster, GamepadButton::North)
        .with(PlayerAction::Buster, KeyCode::KeyL)
        .with(PlayerAction::Buster, KeyCode::ShiftLeft)
        .with(PlayerAction::Buster, KeyCode::ShiftRight)
        // Spin Dash / Boost: RT/R2 | E / Left Ctrl
        .with(PlayerAction::SpinDash, GamepadButton::RightTrigger2)
        .with(PlayerAction::SpinDash, KeyCode::KeyE)
        .with(PlayerAction::SpinDash, KeyCode::ControlLeft)
        // Pause: Start | Esc
        .with(PlayerAction::Pause, GamepadButton::Start)
        .with(PlayerAction::Pause, KeyCode::Escape)
}

/// Builds the default gamepad + keyboard [`InputMap`] for [`MenuAction`].
pub fn default_menu_input_map() -> InputMap<MenuAction> {
    InputMap::default()
        .with(MenuAction::Up, GamepadButton::DPadUp)
        .with(MenuAction::Up, GamepadControlDirection::LEFT_UP)
        .with(MenuAction::Up, KeyCode::ArrowUp)
        .with(MenuAction::Up, KeyCode::KeyW)
        .with(MenuAction::Down, GamepadButton::DPadDown)
        .with(MenuAction::Down, GamepadControlDirection::LEFT_DOWN)
        .with(MenuAction::Down, KeyCode::ArrowDown)
        .with(MenuAction::Down, KeyCode::KeyS)
        .with(MenuAction::Confirm, GamepadButton::South)
        .with(MenuAction::Confirm, KeyCode::Enter)
        .with(MenuAction::Confirm, KeyCode::Space)
        .with(MenuAction::Cancel, GamepadButton::East)
        .with(MenuAction::Cancel, KeyCode::Escape)
        .with(MenuAction::Pause, GamepadButton::Start)
        .with(MenuAction::Pause, KeyCode::Escape)
}

/// Plugin registering Leafwing input managers and spawning input carrier entities.
pub struct GameInputPlugin;

impl Plugin for GameInputPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            InputManagerPlugin::<PlayerAction>::default(),
            InputManagerPlugin::<MenuAction>::default(),
        ))
        .configure_sets(
            PreUpdate,
            InputHandlingSet
                .after(InputManagerSystem::Update)
                .before(StateTransitionSet),
        )
        .add_systems(Startup, spawn_input_entities)
        .add_systems(PreUpdate, handle_pause_input.in_set(InputHandlingSet));
    }
}

fn spawn_input_entities(mut commands: Commands) {
    commands.spawn((
        Name::new("MenuInput"),
        MenuInput,
        default_menu_input_map(),
        ActionState::<MenuAction>::default(),
    ));
}

fn handle_pause_input(
    state: Res<State<AppState>>,
    menu_query: Query<&ActionState<MenuAction>, With<MenuInput>>,
    mut state_commands: MessageWriter<GameStateCommand>,
) {
    let Ok(actions) = menu_query.single() else {
        return;
    };

    match *state.get() {
        AppState::Playing => {
            if actions.just_pressed(&MenuAction::Pause) {
                state_commands.write(GameStateCommand::TogglePause);
            }
        }
        AppState::Paused => {
            if actions.just_pressed(&MenuAction::Pause) || actions.just_pressed(&MenuAction::Cancel)
            {
                state_commands.write(GameStateCommand::ResumeGame);
            }
        }
        AppState::Title => {}
    }
}
