//! Native Bevy UI (`Title -> Playing -> Paused` screens, Smash `%` HUD placeholder,
//! controls legend, and graceful Kenney UI audio feedback).

use std::path::Path;

use bevy::audio::Volume;
use bevy::image::{ImageLoaderSettings, ImageSampler};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::FontSize;
use bevy::ui::Pressed;
use bevy::ui_widgets::{Activate, Button as UiButton};
use leafwing_input_manager::prelude::ActionState;

use crate::audio::AudioSettings;
use crate::combat::{
    BusterState, ComboTracker, CounterStance, DamagePercent, GroundDodgeState, LastHitTelemetry,
    LastMoveCallout, MeteorDiveState, ShieldState, SmashChargeState, StageRunTimer,
};
use crate::enemies::{
    ArenaScore, BlastZoneKoEvent, BossState, EncounterStatus, EnemyHealth,
};
use crate::input::{
    ControlConfig, DpadCalibration, DpadWizardState, InputHandlingSet, LiveRawGamepadState,
    MenuAction, MenuInput, PlayerAction,
};
use crate::physics::{KinematicBody, MovementState, PlayerIntent};
use crate::player::{Player, PlayerSpriteVisual};
use crate::rendering::SpriteSheetAnimator;
use crate::state::{AppState, GameStateCommand, StateTransitionSet};

const COLOR_PANEL_BG: Color = Color::srgba(0.05, 0.07, 0.12, 0.92);
const COLOR_OVERLAY_BG: Color = Color::srgba(0.02, 0.03, 0.06, 0.72);
const COLOR_BTN_IDLE: Color = Color::srgb(0.11, 0.15, 0.23);
const COLOR_BTN_FOCUSED: Color = Color::srgb(0.14, 0.36, 0.48);
const COLOR_BTN_PRESSED: Color = Color::srgb(0.95, 0.56, 0.14);
const COLOR_ACCENT_CYAN: Color = Color::srgb(0.20, 0.88, 1.0);
const COLOR_ACCENT_AMBER: Color = Color::srgb(1.0, 0.66, 0.18);
const COLOR_BORDER_IDLE: Color = Color::srgb(0.24, 0.32, 0.46);
const COLOR_TEXT_PRIMARY: Color = Color::srgb(0.94, 0.96, 1.0);
const COLOR_TEXT_MUTED: Color = Color::srgb(0.64, 0.72, 0.84);

/// Curated CC0 Kenney Input Prompt icons loaded gracefully from `assets/ui/prompts/`.
#[derive(Resource, Default, Clone)]
struct PromptIconAssets {
    stick_l: Option<Handle<Image>>,
    xbox_a: Option<Handle<Image>>,
    xbox_x: Option<Handle<Image>>,
    xbox_b: Option<Handle<Image>>,
    xbox_rb: Option<Handle<Image>>,
    xbox_rt: Option<Handle<Image>>,
    xbox_lb: Option<Handle<Image>>,
    xbox_menu: Option<Handle<Image>>,
    key_space: Option<Handle<Image>>,
    key_j: Option<Handle<Image>>,
    key_k: Option<Handle<Image>>,
    key_l: Option<Handle<Image>>,
    key_e: Option<Handle<Image>>,
    key_q: Option<Handle<Image>>,
    key_esc: Option<Handle<Image>>,
}

/// Optional UI audio handles loaded gracefully from `assets/audio/ui/`.
#[derive(Resource, Default)]
struct UiAudioAssets {
    click: Option<Handle<AudioSource>>,
    hover: Option<Handle<AudioSource>>,
}

/// Tracks keyboard/gamepad menu selection index and Moveset overlay visibility.
#[derive(Resource, Default)]
struct MenuFocus {
    index: usize,
    count: usize,
    show_moveset_guide: bool,
    prev_cal_dpad: Vec2,
}

/// Action triggered by an interactive UI menu button.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum MenuButtonAction {
    Play,
    Resume,
    MusicVolume,
    SfxVolume,
    ShieldMode,
    ControlLayout,
    TapJump,
    SmashCharge,
    ToggleMovesetGuide,
    RestartStage,
    CalibrateDpad,
    ReturnToTitle,
    Quit,
}

#[derive(Component)]
struct MusicVolumeLabel;

#[derive(Component)]
struct SfxVolumeLabel;

#[derive(Component)]
struct ShieldModeLabel;

#[derive(Component)]
struct LayoutModeLabel;

#[derive(Component)]
struct TapJumpLabel;

#[derive(Component)]
struct SmashChargeLabel;

/// Live arcade move-name callout text above the P1 Smash % card.
#[derive(Component)]
struct MoveCalloutText;

/// Full-screen Smash Moveset & Live Gamepad Button Inspector overlay (`F1` / menu button).
#[derive(Component)]
struct MovesetGuideOverlay;

#[derive(Component)]
struct LivePadMonitorText;

/// Zero-based ordering index for keyboard/gamepad menu focus.
#[derive(Component, Debug, Clone, Copy)]
struct MenuItemIndex(usize);

/// Marker for the live input / speed / Buster telemetry readout in the `Playing` HUD.
#[derive(Component)]
struct InputReadoutText;

/// Marker for the live P1 Smash `%` label.
#[derive(Component)]
struct PlayerPercentText;

/// Marker for the live target Smash `%` label.
#[derive(Component)]
struct TargetPercentText;

/// Marker for the last-hit knockback / hitstop detail subtitle.
#[derive(Component)]
struct TargetHitDetailText;

/// Marker for the live Zone + KO counter banner in the top-left HUD badge.
#[derive(Component)]
struct ZoneHeaderText;

/// Marker for the dramatic transient `"SMASH KO!"` banner displayed on blast-zone KOs.
#[derive(Component)]
struct KoBannerText {
    timer: f32,
}

/// Marker for the live speedrun stage timer readout (`MM:SS.cc`).
#[derive(Component)]
struct StageTimerText;

/// Marker for the live multi-hit Combo Counter container & text.
#[derive(Component)]
struct ComboBadgeText;

/// Marker for the Ambush Lockdown / Wave / Gate Clear status banner.
#[derive(Component)]
struct EncounterBannerText;

/// Container for the cinematic top-center Boss Health Bar (hidden until Boss is active).
#[derive(Component)]
struct BossHudContainer;

/// Boss title & Phase badge text.
#[derive(Component)]
struct BossNameText;

/// Dynamic width fill bar for the Boss Health Bar.
#[derive(Component)]
struct BossHpFillBar;

/// Persistent modal overlay for the 4-step interactive D-Pad Calibration Wizard (`F2`).
#[derive(Component)]
struct DpadWizardOverlay;

#[derive(Component)]
struct DpadWizardPromptText;

#[derive(Component)]
struct DpadWizardDetailText;

/// Plugin managing all native Bevy UI screens and HUD overlays.
pub struct GameUiPlugin;

impl Plugin for GameUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiAudioAssets>()
            .init_resource::<PromptIconAssets>()
            .init_resource::<MenuFocus>()
            .add_observer(handle_menu_button_activate)
            .add_systems(
                Startup,
                (
                    load_ui_audio_gracefully,
                    load_prompt_icons_gracefully,
                    spawn_dpad_wizard_overlay,
                    spawn_moveset_guide_overlay,
                ),
            )
            .add_systems(OnEnter(AppState::Title), spawn_title_screen)
            .add_systems(OnEnter(AppState::Playing), spawn_playing_hud)
            .add_systems(OnEnter(AppState::Paused), spawn_pause_overlay)
            .add_systems(OnEnter(AppState::Victory), spawn_victory_screen)
            .add_systems(
                PreUpdate,
                handle_menu_keyboard_gamepad_nav
                    .in_set(InputHandlingSet)
                    .before(StateTransitionSet)
                    .run_if(in_menu_state),
            )
            .add_systems(
                Update,
                (
                    handle_menu_mouse_hover,
                    style_menu_buttons,
                    sync_pause_and_control_labels,
                )
                    .chain()
                    .run_if(in_menu_state),
            )
            .add_systems(
                Update,
                (update_dpad_wizard_overlay, update_moveset_guide_overlay),
            )
            .add_systems(
                Update,
                (
                    update_input_readout_hud,
                    update_smash_percent_hud,
                    update_move_callout_hud,
                    update_zone_and_ko_banner,
                    update_encounter_and_boss_hud,
                    update_combo_and_run_timer_hud,
                )
                    .run_if(in_state(AppState::Playing)),
            );
    }
}

