//! Main menu, Worlds (list + create, with a mod picker and branching), Settings
//! (including where new worlds keep their mods), and Mods (the installed list)
//! screens. Each screen is a Bevy UI tree spawned `OnEnter` its `AppState`
//! and despawned `OnExit`; buttons carry a [`MenuButton`] action component
//! and one handler system matches on it.

use bevy::input::keyboard::KeyboardInput;
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, PrimaryWindow};

use crate::config::WorldSettings;
use crate::mods::{create_world_with_mods, resolve_world_mods, scan_mods, set_world_mods, InstalledMods, MenuNotice, ModSetup};
use crate::save::{GameMode, GraphicsSettings, ModStorage, SaveStore, WorldMeta};
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
    /// Tick or untick an installed mod for the world being created.
    ToggleMod(String),
    SetModStorage(ModStorage),
    /// Share this other world's mods instead of choosing any (`None`: don't).
    SetBranch(Option<String>),
    /// From the worlds list: start a new world that branches this one.
    BranchFrom(String),
    /// Settings: where newly created worlds keep their mods.
    ToggleDefaultModStorage,
    /// From the worlds list: change which mods an existing world uses.
    EditMods(String),
    ApplyEditMods,
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
    /// Changing the mods of an existing world (`CreateWorldForm::editing`).
    EditMods,
}

