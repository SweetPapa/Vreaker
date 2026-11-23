//! Application state machine (`Title -> Playing -> Paused -> Victory`), multi-stage
//! campaign progression (`Stage 1: Iron Foundry` -> `Stage 2: Cryo Conduit`), and
//! unlocked Aspect Diamond powers (`Rift` -> `Forge` -> `Stasis`).
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
    Victory,
}

/// Playable campaign stages matching `docs/story-bible.md`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StageId {
    #[default]
    Stage1Foundry,
    Stage2Cryo,
}

impl StageId {
    pub fn ldtk_asset_path(self) -> &'static str {
        match self {
            StageId::Stage1Foundry => "levels/demo_gym.ldtk",
            StageId::Stage2Cryo => "levels/cryo_conduit.ldtk",
        }
    }

    pub fn short_name(self) -> &'static str {
        match self {
            StageId::Stage1Foundry => "IRON FOUNDRY",
            StageId::Stage2Cryo => "CRYO CONDUIT",
        }
    }

    pub fn full_title(self) -> &'static str {
        match self {
            StageId::Stage1Foundry => "STAGE 1: THE FOUNDRY CITADEL",
            StageId::Stage2Cryo => "STAGE 2: THE CRYO CONDUIT",
        }
    }

    pub fn number(self) -> u8 {
        match self {
            StageId::Stage1Foundry => 1,
            StageId::Stage2Cryo => 2,
        }
    }
}

/// Five Aspect Diamonds recovered across the campaign (`docs/story-bible.md`).
/// Cycling unlocked Aspects empowers Levy's Rift Saber, Neutral-B wave, and Dive Slam!
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AspectDiamond {
    /// Starting Cyan Diamond: balanced Rift Saber + Breaker Wave + Rising Cyclone.
    #[default]
    Rift,
    /// Stage 1 Boss Reward (Vulcanus): Molten Amber thermal surge (+25% heavy/wave damage,
    /// 2x thermal melt damage against Breakable Bulkheads & Cryo Ice Walls!).
    Forge,
    /// Stage 2 Boss Reward (Borealis): Glacial Blue frost resonance (+25% freeze hitstun
    /// extension on struck regular enemies + glacial saber arcs!).
    Stasis,
}

impl AspectDiamond {
    pub fn display_name(self) -> &'static str {
        match self {
            AspectDiamond::Rift => "RIFT DIAMOND",
            AspectDiamond::Forge => "FORGE DIAMOND",
            AspectDiamond::Stasis => "STASIS DIAMOND",
        }
    }

    pub fn short_tag(self) -> &'static str {
        match self {
            AspectDiamond::Rift => "RIFT [CYAN]",
            AspectDiamond::Forge => "FORGE [MOLTEN]",
            AspectDiamond::Stasis => "STASIS [CRYO]",
        }
    }

    /// HDR linear RGB glow signature for Levy's saber arcs, waves, and shockwaves.
    pub fn vfx_rgb(self) -> [f32; 3] {
        match self {
            AspectDiamond::Rift => [0.32, 3.9, 5.4],
            AspectDiamond::Forge => [5.2, 2.35, 0.35],
            AspectDiamond::Stasis => [0.45, 3.4, 5.6],
        }
    }

    /// UI accent color matching the equipped Aspect Diamond.
    pub fn ui_color(self) -> Color {
        match self {
            AspectDiamond::Rift => Color::srgb(0.20, 0.88, 1.0),
            AspectDiamond::Forge => Color::srgb(1.0, 0.56, 0.16),
            AspectDiamond::Stasis => Color::srgb(0.45, 0.86, 1.0),
        }
    }
}

/// Persistent in-session campaign progression tracking active stage and unlocked Aspect Diamonds.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CampaignState {
    pub active_stage: StageId,
    pub stage1_cleared: bool,
    pub stage2_cleared: bool,
    pub active_diamond: AspectDiamond,
}

impl CampaignState {
    /// Returns whether `diamond` is currently unlocked and equippable.
    ///
    /// Entering Stage 2 immediately grants Forge Diamond access so players testing Stage 2
    /// directly from the Title Screen can shatter Cryo Ice Bulkheads with Molten Cleave!
    pub fn is_diamond_unlocked(&self, diamond: AspectDiamond) -> bool {
        match diamond {
            AspectDiamond::Rift => true,
            AspectDiamond::Forge => self.stage1_cleared || self.active_stage == StageId::Stage2Cryo,
            AspectDiamond::Stasis => self.stage2_cleared,
        }
    }