fn load_prompt_icon(asset_server: &AssetServer, rel: &str) -> Option<Handle<Image>> {
    if Path::new("assets").join(rel).exists() {
        Some(
            asset_server
                .load_builder()
                .with_settings(|s: &mut ImageLoaderSettings| {
                    s.is_srgb = true;
                    s.sampler = ImageSampler::nearest();
                })
                .load(rel.to_string()),
        )
    } else {
        None
    }
}

fn load_prompt_icons_gracefully(
    asset_server: Res<AssetServer>,
    mut icons: ResMut<PromptIconAssets>,
) {
    icons.stick_l = load_prompt_icon(&asset_server, "ui/prompts/xbox_stick_l.png");
    icons.xbox_a = load_prompt_icon(&asset_server, "ui/prompts/xbox_button_color_a.png");
    icons.xbox_x = load_prompt_icon(&asset_server, "ui/prompts/xbox_button_color_x.png");
    icons.xbox_b = load_prompt_icon(&asset_server, "ui/prompts/xbox_button_color_b.png");
    icons.xbox_rb = load_prompt_icon(&asset_server, "ui/prompts/xbox_rb.png");
    icons.xbox_rt = load_prompt_icon(&asset_server, "ui/prompts/xbox_rt.png");
    icons.xbox_lb = load_prompt_icon(&asset_server, "ui/prompts/xbox_lb.png");
    icons.xbox_menu = load_prompt_icon(&asset_server, "ui/prompts/xbox_button_menu.png");
    icons.key_space = load_prompt_icon(&asset_server, "ui/prompts/keyboard_space.png");
    icons.key_j = load_prompt_icon(&asset_server, "ui/prompts/keyboard_j.png");
    icons.key_k = load_prompt_icon(&asset_server, "ui/prompts/keyboard_k.png");
    icons.key_l = load_prompt_icon(&asset_server, "ui/prompts/keyboard_l.png");
    icons.key_e = load_prompt_icon(&asset_server, "ui/prompts/keyboard_e.png");
    icons.key_q = load_prompt_icon(&asset_server, "ui/prompts/keyboard_q.png");
    icons.key_esc = load_prompt_icon(&asset_server, "ui/prompts/keyboard_escape.png");
}

fn in_menu_state(state: Res<State<AppState>>) -> bool {
    matches!(
        state.get(),
        AppState::Title | AppState::Paused | AppState::Victory
    )
}

fn load_ui_audio_gracefully(asset_server: Res<AssetServer>, mut ui_audio: ResMut<UiAudioAssets>) {
    let click_path = "audio/ui/click1.wav";
    if Path::new("assets").join(click_path).exists() {
        ui_audio.click = Some(asset_server.load(click_path));
    } else {
        warn!(
            "Optional UI click SFX missing at assets/{}; continuing silently.",
            click_path
        );
    }

    let hover_path = "audio/ui/rollover2.wav";
    if Path::new("assets").join(hover_path).exists() {
        ui_audio.hover = Some(asset_server.load(hover_path));
    } else {
        warn!(
            "Optional UI hover SFX missing at assets/{}; continuing silently.",
            hover_path
        );
    }
}

fn play_ui_sfx(commands: &mut Commands, handle: &Option<Handle<AudioSource>>, volume: f32) {
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

fn spawn_icon_if_present(
    parent: &mut ChildSpawnerCommands,
    handle: &Option<Handle<Image>>,
    size_px: f32,
) {
    if let Some(img) = handle {
        parent.spawn((
            ImageNode::new(img.clone()),
            Node {
                width: Val::Px(size_px),
                height: Val::Px(size_px),
                ..default()
            },
        ));
    }
}

fn spawn_title_screen(
    mut commands: Commands,
    mut focus: ResMut<MenuFocus>,
    icons: Res<PromptIconAssets>,
    dpad_cal: Res<DpadCalibration>,
) {
    focus.index = 0;
    focus.count = 4;
    focus.show_moveset_guide = false;

    commands
        .spawn((
            Name::new("TitleScreenRoot"),
            DespawnOnExit(AppState::Title),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: Val::Px(24.0),
                padding: UiRect::all(Val::Px(32.0)),
                ..default()
            },
            BackgroundColor(COLOR_OVERLAY_BG),
        ))
        .with_children(|root| {
            // Main Title Card
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(12.0),
                    padding: UiRect::axes(Val::Px(42.0), Val::Px(28.0)),
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Px(6.0)),
                    ..default()
                },
                BackgroundColor(COLOR_PANEL_BG),
                BorderColor::all(COLOR_ACCENT_CYAN),
            ))
            .with_children(|card| {
                card.spawn((
                    Text::new("VELOCITY // BREAKER"),
                    TextFont {
                        font_size: FontSize::Px(46.0),
                        ..default()
                    },
                    TextColor(COLOR_ACCENT_CYAN),
                ));

                card.spawn((
                    Text::new("MEGA MAN X PRECISION   •   SUPER SMASH BROS. IMPACT"),
                    TextFont {
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    TextColor(COLOR_ACCENT_AMBER),
                ));

                // Buttons container
                card.spawn(Node {
                    margin: UiRect::top(Val::Px(18.0)),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(12.0),
                    ..default()
                })
                .with_children(|btns| {
                    spawn_menu_button(btns, "PLAY STAGE 1  [ENTER / A]", MenuButtonAction::Play, 0);
                    spawn_menu_button(
                        btns,
                        "SMASH MOVESET & PAD MONITOR  [F1]",
                        MenuButtonAction::ToggleMovesetGuide,
                        1,
                    );
                    let cal_label = if dpad_cal.is_active() {
                        "RE-CALIBRATE D-PAD  [F2]  (OK)"
                    } else {
                        "CALIBRATE D-PAD  [F2]"
                    };
                    spawn_menu_button(btns, cal_label, MenuButtonAction::CalibrateDpad, 2);
                    spawn_menu_button(btns, "QUIT", MenuButtonAction::Quit, 3);
                });
            });

            // Controls Legend Card
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(6.0),
                    padding: UiRect::axes(Val::Px(24.0), Val::Px(16.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(COLOR_PANEL_BG),
                BorderColor::all(COLOR_BORDER_IDLE),
            ))
            .with_children(|legend| {
                legend.spawn((
                    Text::new("CONTROLS  (GAMEPAD  |  KEYBOARD)"),
                    TextFont {
                        font_size: FontSize::Px(13.0),
                        ..default()
                    },
                    TextColor(COLOR_ACCENT_AMBER),
                ));

                for (label, pad_icon, key_icon, hint) in [
                    ("MOVE / FAST-FALL", &icons.stick_l, &None, "Stick / D-Pad  |  WASD / Arrows"),
                    ("JUMP / DOUBLE / WALL-JUMP", &icons.xbox_a, &icons.key_space, "A (Cross)  |  Space / Z"),
                    ("MELEE / CHARGEABLE SMASH", &icons.xbox_x, &icons.key_j, "X  |  J / X  (Tap=Tilt, Hold=Smash!)"),
                    ("4-WAY SPECIALS / SWORD SPIN", &icons.xbox_b, &icons.key_k, "B  |  K / C  (Up-B=360° Spin, Down-B=Counter/Dive)"),
                    ("MEGA BUSTER (HOLD=CHG)", &icons.xbox_rb, &icons.key_l, "RB / Y  |  L / Shift"),
                    ("DASH / SPIN CHARGE", &icons.xbox_rt, &icons.key_e, "RT / R2  |  E / Ctrl"),
                    ("SHIELD / ROLL / SPOT-DODGE", &icons.xbox_lb, &icons.key_q, "LB / LT + Dir  |  Q / F + Dir  (Config in Pause)"),
                    ("TAUNT / PAUSE", &icons.xbox_menu, &icons.key_esc, "Select=Taunt, Start=Pause  |  T / Esc"),
                ] {
                    legend
                        .spawn(Node {
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(8.0),
                            ..default()
                        })
                        .with_children(|row| {
                            spawn_icon_if_present(row, pad_icon, 20.0);
                            spawn_icon_if_present(row, key_icon, 20.0);
                            row.spawn((
                                Text::new(format!("{:<26} {}", label, hint)),
                                TextFont {
                                    font_size: FontSize::Px(12.0),
                                    ..default()
                                },
                                TextColor(COLOR_TEXT_MUTED),
                            ));
                        });
                }
            });
        });
}

