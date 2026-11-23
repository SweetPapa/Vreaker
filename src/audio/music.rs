//! Looping background music (BGM) player with smooth `0.8s` Stage → Boss crossfading,
//! Title theme, Stage Clear victory fanfare, Pause ducking, and graceful silent fallback
//! when optional music files are absent.

use std::path::Path;

use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;

use crate::enemies::EncounterStatus;
use crate::state::{AppState, CampaignState, StageId, StageRestartEvent, StageSwitchEvent};

/// Crossfade duration (`seconds`) when transitioning between Stage 1 and Boss themes.
pub const BGM_CROSSFADE_SECS: f32 = 0.8;

/// Live user-adjustable audio levels exposed in the Pause Menu (Gamepad + Keyboard + Mouse).
#[derive(Resource, Debug, Clone, Copy)]
pub struct AudioSettings {
    pub music_volume: f32,
    pub sfx_volume: f32,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            music_volume: 0.55,
            sfx_volume: 0.80,
        }
    }
}

impl AudioSettings {
    pub fn adjust_music(&mut self, delta: f32) {
        self.music_volume = ((self.music_volume + delta) * 20.0).round() / 20.0;
        self.music_volume = self.music_volume.clamp(0.0, 1.0);
    }

    pub fn adjust_sfx(&mut self, delta: f32) {
        self.sfx_volume = ((self.sfx_volume + delta) * 20.0).round() / 20.0;
        self.sfx_volume = self.sfx_volume.clamp(0.0, 1.0);
    }
}

/// Logical BGM track states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MusicTrackId {
    Title,
    Stage1,
    Boss,
    Stage2,
    BossBorealis,
    StageClear,
}

/// Optional BGM asset handles loaded gracefully from `assets/audio/music/`.
#[derive(Resource, Default, Clone)]
pub struct MusicTracks {
    pub title: Option<Handle<AudioSource>>,
    pub stage1: Option<Handle<AudioSource>>,
    pub boss: Option<Handle<AudioSource>>,
    pub stage2: Option<Handle<AudioSource>>,
    pub boss_borealis: Option<Handle<AudioSource>>,
    pub stage_clear: Option<Handle<AudioSource>>,
}

impl MusicTracks {
    pub fn handle_for(&self, id: MusicTrackId) -> Option<Handle<AudioSource>> {
        match id {
            MusicTrackId::Title => self.title.clone(),
            MusicTrackId::Stage1 => self.stage1.clone(),
            MusicTrackId::Boss => self.boss.clone(),
            MusicTrackId::Stage2 => self.stage2.clone(),
            MusicTrackId::BossBorealis => self.boss_borealis.clone(),
            MusicTrackId::StageClear => self.stage_clear.clone(),
        }
    }
}

/// Component attached to each active BGM audio entity tracking its normalized crossfade gain (`0..=1`).
#[derive(Component, Debug, Clone, Copy)]
pub struct BgmChannel {
    pub track: MusicTrackId,
    pub fade: f32,
    pub fading_out: bool,
}

/// Tracks the currently desired BGM track so transitions spawn new channels cleanly without restarts.
#[derive(Resource, Debug, Default)]
struct ActiveMusicState {
    desired: Option<MusicTrackId>,
}

/// Plugin registering looping BGM loading, `0.8s` crossfades, and live Pause volume control.
pub struct MusicPlugin;

impl Plugin for MusicPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AudioSettings>()
            .init_resource::<MusicTracks>()
            .init_resource::<ActiveMusicState>()
            .add_systems(Startup, load_music_tracks_gracefully)
            .add_systems(Update, (update_desired_music_track, tick_bgm_crossfades).chain());
    }
}

fn load_track_if_exists(asset_server: &AssetServer, stem: &str) -> Option<Handle<AudioSource>> {
    for ext in ["ogg", "mp3"] {
        let rel = format!("audio/music/{stem}.{ext}");
        if Path::new("assets").join(&rel).exists() {
            return Some(asset_server.load(rel));
        }
    }
    None
}

fn load_music_tracks_gracefully(asset_server: Res<AssetServer>, mut tracks: ResMut<MusicTracks>) {
    tracks.title = load_track_if_exists(&asset_server, "title_theme");
    tracks.stage1 = load_track_if_exists(&asset_server, "stage1_theme");
    tracks.boss = load_track_if_exists(&asset_server, "boss_theme");
    tracks.stage2 = load_track_if_exists(&asset_server, "stage2_theme");
    tracks.boss_borealis = load_track_if_exists(&asset_server, "boss_borealis");
    tracks.stage_clear = load_track_if_exists(&asset_server, "stage_clear");

    if tracks.stage1.is_none() && tracks.boss.is_none() {
        info!(
            "No BGM files found in assets/audio/music/ (stage1_theme.ogg / boss_theme.ogg); running silently. See assets/audio/music/README.md."
        );
    }
}

