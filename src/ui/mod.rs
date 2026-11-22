//! Native Bevy UI (`Title -> Playing -> Paused` screens, Smash `%` HUD placeholder,
//! controls legend, and graceful Kenney UI audio feedback).

use std::path::Path;

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::FontSize;
use bevy::ui::Pressed;
use bevy::ui_widgets::{Activate, Button as UiButton};
use leafwing_input_manager::prelude::ActionState;

use crate::combat::{BusterState, DamagePercent, LastHitTelemetry};
use crate::enemies::{ArenaScore, BlastZoneKoEvent};
use crate::input::{InputHandlingSet, MenuAction, MenuInput, PlayerAction};
use crate::physics::{KinematicBody, MovementState};
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

/// Optional UI audio handles loaded gracefully from `assets/audio/ui/`.
#[derive(Resource, Default)]
struct UiAudioAssets {
    click: Option<Handle<AudioSource>>,
    hover: Option<Handle<AudioSource>>,
}

/// Tracks keyboard/gamepad menu selection index for the currently active menu.
#[derive(Resource, Default)]
struct MenuFocus {
    index: usize,
    count: usize,
}

/// Action triggered by an interactive UI menu button.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum MenuButtonAction {
    Play,
    Resume,
    RestartStage,
    ReturnToTitle,
    Quit,
}

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

/// Plugin managing all native Bevy UI screens and HUD overlays.
pub struct GameUiPlugin;

impl Plugin for GameUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiAudioAssets>()
            .init_resource::<MenuFocus>()
            .add_observer(handle_menu_button_activate)
            .add_systems(Startup, load_ui_audio_gracefully)
            .add_systems(OnEnter(AppState::Title), spawn_title_screen)
            .add_systems(OnEnter(AppState::Playing), spawn_playing_hud)
            .add_systems(OnEnter(AppState::Paused), spawn_pause_overlay)
            .add_systems(
                PreUpdate,
                handle_menu_keyboard_gamepad_nav
                    .in_set(InputHandlingSet)
                    .before(StateTransitionSet)
                    .run_if(in_menu_state),
            )
            .add_systems(
                Update,
                (handle_menu_mouse_hover, style_menu_buttons)
                    .chain()
                    .run_if(in_menu_state),
            )
            .add_systems(
                Update,
                (
                    update_input_readout_hud,
                    update_smash_percent_hud,
                    update_zone_and_ko_banner,
                )
                    .run_if(in_state(AppState::Playing)),
            );
    }
}

fn in_menu_state(state: Res<State<AppState>>) -> bool {
    matches!(state.get(), AppState::Title | AppState::Paused)
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

fn play_ui_sfx(commands: &mut Commands, handle: &Option<Handle<AudioSource>>) {
    if let Some(source) = handle {
        commands.spawn((AudioPlayer::new(source.clone()), PlaybackSettings::DESPAWN));
    }
}

fn spawn_title_screen(mut commands: Commands, mut focus: ResMut<MenuFocus>) {
    focus.index = 0;
    focus.count = 2;

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
                    Text::new("SONIC FLOW   •   MEGA PRECISION   •   SMASH IMPACT"),
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
                    spawn_menu_button(btns, "PLAY  [ENTER / A]", MenuButtonAction::Play, 0);
                    spawn_menu_button(btns, "QUIT", MenuButtonAction::Quit, 1);
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

                for line in [
                    "MOVE / AIM     : Left Stick / D-Pad   |   WASD / Arrows",
                    "JUMP           : A (Cross)            |   Space / Z",
                    "MELEE ATTACK   : X (Square)           |   J / X",
                    "SPECIAL / UP-B : B (Circle)           |   K / C",
                    "MEGA BUSTER    : RB / Y (Triangle)    |   L / Shift",
                    "SPIN DASH      : RT / R2              |   E / Left Ctrl",
                    "PAUSE          : Start                |   Esc",
                ] {
                    legend.spawn((
                        Text::new(line),
                        TextFont {
                            font_size: FontSize::Px(12.0),
                            ..default()
                        },
                        TextColor(COLOR_TEXT_MUTED),
                    ));
                }
            });
        });
}