fn spawn_playing_hud(mut commands: Commands, icons: Res<PromptIconAssets>) {
    commands
        .spawn((
            Name::new("PlayingHudRoot"),
            DespawnOnExit(AppState::Playing),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::SpaceBetween,
                padding: UiRect::all(Val::Px(16.0)),
                ..default()
            },
        ))
        .with_children(|hud| {
            // Top bar
            hud.spawn(Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::FlexStart,
                ..default()
            })
            .with_children(|top| {
                // Left column: Zone badge + pop-in Combo Counter
                top.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(8.0),
                    align_items: AlignItems::FlexStart,
                    ..default()
                })
                .with_children(|left| {
                    left.spawn((
                        Node {
                            padding: UiRect::axes(Val::Px(12.0), Val::Px(6.0)),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(4.0)),
                            ..default()
                        },
                        BackgroundColor(COLOR_PANEL_BG),
                        BorderColor::all(COLOR_ACCENT_CYAN),
                    ))
                    .with_children(|badge| {
                        badge.spawn((
                            ZoneHeaderText,
                            Text::new("THE FOUNDRY GYM  •  ZONE A: SONIC RUNWAY   |   KOs: 0"),
                            TextFont {
                                font_size: FontSize::Px(13.0),
                                ..default()
                            },
                            TextColor(COLOR_TEXT_PRIMARY),
                        ));
                    });

                    left.spawn((
                        ComboBadgeText,
                        Text::new(""),
                        TextFont {
                            font_size: FontSize::Px(20.0),
                            ..default()
                        },
                        TextColor(COLOR_ACCENT_AMBER),
                    ));
                });

                // Center column: Ambush Lockdown Banner + Boss Health Bar + KO Banner
                top.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(6.0),
                    ..default()
                })
                .with_children(|center| {
                    center.spawn((
                        EncounterBannerText,
                        Text::new(""),
                        TextFont {
                            font_size: FontSize::Px(16.0),
                            ..default()
                        },
                        TextColor(COLOR_ACCENT_AMBER),
                    ));

                    // Cinematic Boss Health Bar (hidden until IRON TITAN: VULCANUS awakens!)
                    center
                        .spawn((
                            BossHudContainer,
                            Node {
                                display: Display::None,
                                flex_direction: FlexDirection::Column,
                                align_items: AlignItems::Center,
                                row_gap: Val::Px(4.0),
                                padding: UiRect::axes(Val::Px(14.0), Val::Px(8.0)),
                                border: UiRect::all(Val::Px(2.0)),
                                border_radius: BorderRadius::all(Val::Px(4.0)),
                                ..default()
                            },
                            BackgroundColor(COLOR_PANEL_BG),
                            BorderColor::all(Color::srgb(0.95, 0.32, 0.18)),
                        ))
                        .with_children(|boss_box| {
                            boss_box.spawn((
                                BossNameText,
                                Text::new("IRON TITAN: VULCANUS  •  PHASE 1"),
                                TextFont {
                                    font_size: FontSize::Px(13.0),
                                    ..default()
                                },
                                TextColor(COLOR_ACCENT_AMBER),
                            ));
                            boss_box
                                .spawn((
                                    Node {
                                        width: Val::Px(380.0),
                                        height: Val::Px(14.0),
                                        border: UiRect::all(Val::Px(1.0)),
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgb(0.08, 0.05, 0.08)),
                                    BorderColor::all(COLOR_BORDER_IDLE),
                                ))
                                .with_children(|bar_bg| {
                                    bar_bg.spawn((
                                        BossHpFillBar,
                                        Node {
                                            width: Val::Percent(100.0),
                                            height: Val::Percent(100.0),
                                            ..default()
                                        },
                                        BackgroundColor(Color::srgb(0.96, 0.24, 0.20)),
                                    ));
                                });
                        });

                    center.spawn((
                        KoBannerText { timer: 0.0 },
                        Text::new(""),
                        TextFont {
                            font_size: FontSize::Px(26.0),
                            ..default()
                        },
                        TextColor(COLOR_ACCENT_AMBER),
                    ));
                });

                // Right group: Speedrun Timer + Pause hint with Kenney icon
                top.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(8.0),
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|right| {
                    right
                        .spawn((
                            Node {
                                padding: UiRect::axes(Val::Px(12.0), Val::Px(6.0)),
                                border: UiRect::all(Val::Px(1.0)),
                                border_radius: BorderRadius::all(Val::Px(4.0)),
                                ..default()
                            },
                            BackgroundColor(COLOR_PANEL_BG),
                            BorderColor::all(COLOR_ACCENT_CYAN),
                        ))
                        .with_children(|timer_box| {
                            timer_box.spawn((
                                StageTimerText,
                                Text::new("00:00.00"),
                                TextFont {
                                    font_size: FontSize::Px(14.0),
                                    ..default()
                                },
                                TextColor(COLOR_ACCENT_CYAN),
                            ));
                        });

                    right
                        .spawn((
                            Node {
                                flex_direction: FlexDirection::Row,
                                align_items: AlignItems::Center,
                                column_gap: Val::Px(6.0),
                                padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)),
                                border: UiRect::all(Val::Px(1.0)),
                                border_radius: BorderRadius::all(Val::Px(4.0)),
                                ..default()
                            },
                            BackgroundColor(COLOR_PANEL_BG),
                            BorderColor::all(COLOR_BORDER_IDLE),
                        ))
                        .with_children(|hint| {
                            spawn_icon_if_present(hint, &icons.xbox_menu, 18.0);
                            spawn_icon_if_present(hint, &icons.key_esc, 18.0);
                            hint.spawn((
                                Text::new("PAUSE"),
                                TextFont {
                                    font_size: FontSize::Px(13.0),
                                    ..default()
                                },
                                TextColor(COLOR_ACCENT_AMBER),
                            ));
                        });
                });
            });

            // Bottom bar: Smash % HUD cards (left) + live telemetry (right)
            hud.spawn(Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::FlexEnd,
                ..default()
            })
            .with_children(|bottom| {
                // Smash % group (Live Move Callout + P1 + Last Struck Target)
                bottom
                    .spawn(Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(6.0),
                        align_items: AlignItems::FlexStart,
                        ..default()
                    })
                    .with_children(|left_col| {
                        left_col.spawn((
                            MoveCalloutText,
                            Text::new(""),
                            TextFont {
                                font_size: FontSize::Px(15.0),
                                ..default()
                            },
                            TextColor(COLOR_ACCENT_AMBER),
                        ));
                        left_col
                            .spawn(Node {
                                flex_direction: FlexDirection::Row,
                                column_gap: Val::Px(12.0),
                                align_items: AlignItems::FlexEnd,
                                ..default()
                            })
                            .with_children(|cards| {
                                cards
                                    .spawn((
                                        Node {
                                            flex_direction: FlexDirection::Column,
                                            padding: UiRect::axes(Val::Px(16.0), Val::Px(10.0)),
                                            border: UiRect::all(Val::Px(2.0)),
                                            border_radius: BorderRadius::all(Val::Px(4.0)),
                                            ..default()
                                        },
                                        BackgroundColor(COLOR_PANEL_BG),
                                        BorderColor::all(COLOR_ACCENT_CYAN),
                                    ))
                                    .with_children(|pct| {
                                        pct.spawn((
                                            Text::new("P1  BREAKER"),
                                            TextFont {
                                                font_size: FontSize::Px(11.0),
                                                ..default()
                                            },
                                            TextColor(COLOR_TEXT_MUTED),
                                        ));
                                        pct.spawn((
                                            PlayerPercentText,
                                            Text::new("0%"),
                                            TextFont {
                                                font_size: FontSize::Px(32.0),
                                                ..default()
                                            },
                                            TextColor(COLOR_TEXT_PRIMARY),
                                        ));
                                    });

                                cards
                                    .spawn((
                                        Node {
                                            flex_direction: FlexDirection::Column,
                                            padding: UiRect::axes(Val::Px(16.0), Val::Px(10.0)),
                                            border: UiRect::all(Val::Px(2.0)),
                                            border_radius: BorderRadius::all(Val::Px(4.0)),
                                            ..default()
                                        },
                                        BackgroundColor(COLOR_PANEL_BG),
                                        BorderColor::all(COLOR_ACCENT_AMBER),
                                    ))
                                    .with_children(|pct| {
                                        pct.spawn((
                                            Text::new("TARGET  SMASH %"),
                                            TextFont {
                                                font_size: FontSize::Px(11.0),
                                                ..default()
                                            },
                                            TextColor(COLOR_TEXT_MUTED),
                                        ));
                                        pct.spawn((
                                            TargetPercentText,
                                            Text::new("0%"),
                                            TextFont {
                                                font_size: FontSize::Px(32.0),
                                                ..default()
                                            },
                                            TextColor(COLOR_TEXT_PRIMARY),
                                        ));
                                        pct.spawn((
                                            TargetHitDetailText,
                                            Text::new("STRIKE ENEMY TO TEST KB"),
                                            TextFont {
                                                font_size: FontSize::Px(10.0),
                                                ..default()
                                            },
                                            TextColor(COLOR_ACCENT_AMBER),
                                        ));
                                    });
                            });
                    });

                bottom
                    .spawn((
                        Node {
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::FlexEnd,
                            row_gap: Val::Px(4.0),
                            padding: UiRect::axes(Val::Px(14.0), Val::Px(8.0)),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(4.0)),
                            ..default()
                        },
                        BackgroundColor(COLOR_PANEL_BG),
                        BorderColor::all(COLOR_BORDER_IDLE),
                    ))
                    .with_children(|telemetry| {
                        telemetry
                            .spawn(Node {
                                flex_direction: FlexDirection::Row,
                                align_items: AlignItems::Center,
                                column_gap: Val::Px(6.0),
                                ..default()
                            })
                            .with_children(|strip| {
                                for (icon, tag) in [
                                    (&icons.xbox_x, "MELEE"),
                                    (&icons.xbox_b, "SPECIAL"),
                                    (&icons.xbox_rb, "BUSTER"),
                                    (&icons.xbox_rt, "DASH"),
                                    (&icons.xbox_lb, "SHIELD"),
                                ] {
                                    spawn_icon_if_present(strip, icon, 16.0);
                                    strip.spawn((
                                        Text::new(tag),
                                        TextFont {
                                            font_size: FontSize::Px(10.0),
                                            ..default()
                                        },
                                        TextColor(COLOR_TEXT_MUTED),
                                    ));
                                }
                            });

                        telemetry.spawn((
                            InputReadoutText,
                            Text::new("MOVE (0.00, 0.00)  |  ACTIONS: READY"),
                            TextFont {
                                font_size: FontSize::Px(13.0),
                                ..default()
                            },
                            TextColor(COLOR_ACCENT_CYAN),
                        ));
                    });
            });
        });
}

