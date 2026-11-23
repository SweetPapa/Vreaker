//! Story Bible lore, character roster dialogue, and skippable in-game comms system
//! for *VELOCITY // BREAKER* (`docs/story-bible.md`).
//!
//! 100% decoupled from rendering (`MinimalPlugins`-safe) so story triggers,
//! relic keepsake tracking, and dialogue advancement can be tested deterministically.

use std::collections::{HashSet, VecDeque};

use bevy::prelude::*;

use crate::physics::KinematicBody;
use crate::player::Player;
use crate::state::{AppState, StageRestartEvent};

/// Cast speakers with matching pixel-art portraits in `assets/ui/portraits/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpeakerId {
    Levy,
    Mina,
    Barnaby,
    Vulcanus,
}

impl SpeakerId {
    pub fn display_name(self) -> &'static str {
        match self {
            SpeakerId::Levy => "LEVY  •  RIFT BREAKER",
            SpeakerId::Mina => "MINA  •  SCRAP-TECH SALVAGER",
            SpeakerId::Barnaby => "UNIT-04 'BARNABY'  •  COMMS DRONE",
            SpeakerId::Vulcanus => "IRON TITAN: VULCANUS",
        }
    }

    pub fn accent_color(self) -> Color {
        match self {
            SpeakerId::Levy => Color::srgb(0.22, 0.90, 1.0),
            SpeakerId::Mina => Color::srgb(1.0, 0.72, 0.24),
            SpeakerId::Barnaby => Color::srgb(0.32, 0.92, 0.78),
            SpeakerId::Vulcanus => Color::srgb(1.0, 0.36, 0.16),
        }
    }
}

/// Single punchy line of in-game comms dialogue.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DialogueLine {
    pub speaker: SpeakerId,
    pub text: &'static str,
    pub duration_secs: f32,
}

/// Canonical Stage 1 narrative beats defined in `docs/story-bible.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StoryBeat {
    Stage1Intro,
    Relic1Compass,
    Relic2Portrait,
    Relic3Locket,
    BossVulcanusIntro,
    BossVulcanusDefeated,
}

/// Named Stage 1 1700s keepsakes yanked through the time rift alongside Levy.
pub const STAGE1_RELIC_NAMES: [&str; 3] = [
    "BRASS POCKET COMPASS",
    "TORN FAMILY PORTRAIT",
    "WIFE'S SILVER LOCKET",
];

/// Classifies a Stage 1 Secret Relic world X position into keepsake index `0..=2`.
pub fn stage1_relic_index_for_x(x: f32) -> usize {
    if x < 2000.0 {
        0
    } else if x < 5000.0 {
        1
    } else {
        2
    }
}

/// Returns the Story Bible dialogue lines for a given narrative beat.
pub fn script_for_beat(beat: StoryBeat) -> &'static [DialogueLine] {
    match beat {
        StoryBeat::Stage1Intro => &[
            DialogueLine {
                speaker: SpeakerId::Mina,
                text: "Signal's spiking dead ahead, Levy—that's the Forge Diamond deep inside the Directorate's Foundry Citadel.",
                duration_secs: 4.4,
            },
            DialogueLine {
                speaker: SpeakerId::Barnaby,
                text: "Historical note: 2085 automated smelters burn at 3,200 degrees—rather like a 1700s blacksmith's hearth, only far more lethal!",
                duration_secs: 4.4,
            },
            DialogueLine {
                speaker: SpeakerId::Levy,
                text: "Save the history lesson, Barnaby. Four stones stand between me and my family. Let's take the first one.",
                duration_secs: 4.0,
            },
        ],
        StoryBeat::Relic1Compass => &[DialogueLine {
            speaker: SpeakerId::Levy,
            text: "My old brass pocket compass... pulled clean through the rift with me. Needle still points home.",
            duration_secs: 4.0,
        }],
        StoryBeat::Relic2Portrait => &[DialogueLine {
            speaker: SpeakerId::Levy,
            text: "Sarah and the kids... torn half in two, but I'm not leaving a piece of them in this iron maze.",
            duration_secs: 4.2,
        }],
        StoryBeat::Relic3Locket => &[DialogueLine {
            speaker: SpeakerId::Levy,
            text: "Her silver locket! Hold tight, Sarah—I'm ripping my way back to the 1700s.",
            duration_secs: 4.0,
        }],
        // Verbatim Pre-Boss Dialogue from `docs/story-bible.md` Section 7
        StoryBeat::BossVulcanusIntro => &[
            DialogueLine {
                speaker: SpeakerId::Vulcanus,
                text: "UNREGISTERED ORGANIC DETECTED IN SECTOR 8. SURRENDER THE RIFT ANOMALY FOR RECLAMATION.",
                duration_secs: 3.8,
            },
            DialogueLine {
                speaker: SpeakerId::Levy,
                text: "I don't know what half those words mean, tin man. That glowing stone in your furnace—hand it over, and I'll be out of your hair.",
                duration_secs: 4.4,
            },
            DialogueLine {
                speaker: SpeakerId::Vulcanus,
                text: "THERMAL OUTPUT: 400%. TERMINATING INTRUDER.",
                duration_secs: 3.0,
            },
            DialogueLine {
                speaker: SpeakerId::Levy,
                text: "...Right. Guess we're doing this the hard way.",
                duration_secs: 2.8,
            },
        ],
        StoryBeat::BossVulcanusDefeated => &[DialogueLine {
            speaker: SpeakerId::Mina,
            text: "Forge Diamond secured! One down, three to go—next signal is freezing cold out in the Cryo Conduit.",
            duration_secs: 4.5,
        }],
    }
}

