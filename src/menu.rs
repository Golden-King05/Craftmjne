//! Main menu, Worlds (list + create), Settings, and Mods (placeholder)
//! screens. Each screen is a Bevy UI tree spawned `OnEnter` its `AppState`
//! and despawned `OnExit`; buttons carry a [`MenuButton`] action component
//! and one handler system matches on it.

use bevy::input::keyboard::KeyboardInput;
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, PrimaryWindow};

use crate::config::WorldSettings;
use crate::save::{GameMode, GraphicsSettings, SaveStore};
use crate::state::{ActiveWorld, AppState, PauseState};

const PANEL_BG: Color = Color::srgba(0.08, 0.09, 0.12, 0.82);
const BUTTON_IDLE: Color = Color::srgba(1.0, 1.0, 1.0, 0.10);
const BUTTON_HOVER: Color = Color::srgba(1.0, 1.0, 1.0, 0.22);
const BUTTON_PRESS: Color = Color::srgba(1.0, 1.0, 1.0, 0.35);
const TEXT_DIM: Color = Color::srgba(1.0, 1.0, 1.0, 0.6);

#[derive(Component, Clone, PartialEq)]
enum MenuButton {
    GoWorlds,
    GoSettings,
    GoMods,
    Quit,
    BackToMainMenu,
    ShowCreateForm,
    CancelCreate,
    SubmitCreate,
    LoadWorld(String),
    FocusField(TextField),
    RenderDistanceDelta(i32),
    SetGameMode(GameMode),
    Resume,
    QuitToMenu,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum TextField {
    #[default]
    Name,
    Seed,
}

#[derive(Resource, Default, Clone, Copy, PartialEq, Eq)]
enum WorldsScreenMode {
    #[default]
    List,
    Create,
}

#[derive(Resource, Default)]
struct CreateWorldForm {
    name: String,
    seed_text: String,
    focus: TextField,
    mode: GameMode,
}

#[derive(Component)]
struct TextInputDisplay(TextField);

#[derive(Component)]
struct GameModeOption(GameMode);

#[derive(Component)]
struct MainMenuRoot;
#[derive(Component)]
struct WorldsRoot;
#[derive(Component)]
struct WorldsContent;
#[derive(Component)]
struct SettingsRoot;
#[derive(Component)]
struct ModsRoot;
#[derive(Component)]
struct RenderDistanceLabel;
#[derive(Component)]
struct PauseRoot;

fn despawn_all<T: Component>(mut commands: Commands, q: Query<Entity, With<T>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

fn full_screen_root() -> impl Bundle {
    Node {
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        row_gap: Val::Px(14.0),
        ..default()
    }
}

fn panel() -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            padding: UiRect::axes(Val::Px(36.0), Val::Px(28.0)),
            row_gap: Val::Px(10.0),
            ..default()
        },
        BackgroundColor(PANEL_BG),
    )
}

fn title(text: &str) -> impl Bundle {
    (
        Text::new(text),
        TextFont { font_size: 32.0, ..default() },
        TextColor(Color::WHITE),
        Node { margin: UiRect::bottom(Val::Px(12.0)), ..default() },
    )
}

