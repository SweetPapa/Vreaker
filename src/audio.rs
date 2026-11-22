//! Audio playback for gameplay SFX with graceful Kenney CC0 `.wav` loading and
//! deterministic in-memory procedural PCM WAV fallback if asset files are absent.

use std::path::Path;

use bevy::prelude::*;

use crate::combat::{BusterShotEvent, CombatHitEvent};
use crate::enemies::BlastZoneKoEvent;
use crate::state::AppState;

/// Loaded (or procedurally synthesized) gameplay SFX handles.
#[derive(Resource, Default)]
pub struct GameAudioAssets {
    pub hit_light: Option<Handle<AudioSource>>,
    pub hit_heavy: Option<Handle<AudioSource>>,
    pub buster_pellet: Option<Handle<AudioSource>>,
    pub buster_charged: Option<Handle<AudioSource>>,
    pub ko_blast: Option<Handle<AudioSource>>,
    pub hazard_hit: Option<Handle<AudioSource>>,
}

/// Plugin registering graceful Kenney SFX loading + procedural fallback audio playback.
pub struct GameAudioPlugin;

impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameAudioAssets>()
            .add_systems(Startup, load_sfx_with_procedural_fallback)
            .add_systems(
                Update,
                play_combat_and_ko_sfx.run_if(in_state(AppState::Playing)),
            );
    }
}

fn load_or_synthesize(
    rel_path: &str,
    asset_server: &AssetServer,
    audio_assets: &mut Assets<AudioSource>,
    freq_start: f32,
    freq_end: f32,
    duration_secs: f32,
    square_wave: bool,
) -> Handle<AudioSource> {
    if Path::new("assets").join(rel_path).exists() {
        asset_server.load(rel_path.to_string())
    } else {
        warn!(
            "Optional SFX missing at assets/{}; using procedural fallback tone.",
            rel_path
        );
        let bytes = synthesize_pcm_wav(freq_start, freq_end, duration_secs, square_wave);
        audio_assets.add(AudioSource {
            bytes: bytes.into(),
        })
    }
}

fn load_sfx_with_procedural_fallback(
    asset_server: Res<AssetServer>,
    mut audio_assets: ResMut<Assets<AudioSource>>,
    mut sfx: ResMut<GameAudioAssets>,
) {
    sfx.hit_light = Some(load_or_synthesize(
        "audio/sfx/hit_light.wav",
        &asset_server,
        &mut audio_assets,
        240.0,
        110.0,
        0.09,
        true,
    ));
    sfx.hit_heavy = Some(load_or_synthesize(
        "audio/sfx/hit_heavy.wav",
        &asset_server,
        &mut audio_assets,
        170.0,
        55.0,
        0.18,
        true,
    ));
    sfx.buster_pellet = Some(load_or_synthesize(
        "audio/sfx/buster_pellet.wav",
        &asset_server,
        &mut audio_assets,
        780.0,
        420.0,
        0.08,
        true,
    ));
    sfx.buster_charged = Some(load_or_synthesize(
        "audio/sfx/buster_charged.wav",
        &asset_server,
        &mut audio_assets,
        520.0,
        140.0,
        0.24,
        true,
    ));
    sfx.ko_blast = Some(load_or_synthesize(
        "audio/sfx/ko_blast.wav",
        &asset_server,
        &mut audio_assets,
        360.0,
        70.0,
        0.42,
        false,
    ));
    sfx.hazard_hit = Some(load_or_synthesize(
        "audio/sfx/hazard_hit.wav",
        &asset_server,
        &mut audio_assets,
        300.0,
        90.0,
        0.15,
        true,
    ));
}

fn play_sfx(commands: &mut Commands, handle: &Option<Handle<AudioSource>>) {
    if let Some(source) = handle {
        commands.spawn((AudioPlayer::new(source.clone()), PlaybackSettings::DESPAWN));
    }
}

fn play_combat_and_ko_sfx(
    mut commands: Commands,
    sfx: Res<GameAudioAssets>,
    mut hit_events: MessageReader<CombatHitEvent>,
    mut shot_events: MessageReader<BusterShotEvent>,
    mut ko_events: MessageReader<BlastZoneKoEvent>,
) {
    for ev in hit_events.read() {
        if ev.attacker == ev.target {
            play_sfx(&mut commands, &sfx.hazard_hit);
        } else if ev.is_charged || ev.knockback_mag >= 300.0 {
            play_sfx(&mut commands, &sfx.hit_heavy);
        } else {
            play_sfx(&mut commands, &sfx.hit_light);
        }
    }

    for shot in shot_events.read() {
        if shot.charged {
            play_sfx(&mut commands, &sfx.buster_charged);
        } else {
            play_sfx(&mut commands, &sfx.buster_pellet);
        }
    }

    for _ko in ko_events.read() {
        play_sfx(&mut commands, &sfx.ko_blast);
    }
}

/// Synthesizes a valid 16-bit mono 22050 Hz RIFF WAV sweep buffer in pure Rust.
pub fn synthesize_pcm_wav(
    freq_start: f32,
    freq_end: f32,
    duration_secs: f32,
    square_wave: bool,
) -> Vec<u8> {
    let sample_rate: u32 = 22_050;
    let num_samples = ((sample_rate as f32) * duration_secs.max(0.02)) as usize;
    let data_bytes = (num_samples * 2) as u32;
    let file_size = 36 + data_bytes;

    let mut buf = Vec::with_capacity((44 + data_bytes) as usize);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&file_size.to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes()); // PCM header size
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM format
    buf.extend_from_slice(&1u16.to_le_bytes()); // Mono
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // Byte rate
    buf.extend_from_slice(&2u16.to_le_bytes()); // Block align
    buf.extend_from_slice(&16u16.to_le_bytes()); // Bits per sample
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_bytes.to_le_bytes());

    let mut phase = 0.0_f32;
    for i in 0..num_samples {
        let t = i as f32 / num_samples as f32;
        let freq = freq_start + (freq_end - freq_start) * t;
        phase += std::f32::consts::TAU * freq / sample_rate as f32;
        let wave = if square_wave {
            if phase.sin() >= 0.0 { 1.0 } else { -1.0 }
        } else {
            phase.sin()
        };
        let env = (1.0 - t) * (1.0 - t);
        let sample = (wave * env * 0.35 * i16::MAX as f32) as i16;
        buf.extend_from_slice(&sample.to_le_bytes());
    }

    buf
}
