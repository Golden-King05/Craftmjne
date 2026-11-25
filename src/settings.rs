//! The Settings screen: a row of sections (Player, Rendering, Sound, Keybinds,
//! Mods) and the body of whichever is selected. Player and Sound are still
//! empty shells waiting for their options; Rendering and Mods hold the two
//! settings that existed before; Keybinds is the full control editor.
//!
//! **Keybinds screen.** One row per control (the game's own, then each mod's) grouped by category, each with a
//! button showing its chord (click it, then press the new key, mouse button or
//! combo - see [`Capture`]), a reset and an unbind button. A search box above
//! the list takes plain words (`jump`), `key:` queries (`key:a`, `key:ctrl+left`)
//! and `is:conflict` / `is:unbound` / `is:changed`. Any two controls sharing the
//! exact same chord are drawn in red, on both rows, with the other one named.
//!
//! The screen is rebuilt wholesale whenever its state changes (the project's
//! spawn-on-change pattern), with the list's scroll position carried across.
//! Buttons are ignored while a key is being captured, and handled *before* the
//! capture system each frame, so the click that ends a capture is never also
//! obeyed as a button press.

use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::MouseWheel;
use bevy::input::ButtonState;
use bevy::prelude::*;

use crate::config::WorldSettings;
use crate::keybinds::{search, Bindings, Capture, CaptureEvent};
use crate::mod_controls::ControlNotes;
use crate::menu::{despawn_all, full_screen_root, spawn_button, title, BUTTON_HOVER, BUTTON_IDLE, BUTTON_PRESS, PANEL_BG, TEXT_DIM};
use crate::save::{GraphicsSettings, ModStorage, SaveStore};
use crate::state::AppState;

const RED: Color = Color::srgb(1.0, 0.35, 0.35);
const YELLOW: Color = Color::srgb(1.0, 0.85, 0.3);
const BORDER_DIM: Color = Color::srgba(1.0, 1.0, 1.0, 0.25);
/// How much the mouse wheel moves the list per notch, in pixels.
const SCROLL_STEP: f32 = 28.0;
/// Longest search text accepted.
const SEARCH_MAX: usize = 40;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Player,
    Rendering,
    Sound,
    Keybinds,
    Mods,
}

impl Section {
    pub const ALL: [Section; 5] = [Section::Player, Section::Rendering, Section::Sound, Section::Keybinds, Section::Mods];

    pub fn label(self) -> &'static str {
        match self {
            Section::Player => "Player",
            Section::Rendering => "Rendering",
            Section::Sound => "Sound",
            Section::Keybinds => "Keybinds",
            Section::Mods => "Mods",
        }
    }
}

/// What the screen is showing right now.
#[derive(Resource)]
pub struct SettingsView {
    pub section: Section,
    pub search: String,
    /// The keybind list's scroll offset, kept so rebuilding doesn't jump it to the top.
    pub scroll: f32,
}

impl Default for SettingsView {
    fn default() -> Self {
        Self { section: Section::Player, search: String::new(), scroll: 0.0 }
    }
}

#[derive(Component)]
struct SettingsRoot;
#[derive(Component)]
struct SettingsContent;
#[derive(Component)]
struct KeybindList;

#[derive(Component, Clone, PartialEq, Debug)]
enum SettingsButton {
    Section(Section),
    RenderDistanceDelta(i32),
    ToggleDefaultModStorage,
    Rebind(String),
    Unbind(String),
    Reset(String),
    ResetAll,
    ClearSearch,
}

// ---------------------------------------------------------------------------
// Pure helpers (tested)
// ---------------------------------------------------------------------------

pub(crate) fn mod_storage_label(storage: ModStorage) -> String {
    match storage {
        ModStorage::Instance => "New worlds keep mods: in the game folder".to_string(),
        ModStorage::World => "New worlds keep mods: in the world".to_string(),
    }
}

/// The search results grouped by category, in the screen's order (the game's own
/// headings first, then each mod's), leaving out headings with nothing under them.
pub(crate) fn keybind_rows(bindings: &Bindings, query: &str) -> Vec<(String, Vec<String>)> {
    let found = search(bindings, query);
    bindings
        .categories()
        .into_iter()
        .filter_map(|cat| {
            let rows: Vec<String> = found.iter().filter(|id| bindings.def(id).is_some_and(|d| d.category == cat)).cloned().collect();
            (!rows.is_empty()).then_some((cat, rows))
        })
        .collect()
}

/// "Also used by: Open chat, Open inventory", or `None` if this control's chord is its own.
pub(crate) fn also_used_by(bindings: &Bindings, id: &str) -> Option<String> {
    let others = bindings.conflicts_with(id);
    (!others.is_empty()).then(|| {
        let names: Vec<&str> = others.iter().map(|o| bindings.def(o).map(|d| d.label.as_str()).unwrap_or(o.as_str())).collect();
        format!("Also used by: {}", names.join(", "))
    })
}