fn spawn_button(parent: &mut ChildSpawnerCommands, label: &str, action: MenuButton) {
    parent
        .spawn((
            Button,
            action,
            Node {
                width: Val::Px(240.0),
                height: Val::Px(44.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(BUTTON_IDLE),
        ))
        .with_children(|p| {
            p.spawn((Text::new(label), TextFont { font_size: 18.0, ..default() }, TextColor(Color::WHITE)));
        });
}

/// Like `spawn_button`, but keeps a border so a separate "selected" state
/// (see `sync_mode_buttons`) can be shown without fighting `button_visuals`,
/// which only ever touches `BackgroundColor`.
fn spawn_mode_button(parent: &mut ChildSpawnerCommands, label: &str, mode: GameMode) {
    parent
        .spawn((
            Button,
            MenuButton::SetGameMode(mode),
            GameModeOption(mode),
            Node {
                width: Val::Px(134.0),
                height: Val::Px(36.0),
                border: UiRect::all(Val::Px(2.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BorderColor(Color::srgba(1.0, 1.0, 1.0, 0.35)),
            BackgroundColor(BUTTON_IDLE),
        ))
        .with_children(|p| {
            p.spawn((Text::new(label), TextFont { font_size: 15.0, ..default() }, TextColor(Color::WHITE)));
        });
}

fn button_visuals(
    mut buttons: Query<(&Interaction, &mut BackgroundColor), (Changed<Interaction>, With<MenuButton>)>,
) {
    for (interaction, mut bg) in &mut buttons {
        *bg = BackgroundColor(match interaction {
            Interaction::Pressed => BUTTON_PRESS,
            Interaction::Hovered => BUTTON_HOVER,
            Interaction::None => BUTTON_IDLE,
        });
    }
}

// ---------------------------------------------------------------------------
// Main menu
// ---------------------------------------------------------------------------

fn setup_main_menu(mut commands: Commands) {
    commands.spawn((MainMenuRoot, full_screen_root())).with_children(|root| {
        root.spawn(panel()).with_children(|p| {
            p.spawn(title("CRAFTMJNE"));
            spawn_button(p, "Worlds", MenuButton::GoWorlds);
            spawn_button(p, "Settings", MenuButton::GoSettings);
            spawn_button(p, "Mods", MenuButton::GoMods);
            spawn_button(p, "Quit Game", MenuButton::Quit);
        });
    });
}

// ---------------------------------------------------------------------------
// Worlds screen (list + create form)
// ---------------------------------------------------------------------------

fn setup_worlds(mut commands: Commands, mut mode: ResMut<WorldsScreenMode>, mut form: ResMut<CreateWorldForm>) {
    *mode = WorldsScreenMode::List;
    *form = CreateWorldForm::default();
    commands.spawn((WorldsRoot, full_screen_root())).with_children(|root| {
        root.spawn(panel()).with_children(|p| {
            p.spawn(title("Select World"));
            p.spawn((WorldsContent, Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), ..default() }));
        });
    });
}

fn mode_label(mode: GameMode) -> &'static str {
    match mode {
        GameMode::Survival => "Survival",
        GameMode::Creative => "Creative",
    }
}

fn rebuild_worlds_content(
    mut commands: Commands,
    mode: Res<WorldsScreenMode>,
    store: Res<SaveStore>,
    content: Query<Entity, With<WorldsContent>>,
) {
    if !mode.is_changed() {
        return;
    }
    let Ok(content) = content.single() else { return };
    commands.entity(content).despawn_related::<Children>();

    match *mode {
        WorldsScreenMode::List => {
            let worlds = store.list_worlds();
            commands.entity(content).with_children(|p| {
                if worlds.is_empty() {
                    p.spawn((
                        Text::new("No worlds yet - create one below."),
                        TextFont { font_size: 15.0, ..default() },
                        TextColor(TEXT_DIM),
                    ));
                }
                for (slug, meta) in &worlds {
                    p.spawn((
                        Button,
                        MenuButton::LoadWorld(slug.clone()),
                        Node {
                            width: Val::Px(360.0),
                            padding: UiRect::axes(Val::Px(14.0), Val::Px(10.0)),
                            flex_direction: FlexDirection::Column,
                            ..default()
                        },
                        BackgroundColor(BUTTON_IDLE),
                    ))
                    .with_children(|row| {
                        row.spawn((Text::new(meta.name.clone()), TextFont { font_size: 17.0, ..default() }, TextColor(Color::WHITE)));
                        row.spawn((
                            Text::new(format!(
                                "seed {}  -  {}  -  {}",
                                meta.seed,
                                mode_label(meta.mode),
                                relative_time(meta.last_played_at)
                            )),
                            TextFont { font_size: 12.0, ..default() },
                            TextColor(TEXT_DIM),
                        ));
                    });
                }
                spawn_button(p, "Create World", MenuButton::ShowCreateForm);
                spawn_button(p, "Back", MenuButton::BackToMainMenu);
            });
        }
        WorldsScreenMode::Create => {
            commands.entity(content).with_children(|p| {
                spawn_text_field(p, "World name", TextField::Name);
                spawn_text_field(p, "Seed (blank = random)", TextField::Seed);
                p.spawn((
                    Text::new("Game mode"),
                    TextFont { font_size: 13.0, ..default() },
                    TextColor(TEXT_DIM),
                ));
                p.spawn(Node {
                    column_gap: Val::Px(8.0),
                    margin: UiRect::bottom(Val::Px(8.0)),
                    ..default()
                })
                .with_children(|row| {
                    spawn_mode_button(row, "Survival", GameMode::Survival);
                    spawn_mode_button(row, "Creative", GameMode::Creative);
                });
                spawn_button(p, "Create", MenuButton::SubmitCreate);
                spawn_button(p, "Cancel", MenuButton::CancelCreate);
            });
        }
    }
}

fn spawn_text_field(parent: &mut ChildSpawnerCommands, label: &str, field: TextField) {
    parent.spawn((Text::new(label), TextFont { font_size: 13.0, ..default() }, TextColor(TEXT_DIM)));
    parent
        .spawn((
            Button,
            MenuButton::FocusField(field),
            Node {
                width: Val::Px(280.0),
                height: Val::Px(36.0),
                align_items: AlignItems::Center,
                padding: UiRect::left(Val::Px(10.0)),
                margin: UiRect::bottom(Val::Px(8.0)),
                ..default()
            },
            BackgroundColor(BUTTON_IDLE),
        ))
        .with_children(|p| {
            p.spawn((
                TextInputDisplay(field),
                Text::new(""),
                TextFont { font_size: 16.0, ..default() },
                TextColor(Color::WHITE),
            ));
        });
}

fn sync_text_inputs(form: Res<CreateWorldForm>, mut texts: Query<(&mut Text, &TextInputDisplay)>) {
    if !form.is_changed() {
        return;
    }
    for (mut text, disp) in &mut texts {
        let (buf, focused, placeholder) = match disp.0 {
            TextField::Name => (&form.name, form.focus == TextField::Name, "World name..."),
            TextField::Seed => (&form.seed_text, form.focus == TextField::Seed, "Random"),
        };
        text.0 = if buf.is_empty() && !focused {
            placeholder.to_string()
        } else if focused {
            format!("{buf}_")
        } else {
            buf.clone()
        };
    }
}

fn sync_mode_buttons(form: Res<CreateWorldForm>, mut options: Query<(&GameModeOption, &mut BorderColor)>) {
    if !form.is_changed() {
        return;
    }
    for (opt, mut border) in &mut options {
        *border = BorderColor(if opt.0 == form.mode {
            Color::WHITE
        } else {
            Color::srgba(1.0, 1.0, 1.0, 0.35)
        });
    }
}

fn handle_text_input(mut events: EventReader<KeyboardInput>, mode: Res<WorldsScreenMode>, mut form: ResMut<CreateWorldForm>) {
    if *mode != WorldsScreenMode::Create {
        events.clear();
        return;
    }
    for ev in events.read() {
        if ev.state != ButtonState::Pressed {
            continue;
        }
        match ev.key_code {
            KeyCode::Tab => {
                form.focus = match form.focus {
                    TextField::Name => TextField::Seed,
                    TextField::Seed => TextField::Name,
                };
                continue;
            }
            KeyCode::Backspace => {
                let focus = form.focus;
                let buf = match focus {
                    TextField::Name => &mut form.name,
                    TextField::Seed => &mut form.seed_text,
                };
                buf.pop();
                continue;
            }
            _ => {}
        }
        if let Some(text) = ev.text.clone() {
            let focus = form.focus;
            let digits_only = focus == TextField::Seed;
            let buf = match focus {
                TextField::Name => &mut form.name,
                TextField::Seed => &mut form.seed_text,
            };
            for ch in text.chars() {
                if ch.is_control() || (digits_only && !ch.is_ascii_digit()) {
                    continue;
                }
                if buf.len() < 32 {
                    buf.push(ch);
                }
            }
        }
    }
}

fn relative_time(unix_secs: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(unix_secs);
    let elapsed = now.saturating_sub(unix_secs);
    if elapsed < 60 {
        "just now".into()
    } else if elapsed < 3600 {
        format!("{} min ago", elapsed / 60)
    } else if elapsed < 86400 {
        format!("{} hr ago", elapsed / 3600)
    } else {
        format!("{} day(s) ago", elapsed / 86400)
    }
}

fn random_seed() -> u32 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u32)
        .unwrap_or(0);
    nanos ^ 0x9e37_79b9
}

