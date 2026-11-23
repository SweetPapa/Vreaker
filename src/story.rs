//! Story Bible lore, character roster dialogue, and skippable in-game comms system
//! for *VELOCITY // BREAKER* (`docs/story-bible.md`).
//!
//! 100% decoupled from rendering (`MinimalPlugins`-safe) so story triggers,
//! relic keepsake tracking, and dialogue advancement can be tested deterministically.

use std::collections::{HashSet, VecDeque};

use bevy::prelude::*;

use crate::physics::KinematicBody;
use crate::player::Player;
use crate::state::{AppState, CampaignState, StageId, StageRestartEvent, StageSwitchEvent};

/// Cast speakers with matching pixel-art portraits in `assets/ui/portraits/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpeakerId {
    Levy,
    Mina,
    Barnaby,
    Vulcanus,
    Borealis,
}

impl SpeakerId {
    pub fn display_name(self) -> &'static str {
        match self {
            SpeakerId::Levy => "LEVY  •  RIFT BREAKER",
            SpeakerId::Mina => "MINA  •  SCRAP-TECH SALVAGER",
            SpeakerId::Barnaby => "UNIT-04 'BARNABY'  •  COMMS DRONE",
            SpeakerId::Vulcanus => "IRON TITAN: VULCANUS",
            SpeakerId::Borealis => "SUB-ZERO WARDEN: BOREALIS",
        }
    }

    pub fn accent_color(self) -> Color {
        match self {
            SpeakerId::Levy => Color::srgb(0.22, 0.90, 1.0),
            SpeakerId::Mina => Color::srgb(1.0, 0.72, 0.24),
            SpeakerId::Barnaby => Color::srgb(0.32, 0.92, 0.78),
            SpeakerId::Vulcanus => Color::srgb(1.0, 0.36, 0.16),
            SpeakerId::Borealis => Color::srgb(0.45, 0.88, 1.0),
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

/// Canonical Stage 1 & Stage 2 narrative beats defined in `docs/story-bible.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StoryBeat {
    Stage1Intro,
    Relic1Compass,
    Relic2Portrait,
    Relic3Locket,
    BossVulcanusIntro,
    BossVulcanusDefeated,
    Stage2Intro,
    Stage2Relic1Horn,
    Stage2Relic2Handkerchief,
    Stage2Relic3Toy,
    BossBorealisIntro,
    BossBorealisDefeated,
}

/// Named Stage 1 1700s keepsakes yanked through the time rift alongside Levy.
pub const STAGE1_RELIC_NAMES: [&str; 3] = [
    "BRASS POCKET COMPASS",
    "TORN FAMILY PORTRAIT",
    "WIFE'S SILVER LOCKET",
];

/// Named Stage 2 1700s keepsakes frozen inside the Cryo Conduit vaults.
pub const STAGE2_RELIC_NAMES: [&str; 3] = [
    "WEATHERED POWDER HORN",
    "SARAH'S HANDKERCHIEF",
    "WOODEN CAVALRY TOY",
];

/// Returns the 3 canonical 1700s keepsake titles for a given [`StageId`].
pub fn stage_relic_names(stage: StageId) -> &'static [&'static str; 3] {
    match stage {
        StageId::Stage1Foundry => &STAGE1_RELIC_NAMES,
        StageId::Stage2Cryo => &STAGE2_RELIC_NAMES,
    }
}

/// Classifies a Secret Relic world X position into keepsake index `0..=2`.
pub fn stage1_relic_index_for_x(x: f32) -> usize {
    if x < 2000.0 {
        0
    } else if x < 5000.0 {
        1
    } else {
        2
    }
}

/// Maps `(StageId, relic_index)` to its Story Bible keepsake beat.
pub fn relic_beat_for_stage(stage: StageId, idx: usize) -> StoryBeat {
    match (stage, idx) {
        (StageId::Stage1Foundry, 0) => StoryBeat::Relic1Compass,
        (StageId::Stage1Foundry, 1) => StoryBeat::Relic2Portrait,
        (StageId::Stage1Foundry, _) => StoryBeat::Relic3Locket,
        (StageId::Stage2Cryo, 0) => StoryBeat::Stage2Relic1Horn,
        (StageId::Stage2Cryo, 1) => StoryBeat::Stage2Relic2Handkerchief,
        (StageId::Stage2Cryo, _) => StoryBeat::Stage2Relic3Toy,
    }
}