#[derive(Resource, Default)]
struct CreateWorldForm {
    name: String,
    seed_text: String,
    focus: TextField,
    mode: GameMode,
    /// Installed mods ticked for this world.
    chosen_mods: Vec<String>,
    mod_storage: ModStorage,
    /// Share another world's mods instead (its slug).
    branch_of: Option<String>,
    /// The world whose mods are being edited (`WorldsScreenMode::EditMods`).
    editing: Option<String>,
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
struct ModStorageLabel;
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

fn setup_worlds(
    mut commands: Commands,
    mut mode: ResMut<WorldsScreenMode>,
    mut form: ResMut<CreateWorldForm>,
    store: Res<SaveStore>,
) {
    *mode = WorldsScreenMode::List;
    *form = CreateWorldForm { mod_storage: store.load_graphics_settings().mod_storage, ..CreateWorldForm::default() };
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

/// The mods to offer when changing a world's mods: (folder, label, ticked). Every
/// installed mod; for an in-world world also the ones it holds (on or switched off);
/// and any mod the world names that can't be found, shown as missing so it can be
/// unticked (which is how a world that won't open for want of a mod is fixed).
fn edit_candidates(
    store: &SaveStore,
    slug: &str,
    meta: &WorldMeta,
    installed: &std::path::Path,
) -> Vec<(String, String, bool)> {
    let mut found: Vec<crate::mods::ModInfo> = scan_mods(installed);
    if meta.mod_storage == ModStorage::World {
        for dir in [store.world_mods_dir(slug), crate::mods::disabled_mods_dir(store, slug)] {
            for m in scan_mods(&dir) {
                if !found.iter().any(|f| f.folder == m.folder) {
                    found.push(m);
                }
            }
        }
    }
    found.sort_by(|a, b| a.folder.cmp(&b.folder));
    let mut out: Vec<(String, String, bool)> = found
        .iter()
        .map(|m| (m.folder.clone(), format!("{} {}", m.name, m.version).trim().to_string(), meta.mods.contains(&m.folder)))
        .collect();
    for folder in &meta.mods {
        if !found.iter().any(|m| &m.folder == folder) {
            out.push((folder.clone(), format!("{folder} (missing)"), true));
        }
    }
    out
}

/// What a world's mod setup looks like in the list, e.g. "2 mods in this world".
fn mods_summary(meta: &WorldMeta) -> Option<String> {
    if let Some(host) = &meta.branch_of {
        return Some(format!("mods shared from {host}"));
    }
    if meta.mods.is_empty() {
        return None;
    }
    let n = meta.mods.len();
    let plural = if n == 1 { "mod" } else { "mods" };
    Some(match meta.mod_storage {
        ModStorage::Instance => format!("{n} {plural} (from the game folder)"),
        ModStorage::World => format!("{n} {plural} (kept in the world)"),
    })
}

/// A small two-state button whose border shows whether it's selected. Used for
/// the mod tick-boxes and the storage choice.
fn spawn_choice_button(parent: &mut ChildSpawnerCommands, label: &str, action: MenuButton, selected: bool, width: f32) {
    parent
        .spawn((
            Button,
            action,
            Node {
                width: Val::Px(width),
                height: Val::Px(32.0),
                border: UiRect::all(Val::Px(2.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BorderColor(if selected { Color::srgba(0.95, 0.85, 0.3, 0.95) } else { Color::srgba(1.0, 1.0, 1.0, 0.25) }),
            BackgroundColor(BUTTON_IDLE),
        ))
        .with_children(|p| {
            p.spawn((Text::new(label), TextFont { font_size: 14.0, ..default() }, TextColor(Color::WHITE)));
        });
}

fn dim_text(parent: &mut ChildSpawnerCommands, text: impl Into<String>, size: f32) {
    parent.spawn((Text::new(text.into()), TextFont { font_size: size, ..default() }, TextColor(TEXT_DIM)));
}

#[allow(clippy::too_many_arguments)]
fn rebuild_worlds_content(
    mut commands: Commands,
    mode: Res<WorldsScreenMode>,
    store: Res<SaveStore>,
    installed: Res<InstalledMods>,
    form: Res<CreateWorldForm>,
    mut notice: ResMut<MenuNotice>,
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
            // Why a world wouldn't open last time (a mod went missing), once.
            let message = notice.0.take();
            commands.entity(content).with_children(|p| {
                if let Some(message) = message {
                    p.spawn((
                        Text::new(message),
                        TextFont { font_size: 14.0, ..default() },
                        TextColor(Color::srgb(0.95, 0.6, 0.4)),
                        Node { width: Val::Px(400.0), ..default() },
                    ));
                }
                if worlds.is_empty() {
                    p.spawn((
                        Text::new("No worlds yet - create one below."),
                        TextFont { font_size: 15.0, ..default() },
                        TextColor(TEXT_DIM),
                    ));
                }
                for (slug, meta) in &worlds {
                    // A world that can't be opened is shown, but not as a
                    // button, and says why: a newer build wrote it (entering
                    // would rewrite it in this older format), or a mod it
                    // needs is missing (entering would drop that mod's blocks
                    // from the next save). Leaving it out of the list would
                    // be worse - it would just look like the world had vanished.
                    let blocked: Option<String> = if matches!(
                        crate::save::compatibility(meta.format),
                        crate::save::SaveCompatibility::TooNew { .. }
                    ) {
                        Some("made by a newer version - update Craftmjne to open it".to_string())
                    } else {
                        resolve_world_mods(&store, slug, meta, &installed.0).err().map(|e| e.message())
                    };
                    let can_branch = meta.mod_storage == ModStorage::World && meta.branch_of.is_none() && blocked.is_none();
                    // A world with a missing mod can still have its mods changed - that is
                    // how it's fixed. One from a newer version can't be touched at all.
                    let too_new = matches!(crate::save::compatibility(meta.format), crate::save::SaveCompatibility::TooNew { .. });
                    let can_edit = meta.branch_of.is_none() && !too_new;
                    let side = (can_branch as u32 + can_edit as u32) as f32 * 70.0;
                    p.spawn(Node { column_gap: Val::Px(6.0), ..default() }).with_children(|line| {
                        let row_node = Node {
                            width: Val::Px(360.0 - side),
                            padding: UiRect::axes(Val::Px(14.0), Val::Px(10.0)),
                            flex_direction: FlexDirection::Column,
                            ..default()
                        };
                        let mut row = if blocked.is_some() {
                            line.spawn((row_node, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.25))))
                        } else {
                            line.spawn((Button, MenuButton::LoadWorld(slug.clone()), row_node, BackgroundColor(BUTTON_IDLE)))
                        };
                        row.with_children(|row| {
                            row.spawn((
                                Text::new(meta.name.clone()),
                                TextFont { font_size: 17.0, ..default() },
                                TextColor(if blocked.is_some() { TEXT_DIM } else { Color::WHITE }),
                            ));
                            let subtitle = blocked.clone().unwrap_or_else(|| {
                                let mut s = format!(
                                    "seed {}  -  {}  -  {}",
                                    meta.seed,
                                    mode_label(meta.mode),
                                    relative_time(meta.last_played_at)
                                );
                                if let Some(m) = mods_summary(meta) {
                                    s.push_str("  -  ");
                                    s.push_str(&m);
                                }
                                s
                            });
                            row.spawn((
                                Text::new(subtitle),
                                TextFont { font_size: 12.0, ..default() },
                                TextColor(TEXT_DIM),
                                Node { width: Val::Px(260.0), ..default() },
                            ));
                        });
                        let small = |line: &mut ChildSpawnerCommands, label: &str, action: MenuButton| {
                            line.spawn((
                                Button,
                                action,
                                Node {
                                    width: Val::Px(64.0),
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    ..default()
                                },
                                BackgroundColor(BUTTON_IDLE),
                            ))
                            .with_children(|b| {
                                b.spawn((Text::new(label.to_string()), TextFont { font_size: 13.0, ..default() }, TextColor(Color::WHITE)));
                            });
                        };
                        if can_edit {
                            small(line, "Mods", MenuButton::EditMods(slug.clone()));
                        }
                        if can_branch {
                            small(line, "Branch", MenuButton::BranchFrom(slug.clone()));
                        }
                    });
                }
                spawn_button(p, "Create World", MenuButton::ShowCreateForm);
                spawn_button(p, "Back", MenuButton::BackToMainMenu);
            });
        }
        WorldsScreenMode::EditMods => {
            let editing = form.editing.clone().unwrap_or_default();
            let meta = store.load_meta(&editing).ok();
            let candidates = meta.as_ref().map(|m| edit_candidates(&store, &editing, m, &installed.0)).unwrap_or_default();
            let storage = meta.as_ref().map_or(ModStorage::Instance, |m| m.mod_storage);
            commands.entity(content).with_children(|p| {
                p.spawn((
                    Text::new(format!("Mods for {}", meta.as_ref().map_or("this world", |m| m.name.as_str()))),
                    TextFont { font_size: 18.0, ..default() },
                    TextColor(Color::WHITE),
                ));
                dim_text(
                    p,
                    match storage {
                        ModStorage::Instance => "These run from the game's mods folder.",
                        ModStorage::World => "Turning one off puts it aside in this world; nothing is deleted.",
                    },
                    12.0,
                );
                dim_text(p, "Anything made with a mod you turn off stays in the world's save and comes back when you turn it on again.", 12.0);
                if candidates.is_empty() {
                    dim_text(p, format!("No mods installed. Put mod folders in {}", installed.0.display()), 12.0);
                }
                p.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), margin: UiRect::bottom(Val::Px(8.0)), ..default() })
                    .with_children(|list| {
                        for (folder, label, _) in &candidates {
                            let on = form.chosen_mods.contains(folder);
                            let text = format!("[{}] {}", if on { "x" } else { " " }, label);
                            spawn_choice_button(list, &text, MenuButton::ToggleMod(folder.clone()), on, 320.0);
                        }
                    });
                spawn_button(p, "Apply", MenuButton::ApplyEditMods);
                spawn_button(p, "Cancel", MenuButton::CancelCreate);
            });
        }
        WorldsScreenMode::Create => {
            let mods = scan_mods(&installed.0);
            let hosts: Vec<(String, WorldMeta)> = store
                .list_worlds()
                .into_iter()
                .filter(|(_, m)| m.mod_storage == ModStorage::World && m.branch_of.is_none())
                .collect();
            commands.entity(content).with_children(|p| {
                spawn_text_field(p, "World name", TextField::Name);
                spawn_text_field(p, "Seed (blank = random)", TextField::Seed);
                dim_text(p, "Game mode", 13.0);
                p.spawn(Node {
                    column_gap: Val::Px(8.0),
                    margin: UiRect::bottom(Val::Px(8.0)),
                    ..default()
                })
                .with_children(|row| {
                    spawn_mode_button(row, "Survival", GameMode::Survival);
                    spawn_mode_button(row, "Creative", GameMode::Creative);
                });

                // Mods: share another world's, or pick from the installed ones.
                if !hosts.is_empty() {
                    dim_text(p, "Share the mods of another world (a branch)", 13.0);
                    p.spawn(Node { column_gap: Val::Px(6.0), flex_wrap: FlexWrap::Wrap, margin: UiRect::bottom(Val::Px(8.0)), ..default() })
                        .with_children(|row| {
                            spawn_choice_button(row, "No", MenuButton::SetBranch(None), form.branch_of.is_none(), 50.0);
                            for (slug, meta) in &hosts {
                                let name: String = meta.name.chars().take(14).collect();
                                spawn_choice_button(row, &name, MenuButton::SetBranch(Some(slug.clone())), form.branch_of.as_deref() == Some(slug), 120.0);
                            }
                        });
                }
                if form.branch_of.is_some() {
                    dim_text(p, "This world will use that world's mods and keep none of its own.", 12.0);
                } else if mods.is_empty() {
                    dim_text(p, format!("No mods installed. Put mod folders in {}", installed.0.display()), 12.0);
                } else {
                    dim_text(p, "Where this world keeps its mods", 13.0);
                    p.spawn(Node { column_gap: Val::Px(8.0), margin: UiRect::bottom(Val::Px(6.0)), ..default() })
                        .with_children(|row| {
                            spawn_choice_button(row, "Game folder", MenuButton::SetModStorage(ModStorage::Instance), form.mod_storage == ModStorage::Instance, 134.0);
                            spawn_choice_button(row, "This world", MenuButton::SetModStorage(ModStorage::World), form.mod_storage == ModStorage::World, 134.0);
                        });
                    dim_text(p, "Mods to use", 13.0);
                    p.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), margin: UiRect::bottom(Val::Px(8.0)), ..default() })
                        .with_children(|list| {
                            for m in &mods {
                                let on = form.chosen_mods.contains(&m.folder);
                                let label = format!("[{}] {} {}", if on { "x" } else { " " }, m.name, m.version);
                                spawn_choice_button(list, label.trim_end(), MenuButton::ToggleMod(m.folder.clone()), on, 280.0);
                            }
                        });
                }
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

