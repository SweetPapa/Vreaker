//! Gamepad-first input bindings with full keyboard fallback via `leafwing-input-manager`,
//! Smash-style configurable [`ControlConfig`] (`assets/config/controls.ron`), and raw
//! gamepad D-Pad calibration (`F2` / menu button) persisted to `assets/config/gamepad_dpad.ron`.
//!
//! Per Bevy 0.20 / Leafwing 0.22 rules, `ActionState` and `InputMap` are attached
//! strictly as components on dedicated entities (never resources).

use std::collections::HashMap;
use std::path::Path;

use bevy::input::gamepad::{
    GamepadAxisChangedEvent, GamepadButtonChangedEvent, RawGamepadAxisChangedEvent,
    RawGamepadButtonChangedEvent, RawGamepadEvent,
};
use bevy::prelude::*;
use leafwing_input_manager::plugin::InputManagerSystem;
use leafwing_input_manager::prelude::*;
use serde::{Deserialize, Serialize};

use crate::player::Player;
use crate::state::{AppState, GameStateCommand, StateTransitionSet};

/// Persistent RON path storing user-calibrated gamepad D-Pad bindings.
pub const DPAD_CALIBRATION_PATH: &str = "assets/config/gamepad_dpad.ron";

/// Persistent RON path storing Smash-style player control settings.
pub const CONTROL_CONFIG_PATH: &str = "assets/config/controls.ron";

/// Gameplay actions matching `SPEC.md` Section 5 (+ Smash Shield / Roll / Dodge / Heavy / C-Stick / Taunt).
#[derive(Actionlike, PartialEq, Eq, Clone, Copy, Hash, Debug, Reflect)]
pub enum PlayerAction {
    #[actionlike(DualAxis)]
    Move,
    /// Super Smash Bros. Right-Stick (C-Stick) for instant directional Smashes & Aerials!
    #[actionlike(DualAxis)]
    SmashStick,
    Jump,
    MeleeAttack,
    /// Dedicated Heavy / Chargeable Smash attack button (`U` / `V` on keyboard, `R3` on gamepad).
    HeavySmash,
    Special,
    Buster,
    SpinDash,
    Guard,
    Taunt,
    Pause,
}

/// Menu navigation and pause actions attached to the dedicated [`MenuInput`] entity.
#[derive(Actionlike, PartialEq, Eq, Clone, Copy, Hash, Debug, Reflect)]
pub enum MenuAction {
    Up,
    Down,
    Left,
    Right,
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

/// Configurable gamepad Shield button mode (also provides an instant 1-click fix for
/// third-party / remapped controllers where D-Pad Right collides with `LB` or `LT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ShieldButtonMode {
    /// Both `LB/L1` and analog `LT/L2` trigger Shield (plus keyboard `Q / F / RCtrl`).
    #[default]
    LbAndLt,
    /// Only `LT/L2` triggers gamepad Shield (`LB/L1` ignored — fixes D-Pad -> LB hardware remaps!).
    LtOnly,
    /// Only `LB/L1` triggers gamepad Shield (analog `LT/L2` ignored).
    LbOnly,
    /// `RB/R1` triggers Shield (classic Smash right-shoulder guard; Buster stays on `Y` / `North`).
    RbOnly,
}

impl ShieldButtonMode {
    pub fn cycle(self, dir: i32) -> Self {
        let modes = [Self::LbAndLt, Self::LtOnly, Self::LbOnly, Self::RbOnly];
        let idx = modes.iter().position(|&m| m == self).unwrap_or(0) as i32;
        let next = (idx + dir).rem_euclid(modes.len() as i32) as usize;
        modes[next]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::LbAndLt => "LB + LT (DEFAULT)",
            Self::LtOnly => "LT / L2 ONLY (FIX LB)",
            Self::LbOnly => "LB / L1 ONLY",
            Self::RbOnly => "RB / R1 (SMASH R)",
        }
    }
}

/// Smash-style button layout preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ControlLayout {
    /// Standard Action Platformer: `A` Jump, `X` Melee, `B` Special, `Y/RB` Buster, `RT` Dash.
    #[default]
    Standard,
    /// Super Smash Pro Style: `X/Y` Jump, `A` Melee, `B` Special, `RB` Buster, `RT` Dash.
    SmashPro,
}