// ---------------------------------------------------------------------------
// Settings screen
// ---------------------------------------------------------------------------

fn setup_settings(mut commands: Commands, settings: Res<WorldSettings>) {
    commands.spawn((SettingsRoot, full_screen_root())).with_children(|root| {
        root.spawn(panel()).with_children(|p| {
            p.spawn(title("Settings"));
            p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(14.0), ..default() })
                .with_children(|row| {
                    spawn_small_button(row, "-", MenuButton::RenderDistanceDelta(-1));
                    row.spawn((
                        RenderDistanceLabel,
                        Text::new(format!("Render distance: {}", settings.render_distance)),
                        TextFont { font_size: 16.0, ..default() },
                        TextColor(Color::WHITE),
                    ));
                    spawn_small_button(row, "+", MenuButton::RenderDistanceDelta(1));
                });
            p.spawn((
                Text::new("Takes effect after restart"),
                TextFont { font_size: 12.0, ..default() },
                TextColor(TEXT_DIM),
            ));
            spawn_button(p, "Back", MenuButton::BackToMainMenu);
        });
    });
}

fn spawn_small_button(parent: &mut ChildSpawnerCommands, label: &str, action: MenuButton) {
    parent
        .spawn((
            Button,
            action,
            Node {
                width: Val::Px(36.0),
                height: Val::Px(36.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(BUTTON_IDLE),
        ))
        .with_children(|p| {
            p.spawn((Text::new(label), TextFont { font_size: 20.0, ..default() }, TextColor(Color::WHITE)));
        });
}

fn sync_render_distance_label(settings: Res<WorldSettings>, mut labels: Query<&mut Text, With<RenderDistanceLabel>>) {
    if !settings.is_changed() {
        return;
    }
    for mut text in &mut labels {
        text.0 = format!("Render distance: {}", settings.render_distance);
    }
}

// ---------------------------------------------------------------------------
// Mods screen (placeholder)
// ---------------------------------------------------------------------------

fn setup_mods(mut commands: Commands) {
    commands.spawn((ModsRoot, full_screen_root())).with_children(|root| {
        root.spawn(panel()).with_children(|p| {
            p.spawn(title("Mods"));
            p.spawn((
                Text::new("Mod support is coming soon."),
                TextFont { font_size: 16.0, ..default() },
                TextColor(TEXT_DIM),
            ));
            spawn_button(p, "Back", MenuButton::BackToMainMenu);
        });
    });
}

// ---------------------------------------------------------------------------
// Pause screen (shown over the still-rendering world; see `PauseState`)
// ---------------------------------------------------------------------------

fn spawn_pause_screen(commands: &mut Commands) {
    commands.spawn((PauseRoot, full_screen_root())).with_children(|root| {
        root.spawn(panel()).with_children(|p| {
            p.spawn(title("Paused"));
            spawn_button(p, "Resume", MenuButton::Resume);
            spawn_button(p, "Quit to Menu", MenuButton::QuitToMenu);
            spawn_button(p, "Quit Game", MenuButton::Quit);
        });
    });
}

/// Spawns/despawns the pause overlay in step with `PauseState`, which is
/// toggled by `player::cursor_grab` on Escape (and by the Resume/Quit
/// buttons here, via `handle_menu_buttons`).
fn sync_pause_screen(mut commands: Commands, paused: Res<PauseState>, roots: Query<Entity, With<PauseRoot>>) {
    if !paused.is_changed() {
        return;
    }
    if paused.open {
        if roots.is_empty() {
            spawn_pause_screen(&mut commands);
        }
    } else {
        for e in &roots {
            commands.entity(e).despawn();
        }
    }
}

/// Safety net for leaving `InGame` (e.g. via "Quit to Menu") while paused,
/// so the overlay never lingers into the main menu.
fn despawn_pause(mut commands: Commands, roots: Query<Entity, With<PauseRoot>>) {
    for e in &roots {
        commands.entity(e).despawn();
    }
}

// ---------------------------------------------------------------------------
// Button action dispatch
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn handle_menu_buttons(
    mut commands: Commands,
    mut interactions: Query<(&Interaction, &MenuButton), Changed<Interaction>>,
    mut next_state: ResMut<NextState<AppState>>,
    mut mode: ResMut<WorldsScreenMode>,
    mut form: ResMut<CreateWorldForm>,
    mut settings: ResMut<WorldSettings>,
    mut paused: ResMut<PauseState>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    store: Res<SaveStore>,
    mut exit: EventWriter<AppExit>,
) {
    for (interaction, action) in &mut interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match action.clone() {
            MenuButton::GoWorlds => next_state.set(AppState::Worlds),
            MenuButton::GoSettings => next_state.set(AppState::Settings),
            MenuButton::GoMods => next_state.set(AppState::Mods),
            MenuButton::BackToMainMenu => next_state.set(AppState::MainMenu),
            MenuButton::Quit => {
                exit.write(AppExit::Success);
            }
            MenuButton::ShowCreateForm => *mode = WorldsScreenMode::Create,
            MenuButton::CancelCreate => *mode = WorldsScreenMode::List,
            MenuButton::FocusField(field) => form.focus = field,
            MenuButton::SetGameMode(game_mode) => form.mode = game_mode,
            MenuButton::SubmitCreate => {
                let seed = form.seed_text.trim().parse().unwrap_or_else(|_| random_seed());
                if let Ok((slug, meta)) = store.create_world(&form.name, seed, form.mode) {
                    store.touch_last_played(&slug);
                    commands.insert_resource(ActiveWorld { slug, meta });
                    next_state.set(AppState::InGame);
                }
            }
            MenuButton::LoadWorld(slug) => {
                if let Ok(meta) = store.load_meta(&slug) {
                    store.touch_last_played(&slug);
                    commands.insert_resource(ActiveWorld { slug, meta });
                    next_state.set(AppState::InGame);
                }
            }
            MenuButton::RenderDistanceDelta(d) => {
                settings.render_distance = (settings.render_distance + d).clamp(2, 16);
                let _ = store.save_graphics_settings(&GraphicsSettings { render_distance: settings.render_distance });
            }
            MenuButton::Resume => {
                paused.open = false;
                if let Ok(mut window) = windows.single_mut() {
                    window.cursor_options.grab_mode = CursorGrabMode::Locked;
                    window.cursor_options.visible = false;
                }
            }
            MenuButton::QuitToMenu => {
                paused.open = false;
                next_state.set(AppState::MainMenu);
            }
        }
    }
}

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldsScreenMode>()
            .init_resource::<CreateWorldForm>()
            .add_systems(OnEnter(AppState::MainMenu), setup_main_menu)
            .add_systems(OnExit(AppState::MainMenu), despawn_all::<MainMenuRoot>)
            .add_systems(OnEnter(AppState::Worlds), setup_worlds)
            .add_systems(OnExit(AppState::Worlds), despawn_all::<WorldsRoot>)
            .add_systems(OnEnter(AppState::Settings), setup_settings)
            .add_systems(OnExit(AppState::Settings), despawn_all::<SettingsRoot>)
            .add_systems(OnEnter(AppState::Mods), setup_mods)
            .add_systems(OnExit(AppState::Mods), despawn_all::<ModsRoot>)
            .add_systems(OnExit(AppState::InGame), despawn_pause)
            .add_systems(
                Update,
                (
                    button_visuals,
                    handle_menu_buttons,
                    rebuild_worlds_content.run_if(in_state(AppState::Worlds)),
                    handle_text_input.run_if(in_state(AppState::Worlds)),
                    sync_text_inputs.run_if(in_state(AppState::Worlds)),
                    sync_mode_buttons.run_if(in_state(AppState::Worlds)),
                    sync_render_distance_label.run_if(in_state(AppState::Settings)),
                    sync_pause_screen.run_if(in_state(AppState::InGame)),
                ),
            );
    }
}