fn spawn_pause_overlay(
    mut commands: Commands,
    mut focus: ResMut<MenuFocus>,
    audio_settings: Res<AudioSettings>,
    control_cfg: Res<ControlConfig>,
) {
    focus.index = 0;
    focus.count = 12;
    focus.show_moveset_guide = false;

    commands
        .spawn((
            Name::new("PauseOverlayRoot"),
            DespawnOnExit(AppState::Paused),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(COLOR_OVERLAY_BG),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(6.0),
                    padding: UiRect::axes(Val::Px(36.0), Val::Px(18.0)),
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Px(6.0)),
                    ..default()
                },
                BackgroundColor(COLOR_PANEL_BG),
                BorderColor::all(COLOR_ACCENT_AMBER),
            ))
            .with_children(|modal| {
                modal.spawn((
                    Text::new("PAUSED  •  SMASH CONFIG"),
                    TextFont {
                        font_size: FontSize::Px(28.0),
                        ..default()
                    },
                    TextColor(COLOR_ACCENT_AMBER),
                ));

                spawn_compact_menu_button(modal, "RESUME  [ESC / START]", MenuButtonAction::Resume, 0);
                spawn_config_button(
                    modal,
                    format!("SHIELD BTN: < {} >", control_cfg.shield_mode.label()),
                    MenuButtonAction::ShieldMode,
                    1,
                    ShieldModeLabel,
                );
                spawn_config_button(
                    modal,
                    format!("LAYOUT: < {} >", control_cfg.layout.label()),
                    MenuButtonAction::ControlLayout,
                    2,
                    LayoutModeLabel,
                );
                spawn_config_button(
                    modal,
                    format!(
                        "TAP-JUMP (UP): < {} >",
                        if control_cfg.tap_jump { "ON" } else { "OFF" }
                    ),
                    MenuButtonAction::TapJump,
                    3,
                    TapJumpLabel,
                );
                spawn_config_button(
                    modal,
                    format!(
                        "SMASH CHARGE: < {} >",
                        if control_cfg.smash_charge { "ON" } else { "OFF" }
                    ),
                    MenuButtonAction::SmashCharge,
                    4,
                    SmashChargeLabel,
                );
                spawn_config_button(
                    modal,
                    format!("MUSIC VOLUME: < {:3.0}% >", audio_settings.music_volume * 100.0),
                    MenuButtonAction::MusicVolume,
                    5,
                    MusicVolumeLabel,
                );
                spawn_config_button(
                    modal,
                    format!("SFX VOLUME: < {:3.0}% >", audio_settings.sfx_volume * 100.0),
                    MenuButtonAction::SfxVolume,
                    6,
                    SfxVolumeLabel,
                );
                spawn_compact_menu_button(
                    modal,
                    "MOVESET GUIDE & PAD MONITOR  [F1]",
                    MenuButtonAction::ToggleMovesetGuide,
                    7,
                );
                spawn_compact_menu_button(
                    modal,
                    "CALIBRATE D-PAD  [F2]",
                    MenuButtonAction::CalibrateDpad,
                    8,
                );
                spawn_compact_menu_button(modal, "RESTART STAGE", MenuButtonAction::RestartStage, 9);
                spawn_compact_menu_button(modal, "RETURN TO TITLE", MenuButtonAction::ReturnToTitle, 10);
                spawn_compact_menu_button(modal, "QUIT", MenuButtonAction::Quit, 11);
                modal.spawn((
                    Text::new("TIP: Left/Right adjusts options  •  If Right D-Pad triggers Shield, switch SHIELD BTN to LT ONLY or press F2!"),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(COLOR_TEXT_MUTED),
                ));
            });
        });
}

fn spawn_compact_menu_button(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    action: MenuButtonAction,
    index: usize,
) {
    parent
        .spawn((
            UiButton,
            Hovered::default(),
            action,
            MenuItemIndex(index),
            Node {
                width: Val::Px(420.0),
                height: Val::Px(34.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(COLOR_BTN_IDLE),
            BorderColor::all(COLOR_BORDER_IDLE),
        ))
        .with_children(|btn| {
            btn.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(13.0),
                    ..default()
                },
                TextColor(COLOR_TEXT_PRIMARY),
            ));
        });
}

fn spawn_config_button<M: Component>(
    parent: &mut ChildSpawnerCommands,
    label: String,
    action: MenuButtonAction,
    index: usize,
    marker: M,
) {
    parent
        .spawn((
            UiButton,
            Hovered::default(),
            action,
            MenuItemIndex(index),
            Node {
                width: Val::Px(420.0),
                height: Val::Px(34.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(COLOR_BTN_IDLE),
            BorderColor::all(COLOR_BORDER_IDLE),
        ))
        .with_children(|btn| {
            btn.spawn((
                marker,
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(13.0),
                    ..default()
                },
                TextColor(COLOR_TEXT_PRIMARY),
            ));
        });
}

fn spawn_victory_screen(
    mut commands: Commands,
    mut focus: ResMut<MenuFocus>,
    score: Res<ArenaScore>,
    combo: Res<ComboTracker>,
    run_timer: Res<StageRunTimer>,
) {
    focus.index = 0;
    focus.count = 2;

    let total = run_timer.elapsed_secs.max(0.0);
    let mins = (total / 60.0).floor() as u32;
    let secs = (total % 60.0).floor() as u32;
    let cs = (total.fract() * 100.0).floor() as u32;

    commands
        .spawn((
            Name::new("VictoryScreenRoot"),
            DespawnOnExit(AppState::Victory),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(COLOR_OVERLAY_BG),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(14.0),
                    padding: UiRect::axes(Val::Px(48.0), Val::Px(34.0)),
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Px(6.0)),
                    ..default()
                },
                BackgroundColor(COLOR_PANEL_BG),
                BorderColor::all(COLOR_ACCENT_CYAN),
            ))
            .with_children(|card| {
                card.spawn((
                    Text::new("STAGE 1 CLEAR!"),
                    TextFont {
                        font_size: FontSize::Px(44.0),
                        ..default()
                    },
                    TextColor(COLOR_ACCENT_CYAN),
                ));
                card.spawn((
                    Text::new("THE FOUNDRY CITADEL CONQUERED  •  VULCANUS DEFEATED"),
                    TextFont {
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    TextColor(COLOR_ACCENT_AMBER),
                ));
                card.spawn((
                    Text::new(format!(
                        "CLEAR TIME: {mins:02}:{secs:02}.{cs:02}   |   KOs: {}   |   ARENAS: {}   |   BEST COMBO: {} HITS",
                        score.ko_count, score.encounters_cleared, combo.best_hits
                    )),
                    TextFont {
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    TextColor(COLOR_TEXT_PRIMARY),
                ));

                card.spawn(Node {
                    margin: UiRect::top(Val::Px(14.0)),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(12.0),
                    ..default()
                })
                .with_children(|btns| {
                    spawn_menu_button(btns, "PLAY AGAIN  [ENTER / A]", MenuButtonAction::RestartStage, 0);
                    spawn_menu_button(btns, "RETURN TO TITLE", MenuButtonAction::ReturnToTitle, 1);
                });
            });
        });
}