impl ControlLayout {
    pub fn toggle(self) -> Self {
        match self {
            Self::Standard => Self::SmashPro,
            Self::SmashPro => Self::Standard,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Standard => "STANDARD (A:JUMP X:ATK B:SPEC)",
            Self::SmashPro => "SMASH PRO (X/Y:JUMP A:ATK B:SPEC)",
        }
    }
}

/// Persistent Smash Bros.-style controller configuration (`assets/config/controls.ron`).
#[derive(Resource, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlConfig {
    pub shield_mode: ShieldButtonMode,
    pub layout: ControlLayout,
    /// Classic Smash Tap-Jump (pressing Up jumps). Off by default so Up-Tilt/Up-Smash feel crisp!
    pub tap_jump: bool,
    /// Hold Melee on Smash attacks to charge up to `1.55x` damage & knockback!
    pub smash_charge: bool,
}

impl Default for ControlConfig {
    fn default() -> Self {
        Self {
            shield_mode: ShieldButtonMode::LbAndLt,
            layout: ControlLayout::Standard,
            tap_jump: false,
            smash_charge: true,
        }
    }
}

impl ControlConfig {
    pub fn load_default() -> Self {
        let path = Path::new(CONTROL_CONFIG_PATH);
        if let Ok(text) = std::fs::read_to_string(path)
            && let Ok(cfg) = ron::from_str::<ControlConfig>(&text)
        {
            return cfg;
        }
        Self::default()
    }

    pub fn save_to_disk(&self) {
        let path = Path::new(CONTROL_CONFIG_PATH);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(pretty) = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()) {
            let _ = std::fs::write(path, pretty);
        }
    }
}

/// Serializable mirror of [`GamepadButton`] so calibrated D-Pad configs save cleanly to RON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SavedGamepadButton {
    South,
    East,
    North,
    West,
    C,
    Z,
    LeftTrigger,
    LeftTrigger2,
    RightTrigger,
    RightTrigger2,
    Select,
    Start,
    Mode,
    LeftThumb,
    RightThumb,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
    Other(u32),
}

impl From<GamepadButton> for SavedGamepadButton {
    fn from(b: GamepadButton) -> Self {
        match b {
            GamepadButton::South => Self::South,
            GamepadButton::East => Self::East,
            GamepadButton::North => Self::North,
            GamepadButton::West => Self::West,
            GamepadButton::C => Self::C,
            GamepadButton::Z => Self::Z,
            GamepadButton::LeftTrigger => Self::LeftTrigger,
            GamepadButton::LeftTrigger2 => Self::LeftTrigger2,
            GamepadButton::RightTrigger => Self::RightTrigger,
            GamepadButton::RightTrigger2 => Self::RightTrigger2,
            GamepadButton::Select => Self::Select,
            GamepadButton::Start => Self::Start,
            GamepadButton::Mode => Self::Mode,
            GamepadButton::LeftThumb => Self::LeftThumb,
            GamepadButton::RightThumb => Self::RightThumb,
            GamepadButton::DPadUp => Self::DPadUp,
            GamepadButton::DPadDown => Self::DPadDown,
            GamepadButton::DPadLeft => Self::DPadLeft,
            GamepadButton::DPadRight => Self::DPadRight,
            GamepadButton::Other(c) => Self::Other(c),
        }
    }
}

impl From<SavedGamepadButton> for GamepadButton {
    fn from(b: SavedGamepadButton) -> Self {
        match b {
            SavedGamepadButton::South => Self::South,
            SavedGamepadButton::East => Self::East,
            SavedGamepadButton::North => Self::North,
            SavedGamepadButton::West => Self::West,
            SavedGamepadButton::C => Self::C,
            SavedGamepadButton::Z => Self::Z,
            SavedGamepadButton::LeftTrigger => Self::LeftTrigger,
            SavedGamepadButton::LeftTrigger2 => Self::LeftTrigger2,
            SavedGamepadButton::RightTrigger => Self::RightTrigger,
            SavedGamepadButton::RightTrigger2 => Self::RightTrigger2,
            SavedGamepadButton::Select => Self::Select,
            SavedGamepadButton::Start => Self::Start,
            SavedGamepadButton::Mode => Self::Mode,
            SavedGamepadButton::LeftThumb => Self::LeftThumb,
            SavedGamepadButton::RightThumb => Self::RightThumb,
            SavedGamepadButton::DPadUp => Self::DPadUp,
            SavedGamepadButton::DPadDown => Self::DPadDown,
            SavedGamepadButton::DPadLeft => Self::DPadLeft,
            SavedGamepadButton::DPadRight => Self::DPadRight,
            SavedGamepadButton::Other(c) => Self::Other(c),
        }
    }
}