/// Message emitted by gameplay systems to trigger a Story Bible comms sequence.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoryTriggerEvent(pub StoryBeat);

/// Live headless-safe dialogue queue & Stage 1 keepsake lore tracker.
#[derive(Resource, Debug, Default, Clone)]
pub struct StoryCommsState {
    pub current: Option<DialogueLine>,
    pub queue: VecDeque<DialogueLine>,
    pub elapsed_in_line: f32,
    pub triggered: HashSet<StoryBeat>,
    pub found_relics: [bool; 3],
}

impl StoryCommsState {
    /// Queues `beat` if it has not already played during this stage run.
    pub fn trigger(&mut self, beat: StoryBeat) -> bool {
        if !self.triggered.insert(beat) {
            return false;
        }
        match beat {
            StoryBeat::Relic1Compass => self.found_relics[0] = true,
            StoryBeat::Relic2Portrait => self.found_relics[1] = true,
            StoryBeat::Relic3Locket => self.found_relics[2] = true,
            _ => {}
        }
        // High-priority Boss confrontation replaces any lingering chatter cleanly.
        if matches!(
            beat,
            StoryBeat::BossVulcanusIntro | StoryBeat::BossVulcanusDefeated
        ) {
            self.current = None;
            self.queue.clear();
            self.elapsed_in_line = 0.0;
        }
        for &line in script_for_beat(beat) {
            self.queue.push_back(line);
        }
        if self.current.is_none() {
            self.pop_next();
        }
        true
    }

    pub fn pop_next(&mut self) {
        self.current = self.queue.pop_front();
        self.elapsed_in_line = 0.0;
    }

    /// Advances typewriter reveal immediately, or skips straight to the next comms line.
    pub fn advance_or_skip(&mut self) {
        let Some(line) = self.current else {
            return;
        };
        let total_chars = line.text.chars().count();
        let revealed = (self.elapsed_in_line * 62.0) as usize;
        if revealed + 4 < total_chars {
            self.elapsed_in_line = (total_chars as f32) / 62.0 + 0.05;
        } else {
            self.pop_next();
        }
    }

    pub fn skip_all(&mut self) {
        self.current = None;
        self.queue.clear();
        self.elapsed_in_line = 0.0;
    }

    pub fn visible_text(&self) -> String {
        let Some(line) = self.current else {
            return String::new();
        };
        let count = ((self.elapsed_in_line * 62.0) as usize).max(1);
        line.text.chars().take(count).collect()
    }

    pub fn all_stage1_relics_found(&self) -> bool {
        self.found_relics.iter().all(|&b| b)
    }

    pub fn reset(&mut self) {
        self.current = None;
        self.queue.clear();
        self.elapsed_in_line = 0.0;
        self.triggered.clear();
        self.found_relics = [false; 3];
    }
}

/// Plugin registering headless-safe Story Bible comms state and progression triggers.
pub struct StoryPlugin;

impl Plugin for StoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StoryCommsState>()
            .add_message::<StoryTriggerEvent>()
            .add_systems(OnEnter(AppState::Title), reset_story_on_title)
            .add_systems(OnEnter(AppState::Playing), trigger_stage1_intro_once)
            .add_systems(Update, reset_story_on_stage_restart)
            .add_systems(
                FixedUpdate,
                (
                    process_story_trigger_events,
                    detect_sanctum_boss_comms,
                    tick_story_comms,
                )
                    .chain()
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(
                Update,
                handle_comms_skip_input.run_if(in_state(AppState::Playing)),
            );
    }
}

fn reset_story_on_title(mut comms: ResMut<StoryCommsState>) {
    comms.reset();
}

fn reset_story_on_stage_restart(
    mut restarts: MessageReader<StageRestartEvent>,
    mut comms: ResMut<StoryCommsState>,
) {
    if restarts.read().next().is_some() {
        comms.reset();
        comms.trigger(StoryBeat::Stage1Intro);
    }
}

fn trigger_stage1_intro_once(mut comms: ResMut<StoryCommsState>) {
    comms.trigger(StoryBeat::Stage1Intro);
}

fn process_story_trigger_events(
    mut events: MessageReader<StoryTriggerEvent>,
    mut comms: ResMut<StoryCommsState>,
) {
    for ev in events.read() {
        comms.trigger(ev.0);
    }
}

/// Fires the verbatim Vulcanus confrontation as Levy steps onto the Sector 8 Sanctum threshold.
fn detect_sanctum_boss_comms(
    players: Query<&KinematicBody, With<Player>>,
    mut comms: ResMut<StoryCommsState>,
) {
    let Ok(body) = players.single() else {
        return;
    };
    if body.position.x >= 8360.0 {
        comms.trigger(StoryBeat::BossVulcanusIntro);
    }
}

fn tick_story_comms(fixed_time: Res<Time<Fixed>>, mut comms: ResMut<StoryCommsState>) {
    let Some(line) = comms.current else {
        return;
    };
    comms.elapsed_in_line += fixed_time.delta_secs();
    if comms.elapsed_in_line >= line.duration_secs {
        comms.pop_next();
    }
}

fn handle_comms_skip_input(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    gamepads: Query<&Gamepad>,
    mut comms: ResMut<StoryCommsState>,
) {
    if comms.current.is_none() {
        return;
    }
    let key_skip = keys.as_deref().is_some_and(|k| {
        k.just_pressed(KeyCode::Tab) || k.just_pressed(KeyCode::Enter)
    });
    let pad_skip = gamepads
        .iter()
        .any(|gp| gp.just_pressed(GamepadButton::Select));
    if key_skip || pad_skip {
        comms.advance_or_skip();
    }
}