fn spawn_dpad_wizard_overlay(mut commands: Commands) {
    commands
        .spawn((
            Name::new("DpadWizardOverlay"),
            DpadWizardOverlay,
            GlobalZIndex(100),
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.01, 0.02, 0.05, 0.88)),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(14.0),
                    padding: UiRect::axes(Val::Px(42.0), Val::Px(30.0)),
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Px(6.0)),
                    ..default()
                },
                BackgroundColor(COLOR_PANEL_BG),
                BorderColor::all(COLOR_ACCENT_CYAN),
            ))
            .with_children(|box_node| {
                box_node.spawn((
                    Text::new("GAMEPAD D-PAD CALIBRATION"),
                    TextFont {
                        font_size: FontSize::Px(24.0),
                        ..default()
                    },
                    TextColor(COLOR_ACCENT_CYAN),
                ));
                box_node.spawn((
                    DpadWizardPromptText,
                    Text::new("STEP 1 / 4 :  PRESS  D-PAD  LEFT"),
                    TextFont {
                        font_size: FontSize::Px(20.0),
                        ..default()
                    },
                    TextColor(COLOR_ACCENT_AMBER),
                ));
                box_node.spawn((
                    DpadWizardDetailText,
                    Text::new("Captures raw buttons, axes & HID hat switches  |  ESC to cancel"),
                    TextFont {
                        font_size: FontSize::Px(13.0),
                        ..default()
                    },
                    TextColor(COLOR_TEXT_MUTED),
                ));
            });
        });
}

fn spawn_moveset_guide_overlay(mut commands: Commands) {
    commands
        .spawn((
            Name::new("MovesetGuideOverlay"),
            MovesetGuideOverlay,
            GlobalZIndex(95),
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.01, 0.02, 0.05, 0.92)),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(10.0),
                    padding: UiRect::axes(Val::Px(32.0), Val::Px(22.0)),
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Px(6.0)),
                    ..default()
                },
                BackgroundColor(COLOR_PANEL_BG),
                BorderColor::all(COLOR_ACCENT_CYAN),
            ))
            .with_children(|box_node| {
                box_node.spawn((
                    Text::new("SMASH BROS. MOVESET GUIDE  &  LIVE PAD MONITOR"),
                    TextFont {
                        font_size: FontSize::Px(22.0),
                        ..default()
                    },
                    TextColor(COLOR_ACCENT_CYAN),
                ));

                for (title, detail) in [
                    ("3-HIT JAB COMBO", "Neutral + Melee (X / J) x3  ->  Slice -> Cross Slash -> Breaker Thrust!"),
                    ("UP-TILT / DOWN-TILT", "Quick Tap Up/Down + Melee  ->  Fast juggle & low sweep combo starters"),
                    ("CHARGEABLE F/U/D-SMASH", "Hold Side/Up/Down + Melee (or Boost+Dir+Melee)  ->  Charges up to 155%! Down-Smash sweeps 360°!"),
                    ("5-WAY AERIALS", "In Air + Melee: N-Air (360° Spin), F-Air, B-Air (Kill), U-Air, D-Air (Meteor Spike + Bounce!)"),
                    ("UP-B: SWORD SPIN", "Grounded Up + Special (B/K): 360° HURRICANE SWORD SPIN!  |  Airborne Up-B: Rising Cyclone Recovery!"),
                    ("NEUTRAL / SIDE / DOWN-B", "Neutral-B: Breaker Wave  |  Side-B: Plasma Blitz  |  Down-B: Aegis Counter (Ground) / Meteor Dive (Air)"),
                    ("SHIELD / ROLL / SPOT / AIR", "Hold Shield + Left/Right = Roll-Dodge  |  Shield + Down = Spot-Dodge  |  Air Shield = Air-Dodge"),
                    ("FAST-FALL / TAUNT", "Tap Down past jump apex = Fast-Fall  |  Select / T = Victory Taunt"),
                ] {
                    box_node.spawn((
                        Text::new(format!("{:<24} {}", title, detail)),
                        TextFont {
                            font_size: FontSize::Px(12.0),
                            ..default()
                        },
                        TextColor(COLOR_TEXT_PRIMARY),
                    ));
                }

                // Live Raw Gamepad Monitor Box (zero console spam!)
                box_node
                    .spawn((
                        Node {
                            margin: UiRect::top(Val::Px(8.0)),
                            padding: UiRect::axes(Val::Px(16.0), Val::Px(8.0)),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(4.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.03, 0.05, 0.09, 0.95)),
                        BorderColor::all(COLOR_ACCENT_AMBER),
                    ))
                    .with_children(|mon| {
                        mon.spawn((
                            LivePadMonitorText,
                            Text::new("LIVE PAD: IDLE (press any controller button to inspect)"),
                            TextFont {
                                font_size: FontSize::Px(12.0),
                                ..default()
                            },
                            TextColor(COLOR_ACCENT_AMBER),
                        ));
                    });

                box_node.spawn((
                    Text::new("PRESS  F1,  B (CIRCLE),  OR  ESC  TO CLOSE"),
                    TextFont {
                        font_size: FontSize::Px(12.0),
                        ..default()
                    },
                    TextColor(COLOR_TEXT_MUTED),
                ));
            });
        });
}

fn spawn_menu_button(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    action: MenuButtonAction,
    index: usize,
) {
    parent
        .spawn((
            UiButton,
            Hovered::default(),
            action,
            MenuItemIndex(index),
            Node {
                width: Val::Px(280.0),
                height: Val::Px(46.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(COLOR_BTN_IDLE),
            BorderColor::all(COLOR_BORDER_IDLE),
        ))
        .with_children(|btn| {
            btn.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(16.0),
                    ..default()
                },
                TextColor(COLOR_TEXT_PRIMARY),
            ));
        });
}