/// Serializable mirror of [`GamepadAxis`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SavedGamepadAxis {
    LeftStickX,
    LeftStickY,
    LeftZ,
    RightStickX,
    RightStickY,
    RightZ,
    Other(u32),
}

impl From<GamepadAxis> for SavedGamepadAxis {
    fn from(a: GamepadAxis) -> Self {
        match a {
            GamepadAxis::LeftStickX => Self::LeftStickX,
            GamepadAxis::LeftStickY => Self::LeftStickY,
            GamepadAxis::LeftZ => Self::LeftZ,
            GamepadAxis::RightStickX => Self::RightStickX,
            GamepadAxis::RightStickY => Self::RightStickY,
            GamepadAxis::RightZ => Self::RightZ,
            GamepadAxis::Other(c) => Self::Other(c),
        }
    }
}

impl From<SavedGamepadAxis> for GamepadAxis {
    fn from(a: SavedGamepadAxis) -> Self {
        match a {
            SavedGamepadAxis::LeftStickX => Self::LeftStickX,
            SavedGamepadAxis::LeftStickY => Self::LeftStickY,
            SavedGamepadAxis::LeftZ => Self::LeftZ,
            SavedGamepadAxis::RightStickX => Self::RightStickX,
            SavedGamepadAxis::RightStickY => Self::RightStickY,
            SavedGamepadAxis::RightZ => Self::RightZ,
            SavedGamepadAxis::Other(c) => Self::Other(c),
        }
    }
}

/// Single calibrated D-Pad direction binding supporting digital buttons, `Other(u32)` raw codes,
/// split analog axes, AND single-axis discrete HID hat switches!
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum DpadBinding {
    /// Pressed when raw button value >= 0.50.
    Button(SavedGamepadButton),
    /// Active when raw axis value is within `tolerance` of `target` AND away from `neutral`.
    AxisTarget {
        axis: SavedGamepadAxis,
        target: f32,
        neutral: f32,
        tolerance: f32,
    },
}

impl DpadBinding {
    pub fn describe(self) -> String {
        match self {
            DpadBinding::Button(b) => format!("BTN {:?}", b),
            DpadBinding::AxisTarget { axis, target, .. } => format!("AXIS {:?}@{:+.2}", axis, target),
        }
    }
}

/// Persistent calibrated D-Pad profile overcoming hardware / OS HID remapping quirks.
#[derive(Resource, Debug, Clone, Default, Serialize, Deserialize)]
pub struct DpadCalibration {
    pub left: Option<DpadBinding>,
    pub right: Option<DpadBinding>,
    pub up: Option<DpadBinding>,
    pub down: Option<DpadBinding>,
}

impl DpadCalibration {
    pub fn is_active(&self) -> bool {
        self.left.is_some() || self.right.is_some() || self.up.is_some() || self.down.is_some()
    }

    pub fn load_default() -> Self {
        let path = Path::new(DPAD_CALIBRATION_PATH);
        if let Ok(text) = std::fs::read_to_string(path)
            && let Ok(cal) = ron::from_str::<DpadCalibration>(&text)
        {
            return cal;
        }
        Self::default()
    }

    pub fn save_to_disk(&self) {
        let path = Path::new(DPAD_CALIBRATION_PATH);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(pretty) = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()) {
            let _ = std::fs::write(path, pretty);
        }
    }

    /// Returns true if a calibrated D-Pad direction uses `button` (so accidental Shield/Guard
    /// collisions on mis-mapped gamepads are automatically suppressed!).
    pub fn claims_button(&self, button: GamepadButton) -> bool {
        let saved = SavedGamepadButton::from(button);
        [self.left, self.right, self.up, self.down]
            .into_iter()
            .flatten()
            .any(|b| matches!(b, DpadBinding::Button(sb) if sb == saved))
    }

    /// Evaluates calibrated directional vector from live raw gamepad button/axis values.
    pub fn sample_vector(&self, raw: &LiveRawGamepadState) -> Vec2 {
        let mut v = Vec2::ZERO;
        if self.left.is_some_and(|b| raw.is_binding_active(b)) {
            v.x -= 1.0;
        }
        if self.right.is_some_and(|b| raw.is_binding_active(b)) {
            v.x += 1.0;
        }
        if self.up.is_some_and(|b| raw.is_binding_active(b)) {
            v.y += 1.0;
        }
        if self.down.is_some_and(|b| raw.is_binding_active(b)) {
            v.y -= 1.0;
        }
        v
    }
}