/// Computes which Stage 1 track should currently be playing (kept for backward compatibility).
pub fn resolve_desired_track(
    app_state: AppState,
    enc_status: &EncounterStatus,
    current_desired: Option<MusicTrackId>,
) -> Option<MusicTrackId> {
    resolve_desired_track_for_stage(
        app_state,
        StageId::Stage1Foundry,
        enc_status,
        current_desired,
    )
}

/// Computes which track should currently be playing based on `AppState`, active [`StageId`],
/// and Boss Encounter state.
pub fn resolve_desired_track_for_stage(
    app_state: AppState,
    stage: StageId,
    enc_status: &EncounterStatus,
    current_desired: Option<MusicTrackId>,
) -> Option<MusicTrackId> {
    let stage_track = match stage {
        StageId::Stage1Foundry => MusicTrackId::Stage1,
        StageId::Stage2Cryo => MusicTrackId::Stage2,
    };
    let boss_track = match stage {
        StageId::Stage1Foundry => MusicTrackId::Boss,
        StageId::Stage2Cryo => MusicTrackId::BossBorealis,
    };

    match app_state {
        AppState::Title => Some(MusicTrackId::Title),
        AppState::Victory => Some(MusicTrackId::StageClear),
        AppState::Paused => current_desired.or(Some(stage_track)),
        AppState::Playing => {
            if enc_status.victory_delay_timer.is_some() {
                // Boss just exploded — fade down Boss theme cleanly right before Stage Clear fanfare!
                None
            } else if enc_status.is_boss && enc_status.active_title.is_some() {
                Some(boss_track)
            } else {
                Some(stage_track)
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn update_desired_music_track(
    mut commands: Commands,
    state: Res<State<AppState>>,
    campaign: Res<CampaignState>,
    enc_status: Res<EncounterStatus>,
    tracks: Res<MusicTracks>,
    mut restarts: MessageReader<StageRestartEvent>,
    mut switches: MessageReader<StageSwitchEvent>,
    mut active: ResMut<ActiveMusicState>,
    mut channels: Query<&mut BgmChannel>,
) {
    let restarted = restarts.read().next().is_some() || switches.read().next().is_some();
    let desired = resolve_desired_track_for_stage(
        *state.get(),
        campaign.active_stage,
        &enc_status,
        active.desired,
    );

    if desired == active.desired && !restarted {
        return;
    }
    active.desired = desired;

    // Fade out any currently playing channel that does not match the new desired track (or all on restart)
    let mut already_playing_desired = false;
    for mut ch in &mut channels {
        if !restarted && Some(ch.track) == desired {
            ch.fading_out = false;
            already_playing_desired = true;
        } else {
            ch.fading_out = true;
        }
    }

    if let Some(track_id) = desired
        && !already_playing_desired
        && let Some(handle) = tracks.handle_for(track_id)
    {
        let mode = if track_id == MusicTrackId::StageClear {
            PlaybackMode::Despawn
        } else {
            PlaybackMode::Loop
        };
        let initial_fade = if track_id == MusicTrackId::StageClear {
            1.0
        } else {
            0.01
        };
        commands.spawn((
            Name::new(format!("BgmChannel_{track_id:?}")),
            BgmChannel {
                track: track_id,
                fade: initial_fade,
                fading_out: false,
            },
            AudioPlayer::new(handle),
            PlaybackSettings {
                mode,
                volume: Volume::Linear(0.0),
                ..default()
            },
        ));
    }
}

fn tick_bgm_crossfades(
    mut commands: Commands,
    time: Res<Time>,
    state: Res<State<AppState>>,
    settings: Res<AudioSettings>,
    mut channels: Query<(Entity, &mut BgmChannel, Option<&mut AudioSink>)>,
) {
    let dt = time.delta_secs();
    let fade_rate = 1.0 / BGM_CROSSFADE_SECS;
    let pause_duck = if *state.get() == AppState::Paused {
        0.52
    } else {
        1.0
    };

    for (entity, mut ch, maybe_sink) in &mut channels {
        if ch.fading_out {
            ch.fade = (ch.fade - dt * fade_rate).max(0.0);
            if ch.fade <= 0.001 {
                commands.entity(entity).despawn();
                continue;
            }
        } else {
            ch.fade = (ch.fade + dt * fade_rate).min(1.0);
        }

        if let Some(mut sink) = maybe_sink {
            let linear = (ch.fade * settings.music_volume * pause_duck).clamp(0.0, 1.0);
            sink.set_volume(Volume::Linear(linear));
        }
    }
}
