//! Audio subsystem: gameplay SFX (Kenney CC0 `.wav` + deterministic pure-Rust PCM fallback)
//! and looping BGM with Stage → Boss crossfading (`music.rs`) + live volume settings.

pub mod music;

use std::path::Path;

use bevy::audio::Volume;
use bevy::prelude::*;

use crate::combat::{BusterShotEvent, CombatHitEvent, CombatVisualEvent};
use crate::enemies::BlastZoneKoEvent;
use crate::physics::{MovementEvent, MovementFxKind};
use crate::state::AppState;

pub use music::{AudioSettings, MusicPlugin, MusicTrackId, MusicTracks};

/// Loaded (or procedurally synthesized) gameplay SFX handles.
#[derive(Resource, Default)]
pub struct GameAudioAssets {
    pub hit_light: Option<Handle<AudioSource>>,
    pub hit_heavy: Option<Handle<AudioSource>>,
    pub buster_pellet: Option<Handle<AudioSource>>,
    pub buster_charged: Option<Handle<AudioSource>>,
    pub ko_blast: Option<Handle<AudioSource>>,
    pub hazard_hit: Option<Handle<AudioSource>>,
    pub jump: Option<Handle<AudioSource>>,
    pub boost_pad: Option<Handle<AudioSource>>,
    pub land: Option<Handle<AudioSource>>,
    pub spin_rev: Option<Handle<AudioSource>>,
    pub shield_block: Option<Handle<AudioSource>>,
    pub air_dodge: Option<Handle<AudioSource>>,
    pub slash_light: Option<Handle<AudioSource>>,
    pub slash_heavy: Option<Handle<AudioSource>>,
    pub counter_boom: Option<Handle<AudioSource>>,
    pub ground_slam: Option<Handle<AudioSource>>,
    pub side_blitz: Option<Handle<AudioSource>>,
    pub enemy_blink: Option<Handle<AudioSource>>,
    pub boss_laser_charge: Option<Handle<AudioSource>>,
    pub cyclone_spin: Option<Handle<AudioSource>>,
    pub smash_release: Option<Handle<AudioSource>>,
    pub fast_fall: Option<Handle<AudioSource>>,
    pub arcane_erupt: Option<Handle<AudioSource>>,
    pub jump_pad: Option<Handle<AudioSource>>,
    pub checkpoint: Option<Handle<AudioSource>>,
    pub collect_core: Option<Handle<AudioSource>>,
    pub collect_relic: Option<Handle<AudioSource>>,
    pub wall_break: Option<Handle<AudioSource>>,
}

/// Plugin registering graceful Kenney SFX loading, procedural fallbacks, and looping BGM crossfades.
pub struct GameAudioPlugin;

impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameAudioAssets>()
            .add_plugins(MusicPlugin)
            .add_systems(Startup, load_sfx_with_procedural_fallback)
            .add_systems(
                Update,
                (
                    play_combat_and_ko_sfx,
                    play_combat_visual_sfx,
                    play_movement_sfx,
                )
                    .run_if(in_state(AppState::Playing)),
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

fn load_or_synthesize_rising(
    rel_path: &str,
    asset_server: &AssetServer,
    audio_assets: &mut Assets<AudioSource>,
    freq_start: f32,
    freq_end: f32,
    duration_secs: f32,
) -> Handle<AudioSource> {
    if Path::new("assets").join(rel_path).exists() {
        asset_server.load(rel_path.to_string())
    } else {
        let bytes = synthesize_rising_charge_wav(freq_start, freq_end, duration_secs);
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
    sfx.jump = Some(load_or_synthesize(
        "audio/sfx/jump.wav",
        &asset_server,
        &mut audio_assets,
        310.0,
        620.0,
        0.09,
        true,
    ));
    sfx.boost_pad = Some(load_or_synthesize(
        "audio/sfx/boost_pad.wav",
        &asset_server,
        &mut audio_assets,
        240.0,
        920.0,
        0.22,
        false,
    ));
    sfx.land = Some(load_or_synthesize(
        "audio/sfx/land.wav",
        &asset_server,
        &mut audio_assets,
        135.0,
        62.0,
        0.06,
        false,
    ));
    sfx.spin_rev = Some(load_or_synthesize(
        "audio/sfx/spin_rev.wav",
        &asset_server,
        &mut audio_assets,
        220.0,
        560.0,
        0.08,
        true,
    ));
    sfx.shield_block = Some(load_or_synthesize(
        "audio/sfx/shield_block.wav",
        &asset_server,
        &mut audio_assets,
        940.0,
        520.0,
        0.11,
        false,
    ));
    sfx.air_dodge = Some(load_or_synthesize(
        "audio/sfx/air_dodge.wav",
        &asset_server,
        &mut audio_assets,
        680.0,
        290.0,
        0.10,
        false,
    ));
    sfx.slash_light = Some(load_or_synthesize(
        "audio/sfx/slash_light.wav",
        &asset_server,
        &mut audio_assets,
        520.0,
        210.0,
        0.065,
        false,
    ));
    sfx.slash_heavy = Some(load_or_synthesize(
        "audio/sfx/slash_heavy.wav",
        &asset_server,
        &mut audio_assets,
        360.0,
        115.0,
        0.13,
        true,
    ));
    sfx.counter_boom = Some(load_or_synthesize(
        "audio/sfx/counter_boom.wav",
        &asset_server,
        &mut audio_assets,
        1040.0,
        180.0,
        0.24,
        true,
    ));
    sfx.ground_slam = Some(load_or_synthesize(
        "audio/sfx/ground_slam.wav",
        &asset_server,
        &mut audio_assets,
        165.0,
        48.0,
        0.22,
        true,
    ));
    sfx.side_blitz = Some(load_or_synthesize(
        "audio/sfx/side_blitz.wav",
        &asset_server,
        &mut audio_assets,
        440.0,
        980.0,
        0.14,
        true,
    ));
    sfx.enemy_blink = Some(load_or_synthesize(
        "audio/sfx/enemy_blink.wav",
        &asset_server,
        &mut audio_assets,
        880.0,
        340.0,
        0.12,
        false,
    ));
    sfx.boss_laser_charge = Some(load_or_synthesize_rising(
        "audio/sfx/boss_laser_charge.wav",
        &asset_server,
        &mut audio_assets,
        240.0,
        1280.0,
        0.42,
    ));
    sfx.cyclone_spin = Some(load_or_synthesize_rising(
        "audio/sfx/cyclone_spin.wav",
        &asset_server,
        &mut audio_assets,
        320.0,
        880.0,
        0.24,
    ));
    sfx.smash_release = Some(load_or_synthesize(
        "audio/sfx/smash_release.wav",
        &asset_server,
        &mut audio_assets,
        640.0,
        130.0,
        0.20,
        true,
    ));
    sfx.fast_fall = Some(load_or_synthesize(
        "audio/sfx/fast_fall.wav",
        &asset_server,
        &mut audio_assets,
        820.0,
        380.0,
        0.06,
        false,
    ));
    sfx.arcane_erupt = Some(load_or_synthesize(
        "audio/sfx/arcane_erupt.wav",
        &asset_server,
        &mut audio_assets,
        280.0,
        960.0,
        0.22,
        true,
    ));
    sfx.jump_pad = Some(load_or_synthesize_rising(
        "audio/sfx/jump_pad.wav",
        &asset_server,
        &mut audio_assets,
        220.0,
        960.0,
        0.18,
    ));
    sfx.checkpoint = Some(load_or_synthesize_rising(
        "audio/sfx/checkpoint.wav",
        &asset_server,
        &mut audio_assets,
        440.0,
        1180.0,
        0.28,
    ));
    sfx.collect_core = Some(load_or_synthesize(
        "audio/sfx/collect_core.wav",
        &asset_server,
        &mut audio_assets,
        740.0,
        1240.0,
        0.08,
        false,
    ));
    sfx.collect_relic = Some(load_or_synthesize_rising(
        "audio/sfx/collect_relic.wav",
        &asset_server,
        &mut audio_assets,
        520.0,
        1560.0,
        0.36,
    ));
    sfx.wall_break = Some(load_or_synthesize(
        "audio/sfx/wall_break.wav",
        &asset_server,
        &mut audio_assets,
        180.0,
        42.0,
        0.26,
        true,
    ));
}

fn play_sfx(commands: &mut Commands, handle: &Option<Handle<AudioSource>>, volume: f32) {
    if volume <= 0.005 {
        return;
    }
    if let Some(source) = handle {
        commands.spawn((
            AudioPlayer::new(source.clone()),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(volume.clamp(0.0, 1.0))),
        ));
    }
}

fn play_movement_sfx(
    mut commands: Commands,
    sfx: Res<GameAudioAssets>,
    settings: Res<AudioSettings>,
    mut move_events: MessageReader<MovementEvent>,
) {
    let vol = settings.sfx_volume;
    for ev in move_events.read() {
        match ev.kind {
            MovementFxKind::Jump | MovementFxKind::WallJump => {
                play_sfx(&mut commands, &sfx.jump, vol);
            }
            MovementFxKind::Land => {
                play_sfx(&mut commands, &sfx.land, vol);
            }
            MovementFxKind::BoostPad | MovementFxKind::SpinDashRelease => {
                play_sfx(&mut commands, &sfx.boost_pad, vol);
            }
            MovementFxKind::JumpPad => {
                play_sfx(&mut commands, &sfx.jump_pad, vol);
            }
            MovementFxKind::Checkpoint => {
                play_sfx(&mut commands, &sfx.checkpoint, vol);
            }
            MovementFxKind::CollectCore => {
                play_sfx(&mut commands, &sfx.collect_core, vol * 0.85);
            }
            MovementFxKind::CollectRelic | MovementFxKind::CollectHeal => {
                play_sfx(&mut commands, &sfx.collect_relic, vol);
            }
            MovementFxKind::WallBreak => {
                play_sfx(&mut commands, &sfx.wall_break, vol);
            }
            MovementFxKind::SpinDashRev => {
                play_sfx(&mut commands, &sfx.spin_rev, vol);
            }
            MovementFxKind::ShieldBlock => {
                play_sfx(&mut commands, &sfx.shield_block, vol);
            }
            MovementFxKind::AirDodge | MovementFxKind::RollDodge | MovementFxKind::SpotDodge => {
                play_sfx(&mut commands, &sfx.air_dodge, vol);
            }
            MovementFxKind::FastFall => {
                play_sfx(&mut commands, &sfx.fast_fall, vol);
            }
            MovementFxKind::Taunt => {
                play_sfx(&mut commands, &sfx.boost_pad, vol * 0.7);
            }
            MovementFxKind::DiamondSwitch => {
                play_sfx(&mut commands, &sfx.checkpoint, vol * 0.85);
            }
        }
    }
}

fn play_combat_visual_sfx(
    mut commands: Commands,
    sfx: Res<GameAudioAssets>,
    settings: Res<AudioSettings>,
    mut vis_events: MessageReader<CombatVisualEvent>,
) {
    let vol = settings.sfx_volume;
    for ev in vis_events.read() {
        match ev {
            CombatVisualEvent::SlashArc { heavy, .. } => {
                if *heavy {
                    play_sfx(&mut commands, &sfx.slash_heavy, vol);
                } else {
                    play_sfx(&mut commands, &sfx.slash_light, vol);
                }
            }
            CombatVisualEvent::CounterTrigger { .. } => {
                play_sfx(&mut commands, &sfx.counter_boom, vol);
            }
            CombatVisualEvent::GroundSlam { .. } => {
                play_sfx(&mut commands, &sfx.ground_slam, vol);
            }
            CombatVisualEvent::SideBlitz { .. } => {
                play_sfx(&mut commands, &sfx.side_blitz, vol);
            }
            CombatVisualEvent::EnemyBlink { .. } => {
                play_sfx(&mut commands, &sfx.enemy_blink, vol);
            }
            CombatVisualEvent::BossLaserTelegraph { .. } => {
                play_sfx(&mut commands, &sfx.boss_laser_charge, vol);
            }
            CombatVisualEvent::CycloneSpin { .. } => {
                play_sfx(&mut commands, &sfx.cyclone_spin, vol);
            }
            CombatVisualEvent::SmashChargeRelease { .. } => {
                play_sfx(&mut commands, &sfx.smash_release, vol);
            }
            CombatVisualEvent::ArcanePillarTelegraph { .. } => {
                play_sfx(&mut commands, &sfx.enemy_blink, vol * 0.75);
            }
            CombatVisualEvent::ArcanePillarErupt { .. } => {
                play_sfx(&mut commands, &sfx.arcane_erupt, vol);
            }
            CombatVisualEvent::ShockwaveRing { .. } => {}
        }
    }
}

fn play_combat_and_ko_sfx(
    mut commands: Commands,
    sfx: Res<GameAudioAssets>,
    settings: Res<AudioSettings>,
    mut hit_events: MessageReader<CombatHitEvent>,
    mut shot_events: MessageReader<BusterShotEvent>,
    mut ko_events: MessageReader<BlastZoneKoEvent>,
) {
    let vol = settings.sfx_volume;
    for ev in hit_events.read() {
        if ev.attacker == ev.target {
            play_sfx(&mut commands, &sfx.hazard_hit, vol);
        } else if ev.is_charged || ev.knockback_mag >= 300.0 {
            play_sfx(&mut commands, &sfx.hit_heavy, vol);
        } else {
            play_sfx(&mut commands, &sfx.hit_light, vol);
        }
    }

    for shot in shot_events.read() {
        if shot.charged {
            play_sfx(&mut commands, &sfx.buster_charged, vol);
        } else {
            play_sfx(&mut commands, &sfx.buster_pellet, vol);
        }
    }

    for _ko in ko_events.read() {
        play_sfx(&mut commands, &sfx.ko_blast, vol);
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
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    buf.extend_from_slice(&2u16.to_le_bytes());
    buf.extend_from_slice(&16u16.to_le_bytes());
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

/// Synthesizes a rising-envelope high-tension charge-up sweep for Vulcanus's Phase 2 laser telegraph!
fn synthesize_rising_charge_wav(freq_start: f32, freq_end: f32, duration_secs: f32) -> Vec<u8> {
    let sample_rate: u32 = 22_050;
    let num_samples = ((sample_rate as f32) * duration_secs.max(0.05)) as usize;
    let data_bytes = (num_samples * 2) as u32;
    let file_size = 36 + data_bytes;

    let mut buf = Vec::with_capacity((44 + data_bytes) as usize);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&file_size.to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    buf.extend_from_slice(&2u16.to_le_bytes());
    buf.extend_from_slice(&16u16.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_bytes.to_le_bytes());

    let mut phase = 0.0_f32;
    for i in 0..num_samples {
        let t = i as f32 / num_samples as f32;
        let freq = freq_start + (freq_end - freq_start) * t * t;
        phase += std::f32::consts::TAU * freq / sample_rate as f32;
        let harmonic = 0.65 * phase.sin() + 0.35 * (phase * 2.0).sin();
        let tremolo = 0.78 + 0.22 * (t * 28.0 * std::f32::consts::TAU).sin();
        // Swells smoothly up until right before firing, then crisp release tail
        let env = if t < 0.88 {
            (t / 0.88).sqrt()
        } else {
            ((1.0 - t) / 0.12).max(0.0)
        };
        let sample = (harmonic * tremolo * env * 0.40 * i16::MAX as f32) as i16;
        buf.extend_from_slice(&sample.to_le_bytes());
    }

    buf
}