/// Live snapshot of EVERY raw gamepad button & axis (including `Other(u32)` codes!) updated every frame.
#[derive(Resource, Debug, Default, Clone)]
pub struct LiveRawGamepadState {
    pub buttons: HashMap<GamepadButton, f32>,
    pub axes: HashMap<GamepadAxis, f32>,
}

impl LiveRawGamepadState {
    pub fn button_value(&self, b: GamepadButton) -> f32 {
        self.buttons.get(&b).copied().unwrap_or(0.0)
    }

    pub fn axis_value(&self, a: GamepadAxis) -> f32 {
        self.axes.get(&a).copied().unwrap_or(0.0)
    }

    pub fn is_binding_active(&self, binding: DpadBinding) -> bool {
        match binding {
            DpadBinding::Button(saved_btn) => {
                let btn = GamepadButton::from(saved_btn);
                self.button_value(btn) >= 0.50
            }
            DpadBinding::AxisTarget {
                axis,
                target,
                neutral,
                tolerance,
            } => {
                let Some(&cur) = self.axes.get(&GamepadAxis::from(axis)) else {
                    return false;
                };
                let near_target = (cur - target).abs() <= tolerance.max(0.12);
                let away_from_neutral = (cur - neutral).abs() >= 0.14;
                near_target && away_from_neutral
            }
        }
    }

    /// Formats currently active raw buttons/axes for the in-game Controls Config monitor (zero log spam!).
    pub fn active_summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        for (&btn, &val) in &self.buttons {
            if val >= 0.45 {
                parts.push(format!("{:?}({:.1})", btn, val));
            }
        }
        parts.sort();
        let mut ax_parts: Vec<String> = Vec::new();
        for (&ax, &val) in &self.axes {
            if val.abs() >= 0.35 {
                ax_parts.push(format!("{:?}({:+.2})", ax, val));
            }
        }
        ax_parts.sort();
        parts.extend(ax_parts);
        if parts.is_empty() {
            "IDLE (press any controller button to inspect)".to_string()
        } else {
            parts.join("  +  ")
        }
    }
}

/// Interactive 4-step in-game D-Pad calibration wizard (`LEFT -> RIGHT -> UP -> DOWN`).
#[derive(Resource, Debug, Default, Clone)]
pub struct DpadWizardState {
    pub active: bool,
    /// `0` = Left, `1` = Right, `2` = Up, `3` = Down
    pub step: u8,
    /// Brief settle timer requiring all inputs to return to neutral between steps.
    pub settle_timer: f32,
    pub baseline_buttons: HashMap<GamepadButton, f32>,
    pub baseline_axes: HashMap<GamepadAxis, f32>,
    pub draft: DpadCalibration,
    pub last_captured_label: String,
    pub completed_banner_timer: f32,
}

impl DpadWizardState {
    pub fn start(&mut self, raw: &LiveRawGamepadState) {
        self.active = true;
        self.step = 0;
        self.settle_timer = 0.25;
        self.baseline_buttons = raw.buttons.clone();
        self.baseline_axes = raw.axes.clone();
        self.draft = DpadCalibration::default();
        self.last_captured_label.clear();
        self.completed_banner_timer = 0.0;
    }

    pub fn cancel(&mut self) {
        self.active = false;
        self.last_captured_label.clear();
    }

    pub fn current_step_prompt(&self) -> &'static str {
        match self.step {
            0 => "STEP 1 / 4 :  PRESS  D-PAD  LEFT",
            1 => "STEP 2 / 4 :  PRESS  D-PAD  RIGHT",
            2 => "STEP 3 / 4 :  PRESS  D-PAD  UP",
            _ => "STEP 4 / 4 :  PRESS  D-PAD  DOWN",
        }
    }
}