fn mod_storage_label(storage: ModStorage) -> String {
    match storage {
        ModStorage::Instance => "New worlds keep mods: in the game folder".to_string(),
        ModStorage::World => "New worlds keep mods: in the world".to_string(),
    }
}

fn setup_settings(mut commands: Commands, settings: Res<WorldSettings>, store: Res<SaveStore>) {
    let mod_storage = store.load_graphics_settings().mod_storage;
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
            // Where a newly created world keeps its mods.
            p.spawn((
                Button,
                MenuButton::ToggleDefaultModStorage,
                Node {
                    width: Val::Px(360.0),
                    height: Val::Px(40.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                BackgroundColor(BUTTON_IDLE),
            ))
            .with_children(|b| {
                b.spawn((
                    ModStorageLabel,
                    Text::new(mod_storage_label(mod_storage)),
                    TextFont { font_size: 15.0, ..default() },
                    TextColor(Color::WHITE),
                ));
            });
            p.spawn((
                Text::new("Game folder: one copy of each mod, shared by every world (remove it and worlds using it won't open).\nIn the world: each world gets its own copy, so it can have mods the game doesn't."),
                TextFont { font_size: 12.0, ..default() },
                TextColor(TEXT_DIM),
                Node { width: Val::Px(360.0), ..default() },
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

fn setup_mods(mut commands: Commands, installed: Res<InstalledMods>) {
    let (mods, broken) = crate::mods::scan_all(&installed.0);
    commands.spawn((ModsRoot, full_screen_root())).with_children(|root| {
        root.spawn(panel()).with_children(|p| {
            p.spawn(title("Mods"));
            if mods.is_empty() && broken.is_empty() {
                dim_text(p, format!("No mods installed. Put mod folders in:\n{}", installed.0.display()), 14.0);
            } else {
                for m in &mods {
                    p.spawn(Node { flex_direction: FlexDirection::Column, width: Val::Px(380.0), ..default() }).with_children(|row| {
                        row.spawn((
                            Text::new(format!("{} {}  ({})", m.name, m.version, m.id).replace("  (", " (")),
                            TextFont { font_size: 16.0, ..default() },
                            TextColor(Color::WHITE),
                        ));
                        if !m.description.is_empty() {
                            dim_text(row, m.description.clone(), 12.0);
                        }
                    });
                }
            }
            for (_, why) in &broken {
                p.spawn((
                    Text::new(format!("Won't work: {why}")),
                    TextFont { font_size: 12.0, ..default() },
                    TextColor(Color::srgb(0.95, 0.6, 0.4)),
                    Node { width: Val::Px(380.0), ..default() },
                ));
            }
            dim_text(p, "Pick which mods a world uses when you create it. Mods sit on top of the game and never change its files.", 12.0);
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
    installed: Res<InstalledMods>,
    mut notice: ResMut<MenuNotice>,
    mut storage_labels: Query<&mut Text, With<ModStorageLabel>>,
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
            MenuButton::ToggleMod(id) => {
                match form.chosen_mods.iter().position(|m| *m == id) {
                    Some(i) => {
                        form.chosen_mods.remove(i);
                    }
                    None => form.chosen_mods.push(id),
                }
                mode.set_changed(); // redraw the ticks
            }
            MenuButton::SetModStorage(storage) => {
                form.mod_storage = storage;
                mode.set_changed();
            }
            MenuButton::SetBranch(host) => {
                form.branch_of = host;
                mode.set_changed();
            }
            MenuButton::BranchFrom(host) => {
                form.branch_of = Some(host);
                *mode = WorldsScreenMode::Create;
            }
            MenuButton::EditMods(slug) => {
                if let Ok(meta) = store.load_meta(&slug) {
                    form.chosen_mods = meta.mods.clone();
                    form.editing = Some(slug);
                    *mode = WorldsScreenMode::EditMods;
                }
            }
            MenuButton::ApplyEditMods => {
                if let Some(slug) = form.editing.take() {
                    notice.0 = Some(match set_world_mods(&store, &slug, &installed.0, &form.chosen_mods) {
                        Ok(_) => "Mods updated.".to_string(),
                        Err(err) => format!("Couldn't change the mods: {err}"),
                    });
                }
                *mode = WorldsScreenMode::List;
            }
            MenuButton::ToggleDefaultModStorage => {
                let mut saved = store.load_graphics_settings();
                saved.mod_storage = match saved.mod_storage {
                    ModStorage::Instance => ModStorage::World,
                    ModStorage::World => ModStorage::Instance,
                };
                let _ = store.save_graphics_settings(&saved);
                for mut text in &mut storage_labels {
                    text.0 = mod_storage_label(saved.mod_storage);
                }
            }
            MenuButton::SubmitCreate => {
                let seed = form.seed_text.trim().parse().unwrap_or_else(|_| random_seed());
                let setup = match &form.branch_of {
                    Some(host) => ModSetup::Branch(host.clone()),
                    None if form.mod_storage == ModStorage::World => ModSetup::World(form.chosen_mods.clone()),
                    None => ModSetup::Instance(form.chosen_mods.clone()),
                };
                match create_world_with_mods(&store, &form.name, seed, form.mode, &setup, &installed.0) {
                    Ok((slug, meta)) => {
                        store.touch_last_played(&slug);
                        commands.insert_resource(ActiveWorld { slug, meta });
                        next_state.set(AppState::InGame);
                    }
                    Err(err) => {
                        notice.0 = Some(format!("Couldn't create the world: {err}"));
                        *mode = WorldsScreenMode::List;
                    }
                }
            }
            MenuButton::LoadWorld(slug) => {
                // `open_world` is what refuses a world saved by a newer
                // build and migrates an older one forward - never bypass it
                // with a bare `load_meta`. The list already renders a
                // too-new world as unclickable, so reaching the `Err` arm
                // here means the save changed underneath us.
                if let Ok(meta) = store.open_world(&slug) {
                    // A world whose mods are missing must not open (loading it
                    // without them would drop their blocks from its next save);
                    // say why and stay here. The list already shows such a world
                    // as unclickable, so this is for the folder changing since.
                    match resolve_world_mods(&store, &slug, &meta, &installed.0) {
                        Ok(_) => {
                            commands.insert_resource(ActiveWorld { slug, meta });
                            next_state.set(AppState::InGame);
                        }
                        Err(err) => {
                            notice.0 = Some(err.message());
                            mode.set_changed();
                        }
                    }
                }
            }
            MenuButton::RenderDistanceDelta(d) => {
                settings.render_distance = (settings.render_distance + d).clamp(2, 16);
                let _ = store.save_graphics_settings(&GraphicsSettings { render_distance: settings.render_distance, ..store.load_graphics_settings() });
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

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(storage: ModStorage, mods: &[&str], branch: Option<&str>) -> WorldMeta {
        WorldMeta {
            name: "W".into(),
            seed: 1,
            format: 0,
            mode: GameMode::Survival,
            cheats: false,
            mods: mods.iter().map(|s| s.to_string()).collect(),
            mod_storage: storage,
            branch_of: branch.map(str::to_string),
            mod_ids: Default::default(),
            created_at: 0,
            last_played_at: 0,
        }
    }

    #[test]
    fn the_worlds_list_says_where_a_worlds_mods_come_from() {
        assert_eq!(mods_summary(&meta(ModStorage::Instance, &[], None)), None);
        assert_eq!(mods_summary(&meta(ModStorage::Instance, &["a"], None)).unwrap(), "1 mod (from the game folder)");
        assert_eq!(mods_summary(&meta(ModStorage::World, &["a", "b"], None)).unwrap(), "2 mods (kept in the world)");
        assert_eq!(mods_summary(&meta(ModStorage::World, &["a"], Some("host"))).unwrap(), "mods shared from host");
    }

    #[test]
    fn the_storage_setting_label_names_the_current_choice() {
        assert!(mod_storage_label(ModStorage::Instance).contains("game folder"));
        assert!(mod_storage_label(ModStorage::World).contains("in the world"));
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "craftmjne-menu-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_mod(dir: &std::path::Path, folder: &str) {
        std::fs::create_dir_all(dir.join(folder)).unwrap();
        std::fs::write(dir.join(folder).join(crate::mods::MANIFEST), format!(r#"{{"id": "{folder}"}}"#)).unwrap();
    }

    #[test]
    fn the_mod_list_for_editing_a_world_shows_installed_ones_and_flags_a_missing_one() {
        let root = scratch("edit");
        let installed = root.join("installed");
        make_mod(&installed, "a");
        make_mod(&installed, "b");
        let store = SaveStore::at(root.join("data"));
        let mut meta = meta(ModStorage::Instance, &["a", "ghost"], None);
        meta.mods = vec!["a".into(), "ghost".into()];
        let list = edit_candidates(&store, "w", &meta, &installed);
        assert_eq!(
            list,
            vec![
                ("a".to_string(), "a".to_string(), true),
                ("b".to_string(), "b".to_string(), false),
                ("ghost".to_string(), "ghost (missing)".to_string(), true),
            ],
            "a missing mod is listed, ticked, so it can be unticked to fix the world"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_in_world_worlds_list_also_offers_the_mods_it_holds_on_or_off() {
        let root = scratch("edit-world");
        let installed = root.join("installed");
        make_mod(&installed, "a");
        let store = SaveStore::at(root.join("data"));
        make_mod(&store.world_mods_dir("w"), "kept");
        make_mod(&crate::mods::disabled_mods_dir(&store, "w"), "aside");
        let meta = meta(ModStorage::World, &["kept"], None);
        let folders: Vec<(String, bool)> =
            edit_candidates(&store, "w", &meta, &installed).into_iter().map(|(f, _, on)| (f, on)).collect();
        assert_eq!(folders, vec![("a".to_string(), false), ("aside".to_string(), false), ("kept".to_string(), true)]);
        let _ = std::fs::remove_dir_all(root);
    }
}