fn spawn_playing_hud(mut commands: Commands) {
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
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|top| {
                top.spawn((
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

                top.spawn((
                    KoBannerText { timer: 0.0 },
                    Text::new(""),
                    TextFont {
                        font_size: FontSize::Px(26.0),
                        ..default()
                    },
                    TextColor(COLOR_ACCENT_AMBER),
                ));

                top.spawn((
                    Node {
                        padding: UiRect::axes(Val::Px(12.0), Val::Px(6.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(4.0)),
                        ..default()
                    },
                    BackgroundColor(COLOR_PANEL_BG),
                    BorderColor::all(COLOR_BORDER_IDLE),
                ))
                .with_children(|hint| {
                    hint.spawn((
                        Text::new("[ESC / START] PAUSE"),
                        TextFont {
                            font_size: FontSize::Px(13.0),
                            ..default()
                        },
                        TextColor(COLOR_ACCENT_AMBER),
                    ));
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
                // Smash % group (P1 + Last Struck Target)
                bottom
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

                bottom
                    .spawn((
                        Node {
                            padding: UiRect::axes(Val::Px(14.0), Val::Px(8.0)),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(4.0)),
                            ..default()
                        },
                        BackgroundColor(COLOR_PANEL_BG),
                        BorderColor::all(COLOR_BORDER_IDLE),
                    ))
                    .with_children(|telemetry| {
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

fn spawn_pause_overlay(mut commands: Commands, mut focus: ResMut<MenuFocus>) {
    focus.index = 0;
    focus.count = 4;

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
                    row_gap: Val::Px(14.0),
                    padding: UiRect::axes(Val::Px(44.0), Val::Px(32.0)),
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Px(6.0)),
                    ..default()
                },
                BackgroundColor(COLOR_PANEL_BG),
                BorderColor::all(COLOR_ACCENT_AMBER),
            ))
            .with_children(|modal| {
                modal.spawn((
                    Text::new("PAUSED"),
                    TextFont {
                        font_size: FontSize::Px(36.0),
                        ..default()
                    },
                    TextColor(COLOR_ACCENT_AMBER),
                ));

                spawn_menu_button(modal, "RESUME  [ESC / START]", MenuButtonAction::Resume, 0);
                spawn_menu_button(modal, "RESTART STAGE", MenuButtonAction::RestartStage, 1);
                spawn_menu_button(modal, "RETURN TO TITLE", MenuButtonAction::ReturnToTitle, 2);
                spawn_menu_button(modal, "QUIT", MenuButtonAction::Quit, 3);
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
    state_commands: &mut MessageWriter<GameStateCommand>,
) {
    match action {
        MenuButtonAction::Play => state_commands.write(GameStateCommand::StartGame),
        MenuButtonAction::Resume => state_commands.write(GameStateCommand::ResumeGame),
        MenuButtonAction::RestartStage => state_commands.write(GameStateCommand::RestartStage),
        MenuButtonAction::ReturnToTitle => state_commands.write(GameStateCommand::ReturnToTitle),
        MenuButtonAction::Quit => state_commands.write(GameStateCommand::QuitGame),
    };
}

fn handle_menu_keyboard_gamepad_nav(
    mut commands: Commands,
    mut focus: ResMut<MenuFocus>,
    ui_audio: Res<UiAudioAssets>,
    menu_input: Query<&ActionState<MenuAction>, With<MenuInput>>,
    buttons: Query<(&MenuItemIndex, &MenuButtonAction)>,
    mut state_commands: MessageWriter<GameStateCommand>,
) {
    let Ok(actions) = menu_input.single() else {
        return;
    };
    if focus.count == 0 {
        return;
    }

    if actions.just_pressed(&MenuAction::Up) {
        focus.index = (focus.index + focus.count - 1) % focus.count;
        play_ui_sfx(&mut commands, &ui_audio.hover);
    } else if actions.just_pressed(&MenuAction::Down) {
        focus.index = (focus.index + 1) % focus.count;
        play_ui_sfx(&mut commands, &ui_audio.hover);
    }

    if actions.just_pressed(&MenuAction::Confirm) {
        for (item_idx, action) in &buttons {
            if item_idx.0 == focus.index {
                play_ui_sfx(&mut commands, &ui_audio.click);
                dispatch_menu_action(*action, &mut state_commands);
                break;
            }
        }
    }
}

fn handle_menu_button_activate(
    activate: On<Activate>,
    mut commands: Commands,
    mut focus: ResMut<MenuFocus>,
    ui_audio: Res<UiAudioAssets>,
    buttons: Query<(&MenuItemIndex, &MenuButtonAction)>,
    mut state_commands: MessageWriter<GameStateCommand>,
) {
    let Ok((item_idx, action)) = buttons.get(activate.entity) else {
        return;
    };

    focus.index = item_idx.0;
    play_ui_sfx(&mut commands, &ui_audio.click);
    dispatch_menu_action(*action, &mut state_commands);
}

fn handle_menu_mouse_hover(
    mut commands: Commands,
    mut focus: ResMut<MenuFocus>,
    ui_audio: Res<UiAudioAssets>,
    hovered_buttons: Query<(&Hovered, &MenuItemIndex), Changed<Hovered>>,
) {
    for (hovered, item_idx) in &hovered_buttons {
        if hovered.get() && focus.index != item_idx.0 {
            focus.index = item_idx.0;
            play_ui_sfx(&mut commands, &ui_audio.hover);
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
            if b.position.x >= 1030.0 {
                "ZONE C: SMASH ARENA"
            } else if b.position.x >= 775.0 {
                "ZONE B: MEGA SHAFT"
            } else {
                "ZONE A: SONIC RUNWAY"
            }
        })
        .unwrap_or("ZONE A: SONIC RUNWAY");

    if let Ok(mut header) = header_labels.single_mut() {
        **header = format!(
            "THE FOUNDRY GYM  •  {}   |   KOs: {}   |   TURRETS: {}",
            zone_name, score.ko_count, score.turrets_destroyed
        );
    }

    let mut latest_ko: Option<BlastZoneKoEvent> = None;
    for ev in ko_events.read() {
        latest_ko = Some(*ev);
    }

    if let Ok((mut banner, mut text)) = banner_labels.single_mut() {
        if let Some(ko) = latest_ko {
            banner.timer = 2.0;
            **text = format!(
                "SMASH KO!  {} ({:.0}%)",
                ko.archetype.display_name(),
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

fn update_input_readout_hud(
    player_query: Query<(&ActionState<PlayerAction>, &KinematicBody, &BusterState), With<Player>>,
    hero_anim: Query<&SpriteSheetAnimator, With<PlayerSpriteVisual>>,
    mut labels: Query<&mut Text, With<InputReadoutText>>,
) {
    let Ok((actions, body, buster)) = player_query.single() else {
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
        active.push("UP-B");
    }
    if actions.pressed(&PlayerAction::Buster) {
        active.push("BUSTER");
    }
    if actions.pressed(&PlayerAction::SpinDash) {
        active.push("BOOST");
    }

    let action_summary = if active.is_empty() {
        "READY".to_string()
    } else {
        active.join("+")
    };

    let state_label = match body.state {
        MovementState::Grounded => "GROUND",
        MovementState::Airborne => "AIR",
        MovementState::WallSliding { .. } => "WALL-SLIDE",
        MovementState::LoopLocked { .. } => "LOOP 360°",
    };

    let charge_tag = if buster.is_fully_charged() {
        "  |  BUSTER: CHARGED!".to_string()
    } else if buster.charge_secs >= 0.15 {
        format!("  |  CHG: {:3.0}%", buster.charge_ratio() * 100.0)
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