/// Builds a gamepad + keyboard [`InputMap`] tailored to the player's Smash [`ControlConfig`].
pub fn player_input_map_for_config(cfg: &ControlConfig) -> InputMap<PlayerAction> {
    let mut map = InputMap::default()
        // Move / Aim / Fast-Fall: Left Stick / D-Pad | WASD / Arrow Keys
        .with_dual_axis(PlayerAction::Move, GamepadStick::LEFT)
        .with_dual_axis(PlayerAction::Move, VirtualDPad::dpad())
        .with_dual_axis(PlayerAction::Move, VirtualDPad::wasd())
        .with_dual_axis(PlayerAction::Move, VirtualDPad::arrow_keys())
        // Classic Smash Bros. Right Stick (C-Stick): Instant F/U/D-Smash & Aerials!
        .with_dual_axis(PlayerAction::SmashStick, GamepadStick::RIGHT)
        // Keyboard gameplay defaults (always available regardless of gamepad layout)
        .with(PlayerAction::Jump, KeyCode::Space)
        .with(PlayerAction::Jump, KeyCode::KeyZ)
        .with(PlayerAction::MeleeAttack, KeyCode::KeyJ)
        .with(PlayerAction::MeleeAttack, KeyCode::KeyX)
        .with(PlayerAction::HeavySmash, KeyCode::KeyU)
        .with(PlayerAction::HeavySmash, KeyCode::KeyV)
        .with(PlayerAction::HeavySmash, GamepadButton::RightThumb)
        .with(PlayerAction::Special, KeyCode::KeyK)
        .with(PlayerAction::Special, KeyCode::KeyC)
        .with(PlayerAction::Buster, KeyCode::KeyL)
        .with(PlayerAction::Buster, KeyCode::ShiftLeft)
        .with(PlayerAction::Buster, KeyCode::ShiftRight)
        .with(PlayerAction::SpinDash, GamepadButton::RightTrigger2)
        .with(PlayerAction::SpinDash, KeyCode::KeyE)
        .with(PlayerAction::SpinDash, KeyCode::ControlLeft)
        .with(PlayerAction::Guard, KeyCode::KeyQ)
        .with(PlayerAction::Guard, KeyCode::KeyF)
        .with(PlayerAction::Guard, KeyCode::ControlRight)
        // Taunt / Victory Pose: Select/Back | T
        .with(PlayerAction::Taunt, GamepadButton::Select)
        .with(PlayerAction::Taunt, KeyCode::KeyT)
        // Pause: Start | Esc
        .with(PlayerAction::Pause, GamepadButton::Start)
        .with(PlayerAction::Pause, KeyCode::Escape);

    // Layout-specific gamepad face buttons
    match cfg.layout {
        ControlLayout::Standard => {
            map = map
                .with(PlayerAction::Jump, GamepadButton::South)
                .with(PlayerAction::MeleeAttack, GamepadButton::West)
                .with(PlayerAction::Special, GamepadButton::East)
                .with(PlayerAction::Buster, GamepadButton::North);
        }
        ControlLayout::SmashPro => {
            map = map
                .with(PlayerAction::Jump, GamepadButton::West)
                .with(PlayerAction::Jump, GamepadButton::North)
                .with(PlayerAction::MeleeAttack, GamepadButton::South)
                .with(PlayerAction::Special, GamepadButton::East);
        }
    }

    // Shield / Buster shoulder routing per ShieldButtonMode
    match cfg.shield_mode {
        ShieldButtonMode::LbAndLt | ShieldButtonMode::LbOnly => {
            map = map
                .with(PlayerAction::Guard, GamepadButton::LeftTrigger)
                .with(PlayerAction::Buster, GamepadButton::RightTrigger);
        }
        ShieldButtonMode::LtOnly => {
            // LB/L1 intentionally NOT bound to Guard so mis-mapped D-Pad Right -> LB can NEVER shield!
            map = map.with(PlayerAction::Buster, GamepadButton::RightTrigger);
        }
        ShieldButtonMode::RbOnly => {
            map = map.with(PlayerAction::Guard, GamepadButton::RightTrigger);
        }
    }

    map
}

