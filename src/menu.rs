//! Main menu, Worlds (list + create, with a mod picker and branching), and Mods
//! (the installed list) screens. (Settings has its own module, `settings`.) Each screen is a Bevy UI tree spawned `OnEnter` its `AppState`
//! and despawned `OnExit`; buttons carry a [`MenuButton`] action component
//! and one handler system matches on it.

use bevy::input::keyboard::KeyboardInput;
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, PrimaryWindow};

use crate::mods::{create_world_with_mods, resolve_world_mods, scan_mods, set_world_mods, InstalledMods, MenuNotice, ModSetup};
use crate::save::{GameMode, ModStorage, SaveStore, WorldMeta};
use crate::state::{ActiveWorld, AppState, PauseState};
use crate::ui_theme::{self as theme, ButtonOpts, ButtonStyle, ThemedButton};
use bevy::input::mouse::MouseWheel;

/// Where the world list and mod list scroll when there are more than fit.
const LIST_HEIGHT: f32 = 330.0;

#[derive(Component, Clone, PartialEq)]
pub(crate) enum MenuButton {
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
    SetGameMode(GameMode),
    /// Tick or untick an installed mod for the world being created.
    ToggleMod(String),
    SetModStorage(ModStorage),
    /// Share this other world's mods instead of choosing any (`None`: don't).
    SetBranch(Option<String>),
    /// From the worlds list: start a new world that branches this one.
    BranchFrom(String),
    /// From the worlds list: change which mods an existing world uses.
    EditMods(String),
    ApplyEditMods,
    Resume,
    QuitToMenu,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum TextField {
    #[default]
    Name,
    Seed,
}

#[derive(Resource, Default, Clone, Copy, PartialEq, Eq)]
pub enum WorldsScreenMode {
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
/// The worlds panel's title, which follows the mode (list / create / change mods).
#[derive(Component)]
struct WorldsHeading;
#[derive(Component)]
struct ModsRoot;
#[derive(Component)]
struct PauseRoot;

pub(crate) fn despawn_all<T: Component>(mut commands: Commands, q: Query<Entity, With<T>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

/// A full-width action button used by the main menu, pause menu and footers.
fn nav_button(parent: &mut ChildSpawnerCommands, label: &str, action: MenuButton, style: ButtonStyle) {
    theme::button(parent, action, label, ButtonOpts::new(style));
}

/// A button that is one of several choices (game mode, where mods live, a mod's tick).
fn choice(parent: &mut ChildSpawnerCommands, label: &str, action: MenuButton, selected: bool, width: f32) {
    theme::button(parent, action, label, ButtonOpts::new(ButtonStyle::Choice).size(width, 38.0).font(15.0).selected(selected));
}

/// Marks a scrolling list (worlds, mods) for the wheel system.
#[derive(Component)]
struct MenuScroll;

/// A column that scrolls once it is taller than `LIST_HEIGHT`.
fn scroll_list() -> impl Bundle {
    (
        MenuScroll,
        ScrollPosition::default(),
        Node {
            width: Val::Percent(100.0),
            max_height: Val::Px(LIST_HEIGHT),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(8.0),
            overflow: Overflow::scroll_y(),
            padding: UiRect::right(Val::Px(6.0)),
            ..default()
        },
    )
}

fn scroll_menu_lists(mut wheel: EventReader<MouseWheel>, mut lists: Query<&mut ScrollPosition, With<MenuScroll>>) {
    let dy: f32 = wheel.read().map(|e| e.y).sum();
    if dy != 0.0 {
        for mut pos in &mut lists {
            pos.offset_y = (pos.offset_y - dy * 30.0).max(0.0);
        }
    }
}

/// A card: a bordered, slightly lighter box.
fn card() -> impl Bundle {
    (
        Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(Val::Px(16.0), Val::Px(12.0)),
            row_gap: Val::Px(3.0),
            border: UiRect::all(Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(theme::CARD_BG),
        BorderColor(theme::PANEL_BORDER),
    )
}

// ---------------------------------------------------------------------------
// Main menu
// ---------------------------------------------------------------------------

fn setup_main_menu(mut commands: Commands) {
    commands.spawn((MainMenuRoot, theme::screen_root())).with_children(|root| {
        theme::spawn_backdrop(root, 11);
        theme::title_block(root, "build  -  explore  -  survive");
        root.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(12.0), ..default() }).with_children(|p| {
            nav_button(p, "Worlds", MenuButton::GoWorlds, ButtonStyle::Primary);
            nav_button(p, "Settings", MenuButton::GoSettings, ButtonStyle::Normal);
            nav_button(p, "Mods", MenuButton::GoMods, ButtonStyle::Normal);
            nav_button(p, "Quit Game", MenuButton::Quit, ButtonStyle::Danger);
        });
        root.spawn((
            Text::new(format!("v{}", crate::mods::GAME_VERSION)),
            TextFont { font_size: 14.0, ..default() },
            TextColor(theme::TEXT_DIM),
            Node { position_type: PositionType::Absolute, left: Val::Px(14.0), bottom: Val::Px(10.0), ..default() },
        ));
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
    commands.spawn((WorldsRoot, theme::screen_root())).with_children(|root| {
        theme::spawn_backdrop(root, 23);
        root.spawn(theme::panel(620.0)).with_children(|p| {
            theme::heading_with(p, "Select World", WorldsHeading);
            p.spawn((WorldsContent, Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: Val::Px(10.0), width: Val::Percent(100.0), ..default() }));
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
        .map(|m| (m.folder.clone(), with_warning_mark(format!("{} {}", m.name, m.version).trim().to_string(), m), meta.mods.contains(&m.folder)))
        .collect();
    for folder in &meta.mods {
        if !found.iter().any(|m| &m.folder == folder) {
            out.push((folder.clone(), format!("{folder} (missing)"), true));
        }
    }
    out
}

/// Adds " (!)" to a mod's label when it isn't made for this game version.
fn with_warning_mark(label: String, m: &crate::mods::ModInfo) -> String {
    if m.warning(crate::mods::GAME_VERSION).is_some() { format!("{label} (!)") } else { label }
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

/// The colour of a game mode's badge.
fn mode_color(mode: GameMode) -> Color {
    match mode {
        GameMode::Survival => theme::ACCENT,
        GameMode::Creative => Color::srgb(0.50, 0.72, 1.0),
    }
}

/// Spawns one saved world in the list: a clickable card (or, for a world that can't
/// be opened, a greyed card that says why), and the Mods / Branch buttons beside it.
fn spawn_world_row(p: &mut ChildSpawnerCommands, slug: &str, meta: &WorldMeta, blocked: Option<&str>, can_edit: bool, can_branch: bool) {
    p.spawn(Node { width: Val::Percent(100.0), column_gap: Val::Px(8.0), ..default() }).with_children(|line| {
        let card_node = Node {
            flex_grow: 1.0,
            flex_shrink: 1.0,
            min_width: Val::Px(0.0),
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            column_gap: Val::Px(12.0),
            padding: UiRect::axes(Val::Px(16.0), Val::Px(12.0)),
            border: UiRect::all(Val::Px(2.0)),
            ..default()
        };
        let mut row = if blocked.is_some() {
            line.spawn((card_node, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.30)), BorderColor(theme::PANEL_BORDER)))
        } else {
            let (bg, border) = theme::button_colors(ButtonStyle::Normal, Interaction::None, false, None);
            line.spawn((
                Button,
                MenuButton::LoadWorld(slug.to_string()),
                ThemedButton { style: ButtonStyle::Normal, selected: false, accent: None },
                card_node,
                BackgroundColor(bg),
                BorderColor(border),
            ))
        };
        row.with_children(|row| {
            row.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(3.0), flex_shrink: 1.0, ..default() }).with_children(|text| {
                text.spawn((
                    Text::new(meta.name.clone()),
                    TextFont { font_size: 20.0, ..default() },
                    TextColor(if blocked.is_some() { theme::TEXT_DIM } else { theme::TEXT }),
                ));
                let subtitle = blocked.map(str::to_string).unwrap_or_else(|| {
                    let mut s = format!("seed {}  -  {}", meta.seed, relative_time(meta.last_played_at));
                    if let Some(m) = mods_summary(meta) {
                        s.push_str("  -  ");
                        s.push_str(&m);
                    }
                    s
                });
                text.spawn((
                    Text::new(subtitle),
                    TextFont { font_size: 13.0, ..default() },
                    TextColor(if blocked.is_some() { theme::WARN } else { theme::TEXT_DIM }),
                    Node { max_width: Val::Px(380.0), ..default() },
                ));
            });
            if blocked.is_none() {
                theme::badge(row, mode_label(meta.mode), mode_color(meta.mode));
            }
        });
        let small = |line: &mut ChildSpawnerCommands, label: &str, action: MenuButton| {
            theme::button(line, action, label, ButtonOpts::new(ButtonStyle::Normal).size(78.0, 70.0).font(14.0));
        };
        if can_edit {
            small(line, "Mods", MenuButton::EditMods(slug.to_string()));
        }
        if can_branch {
            small(line, "Branch", MenuButton::BranchFrom(slug.to_string()));
        }
    });
}

/// The two buttons that end a form: the main action and a way out.
fn footer(p: &mut ChildSpawnerCommands, primary: (&str, MenuButton), secondary: (&str, MenuButton)) {
    p.spawn(Node { column_gap: Val::Px(12.0), margin: UiRect::top(Val::Px(10.0)), ..default() }).with_children(|row| {
        theme::button(row, primary.1, primary.0, ButtonOpts::new(ButtonStyle::Primary).size(250.0, 46.0));
        theme::button(row, secondary.1, secondary.0, ButtonOpts::new(ButtonStyle::Normal).size(170.0, 46.0));
    });
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
    mut headings: Query<&mut Text, With<WorldsHeading>>,
) {
    if !mode.is_changed() {
        return;
    }
    for mut heading in &mut headings {
        heading.0 = match *mode {
            WorldsScreenMode::List => "Select World",
            WorldsScreenMode::Create => "Create World",
            WorldsScreenMode::EditMods => "World Mods",
        }
        .to_string();
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
                    p.spawn((Text::new(message), TextFont { font_size: 14.0, ..default() }, TextColor(theme::WARN), Node { max_width: Val::Px(540.0), ..default() }));
                }
                if worlds.is_empty() {
                    theme::hint(p, "No worlds yet - create one below.", 16.0);
                }
                p.spawn(scroll_list()).with_children(|list| {
                    for (slug, meta) in &worlds {
                        // A world that can't be opened is shown, but not as a
                        // button, and says why: a newer build wrote it (entering
                        // would rewrite it in this older format), or a mod it
                        // needs is missing (entering would drop that mod's blocks
                        // from the next save). Leaving it out of the list would
                        // be worse - it would just look like the world had vanished.
                        let too_new = matches!(crate::save::compatibility(meta.format), crate::save::SaveCompatibility::TooNew { .. });
                        let blocked: Option<String> = if too_new {
                            Some("made by a newer version - update Craftmjne to open it".to_string())
                        } else {
                            resolve_world_mods(&store, slug, meta, &installed.0).err().map(|e| e.message())
                        };
                        let can_branch = meta.mod_storage == ModStorage::World && meta.branch_of.is_none() && blocked.is_none();
                        // A world with a missing mod can still have its mods changed - that is
                        // how it's fixed. One from a newer version can't be touched at all.
                        let can_edit = meta.branch_of.is_none() && !too_new;
                        spawn_world_row(list, slug, meta, blocked.as_deref(), can_edit, can_branch);
                    }
                });
                footer(p, ("Create World", MenuButton::ShowCreateForm), ("Back", MenuButton::BackToMainMenu));
            });
        }
        WorldsScreenMode::EditMods => {
            let editing = form.editing.clone().unwrap_or_default();
            let meta = store.load_meta(&editing).ok();
            let candidates = meta.as_ref().map(|m| edit_candidates(&store, &editing, m, &installed.0)).unwrap_or_default();
            let storage = meta.as_ref().map_or(ModStorage::Instance, |m| m.mod_storage);
            commands.entity(content).with_children(|p| {
                p.spawn((Text::new(format!("Mods for {}", meta.as_ref().map_or("this world", |m| m.name.as_str()))), TextFont { font_size: 20.0, ..default() }, TextColor(theme::TEXT)));
                theme::hint(
                    p,
                    match storage {
                        ModStorage::Instance => "These run from the game's mods folder.",
                        ModStorage::World => "Turning one off puts it aside in this world; nothing is deleted.",
                    },
                    13.0,
                );
                theme::hint(p, "Anything made with a mod you turn off stays in the world's save and comes back when you turn it on again.", 13.0);
                if candidates.is_empty() {
                    theme::hint(p, format!("No mods installed. Put mod folders in {}", installed.0.display()), 13.0);
                }
                p.spawn(scroll_list()).with_children(|list| {
                    for (folder, label, _) in &candidates {
                        let on = form.chosen_mods.contains(folder);
                        let text = format!("[{}] {}", if on { "x" } else { " " }, label);
                        choice(list, &text, MenuButton::ToggleMod(folder.clone()), on, 520.0);
                    }
                });
                footer(p, ("Apply", MenuButton::ApplyEditMods), ("Cancel", MenuButton::CancelCreate));
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
                spawn_text_field(p, "World name", TextField::Name, form.focus == TextField::Name);
                spawn_text_field(p, "Seed (blank = random)", TextField::Seed, form.focus == TextField::Seed);
                theme::label(p, "Game mode");
                p.spawn(Node { column_gap: Val::Px(8.0), margin: UiRect::bottom(Val::Px(6.0)), ..default() }).with_children(|row| {
                    for (label, game_mode) in [("Survival", GameMode::Survival), ("Creative", GameMode::Creative)] {
                        theme::button(
                            row,
                            (MenuButton::SetGameMode(game_mode), GameModeOption(game_mode)),
                            label,
                            ButtonOpts::new(ButtonStyle::Choice).size(216.0, 40.0).font(16.0).selected(form.mode == game_mode),
                        );
                    }
                });

                // Mods: share another world's, or pick from the installed ones.
                if !hosts.is_empty() {
                    theme::label(p, "Share the mods of another world (a branch)");
                    p.spawn(Node { column_gap: Val::Px(6.0), flex_wrap: FlexWrap::Wrap, row_gap: Val::Px(6.0), margin: UiRect::bottom(Val::Px(6.0)), ..default() })
                        .with_children(|row| {
                            choice(row, "No", MenuButton::SetBranch(None), form.branch_of.is_none(), 56.0);
                            for (slug, meta) in &hosts {
                                let name: String = meta.name.chars().take(14).collect();
                                choice(row, &name, MenuButton::SetBranch(Some(slug.clone())), form.branch_of.as_deref() == Some(slug), 130.0);
                            }
                        });
                }
                if form.branch_of.is_some() {
                    theme::hint(p, "This world will use that world's mods and keep none of its own.", 13.0);
                } else if mods.is_empty() {
                    theme::hint(p, format!("No mods installed. Put mod folders in {}", installed.0.display()), 13.0);
                } else {
                    theme::label(p, "Where this world keeps its mods");
                    p.spawn(Node { column_gap: Val::Px(8.0), margin: UiRect::bottom(Val::Px(6.0)), ..default() }).with_children(|row| {
                        choice(row, "Game folder", MenuButton::SetModStorage(ModStorage::Instance), form.mod_storage == ModStorage::Instance, 216.0);
                        choice(row, "This world", MenuButton::SetModStorage(ModStorage::World), form.mod_storage == ModStorage::World, 216.0);
                    });
                    theme::label(p, "Mods to use");
                    p.spawn(scroll_list()).with_children(|list| {
                        for m in &mods {
                            let on = form.chosen_mods.contains(&m.folder);
                            let label = with_warning_mark(format!("[{}] {} {}", if on { "x" } else { " " }, m.name, m.version).trim_end().to_string(), m);
                            choice(list, label.trim_end(), MenuButton::ToggleMod(m.folder.clone()), on, 440.0);
                        }
                    });
                }
                footer(p, ("Create", MenuButton::SubmitCreate), ("Cancel", MenuButton::CancelCreate));
            });
        }
    }
}