/// Returns the Story Bible dialogue lines for a given narrative beat.
pub fn script_for_beat(beat: StoryBeat) -> &'static [DialogueLine] {
    match beat {
        StoryBeat::Stage1Intro => &[
            DialogueLine {
                speaker: SpeakerId::Mina,
                text: "Signal's spiking dead ahead, Levy—that's the Forge Diamond deep inside the Directorate's Foundry Citadel.",
                duration_secs: 5.5,
            },
            DialogueLine {
                speaker: SpeakerId::Barnaby,
                text: "Historical note: 2085 automated smelters burn at 3,200 degrees—rather like a 1700s blacksmith's hearth, only far more lethal!",
                duration_secs: 5.8,
            },
            DialogueLine {
                speaker: SpeakerId::Levy,
                text: "Save the history lesson, Barnaby. Four stones stand between me and my family. Let's take the first one.",
                duration_secs: 5.2,
            },
        ],
        StoryBeat::Relic1Compass => &[DialogueLine {
            speaker: SpeakerId::Levy,
            text: "My old brass pocket compass... pulled clean through the rift with me. Needle still points home.",
            duration_secs: 6.4,
        }],
        StoryBeat::Relic2Portrait => &[DialogueLine {
            speaker: SpeakerId::Levy,
            text: "Sarah and the kids... torn half in two, but I'm not leaving a piece of them in this iron maze.",
            duration_secs: 6.4,
        }],
        StoryBeat::Relic3Locket => &[DialogueLine {
            speaker: SpeakerId::Levy,
            text: "Her silver locket! Hold tight, Sarah—I'm ripping my way back to the 1700s.",
            duration_secs: 6.0,
        }],
        // Verbatim Pre-Boss Dialogue from `docs/story-bible.md` Section 7
        StoryBeat::BossVulcanusIntro => &[
            DialogueLine {
                speaker: SpeakerId::Vulcanus,
                text: "UNREGISTERED ORGANIC DETECTED IN SECTOR 8. SURRENDER THE RIFT ANOMALY FOR RECLAMATION.",
                duration_secs: 5.0,
            },
            DialogueLine {
                speaker: SpeakerId::Levy,
                text: "I don't know what half those words mean, tin man. That glowing stone in your furnace—hand it over, and I'll be out of your hair.",
                duration_secs: 5.6,
            },
            DialogueLine {
                speaker: SpeakerId::Vulcanus,
                text: "THERMAL OUTPUT: 400%. TERMINATING INTRUDER.",
                duration_secs: 4.5,
            },
            DialogueLine {
                speaker: SpeakerId::Levy,
                text: "...Right. Guess we're doing this the hard way.",
                duration_secs: 4.5,
            },
        ],
        StoryBeat::BossVulcanusDefeated => &[DialogueLine {
            speaker: SpeakerId::Mina,
            text: "Forge Diamond secured! Press R or L3 to channel Molten Cleave—next signal is freezing cold out in the Cryo Conduit.",
            duration_secs: 6.2,
        }],
        // Stage 2: THE CRYO CONDUIT (Sub-zero AI server farm & liquid-nitrogen cooling towers)
        StoryBeat::Stage2Intro => &[
            DialogueLine {
                speaker: SpeakerId::Mina,
                text: "Welcome to the Cryo Conduit—2085's sub-zero AI server towers. The Stasis Diamond is locked at the core, and those mirror-ice decks are slick!",
                duration_secs: 6.0,
            },
            DialogueLine {
                speaker: SpeakerId::Barnaby,
                text: "Tactical tip: Press R or L3 to ignite your new Forge Diamond! Its molten heat shatters Cryo Ice Bulkheads twice as fast!",
                duration_secs: 6.0,
            },
            DialogueLine {
                speaker: SpeakerId::Levy,
                text: "Survived three winters on the frontier without a fancy metal coat. Let's crack this icebox wide open.",
                duration_secs: 5.4,
            },
        ],
        StoryBeat::Stage2Relic1Horn => &[DialogueLine {
            speaker: SpeakerId::Levy,
            text: "My carved powder horn—frozen solid in a 2085 server rack. Least the frost kept the timber dry.",
            duration_secs: 6.4,
        }],
        StoryBeat::Stage2Relic2Handkerchief => &[DialogueLine {
            speaker: SpeakerId::Levy,
            text: "Sarah's stitched handkerchief... she tied it around my wrist the morning the sky tore open. Not letting go.",
            duration_secs: 6.4,
        }],
        StoryBeat::Stage2Relic3Toy => &[DialogueLine {
            speaker: SpeakerId::Levy,
            text: "The little wooden cavalry horse I whittled for Thomas... I swear I'm bringing every piece of home back with me.",
            duration_secs: 6.4,
        }],
        StoryBeat::BossBorealisIntro => &[
            DialogueLine {
                speaker: SpeakerId::Borealis,
                text: "THERMAL ANOMALY BREACHED CORE SANCTUM. INITIATING ABSOLUTE-ZERO PRESERVATION PROTOCOL.",
                duration_secs: 5.2,
            },
            DialogueLine {
                speaker: SpeakerId::Levy,
                text: "You machine folk sure love big words. Hand over that blue ice stone in your chest before I thaw you with Vulcanus's fire.",
                duration_secs: 5.8,
            },
            DialogueLine {
                speaker: SpeakerId::Borealis,
                text: "FORGE SIGNATURE DETECTED. SUB-ZERO WARDEN BOREALIS ENGAGING. YOU WILL BE FROZEN IN TIME.",
                duration_secs: 5.0,
            },
            DialogueLine {
                speaker: SpeakerId::Levy,
                text: "Already fell out of time once, pal—don't plan on staying frozen!",
                duration_secs: 4.8,
            },
        ],
        StoryBeat::BossBorealisDefeated => &[DialogueLine {
            speaker: SpeakerId::Mina,
            text: "Stasis Diamond locked in—two down, two to go! Cycle it with R/L3 to freeze foes longer. Next up: Vex-9 in the Neon Skyline!",
            duration_secs: 6.4,
        }],
    }
}