/// Builds the default gamepad + keyboard [`InputMap`] for [`PlayerAction`].
pub fn default_player_input_map() -> InputMap<PlayerAction> {
    player_input_map_for_config(&ControlConfig::default())
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
        .with(MenuAction::Left, GamepadButton::DPadLeft)
        .with(MenuAction::Left, GamepadControlDirection::LEFT_LEFT)
        .with(MenuAction::Left, KeyCode::ArrowLeft)
        .with(MenuAction::Left, KeyCode::KeyA)
        .with(MenuAction::Right, GamepadButton::DPadRight)
        .with(MenuAction::Right, GamepadControlDirection::LEFT_RIGHT)
        .with(MenuAction::Right, KeyCode::ArrowRight)
        .with(MenuAction::Right, KeyCode::KeyD)
        .with(MenuAction::Confirm, GamepadButton::South)
        .with(MenuAction::Confirm, KeyCode::Enter)
        .with(MenuAction::Confirm, KeyCode::Space)
        .with(MenuAction::Cancel, GamepadButton::East)
        .with(MenuAction::Cancel, KeyCode::Escape)
        .with(MenuAction::Pause, GamepadButton::Start)
        .with(MenuAction::Pause, KeyCode::Escape)
}

/// Plugin registering Leafwing input managers, Smash [`ControlConfig`], raw D-Pad calibration, and menu input.
pub struct GameInputPlugin;

impl Plugin for GameInputPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(DpadCalibration::load_default())
            .insert_resource(ControlConfig::load_default())
            .init_resource::<LiveRawGamepadState>()
            .init_resource::<DpadWizardState>()
            .add_plugins((
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
            .add_systems(
                PreUpdate,
                (
                    sync_raw_gamepad_state,
                    apply_control_config_to_player,
                    update_dpad_calibration_wizard,
                    handle_pause_input,
                )
                    .chain()
                    .in_set(InputHandlingSet),
            );
    }
}

/// Dynamically updates the player's [`InputMap`] whenever [`ControlConfig`] changes in the menu.
fn apply_control_config_to_player(
    config: Res<ControlConfig>,
    mut players: Query<&mut InputMap<PlayerAction>, With<Player>>,
) {
    if !config.is_changed() {
        return;
    }
    let next_map = player_input_map_for_config(&config);
    for mut map in &mut players {
        *map = next_map.clone();
    }
}

/// Keeps [`LiveRawGamepadState`] synced with EVERY raw button and axis event (including `Other(u32)`).
#[allow(clippy::too_many_arguments)]
fn sync_raw_gamepad_state(
    mut raw: ResMut<LiveRawGamepadState>,
    mut raw_events: MessageReader<RawGamepadEvent>,
    mut raw_btns: MessageReader<RawGamepadButtonChangedEvent>,
    mut raw_axes: MessageReader<RawGamepadAxisChangedEvent>,
    mut btn_events: MessageReader<GamepadButtonChangedEvent>,
    mut axis_events: MessageReader<GamepadAxisChangedEvent>,
    gamepads: Query<&Gamepad>,
) {
    for ev in raw_events.read() {
        match ev {
            RawGamepadEvent::Button(b) => {
                raw.buttons.insert(b.button, b.value);
            }
            RawGamepadEvent::Axis(a) => {
                raw.axes.insert(a.axis, a.value);
            }
            RawGamepadEvent::Connection(_) => {}
        }
    }
    for ev in raw_btns.read() {
        raw.buttons.insert(ev.button, ev.value);
    }
    for ev in raw_axes.read() {
        raw.axes.insert(ev.axis, ev.value);
    }
    for ev in btn_events.read() {
        raw.buttons.insert(ev.button, ev.value);
    }
    for ev in axis_events.read() {
        raw.axes.insert(ev.axis, ev.value);
    }
    for gp in &gamepads {
        for btn in [
            GamepadButton::DPadLeft,
            GamepadButton::DPadRight,
            GamepadButton::DPadUp,
            GamepadButton::DPadDown,
            GamepadButton::LeftTrigger,
            GamepadButton::LeftTrigger2,
            GamepadButton::RightTrigger,
            GamepadButton::RightTrigger2,
            GamepadButton::South,
            GamepadButton::East,
            GamepadButton::North,
            GamepadButton::West,
            GamepadButton::Select,
            GamepadButton::Start,
        ] {
            if let Some(v) = gp.get(btn) {
                raw.buttons.insert(btn, v);
            }
        }
        for ax in [
            GamepadAxis::LeftStickX,
            GamepadAxis::LeftStickY,
            GamepadAxis::LeftZ,
            GamepadAxis::RightStickX,
            GamepadAxis::RightStickY,
            GamepadAxis::RightZ,
        ] {
            if let Some(v) = gp.get(ax) {
                raw.axes.insert(ax, v);
            }
        }
    }
}