    pub fn unlocked_diamonds(&self) -> Vec<AspectDiamond> {
        [
            AspectDiamond::Rift,
            AspectDiamond::Forge,
            AspectDiamond::Stasis,
        ]
        .into_iter()
        .filter(|&d| self.is_diamond_unlocked(d))
        .collect()
    }

    /// Cycles to the next unlocked Aspect Diamond; returns `Some(new)` if the active aspect changed.
    pub fn cycle_diamond(&mut self) -> Option<AspectDiamond> {
        let unlocked = self.unlocked_diamonds();
        if unlocked.len() <= 1 {
            return None;
        }
        let idx = unlocked
            .iter()
            .position(|&d| d == self.active_diamond)
            .unwrap_or(0);
        let next = unlocked[(idx + 1) % unlocked.len()];
        self.active_diamond = next;
        Some(next)
    }

    /// Marks `stage` cleared and unlocks its boss's Aspect Diamond!
    pub fn mark_stage_cleared(&mut self, stage: StageId) {
        match stage {
            StageId::Stage1Foundry => {
                self.stage1_cleared = true;
            }
            StageId::Stage2Cryo => {
                self.stage1_cleared = true;
                self.stage2_cleared = true;
            }
        }
    }

    /// Selects `stage` as active, auto-equipping Forge Diamond on first entry to Stage 2 if unlocked.
    pub fn select_stage(&mut self, stage: StageId) {
        self.active_stage = stage;
        if stage == StageId::Stage2Cryo && self.active_diamond == AspectDiamond::Rift {
            // Let players immediately feel their newly forged Stage 1 reward in Stage 2 (still toggleable!)
            self.active_diamond = AspectDiamond::Forge;
        } else if !self.is_diamond_unlocked(self.active_diamond) {
            self.active_diamond = AspectDiamond::Rift;
        }
    }
}

/// Commands requesting a transition between [`AppState`] variants, stage switching, or app exit.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameStateCommand {
    StartGame,
    SelectStageAndPlay(StageId),
    TogglePause,
    ResumeGame,
    RestartStage,
    StageCleared,
    ReturnToTitle,
    QuitGame,
}

/// Message emitted whenever the player requests a full stage restart from Pause/Title.
#[derive(Message, Debug, Clone, Copy, Default)]
pub struct StageRestartEvent;

/// Message emitted whenever the active campaign [`StageId`] changes and must hot-swap LDtk + biome assets.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct StageSwitchEvent(pub StageId);

/// Marker attached to every stage-specific world geometry, prop, and decoration entity so
/// switching between Stage 1 and Stage 2 despawns the previous stage with zero leftovers.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct StageWorldEntity;

/// System set for applying queued [`GameStateCommand`] messages.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct StateTransitionSet;

/// Plugin registering [`AppState`], [`CampaignState`], and [`GameStateCommand`] handling.
pub struct GameStatePlugin;

impl Plugin for GameStatePlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<AppState>()
            .init_resource::<CampaignState>()
            .add_message::<GameStateCommand>()
            .add_message::<StageRestartEvent>()
            .add_message::<StageSwitchEvent>()
            .add_systems(
                PreUpdate,
                apply_game_state_commands.in_set(StateTransitionSet),
            );
    }
}

/// Processes queued [`GameStateCommand`] messages and schedules `NextState` transitions.
#[allow(clippy::too_many_arguments)]
pub fn apply_game_state_commands(
    mut commands_reader: MessageReader<GameStateCommand>,
    current_state: Res<State<AppState>>,
    mut campaign: ResMut<CampaignState>,
    mut next_state: ResMut<NextState<AppState>>,
    mut restart_writer: MessageWriter<StageRestartEvent>,
    mut switch_writer: MessageWriter<StageSwitchEvent>,
    mut exit_writer: MessageWriter<AppExit>,
) {
    let mut logical_state = *current_state.get();
    let mut changed = false;

    for command in commands_reader.read() {
        match *command {
            GameStateCommand::StartGame => {
                logical_state = AppState::Playing;
                changed = true;
            }
            GameStateCommand::SelectStageAndPlay(stage) => {
                let stage_changed = campaign.active_stage != stage;
                campaign.select_stage(stage);
                if stage_changed {
                    switch_writer.write(StageSwitchEvent(stage));
                } else {
                    restart_writer.write(StageRestartEvent);
                }
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
                AppState::Title | AppState::Victory => {}
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
            GameStateCommand::StageCleared => {
                let cleared_stage = campaign.active_stage;
                campaign.mark_stage_cleared(cleared_stage);
                logical_state = AppState::Victory;
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