/// How many controls are in a conflict, as a sentence, or `None` when none are.
pub(crate) fn conflict_summary(bindings: &Bindings) -> Option<String> {
    let n = bindings.conflicting().len();
    (n > 0).then(|| format!("{n} controls share a key with another (shown in red)."))
}

/// The label on a section tab: Keybinds says how many controls clash.
pub(crate) fn tab_label(section: Section, bindings: &Bindings) -> String {
    let n = bindings.conflicting().len();
    if section == Section::Keybinds && n > 0 {
        format!("Keybinds ({n} in conflict)")
    } else {
        section.label().to_string()
    }
}

fn save_bindings(store: &SaveStore, bindings: &Bindings) {
    let _ = store.save_keybinds(&bindings.overrides());
}

// ---------------------------------------------------------------------------
// Building the screen
// ---------------------------------------------------------------------------

fn setup_settings(mut commands: Commands, mut view: ResMut<SettingsView>, mut capture: ResMut<Capture>) {
    *view = SettingsView::default();
    capture.target = None;
    commands.spawn((SettingsRoot, full_screen_root())).with_children(|root| {
        root.spawn((
            Node {
                width: Val::Px(680.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(28.0), Val::Px(22.0)),
                row_gap: Val::Px(10.0),
                ..default()
            },
            BackgroundColor(PANEL_BG),
        ))
        .with_children(|p| {
            p.spawn(title("Settings"));
            p.spawn((SettingsContent, Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: Val::Px(10.0), width: Val::Percent(100.0), ..default() }));
            spawn_button(p, "Back", crate::menu::MenuButton::BackToMainMenu);
        });
    });
}

fn dim(parent: &mut ChildSpawnerCommands, text: impl Into<String>, size: f32) {
    parent.spawn((Text::new(text), TextFont { font_size: size, ..default() }, TextColor(TEXT_DIM), Node { max_width: Val::Px(620.0), ..default() }));
}

fn spawn_settings_button(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    action: SettingsButton,
    width: f32,
    height: f32,
    font: f32,
    color: Color,
    border: Color,
) {
    parent
        .spawn((
            Button,
            action,
            Node {
                width: Val::Px(width),
                height: Val::Px(height),
                border: UiRect::all(Val::Px(2.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BorderColor(border),
            BackgroundColor(BUTTON_IDLE),
        ))
        .with_children(|b| {
            b.spawn((Text::new(label), TextFont { font_size: font, ..default() }, TextColor(color)));
        });
}

fn rebuild_settings(
    mut commands: Commands,
    view: Res<SettingsView>,
    bindings: Res<Bindings>,
    capture: Res<Capture>,
    notes: Res<ControlNotes>,
    world_settings: Res<WorldSettings>,
    store: Res<SaveStore>,
    content: Query<(Entity, Option<&Children>), With<SettingsContent>>,
) {
    let Ok((root, children)) = content.single() else { return };
    let empty = children.is_none_or(|c| c.is_empty());
    if !(empty || view.is_changed() || bindings.is_changed() || capture.is_changed() || notes.is_changed() || world_settings.is_changed()) {
        return;
    }
    commands.entity(root).despawn_related::<Children>();
    commands.entity(root).with_children(|p| {
        // Section tabs.
        p.spawn(Node { column_gap: Val::Px(6.0), ..default() }).with_children(|row| {
            for section in Section::ALL {
                let selected = section == view.section;
                let clash = section == Section::Keybinds && !bindings.conflicting().is_empty();
                let text = if clash { RED } else { Color::WHITE };
                let border = if selected { Color::WHITE } else { BORDER_DIM };
                let width = if section == Section::Keybinds { 190.0 } else { 100.0 };
                spawn_settings_button(row, &tab_label(section, &bindings), SettingsButton::Section(section), width, 34.0, 15.0, text, border);
            }
        });
        // The selected section, in a box of fixed height so switching doesn't resize the panel.
        p.spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Px(440.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(8.0),
            padding: UiRect::top(Val::Px(8.0)),
            ..default()
        })
        .with_children(|body| match view.section {
            Section::Player => {
                dim(body, "Player settings - field of view, mouse sensitivity and the like - will live here.", 14.0);
            }
            Section::Rendering => {
                body.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(14.0), ..default() }).with_children(|row| {
                    spawn_settings_button(row, "-", SettingsButton::RenderDistanceDelta(-1), 36.0, 36.0, 20.0, Color::WHITE, Color::NONE);
                    row.spawn((Text::new(format!("Render distance: {}", world_settings.render_distance)), TextFont { font_size: 16.0, ..default() }, TextColor(Color::WHITE)));
                    spawn_settings_button(row, "+", SettingsButton::RenderDistanceDelta(1), 36.0, 36.0, 20.0, Color::WHITE, Color::NONE);
                });
                dim(body, "Takes effect after restart.", 12.0);
                dim(body, "More video options (fullscreen, vsync, brightness) will appear here.", 14.0);
            }
            Section::Sound => {
                dim(body, "There is no sound in the game yet. Volume controls will live here.", 14.0);
            }
            Section::Mods => {
                let storage = store.load_graphics_settings().mod_storage;
                spawn_settings_button(body, &mod_storage_label(storage), SettingsButton::ToggleDefaultModStorage, 380.0, 40.0, 15.0, Color::WHITE, Color::NONE);
                dim(body, "Game folder: one copy of each mod, shared by every world (remove it and worlds using it won't open).\nIn the world: each world gets its own copy, so it can have mods the game doesn't.", 12.0);
            }
            Section::Keybinds => spawn_keybinds(body, &view, &bindings, &capture, &notes),
        });
    });
}