/// Runs the interactive 4-step D-Pad calibration wizard (`F2` hotkey or UI menu button).
fn update_dpad_calibration_wizard(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    raw: Res<LiveRawGamepadState>,
    mut wizard: ResMut<DpadWizardState>,
    mut calibration: ResMut<DpadCalibration>,
) {
    let dt = time.delta_secs();
    if wizard.completed_banner_timer > 0.0 {
        wizard.completed_banner_timer = (wizard.completed_banner_timer - dt).max(0.0);
    }

    if keys.just_pressed(KeyCode::F3) {
        *calibration = DpadCalibration::default();
        let _ = std::fs::remove_file(DPAD_CALIBRATION_PATH);
        wizard.cancel();
        wizard.completed_banner_timer = 2.5;
        return;
    }

    if keys.just_pressed(KeyCode::F2) {
        if wizard.active {
            wizard.cancel();
        } else {
            wizard.start(&raw);
        }
        return;
    }

    if !wizard.active {
        return;
    }

    if keys.just_pressed(KeyCode::Escape) {
        wizard.cancel();
        return;
    }

    // Brief settle pause between steps so releasing the previous direction doesn't double-capture
    if wizard.settle_timer > 0.0 {
        wizard.settle_timer = (wizard.settle_timer - dt).max(0.0);
        if wizard.settle_timer <= 0.0 {
            wizard.baseline_buttons = raw.buttons.clone();
            wizard.baseline_axes = raw.axes.clone();
        }
        return;
    }

    // Detect any button rising cleanly above its baseline!
    // (Uncalibrated resting triggers at ~0.50 are ignored because their delta < 0.45,
    // while genuine D-Pad remaps onto LeftTrigger or LeftTrigger2 ARE captured!)
    let mut captured: Option<DpadBinding> = None;
    let mut best_btn_delta = 0.0_f32;
    for (&btn, &val) in &raw.buttons {
        let base = wizard.baseline_buttons.get(&btn).copied().unwrap_or(0.0);
        let delta = val - base;
        let is_analog_trigger =
            matches!(btn, GamepadButton::LeftTrigger2 | GamepadButton::RightTrigger2);
        let threshold = if is_analog_trigger { 0.48 } else { 0.35 };
        if val >= 0.55 && delta >= threshold && delta > best_btn_delta {
            best_btn_delta = delta;
            captured = Some(DpadBinding::Button(SavedGamepadButton::from(btn)));
        }
    }

    // Otherwise detect any axis deviating significantly from its baseline (covers split axes & HID hat switches!)
    if captured.is_none() {
        let mut best_delta = 0.24_f32;
        for (&axis, &val) in &raw.axes {
            let base = wizard.baseline_axes.get(&axis).copied().unwrap_or(0.0);
            let delta = (val - base).abs();
            if delta > best_delta {
                best_delta = delta;
                captured = Some(DpadBinding::AxisTarget {
                    axis: SavedGamepadAxis::from(axis),
                    target: val,
                    neutral: base,
                    tolerance: 0.18,
                });
            }
        }
    }

    let Some(binding) = captured else {
        return;
    };

    let dir_name = match wizard.step {
        0 => {
            wizard.draft.left = Some(binding);
            "LEFT"
        }
        1 => {
            wizard.draft.right = Some(binding);
            "RIGHT"
        }
        2 => {
            wizard.draft.up = Some(binding);
            "UP"
        }
        _ => {
            wizard.draft.down = Some(binding);
            "DOWN"
        }
    };

    wizard.last_captured_label = format!("Captured {}: {}", dir_name, binding.describe());

    wizard.step += 1;
    if wizard.step >= 4 {
        *calibration = wizard.draft.clone();
        calibration.save_to_disk();
        wizard.active = false;
        wizard.completed_banner_timer = 3.5;
    } else {
        wizard.settle_timer = 0.35;
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
    wizard: Res<DpadWizardState>,
    menu_input: Query<&ActionState<MenuAction>, With<MenuInput>>,
    mut state_commands: MessageWriter<GameStateCommand>,
) {
    if wizard.active {
        return;
    }
    let Ok(actions) = menu_input.single() else {
        return;
    };

    match state.get() {
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
        AppState::Title | AppState::Victory => {}
    }
}