/// Marks the box of a text field so its focus ring can follow `CreateWorldForm::focus`.
#[derive(Component)]
struct TextFieldBox(TextField);

fn spawn_text_field(parent: &mut ChildSpawnerCommands, label: &str, field: TextField, focused: bool) {
    theme::label(parent, label);
    let (bg, border) = theme::button_colors(ButtonStyle::Field, Interaction::None, focused, None);
    parent
        .spawn((
            Button,
            MenuButton::FocusField(field),
            TextFieldBox(field),
            ThemedButton { style: ButtonStyle::Field, selected: focused, accent: None },
            Node {
                width: Val::Px(440.0),
                height: Val::Px(42.0),
                border: UiRect::all(Val::Px(2.0)),
                align_items: AlignItems::Center,
                padding: UiRect::left(Val::Px(12.0)),
                margin: UiRect::bottom(Val::Px(6.0)),
                ..default()
            },
            BackgroundColor(bg),
            BorderColor(border),
        ))
        .with_children(|p| {
            p.spawn((TextInputDisplay(field), Text::new(""), TextFont { font_size: 17.0, ..default() }, TextColor(theme::TEXT)));
        });
}

fn sync_text_inputs(
    form: Res<CreateWorldForm>,
    mut texts: Query<(&mut Text, &TextInputDisplay)>,
    mut boxes: Query<(&TextFieldBox, &mut ThemedButton)>,
    fresh: Query<(), Added<TextInputDisplay>>,
) {
    // Also when a field has just been (re)built: it is spawned blank, and the form
    // itself may not have changed since, so it would otherwise stay blank.
    if !form.is_changed() && fresh.is_empty() {
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
    for (field, mut themed) in &mut boxes {
        let focused = field.0 == form.focus;
        if themed.selected != focused {
            themed.selected = focused;
        }
    }
}

fn sync_mode_buttons(form: Res<CreateWorldForm>, mut options: Query<(&GameModeOption, &mut ThemedButton)>) {
    if !form.is_changed() {
        return;
    }
    for (opt, mut themed) in &mut options {
        let selected = opt.0 == form.mode;
        if themed.selected != selected {
            themed.selected = selected;
        }
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
// Mods screen (placeholder)
// ---------------------------------------------------------------------------

fn setup_mods(mut commands: Commands, installed: Res<InstalledMods>) {
    let (mods, broken) = crate::mods::scan_all(&installed.0);
    commands.spawn((ModsRoot, theme::screen_root())).with_children(|root| {
        theme::spawn_backdrop(root, 37);
        root.spawn(theme::panel(620.0)).with_children(|p| {
            theme::heading(p, "Installed Mods");
            if mods.is_empty() && broken.is_empty() {
                theme::hint(p, format!("No mods installed. Put mod folders in:\n{}", installed.0.display()), 15.0);
            }
            p.spawn(scroll_list()).with_children(|list| {
                for m in &mods {
                    list.spawn(card()).with_children(|c| {
                        c.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(10.0), ..default() }).with_children(|top| {
                            top.spawn((Text::new(m.name.clone()), TextFont { font_size: 19.0, ..default() }, TextColor(theme::TEXT)));
                            if !m.version.is_empty() {
                                theme::badge(top, &format!("v{}", m.version.trim_start_matches('v')), theme::ACCENT);
                            }
                            top.spawn((Text::new(m.id.clone()), TextFont { font_size: 13.0, ..default() }, TextColor(theme::TEXT_DIM)));
                        });
                        if !m.description.is_empty() {
                            theme::hint(c, m.description.clone(), 13.0);
                        }
                        if let Some(w) = m.warning(crate::mods::GAME_VERSION) {
                            c.spawn((Text::new(format!("Warning: {w}")), TextFont { font_size: 13.0, ..default() }, TextColor(theme::WARN), Node { max_width: Val::Px(540.0), ..default() }));
                        }
                    });
                }
                for (_, why) in &broken {
                    list.spawn(card()).with_children(|c| {
                        c.spawn((Text::new(format!("Won't work: {why}")), TextFont { font_size: 13.0, ..default() }, TextColor(theme::ERROR), Node { max_width: Val::Px(540.0), ..default() }));
                    });
                }
            });
            theme::hint(p, "Pick which mods a world uses when you create it. Mods sit on top of the game and never change its files.", 13.0);
            nav_button(p, "Back", MenuButton::BackToMainMenu, ButtonStyle::Normal);
        });
    });
}

// ---------------------------------------------------------------------------
// Pause screen (shown over the still-rendering world; see `PauseState`)
// ---------------------------------------------------------------------------

fn spawn_pause_screen(commands: &mut Commands) {
    commands.spawn((PauseRoot, theme::overlay_root())).with_children(|root| {
        root.spawn(theme::panel(420.0)).with_children(|p| {
            theme::heading(p, "Paused");
            nav_button(p, "Resume", MenuButton::Resume, ButtonStyle::Primary);
            nav_button(p, "Quit to Menu", MenuButton::QuitToMenu, ButtonStyle::Normal);
            nav_button(p, "Quit Game", MenuButton::Quit, ButtonStyle::Danger);
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
    mut paused: ResMut<PauseState>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    store: Res<SaveStore>,
    installed: Res<InstalledMods>,
    mut notice: ResMut<MenuNotice>,
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
        if !app.is_plugin_added::<theme::ThemePlugin>() {
            app.add_plugins(theme::ThemePlugin);
        }
        app.init_resource::<WorldsScreenMode>()
            .init_resource::<CreateWorldForm>()
            .add_systems(OnEnter(AppState::MainMenu), setup_main_menu)
            .add_systems(OnExit(AppState::MainMenu), despawn_all::<MainMenuRoot>)
            .add_systems(OnEnter(AppState::Worlds), setup_worlds)
            .add_systems(OnExit(AppState::Worlds), despawn_all::<WorldsRoot>)
            .add_systems(OnEnter(AppState::Mods), setup_mods)
            .add_systems(OnExit(AppState::Mods), despawn_all::<ModsRoot>)
            .add_systems(OnExit(AppState::InGame), despawn_pause)
            .add_systems(
                Update,
                (
                    handle_menu_buttons,
                    scroll_menu_lists,
                    rebuild_worlds_content.run_if(in_state(AppState::Worlds)),
                    handle_text_input.run_if(in_state(AppState::Worlds)),
                    sync_text_inputs.run_if(in_state(AppState::Worlds)),
                    sync_mode_buttons.run_if(in_state(AppState::Worlds)),
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
                ("a".to_string(), "a (!)".to_string(), true),
                ("b".to_string(), "b (!)".to_string(), false),
                ("ghost".to_string(), "ghost (missing)".to_string(), true),
            ],
            "a missing mod is listed, ticked, so it can be unticked to fix the world; (!) marks a mod with no target_ver"
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