fn dispatch_menu_action(
    action: MenuButtonAction,
    focus: &mut MenuFocus,
    wizard: &mut DpadWizardState,
    raw_gp: &LiveRawGamepadState,
    audio_settings: &mut AudioSettings,
    control_cfg: &mut ControlConfig,
    state_commands: &mut MessageWriter<GameStateCommand>,
) {
    match action {
        MenuButtonAction::Play => {
            state_commands.write(GameStateCommand::StartGame);
        }
        MenuButtonAction::Resume => {
            state_commands.write(GameStateCommand::ResumeGame);
        }
        MenuButtonAction::MusicVolume => {
            if audio_settings.music_volume >= 0.99 {
                audio_settings.music_volume = 0.0;
            } else {
                audio_settings.adjust_music(0.10);
            }
        }
        MenuButtonAction::SfxVolume => {
            if audio_settings.sfx_volume >= 0.99 {
                audio_settings.sfx_volume = 0.0;
            } else {
                audio_settings.adjust_sfx(0.10);
            }
        }
        MenuButtonAction::ShieldMode => {
            control_cfg.shield_mode = control_cfg.shield_mode.cycle(1);
            control_cfg.save_to_disk();
        }
        MenuButtonAction::ControlLayout => {
            control_cfg.layout = control_cfg.layout.toggle();
            control_cfg.save_to_disk();
        }
        MenuButtonAction::TapJump => {
            control_cfg.tap_jump = !control_cfg.tap_jump;
            control_cfg.save_to_disk();
        }
        MenuButtonAction::SmashCharge => {
            control_cfg.smash_charge = !control_cfg.smash_charge;
            control_cfg.save_to_disk();
        }
        MenuButtonAction::ToggleMovesetGuide => {
            focus.show_moveset_guide = !focus.show_moveset_guide;
        }
        MenuButtonAction::RestartStage => {
            state_commands.write(GameStateCommand::RestartStage);
        }
        MenuButtonAction::CalibrateDpad => {
            wizard.start(raw_gp);
        }
        MenuButtonAction::ReturnToTitle => {
            state_commands.write(GameStateCommand::ReturnToTitle);
        }
        MenuButtonAction::Quit => {
            state_commands.write(GameStateCommand::QuitGame);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_menu_keyboard_gamepad_nav(
    mut commands: Commands,
    mut focus: ResMut<MenuFocus>,
    mut wizard: ResMut<DpadWizardState>,
    dpad_cal: Res<DpadCalibration>,
    raw_gp: Res<LiveRawGamepadState>,
    keys: Res<ButtonInput<KeyCode>>,
    mut audio_settings: ResMut<AudioSettings>,
    mut control_cfg: ResMut<ControlConfig>,
    ui_audio: Res<UiAudioAssets>,
    menu_input: Query<&ActionState<MenuAction>, With<MenuInput>>,
    buttons: Query<(&MenuItemIndex, &MenuButtonAction)>,
    mut state_commands: MessageWriter<GameStateCommand>,
) {
    if wizard.active {
        return;
    }

    if keys.just_pressed(KeyCode::F1) {
        focus.show_moveset_guide = !focus.show_moveset_guide;
        play_ui_sfx(&mut commands, &ui_audio.click, audio_settings.sfx_volume);
        return;
    }

    let Ok(actions) = menu_input.single() else {
        return;
    };

    if focus.show_moveset_guide {
        if actions.just_pressed(&MenuAction::Cancel) || actions.just_pressed(&MenuAction::Confirm) {
            focus.show_moveset_guide = false;
            play_ui_sfx(&mut commands, &ui_audio.click, audio_settings.sfx_volume);
        }
        return;
    }

    if focus.count == 0 {
        return;
    }

    // Also allow calibrated D-Pad directions to drive menu navigation seamlessly!
    let cal_v = if dpad_cal.is_active() {
        dpad_cal.sample_vector(&raw_gp)
    } else {
        Vec2::ZERO
    };
    let cal_up = cal_v.y > 0.5 && focus.prev_cal_dpad.y <= 0.5;
    let cal_down = cal_v.y < -0.5 && focus.prev_cal_dpad.y >= -0.5;
    let cal_left = cal_v.x < -0.5 && focus.prev_cal_dpad.x >= -0.5;
    let cal_right = cal_v.x > 0.5 && focus.prev_cal_dpad.x <= 0.5;
    focus.prev_cal_dpad = cal_v;

    if actions.just_pressed(&MenuAction::Up) || cal_up {
        focus.index = (focus.index + focus.count - 1) % focus.count;
        play_ui_sfx(&mut commands, &ui_audio.hover, audio_settings.sfx_volume);
    } else if actions.just_pressed(&MenuAction::Down) || cal_down {
        focus.index = (focus.index + 1) % focus.count;
        play_ui_sfx(&mut commands, &ui_audio.hover, audio_settings.sfx_volume);
    }

    let step_dir: i32 = if actions.just_pressed(&MenuAction::Left) || cal_left {
        -1
    } else if actions.just_pressed(&MenuAction::Right) || cal_right {
        1
    } else {
        0
    };
    if step_dir != 0 {
        let delta_f = (step_dir as f32) * 0.10;
        for (item_idx, action) in &buttons {
            if item_idx.0 == focus.index {
                match action {
                    MenuButtonAction::MusicVolume => {
                        audio_settings.adjust_music(delta_f);
                        play_ui_sfx(&mut commands, &ui_audio.click, audio_settings.sfx_volume);
                    }
                    MenuButtonAction::SfxVolume => {
                        audio_settings.adjust_sfx(delta_f);
                        play_ui_sfx(&mut commands, &ui_audio.click, audio_settings.sfx_volume);
                    }
                    MenuButtonAction::ShieldMode => {
                        control_cfg.shield_mode = control_cfg.shield_mode.cycle(step_dir);
                        control_cfg.save_to_disk();
                        play_ui_sfx(&mut commands, &ui_audio.click, audio_settings.sfx_volume);
                    }
                    MenuButtonAction::ControlLayout => {
                        control_cfg.layout = control_cfg.layout.toggle();
                        control_cfg.save_to_disk();
                        play_ui_sfx(&mut commands, &ui_audio.click, audio_settings.sfx_volume);
                    }
                    MenuButtonAction::TapJump => {
                        control_cfg.tap_jump = !control_cfg.tap_jump;
                        control_cfg.save_to_disk();
                        play_ui_sfx(&mut commands, &ui_audio.click, audio_settings.sfx_volume);
                    }
                    MenuButtonAction::SmashCharge => {
                        control_cfg.smash_charge = !control_cfg.smash_charge;
                        control_cfg.save_to_disk();
                        play_ui_sfx(&mut commands, &ui_audio.click, audio_settings.sfx_volume);
                    }
                    _ => {}
                }
                break;
            }
        }
    }

    if actions.just_pressed(&MenuAction::Confirm) {
        for (item_idx, action) in &buttons {
            if item_idx.0 == focus.index {
                dispatch_menu_action(
                    *action,
                    &mut focus,
                    &mut wizard,
                    &raw_gp,
                    &mut audio_settings,
                    &mut control_cfg,
                    &mut state_commands,
                );
                play_ui_sfx(&mut commands, &ui_audio.click, audio_settings.sfx_volume);
                break;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_menu_button_activate(
    activate: On<Activate>,
    mut commands: Commands,
    mut focus: ResMut<MenuFocus>,
    mut wizard: ResMut<DpadWizardState>,
    raw_gp: Res<LiveRawGamepadState>,
    mut audio_settings: ResMut<AudioSettings>,
    mut control_cfg: ResMut<ControlConfig>,
    ui_audio: Res<UiAudioAssets>,
    buttons: Query<(&MenuItemIndex, &MenuButtonAction)>,
    mut state_commands: MessageWriter<GameStateCommand>,
) {
    if wizard.active || focus.show_moveset_guide {
        return;
    }
    let Ok((item_idx, action)) = buttons.get(activate.entity) else {
        return;
    };

    focus.index = item_idx.0;
    dispatch_menu_action(
        *action,
        &mut focus,
        &mut wizard,
        &raw_gp,
        &mut audio_settings,
        &mut control_cfg,
        &mut state_commands,
    );
    play_ui_sfx(&mut commands, &ui_audio.click, audio_settings.sfx_volume);
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn sync_pause_and_control_labels(
    audio_settings: Res<AudioSettings>,
    control_cfg: Res<ControlConfig>,
    mut music_q: Query<
        &mut Text,
        (
            With<MusicVolumeLabel>,
            Without<SfxVolumeLabel>,
            Without<ShieldModeLabel>,
            Without<LayoutModeLabel>,
            Without<TapJumpLabel>,
            Without<SmashChargeLabel>,
        ),
    >,
    mut sfx_q: Query<
        &mut Text,
        (
            With<SfxVolumeLabel>,
            Without<MusicVolumeLabel>,
            Without<ShieldModeLabel>,
            Without<LayoutModeLabel>,
            Without<TapJumpLabel>,
            Without<SmashChargeLabel>,
        ),
    >,
    mut shield_q: Query<
        &mut Text,
        (
            With<ShieldModeLabel>,
            Without<MusicVolumeLabel>,
            Without<SfxVolumeLabel>,
            Without<LayoutModeLabel>,
            Without<TapJumpLabel>,
            Without<SmashChargeLabel>,
        ),
    >,
    mut layout_q: Query<
        &mut Text,
        (
            With<LayoutModeLabel>,
            Without<MusicVolumeLabel>,
            Without<SfxVolumeLabel>,
            Without<ShieldModeLabel>,
            Without<TapJumpLabel>,
            Without<SmashChargeLabel>,
        ),
    >,
    mut tap_q: Query<
        &mut Text,
        (
            With<TapJumpLabel>,
            Without<MusicVolumeLabel>,
            Without<SfxVolumeLabel>,
            Without<ShieldModeLabel>,
            Without<LayoutModeLabel>,
            Without<SmashChargeLabel>,
        ),
    >,
    mut charge_q: Query<
        &mut Text,
        (
            With<SmashChargeLabel>,
            Without<MusicVolumeLabel>,
            Without<SfxVolumeLabel>,
            Without<ShieldModeLabel>,
            Without<LayoutModeLabel>,
            Without<TapJumpLabel>,
        ),
    >,
) {
    if let Ok(mut t) = music_q.single_mut() {
        **t = format!("MUSIC VOLUME: < {:3.0}% >", audio_settings.music_volume * 100.0);
    }
    if let Ok(mut t) = sfx_q.single_mut() {
        **t = format!("SFX VOLUME: < {:3.0}% >", audio_settings.sfx_volume * 100.0);
    }
    if let Ok(mut t) = shield_q.single_mut() {
        **t = format!("SHIELD BTN: < {} >", control_cfg.shield_mode.label());
    }
    if let Ok(mut t) = layout_q.single_mut() {
        **t = format!("LAYOUT: < {} >", control_cfg.layout.label());
    }
    if let Ok(mut t) = tap_q.single_mut() {
        **t = format!(
            "TAP-JUMP (UP): < {} >",
            if control_cfg.tap_jump { "ON" } else { "OFF" }
        );
    }
    if let Ok(mut t) = charge_q.single_mut() {
        **t = format!(
            "SMASH CHARGE: < {} >",
            if control_cfg.smash_charge { "ON" } else { "OFF" }
        );
    }
}

fn update_moveset_guide_overlay(
    focus: Res<MenuFocus>,
    raw_gp: Res<LiveRawGamepadState>,
    mut overlays: Query<&mut Node, With<MovesetGuideOverlay>>,
    mut monitor: Query<&mut Text, With<LivePadMonitorText>>,
) {
    for mut node in &mut overlays {
        node.display = if focus.show_moveset_guide {
            Display::Flex
        } else {
            Display::None
        };
    }
    if focus.show_moveset_guide
        && let Ok(mut text) = monitor.single_mut()
    {
        **text = format!("LIVE PAD: {}", raw_gp.active_summary());
    }
}

fn update_move_callout_hud(
    callout: Res<LastMoveCallout>,
    mut labels: Query<(&mut Text, &mut TextColor), With<MoveCalloutText>>,
) {
    let Ok((mut text, mut color)) = labels.single_mut() else {
        return;
    };
    if callout.timer > 0.0 && !callout.label.is_empty() {
        **text = callout.label.clone();
        *color = TextColor(if callout.is_heavy {
            COLOR_ACCENT_AMBER
        } else {
            COLOR_ACCENT_CYAN
        });
    } else {
        **text = String::new();
    }
}

#[allow(clippy::type_complexity)]
fn update_dpad_wizard_overlay(
    wizard: Res<DpadWizardState>,
    mut overlays: Query<&mut Node, With<DpadWizardOverlay>>,
    mut prompts: Query<&mut Text, (With<DpadWizardPromptText>, Without<DpadWizardDetailText>)>,
    mut details: Query<&mut Text, (With<DpadWizardDetailText>, Without<DpadWizardPromptText>)>,
) {
    for mut node in &mut overlays {
        node.display = if wizard.active {
            Display::Flex
        } else {
            Display::None
        };
    }
    if !wizard.active {
        return;
    }
    if let Ok(mut prompt) = prompts.single_mut() {
        **prompt = if wizard.settle_timer > 0.0 {
            "RELEASE D-PAD TO NEUTRAL...".to_string()
        } else {
            wizard.current_step_prompt().to_string()
        };
    }
    if let Ok(mut detail) = details.single_mut() {
        if wizard.last_captured_label.is_empty() {
            **detail = "Captures raw buttons, axes & HID hat switches  |  ESC to cancel".to_string();
        } else {
            **detail = format!("{}   |   ESC to cancel", wizard.last_captured_label);
        }
    }
}

fn handle_menu_mouse_hover(
    mut commands: Commands,
    mut focus: ResMut<MenuFocus>,
    audio_settings: Res<AudioSettings>,
    ui_audio: Res<UiAudioAssets>,
    hovered_buttons: Query<(&Hovered, &MenuItemIndex), Changed<Hovered>>,
) {
    for (hovered, item_idx) in &hovered_buttons {
        if hovered.get() && focus.index != item_idx.0 {
            focus.index = item_idx.0;
            play_ui_sfx(&mut commands, &ui_audio.hover, audio_settings.sfx_volume);
        }
    }
}

fn style_menu_buttons(
    focus: Res<MenuFocus>,
    mut buttons: Query<(
        &Hovered,
        Has<Pressed>,
        &MenuItemIndex,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
) {
    for (hovered, is_pressed, item_idx, mut bg, mut border) in &mut buttons {
        let is_focused = item_idx.0 == focus.index;
        if is_pressed {
            *bg = BackgroundColor(COLOR_BTN_PRESSED);
            *border = BorderColor::all(COLOR_ACCENT_AMBER);
        } else if hovered.get() || is_focused {
            *bg = BackgroundColor(COLOR_BTN_FOCUSED);
            *border = BorderColor::all(COLOR_ACCENT_CYAN);
        } else {
            *bg = BackgroundColor(COLOR_BTN_IDLE);
            *border = BorderColor::all(COLOR_BORDER_IDLE);
        }
    }
}

fn smash_percent_color(percent: f32) -> Color {
    if percent >= 140.0 {
        Color::srgb(1.0, 0.22, 0.22)
    } else if percent >= 90.0 {
        Color::srgb(1.0, 0.52, 0.16)
    } else if percent >= 40.0 {
        Color::srgb(1.0, 0.86, 0.28)
    } else {
        COLOR_TEXT_PRIMARY
    }
}

#[allow(clippy::type_complexity)]
fn update_smash_percent_hud(
    player_pct: Query<&DamagePercent, With<Player>>,
    telemetry: Res<LastHitTelemetry>,
    mut p1_labels: Query<
        (&mut Text, &mut TextColor),
        (
            With<PlayerPercentText>,
            Without<TargetPercentText>,
            Without<TargetHitDetailText>,
        ),
    >,
    mut target_labels: Query<
        (&mut Text, &mut TextColor),
        (
            With<TargetPercentText>,
            Without<PlayerPercentText>,
            Without<TargetHitDetailText>,
        ),
    >,
    mut detail_labels: Query<
        &mut Text,
        (
            With<TargetHitDetailText>,
            Without<PlayerPercentText>,
            Without<TargetPercentText>,
        ),
    >,
) {
    if let Ok(pct) = player_pct.single()
        && let Ok((mut text, mut color)) = p1_labels.single_mut()
    {
        **text = format!("{:.0}%", pct.0);
        *color = TextColor(smash_percent_color(pct.0));
    }

    if let Ok((mut text, mut color)) = target_labels.single_mut() {
        **text = format!("{:.0}%", telemetry.target_percent);
        *color = TextColor(smash_percent_color(telemetry.target_percent));
    }

    if let Ok(mut detail) = detail_labels.single_mut() {
        if telemetry.has_hit {
            **detail = format!(
                "KB {:.0} px/s  •  {}F STOP",
                telemetry.last_knockback, telemetry.last_hitstop_frames
            );
        } else {
            **detail = "STRIKE ENEMY TO TEST KB".to_string();
        }
    }
}

#[allow(clippy::type_complexity)]
fn update_zone_and_ko_banner(
    time: Res<Time>,
    score: Res<ArenaScore>,
    players: Query<&KinematicBody, With<Player>>,
    mut ko_events: MessageReader<BlastZoneKoEvent>,
    mut header_labels: Query<&mut Text, (With<ZoneHeaderText>, Without<KoBannerText>)>,
    mut banner_labels: Query<(&mut KoBannerText, &mut Text), Without<ZoneHeaderText>>,
) {
    let zone_name = players
        .single()
        .map(|b| {
            if b.position.x >= 4460.0 {
                "ACT 5: VULCANUS SANCTUM (BOSS)"
            } else if b.position.x >= 3920.0 {
                "ACT 5: BOSS APPROACH"
            } else if b.position.x >= 3320.0 {
                "ACT 4: FORGE GAUNTLET"
            } else if b.position.x >= 1460.0 {
                "ACT 4: SKY-FORGE RUN"
            } else if b.position.x >= 840.0 {
                "ACT 3: SMELTER CAGE"
            } else if b.position.x >= 460.0 {
                "ACT 2: WALL-JUMP SHAFT"
            } else {
                "ACT 1: ENTRY DECK"
            }
        })
        .unwrap_or("ACT 1: ENTRY DECK");

    if let Ok(mut header) = header_labels.single_mut() {
        **header = format!(
            "FOUNDRY CITADEL  •  {}   |   KOs: {}   |   F2: D-PAD CAL",
            zone_name, score.ko_count
        );
    }

    let mut latest_ko: Option<BlastZoneKoEvent> = None;
    for ev in ko_events.read() {
        latest_ko = Some(*ev);
    }

    if let Ok((mut banner, mut text)) = banner_labels.single_mut() {
        if let Some(ko) = latest_ko {
            banner.timer = 2.0;
            let prefix = if ko.is_player { "RING OUT!" } else { "KO!" };
            **text = format!(
                "{}  {} ({:.0}%)",
                prefix,
                ko.display_name(),
                ko.final_percent
            );
        } else if banner.timer > 0.0 {
            banner.timer = (banner.timer - time.delta_secs()).max(0.0);
            if banner.timer <= 0.0 {
                **text = String::new();
            }
        }
    }
}

#[allow(clippy::type_complexity)]
fn update_encounter_and_boss_hud(
    enc_status: Res<EncounterStatus>,
    wizard: Res<DpadWizardState>,
    bosses: Query<(&EnemyHealth, &BossState)>,
    mut enc_texts: Query<
        &mut Text,
        (
            With<EncounterBannerText>,
            Without<BossNameText>,
            Without<ZoneHeaderText>,
            Without<KoBannerText>,
        ),
    >,
    mut boss_boxes: Query<&mut Node, (With<BossHudContainer>, Without<BossHpFillBar>)>,
    mut boss_names: Query<&mut Text, (With<BossNameText>, Without<EncounterBannerText>)>,
    mut boss_fills: Query<(&mut Node, &mut BackgroundColor), (With<BossHpFillBar>, Without<BossHudContainer>)>,
) {
    if let Ok(mut text) = enc_texts.single_mut() {
        if wizard.completed_banner_timer > 0.0 {
            **text = "D-PAD CALIBRATED & SAVED!".to_string();
        } else if !enc_status.banner_text.is_empty() {
            **text = enc_status.banner_text.clone();
        } else if let Some(ref title) = enc_status.active_title {
            if enc_status.is_boss {
                **text = format!("LOCKDOWN  •  DEFEAT {}", title);
            } else {
                **text = format!(
                    "LOCKDOWN: {}  •  WAVE {}/{}  •  {} HOSTILES LEFT",
                    title,
                    enc_status.current_wave,
                    enc_status.total_waves,
                    enc_status.enemies_remaining
                );
            }
        } else {
            **text = String::new();
        }
    }

    let active_boss = bosses.iter().next();
    for mut node in &mut boss_boxes {
        node.display = if active_boss.is_some() {
            Display::Flex
        } else {
            Display::None
        };
    }

    if let Some((hp, boss)) = active_boss {
        if let Ok(mut name) = boss_names.single_mut() {
            let phase_tag = if boss.phase >= 2 {
                "PHASE 2 OVERDRIVE!"
            } else {
                "PHASE 1"
            };
            **name = format!(
                "IRON TITAN: VULCANUS  •  {}  ({:.0}/{:.0} HP)",
                phase_tag, hp.current, hp.max
            );
        }
        if let Ok((mut bar_node, mut bg)) = boss_fills.single_mut() {
            bar_node.width = Val::Percent(hp.ratio() * 100.0);
            *bg = BackgroundColor(if boss.phase >= 2 {
                Color::srgb(1.0, 0.62, 0.14)
            } else {
                Color::srgb(0.96, 0.24, 0.20)
            });
        }
    }
}

#[allow(clippy::type_complexity)]
fn update_combo_and_run_timer_hud(
    combo: Res<ComboTracker>,
    run_timer: Res<StageRunTimer>,
    mut timer_labels: Query<&mut Text, (With<StageTimerText>, Without<ComboBadgeText>)>,
    mut combo_labels: Query<&mut Text, (With<ComboBadgeText>, Without<StageTimerText>)>,
) {
    if let Ok(mut t_text) = timer_labels.single_mut() {
        let total = run_timer.elapsed_secs.max(0.0);
        let mins = (total / 60.0).floor() as u32;
        let secs = (total % 60.0).floor() as u32;
        let cs = ((total.fract()) * 100.0).floor() as u32;
        **t_text = format!("{mins:02}:{secs:02}.{cs:02}");
    }

    if let Ok(mut c_text) = combo_labels.single_mut() {
        if combo.hits >= 2 {
            **c_text = format!(
                "{} HIT COMBO!  •  +{:.0}%   (BEST: {})",
                combo.hits, combo.damage, combo.best_hits
            );
        } else if combo.best_hits >= 2 {
            **c_text = format!("BEST COMBO: {} HITS ({:.0}%)", combo.best_hits, combo.best_damage);
        } else {
            **c_text = String::new();
        }
    }
}

#[allow(clippy::type_complexity)]
fn update_input_readout_hud(
    player_query: Query<
        (
            &ActionState<PlayerAction>,
            &PlayerIntent,
            &KinematicBody,
            &BusterState,
            &ShieldState,
            Option<&GroundDodgeState>,
            Option<&SmashChargeState>,
            Option<&CounterStance>,
            Option<&MeteorDiveState>,
        ),
        With<Player>,
    >,
    hero_anim: Query<&SpriteSheetAnimator, With<PlayerSpriteVisual>>,
    mut labels: Query<&mut Text, With<InputReadoutText>>,
) {
    let Ok((actions, intent, body, buster, shield, ground_dodge, smash_charge, counter, dive)) =
        player_query.single()
    else {
        return;
    };
    let Ok(mut text) = labels.single_mut() else {
        return;
    };

    let mut active = Vec::new();
    if actions.pressed(&PlayerAction::Jump) {
        active.push("JUMP");
    }
    if actions.pressed(&PlayerAction::MeleeAttack) {
        active.push("MELEE");
    }
    if actions.pressed(&PlayerAction::Special) {
        active.push("SPECIAL");
    }
    if actions.pressed(&PlayerAction::Buster) {
        active.push("BUSTER");
    }
    if actions.pressed(&PlayerAction::SpinDash) {
        active.push("DASH");
    }
    if intent.guard_held || shield.active {
        active.push("GUARD");
    }

    let action_summary = if active.is_empty() {
        "READY".to_string()
    } else {
        active.join("+")
    };

    let state_label = match body.state {
        MovementState::Grounded => "GROUND".to_string(),
        MovementState::SpinCharging { charge } => format!("SPIN {:3.0}%", charge * 100.0),
        MovementState::Airborne => "AIR".to_string(),
        MovementState::WallSliding { .. } => "WALL-SLIDE".to_string(),
        MovementState::LoopLocked { .. } => "LOOP 360°".to_string(),
    };

    let charge_tag = if let Some(sc) = smash_charge
        && sc.is_visibly_charging()
    {
        format!("  |  SMASH CHG: {:3.0}%", sc.charge_ratio() * 100.0)
    } else if ground_dodge.is_some_and(GroundDodgeState::is_active) {
        "  |  DODGE!".to_string()
    } else if counter.is_some_and(CounterStance::is_active) {
        "  |  AEGIS COUNTER!".to_string()
    } else if dive.is_some_and(|d| d.active) {
        "  |  METEOR DIVE!".to_string()
    } else if buster.is_fully_charged() {
        "  |  BUSTER: CHARGED!".to_string()
    } else if buster.charge_secs >= 0.15 {
        format!("  |  CHG: {:3.0}%", buster.charge_ratio() * 100.0)
    } else if shield.active {
        format!("  |  SHIELD: {:3.0}%", shield.health_ratio() * 100.0)
    } else {
        String::new()
    };

    let anim_tag = hero_anim
        .single()
        .map(|a| format!("ANIM: {} [{:02}]  |  ", a.current_anim, a.current_frame))
        .unwrap_or_default();

    **text = format!(
        "SPD: {:4.0}  |  {}  |  {}{}{}",
        body.speed(),
        state_label,
        anim_tag,
        action_summary,
        charge_tag
    );
}