/// Message emitted by gameplay systems to trigger a Story Bible comms sequence.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoryTriggerEvent(pub StoryBeat);

/// Live headless-safe dialogue queue & stage keepsake lore tracker.
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
            StoryBeat::Relic1Compass | StoryBeat::Stage2Relic1Horn => self.found_relics[0] = true,
            StoryBeat::Relic2Portrait | StoryBeat::Stage2Relic2Handkerchief => {
                self.found_relics[1] = true
            }
            StoryBeat::Relic3Locket | StoryBeat::Stage2Relic3Toy => self.found_relics[2] = true,
            _ => {}
        }
        // High-priority Boss confrontation replaces any lingering chatter cleanly.
        if matches!(
            beat,
            StoryBeat::BossVulcanusIntro
                | StoryBeat::BossVulcanusDefeated
                | StoryBeat::BossBorealisIntro
                | StoryBeat::BossBorealisDefeated
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
            .add_systems(OnEnter(AppState::Playing), trigger_stage_intro_once)
            .add_systems(Update, reset_story_on_stage_restart_or_switch)
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

fn intro_beat_for_stage(stage: StageId) -> StoryBeat {
    match stage {
        StageId::Stage1Foundry => StoryBeat::Stage1Intro,
        StageId::Stage2Cryo => StoryBeat::Stage2Intro,
    }
}

fn reset_story_on_title(mut comms: ResMut<StoryCommsState>) {
    comms.reset();
}

fn reset_story_on_stage_restart_or_switch(
    mut restarts: MessageReader<StageRestartEvent>,
    mut switches: MessageReader<StageSwitchEvent>,
    campaign: Res<CampaignState>,
    mut comms: ResMut<StoryCommsState>,
) {
    let switched = switches.read().last().map(|ev| ev.0);
    let restarted = restarts.read().next().is_some();
    if let Some(stage) = switched {
        comms.reset();
        comms.trigger(intro_beat_for_stage(stage));
    } else if restarted {
        comms.reset();
        comms.trigger(intro_beat_for_stage(campaign.active_stage));
    }
}

fn trigger_stage_intro_once(campaign: Res<CampaignState>, mut comms: ResMut<StoryCommsState>) {
    comms.trigger(intro_beat_for_stage(campaign.active_stage));
}

fn process_story_trigger_events(
    mut events: MessageReader<StoryTriggerEvent>,
    mut comms: ResMut<StoryCommsState>,
) {
    for ev in events.read() {
        comms.trigger(ev.0);
    }
}

/// Fires the stage Boss confrontation as Levy steps onto the Sector 8 Sanctum threshold.
fn detect_sanctum_boss_comms(
    campaign: Res<CampaignState>,
    players: Query<&KinematicBody, With<Player>>,
    mut comms: ResMut<StoryCommsState>,
) {
    let Ok(body) = players.single() else {
        return;
    };
    if body.position.x >= 8360.0 {
        let boss_beat = match campaign.active_stage {
            StageId::Stage1Foundry => StoryBeat::BossVulcanusIntro,
            StageId::Stage2Cryo => StoryBeat::BossBorealisIntro,
        };
        comms.trigger(boss_beat);
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