fn spawn_keybinds(body: &mut ChildSpawnerCommands, view: &SettingsView, bindings: &Bindings, capture: &Capture, notes: &ControlNotes) {
    // Search box + clear + reset all.
    body.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(8.0), width: Val::Percent(100.0), ..default() }).with_children(|row| {
        row.spawn((
            Node {
                flex_grow: 1.0,
                height: Val::Px(34.0),
                border: UiRect::all(Val::Px(2.0)),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(10.0)),
                ..default()
            },
            BorderColor(if capture.active() { BORDER_DIM } else { Color::WHITE }),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
        ))
        .with_children(|b| {
            let (text, color) = if view.search.is_empty() && capture.active() {
                ("Search".to_string(), TEXT_DIM)
            } else if view.search.is_empty() {
                ("Search...  (try key:a)_".to_string(), TEXT_DIM)
            } else if capture.active() {
                (view.search.clone(), Color::WHITE)
            } else {
                (format!("{}_", view.search), Color::WHITE)
            };
            b.spawn((Text::new(text), TextFont { font_size: 15.0, ..default() }, TextColor(color)));
        });
        if !view.search.is_empty() {
            spawn_settings_button(row, "Clear", SettingsButton::ClearSearch, 70.0, 34.0, 14.0, Color::WHITE, BORDER_DIM);
        }
        spawn_settings_button(row, "Reset all", SettingsButton::ResetAll, 90.0, 34.0, 14.0, Color::WHITE, BORDER_DIM);
    });
    dim(body, "key:a = what uses A  -  key:ctrl+left = combos  -  is:conflict, is:unbound, is:changed", 12.0);

    if let Some(summary) = conflict_summary(bindings) {
        body.spawn((Text::new(summary), TextFont { font_size: 13.0, ..default() }, TextColor(RED)));
    }
    if let Some(id) = &capture.target {
        let label = bindings.def(id).map(|d| d.label.as_str()).unwrap_or(id.as_str());
        body.spawn((
            Text::new(format!("Binding \"{label}\": press a key, click, or a combo like Ctrl+Alt+A. Release Ctrl/Alt/Shift alone to bind it by itself. Esc cancels.")),
            TextFont { font_size: 13.0, ..default() },
            TextColor(YELLOW),
            Node { max_width: Val::Px(620.0), ..default() },
        ));
    }

    let rows = keybind_rows(bindings, &view.search);
    let conflicting = bindings.conflicting();
    body.spawn((
        KeybindList,
        ScrollPosition { offset_x: 0.0, offset_y: view.scroll },
        Node {
            width: Val::Percent(100.0),
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            overflow: Overflow::scroll_y(),
            padding: UiRect::right(Val::Px(8.0)),
            ..default()
        },
    ))
    .with_children(|list| {
        if rows.is_empty() {
            dim(list, "No controls match.", 14.0);
        }
        for (category, ids) in rows {
            list.spawn((Text::new(category), TextFont { font_size: 16.0, ..default() }, TextColor(YELLOW), Node { margin: UiRect::top(Val::Px(6.0)), ..default() }));
            for id in ids {
                let Some(def) = bindings.def(&id) else { continue };
                let clash = conflicting.contains(&id);
                let listening = capture.target.as_deref() == Some(id.as_str());
                list.spawn(Node { flex_direction: FlexDirection::Column, ..default() }).with_children(|entry| {
                    entry.spawn(Node { align_items: AlignItems::Center, justify_content: JustifyContent::SpaceBetween, column_gap: Val::Px(6.0), ..default() }).with_children(|row| {
                        row.spawn((Text::new(def.label.clone()), TextFont { font_size: 15.0, ..default() }, TextColor(if clash { RED } else { Color::WHITE })));
                        row.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(6.0), ..default() }).with_children(|buttons| {
                            let (text, color) = match (listening, bindings.get(&id)) {
                                (true, _) => ("Press a key...".to_string(), YELLOW),
                                (false, Some(chord)) => (chord.display(), if clash { RED } else { Color::WHITE }),
                                (false, None) => ("Unbound".to_string(), TEXT_DIM),
                            };
                            let border = if listening { YELLOW } else if clash { RED } else { BORDER_DIM };
                            spawn_settings_button(buttons, &text, SettingsButton::Rebind(id.clone()), 190.0, 30.0, 14.0, color, border);
                            let changed = if bindings.is_default(&id) { TEXT_DIM } else { Color::WHITE };
                            spawn_settings_button(buttons, "Reset", SettingsButton::Reset(id.clone()), 58.0, 30.0, 13.0, changed, BORDER_DIM);
                            let bound = if bindings.get(&id).is_some() { Color::WHITE } else { TEXT_DIM };
                            spawn_settings_button(buttons, "Unbind", SettingsButton::Unbind(id.clone()), 66.0, 30.0, 13.0, bound, BORDER_DIM);
                        });
                    });
                    if let Some(note) = also_used_by(bindings, &id) {
                        entry.spawn((Text::new(note), TextFont { font_size: 12.0, ..default() }, TextColor(RED)));
                    }
                    if let Some(command) = &def.command {
                        entry.spawn((Text::new(format!("runs {command}")), TextFont { font_size: 12.0, ..default() }, TextColor(TEXT_DIM)));
                    }
                });
            }
        }
    });
    for note in &notes.0 {
        body.spawn((Text::new(format!("Mod control skipped: {note}")), TextFont { font_size: 12.0, ..default() }, TextColor(Color::srgb(0.95, 0.6, 0.4)), Node { max_width: Val::Px(620.0), ..default() }));
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

fn settings_button_visuals(mut buttons: Query<(&Interaction, &mut BackgroundColor), (Changed<Interaction>, With<SettingsButton>)>) {
    for (interaction, mut bg) in &mut buttons {
        *bg = BackgroundColor(match interaction {
            Interaction::Pressed => BUTTON_PRESS,
            Interaction::Hovered => BUTTON_HOVER,
            Interaction::None => BUTTON_IDLE,
        });
    }
}

/// Typing goes into the search box while the Keybinds section is open. Typed
/// text is read from the keyboard events (not key codes), same as every other
/// text field here. Ignored - and the events thrown away - while a key is being
/// captured, so the key being bound never also lands in the search.
fn search_input(mut events: EventReader<KeyboardInput>, capture: Res<Capture>, mut view: ResMut<SettingsView>) {
    if view.section != Section::Keybinds || capture.active() {
        events.clear();
        return;
    }
    for ev in events.read() {
        if ev.state != ButtonState::Pressed {
            continue;
        }
        if ev.key_code == KeyCode::Backspace {
            view.search.pop();
            view.scroll = 0.0;
            continue;
        }
        if let Some(text) = &ev.text {
            for ch in text.chars() {
                if !ch.is_control() && view.search.chars().count() < SEARCH_MAX {
                    view.search.push(ch);
                    view.scroll = 0.0;
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_settings_buttons(
    interactions: Query<(&Interaction, &SettingsButton), Changed<Interaction>>,
    mut view: ResMut<SettingsView>,
    mut capture: ResMut<Capture>,
    mut bindings: ResMut<Bindings>,
    mut world_settings: ResMut<WorldSettings>,
    store: Res<SaveStore>,
) {
    for (interaction, button) in &interactions {
        if *interaction != Interaction::Pressed || capture.active() {
            continue;
        }
        match button.clone() {
            SettingsButton::Section(section) => {
                view.section = section;
                view.scroll = 0.0;
            }
            SettingsButton::RenderDistanceDelta(d) => {
                world_settings.render_distance = (world_settings.render_distance + d).clamp(2, 16);
                let _ = store.save_graphics_settings(&GraphicsSettings { render_distance: world_settings.render_distance, ..store.load_graphics_settings() });
            }
            SettingsButton::ToggleDefaultModStorage => {
                let mut saved = store.load_graphics_settings();
                saved.mod_storage = match saved.mod_storage {
                    ModStorage::Instance => ModStorage::World,
                    ModStorage::World => ModStorage::Instance,
                };
                let _ = store.save_graphics_settings(&saved);
                view.set_changed();
            }
            SettingsButton::Rebind(id) => capture.start(id),
            SettingsButton::Unbind(id) => {
                bindings.set(id, None);
                save_bindings(&store, &bindings);
            }
            SettingsButton::Reset(id) => {
                bindings.reset(id);
                save_bindings(&store, &bindings);
            }
            SettingsButton::ResetAll => {
                bindings.reset_all();
                save_bindings(&store, &bindings);
            }
            SettingsButton::ClearSearch => {
                view.search.clear();
                view.scroll = 0.0;
            }
        }
    }
}

/// Resolves a pending rebind. Runs after the button handler (see the module docs).
fn capture_binding(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut capture: ResMut<Capture>,
    mut bindings: ResMut<Bindings>,
    store: Res<SaveStore>,
) {
    let Some(target) = capture.target.clone() else { return };
    // Stepping every frame is not a change worth redrawing the screen for;
    // only an outcome is.
    let event = capture.bypass_change_detection().step(&keys, &mouse);
    match event {
        Some(CaptureEvent::Chosen(chord)) => {
            bindings.set(target, Some(chord));
            save_bindings(&store, &bindings);
            capture.set_changed();
        }
        Some(CaptureEvent::Cancelled) => capture.set_changed(),
        None => {}
    }
}

/// Mouse wheel scrolls the list; the offset is read back every frame (Bevy clamps
/// it to the content) so a rebuild can put it back where it was.
fn scroll_keybind_list(
    mut wheel: EventReader<MouseWheel>,
    mut lists: Query<&mut ScrollPosition, With<KeybindList>>,
    mut view: ResMut<SettingsView>,
) {
    let dy: f32 = wheel.read().map(|e| e.y).sum();
    for mut pos in &mut lists {
        if dy != 0.0 {
            pos.offset_y = (pos.offset_y - dy * SCROLL_STEP).max(0.0);
        }
        if view.scroll != pos.offset_y {
            view.bypass_change_detection().scroll = pos.offset_y;
        }
    }
}

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SettingsView>()
            .init_resource::<Capture>()
            .add_systems(OnEnter(AppState::Settings), setup_settings)
            .add_systems(OnExit(AppState::Settings), despawn_all::<SettingsRoot>)
            .add_systems(
                Update,
                (
                    settings_button_visuals,
                    search_input,
                    handle_settings_buttons,
                    capture_binding,
                    scroll_keybind_list,
                    rebuild_settings,
                )
                    .chain()
                    .run_if(in_state(AppState::Settings)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keybinds::{Action, Chord};

    fn id(a: Action) -> String {
        a.id().to_string()
    }

    #[test]
    fn rows_are_grouped_by_category_and_empty_groups_are_left_out() {
        let b = Bindings::default();
        let all = keybind_rows(&b, "");
        assert_eq!(all.iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>(), ["Movement", "Gameplay", "Inventory", "Interface"]);
        assert_eq!(all.iter().map(|(_, rows)| rows.len()).sum::<usize>(), Action::ALL.len());
        let movement_only = keybind_rows(&b, "key:space");
        assert_eq!(movement_only, vec![("Movement".to_string(), vec![id(Action::Jump)])]);
        assert!(keybind_rows(&b, "key:zzz").is_empty());
    }

    #[test]
    fn a_conflict_names_the_other_control_and_counts_both() {
        let mut b = Bindings::default();
        assert_eq!(conflict_summary(&b), None);
        assert_eq!(also_used_by(&b, "chat"), None);
        b.set(Action::Chat, Chord::parse("E"));
        assert_eq!(also_used_by(&b, "chat").as_deref(), Some("Also used by: Open inventory"));
        assert_eq!(also_used_by(&b, "inventory").as_deref(), Some("Also used by: Open chat"));
        assert_eq!(conflict_summary(&b).as_deref(), Some("2 controls share a key with another (shown in red)."));
        assert_eq!(tab_label(Section::Keybinds, &b), "Keybinds (2 in conflict)");
        assert_eq!(tab_label(Section::Rendering, &b), "Rendering", "only the Keybinds tab warns");
        assert_eq!(tab_label(Section::Keybinds, &Bindings::default()), "Keybinds");
    }

    // ---- the screen as a running Bevy schedule ----------------------------

    use crate::keybinds::KeybindPlugin;
    use bevy::input::keyboard::Key;
    use bevy::state::app::StatesPlugin;
    use std::sync::atomic::{AtomicU64, Ordering};

    static N: AtomicU64 = AtomicU64::new(0);

    struct Screen {
        app: App,
        dir: std::path::PathBuf,
    }

    impl Drop for Screen {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn screen() -> Screen {
        screen_with(None)
    }

    fn screen_with(installed: Option<std::path::PathBuf>) -> Screen {
        let dir = std::env::temp_dir().join(format!("craftmjne-settings-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin));
        app.init_resource::<ButtonInput<KeyCode>>().init_resource::<ButtonInput<MouseButton>>();
        app.add_event::<KeyboardInput>().add_event::<MouseWheel>();
        app.insert_resource(SaveStore::at(dir.clone())).insert_resource(WorldSettings::default());
        app.init_state::<AppState>();
        if let Some(installed) = installed {
            app.insert_resource(crate::mods::InstalledMods(installed));
        }
        app.add_plugins((KeybindPlugin, crate::mod_controls::ModControlsPlugin, SettingsPlugin));
        app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::Settings);
        app.update();
        app.update();
        Screen { app, dir }
    }

    impl Screen {
        fn buttons(&mut self) -> Vec<(Entity, SettingsButton)> {
            let mut q = self.app.world_mut().query::<(Entity, &SettingsButton)>();
            q.iter(self.app.world()).map(|(e, b)| (e, b.clone())).collect()
        }

        fn click(&mut self, button: SettingsButton) {
            let (entity, _) = self.buttons().into_iter().find(|(_, b)| *b == button).unwrap_or_else(|| panic!("no {button:?} on screen"));
            self.app.world_mut().entity_mut(entity).insert(Interaction::Pressed);
            self.app.update();
        }

        fn texts(&mut self) -> Vec<String> {
            let mut q = self.app.world_mut().query::<&Text>();
            q.iter(self.app.world()).map(|t| t.0.clone()).collect()
        }

        fn press(&mut self, key: KeyCode) {
            self.app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
            self.app.update();
            let mut keys = self.app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(key);
            keys.clear();
        }

        fn type_text(&mut self, text: &str) {
            for ch in text.chars() {
                self.app.world_mut().send_event(KeyboardInput {
                    key_code: KeyCode::KeyA,
                    logical_key: Key::Character(ch.to_string().into()),
                    state: ButtonState::Pressed,
                    text: Some(ch.to_string().into()),
                    repeat: false,
                    window: Entity::PLACEHOLDER,
                });
            }
            self.app.update();
        }

        fn bindings(&self) -> Bindings {
            self.app.world().resource::<Bindings>().clone()
        }

        fn rebind_buttons(&mut self) -> Vec<String> {
            self.buttons().into_iter().filter_map(|(_, b)| if let SettingsButton::Rebind(a) = b { Some(a) } else { None }).collect()
        }
    }

    #[test]
    fn the_screen_builds_with_one_tab_per_section_and_opens_on_player() {
        let mut s = screen();
        let tabs: Vec<_> = s.buttons().into_iter().filter_map(|(_, b)| if let SettingsButton::Section(x) = b { Some(x) } else { None }).collect();
        assert_eq!(tabs, Section::ALL.to_vec());
        assert_eq!(s.app.world().resource::<SettingsView>().section, Section::Player);
        assert!(s.texts().iter().any(|t| t.contains("Player settings")));
    }

    #[test]
    fn each_section_shows_its_own_body() {
        let mut s = screen();
        s.click(SettingsButton::Section(Section::Rendering));
        assert!(s.texts().iter().any(|t| t.starts_with("Render distance")));
        s.click(SettingsButton::Section(Section::Sound));
        assert!(s.texts().iter().any(|t| t.contains("no sound")));
        s.click(SettingsButton::Section(Section::Mods));
        assert!(s.texts().iter().any(|t| t.starts_with("New worlds keep mods")));
        s.click(SettingsButton::Section(Section::Keybinds));
        assert_eq!(s.rebind_buttons().len(), Action::ALL.len(), "one row per control");
    }

    #[test]
    fn render_distance_buttons_change_and_save_it() {
        let mut s = screen();
        s.click(SettingsButton::Section(Section::Rendering));
        let before = s.app.world().resource::<WorldSettings>().render_distance;
        s.click(SettingsButton::RenderDistanceDelta(-1));
        assert_eq!(s.app.world().resource::<WorldSettings>().render_distance, before - 1);
        assert!(s.texts().iter().any(|t| *t == format!("Render distance: {}", before - 1)), "the label follows");
        assert_eq!(s.app.world().resource::<SaveStore>().load_graphics_settings().render_distance, before - 1);
    }

    #[test]
    fn clicking_a_control_then_pressing_a_combo_rebinds_and_saves_it() {
        let mut s = screen();
        s.click(SettingsButton::Section(Section::Keybinds));
        s.click(SettingsButton::Rebind(id(Action::Jump)));
        assert!(s.app.world().resource::<Capture>().active());
        assert!(s.texts().iter().any(|t| t == "Press a key..."), "the row says it is listening");

        s.app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ControlLeft);
        s.app.update();
        s.press(KeyCode::KeyJ);
        assert_eq!(s.bindings().get(Action::Jump), Chord::parse("Ctrl+J"), "Ctrl was held when J went down");
        assert!(!s.app.world().resource::<Capture>().active());
        assert!(s.texts().iter().any(|t| t == "Ctrl+J"), "the row shows the new chord");
        assert_eq!(s.app.world().resource::<SaveStore>().load_keybinds()["jump"], "Ctrl+J", "and it was written to disk");
    }

    #[test]
    fn escape_cancels_a_rebind_and_the_key_is_not_typed_into_the_search() {
        let mut s = screen();
        s.click(SettingsButton::Section(Section::Keybinds));
        s.click(SettingsButton::Rebind(id(Action::Jump)));
        s.type_text("x");
        assert_eq!(s.app.world().resource::<SettingsView>().search, "", "typing while capturing is not search text");
        s.press(KeyCode::Escape);
        assert!(!s.app.world().resource::<Capture>().active());
        assert_eq!(s.bindings().get(Action::Jump), Action::Jump.default_chord(), "unchanged");
        assert!(s.app.world().resource::<SaveStore>().load_keybinds().is_empty(), "nothing saved");
    }

    #[test]
    fn buttons_are_ignored_while_a_key_is_being_captured() {
        let mut s = screen();
        s.click(SettingsButton::Section(Section::Keybinds));
        s.click(SettingsButton::Rebind(id(Action::Jump)));
        s.click(SettingsButton::Section(Section::Sound));
        assert_eq!(s.app.world().resource::<SettingsView>().section, Section::Keybinds);
        s.click(SettingsButton::Reset(id(Action::Chat)));
        assert!(s.app.world().resource::<Capture>().active(), "still waiting for the key");
    }

    #[test]
    fn unbind_and_reset_work_and_reset_all_restores_everything() {
        let mut s = screen();
        s.click(SettingsButton::Section(Section::Keybinds));
        s.click(SettingsButton::Unbind(id(Action::Chat)));
        assert_eq!(s.bindings().get(Action::Chat), None);
        assert!(s.texts().iter().any(|t| t == "Unbound"));
        assert_eq!(s.app.world().resource::<SaveStore>().load_keybinds()["chat"], "none");
        s.click(SettingsButton::Reset(id(Action::Chat)));
        assert_eq!(s.bindings().get(Action::Chat), Action::Chat.default_chord());
        s.click(SettingsButton::Unbind(id(Action::Jump)));
        s.click(SettingsButton::Unbind(id(Action::Sprint)));
        s.click(SettingsButton::ResetAll);
        assert_eq!(s.bindings(), Bindings::default());
        assert!(s.app.world().resource::<SaveStore>().load_keybinds().is_empty(), "back to nothing changed");
    }

    #[test]
    fn two_controls_on_one_key_are_flagged_on_both_rows() {
        let mut s = screen();
        s.click(SettingsButton::Section(Section::Keybinds));
        s.app.world_mut().resource_mut::<Bindings>().set(Action::Chat, Chord::parse("E"));
        s.app.update();
        let texts = s.texts();
        assert!(texts.iter().any(|t| t == "Also used by: Open inventory"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "Also used by: Open chat"));
        assert!(texts.iter().any(|t| t == "Keybinds (2 in conflict)"), "the tab warns too");
        // The red is real colour on the rows' text, not just wording.
        let mut q = s.app.world_mut().query::<(&Text, &TextColor)>();
        let red_labels: Vec<_> = q.iter(s.app.world()).filter(|(_, c)| c.0 == RED).map(|(t, _)| t.0.clone()).collect();
        assert!(red_labels.iter().any(|t| t == "Open chat") && red_labels.iter().any(|t| t == "Open inventory"), "{red_labels:?}");
    }

    #[test]
    fn typing_searches_and_key_queries_find_a_key() {
        let mut s = screen();
        s.click(SettingsButton::Section(Section::Keybinds));
        s.type_text("key:a");
        assert_eq!(s.app.world().resource::<SettingsView>().search, "key:a");
        assert_eq!(s.rebind_buttons(), vec![id(Action::Left)], "only the control on A is listed");
        s.click(SettingsButton::ClearSearch);
        assert_eq!(s.rebind_buttons().len(), Action::ALL.len());
        s.type_text("hotbar");
        assert_eq!(s.rebind_buttons().len(), 9);
        s.type_text("zzz");
        assert!(s.rebind_buttons().is_empty());
        assert!(s.texts().iter().any(|t| t == "No controls match."));
    }

    #[test]
    fn typing_outside_the_keybinds_section_does_not_touch_the_search() {
        let mut s = screen();
        s.type_text("abc");
        assert_eq!(s.app.world().resource::<SettingsView>().search, "");
    }

    #[test]
    fn leaving_the_screen_removes_it_and_a_half_finished_rebind() {
        let mut s = screen();
        s.click(SettingsButton::Section(Section::Keybinds));
        s.click(SettingsButton::Rebind(id(Action::Jump)));
        s.app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::MainMenu);
        s.app.update();
        s.app.update();
        assert!(s.buttons().is_empty());
        s.app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::Settings);
        s.app.update();
        s.app.update();
        assert!(!s.app.world().resource::<Capture>().active(), "a rebind doesn't wait around for the next visit");
        assert_eq!(s.app.world().resource::<SettingsView>().section, Section::Player);
    }


    // ---- controls from mods ------------------------------------------------

    use crate::mod_controls::support::{add_mod, install};

    fn mod_screen() -> (Screen, crate::mod_controls::support::Installed) {
        let installed = install(
            "weather",
            &[
                ("noon.json", r#"{"id": "noon", "name": "Skip to noon", "category": "Weather", "default": "Ctrl+N", "command": "/time set noon"}"#),
                ("broken.json", "nope"),
            ],
        );
        let mut s = screen_with(Some(installed.0.clone()));
        s.click(SettingsButton::Section(Section::Keybinds));
        (s, installed)
    }

    #[test]
    fn a_mods_controls_appear_under_their_own_heading_with_what_they_run() {
        let (mut s, _keep) = mod_screen();
        assert!(s.rebind_buttons().contains(&"weather:noon".to_string()));
        let texts = s.texts();
        assert!(texts.iter().any(|t| t == "Weather"), "the mod's heading");
        assert!(texts.iter().any(|t| t == "Skip to noon"));
        assert!(texts.iter().any(|t| t == "Ctrl+N"), "its default chord");
        assert!(texts.iter().any(|t| t == "runs /time set noon"), "and what pressing it does, so it is not a mystery key");
    }

    #[test]
    fn a_broken_mod_file_is_reported_on_the_screen_without_hiding_the_rest() {
        let (mut s, _keep) = mod_screen();
        assert!(s.texts().iter().any(|t| t.starts_with("Mod control skipped:") && t.contains("broken.json")));
        assert_eq!(s.rebind_buttons().len(), Action::ALL.len() + 1, "the good control is there");
    }

    #[test]
    fn mod_controls_can_be_searched_rebound_and_the_change_is_saved() {
        let (mut s, _keep) = mod_screen();
        s.type_text("noon");
        assert_eq!(s.rebind_buttons(), vec!["weather:noon".to_string()], "found by name");
        s.click(SettingsButton::ClearSearch);
        s.type_text("weather");
        assert_eq!(s.rebind_buttons(), vec!["weather:noon".to_string()], "and by its mod's heading");
        s.click(SettingsButton::ClearSearch);
        s.type_text("key:ctrl+n");
        assert_eq!(s.rebind_buttons(), vec!["weather:noon".to_string()], "and by its key");
        s.click(SettingsButton::ClearSearch);

        s.click(SettingsButton::Rebind("weather:noon".into()));
        s.press(KeyCode::F9);
        assert_eq!(s.bindings().get("weather:noon"), Chord::parse("F9"));
        assert_eq!(s.app.world().resource::<SaveStore>().load_keybinds()["weather:noon"], "F9");
    }

    #[test]
    fn a_mod_control_on_a_key_the_game_uses_is_a_flagged_conflict() {
        let (mut s, _keep) = mod_screen();
        s.app.world_mut().resource_mut::<Bindings>().set("weather:noon", Chord::parse("E"));
        s.app.update();
        let texts = s.texts();
        assert!(texts.iter().any(|t| t == "Also used by: Open inventory"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "Also used by: Skip to noon"));
    }

    #[test]
    fn entering_settings_again_picks_up_a_mod_dropped_in_meanwhile() {
        let (mut s, keep) = mod_screen();
        assert!(!s.rebind_buttons().contains(&"later:thing".to_string()));
        add_mod(&keep.0, "later", &[("t.json", r#"{"id": "thing"}"#)]);
        s.app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::MainMenu);
        s.app.update();
        s.app.update();
        s.app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::Settings);
        s.app.update();
        s.app.update();
        s.click(SettingsButton::Section(Section::Keybinds));
        assert!(s.rebind_buttons().contains(&"later:thing".to_string()));
    }

    #[test]
    fn removing_a_mod_while_its_key_was_changed_keeps_the_key_for_later() {
        let (mut s, keep) = mod_screen();
        s.click(SettingsButton::Rebind("weather:noon".into()));
        s.press(KeyCode::F9);
        std::fs::remove_dir_all(keep.0.join("weather")).unwrap();
        s.app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::MainMenu);
        s.app.update();
        s.app.update();
        s.app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::Settings);
        s.app.update();
        s.app.update();
        s.click(SettingsButton::Section(Section::Keybinds));
        assert!(!s.rebind_buttons().contains(&"weather:noon".to_string()), "gone from the screen");
        // Any later change (here: another rebind) saves the file again; the removed mod's key must still be in it.
        s.click(SettingsButton::Unbind("chat".into()));
        let saved = s.app.world().resource::<SaveStore>().load_keybinds();
        assert_eq!(saved["weather:noon"], "F9", "taking a mod out must not cost the player their key for it");
    }

    #[test]
    fn the_sections_are_the_ones_asked_for() {
        let labels: Vec<_> = Section::ALL.iter().map(|s| s.label()).collect();
        assert_eq!(labels, ["Player", "Rendering", "Sound", "Keybinds", "Mods"]);
    }

    #[test]
    fn mod_storage_has_a_label_for_each_choice() {
        assert!(mod_storage_label(ModStorage::Instance).contains("game folder"));
        assert!(mod_storage_label(ModStorage::World).contains("in the world"));
    }
}
