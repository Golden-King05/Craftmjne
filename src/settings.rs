//! The Settings screen: a row of sections (Player, Rendering, Sound, Controls,
//! Mods) and the body of whichever is selected. Player and Sound are still
//! empty shells waiting for their options; Rendering and Mods hold the two
//! settings that existed before; Controls holds the keybind editor and how controls
//! behave (flight mode).
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
use crate::gamepad::PadState;
use crate::keybinds::{search_on, Bindings, Capture, CaptureEvent, ControlOptions, Device, FlightMode, PadCaptureEvent};
use crate::mod_controls::ControlNotes;
use crate::gui_skin::{Element, Skinned};
use crate::menu::{despawn_all, MenuButton};
use crate::ui_theme::{self as theme, ButtonOpts, ButtonStyle, TEXT_DIM};
use crate::save::{GraphicsSettings, ModStorage, SaveStore};
use crate::state::AppState;

const RED: Color = theme::ERROR;
const YELLOW: Color = theme::WARN;
/// How much the mouse wheel moves the list per notch, in pixels.
const SCROLL_STEP: f32 = 28.0;
/// Longest search text accepted.
const SEARCH_MAX: usize = 40;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Player,
    Rendering,
    Sound,
    Saving,
    Language,
    Controls,
    Packs,
    Mods,
}

impl Section {
    pub const ALL: [Section; 8] = [Section::Player, Section::Rendering, Section::Sound, Section::Saving, Section::Language, Section::Controls, Section::Packs, Section::Mods];

    pub fn label(self) -> &'static str {
        match self {
            Section::Player => "Player",
            Section::Rendering => "Rendering",
            Section::Sound => "Sound",
            Section::Saving => "Saving",
            Section::Language => "Language",
            Section::Controls => "Controls",
            Section::Packs => "Packs",
            Section::Mods => "Mods",
        }
    }
}

/// The two halves of the Controls section: which keys do what, and how controls behave.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ControlsTab {
    Keybinds,
    Options,
}

/// What the screen is showing right now.
#[derive(Resource)]
pub struct SettingsView {
    pub section: Section,
    /// Which part of the Controls section is open.
    pub controls_tab: ControlsTab,
    pub search: String,
    /// Whether the Keybinds tab shows the keyboard/mouse bindings or the controller's.
    pub device: Device,
    /// The keybind list's scroll offset, kept so rebuilding doesn't jump it to the top.
    pub scroll: f32,
    /// The language group whose regional variants are showing in the Language list.
    pub lang_open: Option<String>,
}

impl Default for SettingsView {
    fn default() -> Self {
        Self { section: Section::Player, controls_tab: ControlsTab::Keybinds, search: String::new(), device: Device::Keyboard, scroll: 0.0, lang_open: None }
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
    ControlsTab(ControlsTab),
    SetDevice(Device),
    SetFlightMode(FlightMode),
    LookSpeedDelta(i32),
    DeadzoneDelta(i32),
    ToggleInvertLook,
    VolumeDelta(crate::sound::Slider, i32),
    RenderDistanceDelta(i32),
    ToggleDefaultModStorage,
    ToggleAutosave,
    AutosaveMinutesDelta(i32),
    LangGroup(String),
    PickLanguage(String),
    Rebind(String),
    Unbind(String),
    Reset(String),
    ResetAll,
    ClearSearch,
}

// ---------------------------------------------------------------------------
// Pure helpers (tested)
// ---------------------------------------------------------------------------

pub(crate) fn autosave_label(options: &crate::save::AutosaveOptions) -> String {
    crate::lang::tr(if options.enabled { "Autosave: On" } else { "Autosave: Off" })
}

pub(crate) fn autosave_interval_label(options: &crate::save::AutosaveOptions) -> String {
    let m = options.minutes;
    crate::lang::tr_n("autosave.every", m as i64, "Every {n} minute", "Every {n} minutes", &[])
}

pub(crate) fn mod_storage_label(storage: ModStorage) -> String {
    match storage {
        ModStorage::Instance => crate::lang::tr("New worlds keep mods: in the game folder"),
        ModStorage::World => crate::lang::tr("New worlds keep mods: in the world"),
    }
}

/// The search results grouped by category, in the screen's order (the game's own
/// headings first, then each mod's), leaving out headings with nothing under them.
pub(crate) fn keybind_rows(bindings: &Bindings, query: &str) -> Vec<(String, Vec<String>)> {
    keybind_rows_on(bindings, query, Device::Keyboard)
}

pub(crate) fn keybind_rows_on(bindings: &Bindings, query: &str, device: Device) -> Vec<(String, Vec<String>)> {
    let found = search_on(bindings, query, device);
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
    also_used_by_on(bindings, id, Device::Keyboard)
}

pub(crate) fn also_used_by_on(bindings: &Bindings, id: &str, device: Device) -> Option<String> {
    let others = bindings.conflicts_on(device, id);
    (!others.is_empty()).then(|| {
        let names: Vec<String> = others.iter().map(|o| crate::lang::tr(bindings.def(o).map(|d| d.label.as_str()).unwrap_or(o.as_str()))).collect();
        crate::lang::tr_args("Also used by: {names}", &[("names", &names.join(", "))])
    })
}

/// How many controls are in a conflict, as a sentence, or `None` when none are.
pub(crate) fn conflict_summary(bindings: &Bindings) -> Option<String> {
    conflict_summary_on(bindings, Device::Keyboard)
}

pub(crate) fn conflict_summary_on(bindings: &Bindings, device: Device) -> Option<String> {
    let n = bindings.conflicting_on(device).len();
    (n > 0).then(|| match device {
        Device::Keyboard => crate::lang::tr_args("{n} controls share a key with another (shown in red).", &[("n", &n.to_string())]),
        Device::Controller => crate::lang::tr_args("{n} controls share a button with another (shown in red).", &[("n", &n.to_string())]),
    })
}

/// How many controls clash on either device (the tab and section labels count both).
pub(crate) fn conflicts_anywhere(bindings: &Bindings) -> usize {
    bindings.conflicting().union(&bindings.pad_conflicting()).count()
}

/// The label on a section tab: Controls says how many controls clash.
pub(crate) fn tab_label(section: Section, bindings: &Bindings) -> String {
    let n = conflicts_anywhere(bindings);
    if section == Section::Controls && n > 0 {
        crate::lang::tr_args("Controls ({n} in conflict)", &[("n", &n.to_string())])
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
    commands.spawn((SettingsRoot, theme::screen_root())).with_children(|root| {
        theme::spawn_backdrop(root, 53);
        root.spawn(theme::panel(780.0)).with_children(|p| {
            theme::heading(p, "Settings");
            p.spawn((SettingsContent, Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: Val::Px(10.0), width: Val::Percent(100.0), ..default() }));
            theme::button(p, MenuButton::BackToMainMenu, "Back", ButtonOpts::new(ButtonStyle::Normal).size(220.0, 44.0));
        });
    });
}

fn dim(parent: &mut ChildSpawnerCommands, text: impl Into<String>, size: f32) {
    theme::hint(parent, text, size);
}

/// A settings button: `style`, size, optional selected look and accent border.
#[allow(clippy::too_many_arguments)]
fn sbutton(parent: &mut ChildSpawnerCommands, label: &str, action: SettingsButton, style: ButtonStyle, width: f32, height: f32, font: f32, selected: bool, text: Color, accent: Option<Color>) {
    theme::button(parent, action, label, ButtonOpts::new(style).size(width, height).font(font).selected(selected).text(text).accent(accent));
}

/// A value with a minus and a plus beside it.
fn stepper(parent: &mut ChildSpawnerCommands, label: &str, minus: SettingsButton, plus: SettingsButton) {
    parent.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(8.0), ..default() }).with_children(|row| {
        sbutton(row, "-", minus, ButtonStyle::Normal, 44.0, 38.0, 20.0, false, theme::TEXT, None);
        row.spawn((Node { width: Val::Px(220.0), justify_content: JustifyContent::Center, ..default() },)).with_children(|c| {
            c.spawn((Text::new(label), TextFont { font_size: 17.0, ..default() }, TextColor(theme::TEXT)));
        });
        sbutton(row, "+", plus, ButtonStyle::Normal, 44.0, 38.0, 20.0, false, theme::TEXT, None);
    });
}

fn rebuild_settings(
    mut commands: Commands,
    view: Res<SettingsView>,
    bindings: Res<Bindings>,
    capture: Res<Capture>,
    notes: Res<ControlNotes>,
    options: Res<ControlOptions>,
    sound: Res<crate::sound::SoundOptions>,
    autosave: Res<crate::save::AutosaveOptions>,
    languages: Res<crate::lang::Languages>,
    pack_selection: Res<crate::packs::PackSelection>,
    installed_packs: Res<crate::packs::InstalledPacks>,
    world_settings: Res<WorldSettings>,
    store: Res<SaveStore>,
    pad: Option<Res<PadState>>,
    mut pad_was_connected: Local<bool>,
    content: Query<(Entity, Option<&Children>), With<SettingsContent>>,
) {
    let Ok((root, children)) = content.single() else { return };
    let empty = children.is_none_or(|c| c.is_empty());
    // Plugging a controller in or out changes a line of text; nothing else about it matters here.
    let pad_state = pad.as_deref().cloned().unwrap_or_default();
    let pad_changed = *pad_was_connected != pad_state.connected;
    *pad_was_connected = pad_state.connected;
    if !(empty || pad_changed || view.is_changed() || bindings.is_changed() || capture.is_changed() || notes.is_changed() || options.is_changed() || sound.is_changed() || autosave.is_changed() || languages.is_changed() || pack_selection.is_changed() || installed_packs.is_changed() || world_settings.is_changed()) {
        return;
    }
    commands.entity(root).despawn_related::<Children>();
    commands.entity(root).with_children(|p| {
        // Section tabs.
        p.spawn(Node { column_gap: Val::Px(6.0), row_gap: Val::Px(6.0), flex_wrap: FlexWrap::Wrap, justify_content: JustifyContent::Center, ..default() }).with_children(|row| {
            for section in Section::ALL {
                let clash = section == Section::Controls && !bindings.conflicting().is_empty();
                let width = match section {
                    Section::Controls => 140.0,
                    Section::Packs => 84.0,
                    _ => 72.0,
                };
                // A tab grows to fit its label, so a long translation is never drawn outside its button.
                theme::button(
                    row,
                    SettingsButton::Section(section),
                    &tab_label(section, &bindings),
                    ButtonOpts::new(ButtonStyle::Choice).size(width, 38.0).font(14.0).selected(section == view.section).text(if clash { RED } else { theme::TEXT }).accent(clash.then_some(RED)).grow(),
                );
            }
        });
        // The selected section, in a box of fixed height so switching doesn't resize the panel.
        p.spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(430.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(10.0),
                padding: UiRect::all(Val::Px(24.0)),
                ..default()
            },
            theme::card_skin(false),
        ))
        .with_children(|body| match view.section {
            Section::Player => {
                body.spawn((Text::new(crate::lang::tr("Player")), TextFont { font_size: 20.0, ..default() }, TextColor(theme::TEXT)));
                dim(body, "Player settings - field of view, mouse sensitivity and the like - will live here.", 15.0);
            }
            Section::Rendering => {
                body.spawn((Text::new(crate::lang::tr("Video")), TextFont { font_size: 20.0, ..default() }, TextColor(theme::TEXT)));
                body.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(16.0), margin: UiRect::top(Val::Px(8.0)), ..default() }).with_children(|row| {
                    sbutton(row, "-", SettingsButton::RenderDistanceDelta(-1), ButtonStyle::Normal, 46.0, 46.0, 24.0, false, theme::TEXT, None);
                    row.spawn((Text::new(crate::lang::tr_args("Render distance: {n}", &[("n", &world_settings.render_distance.to_string())])), TextFont { font_size: 19.0, ..default() }, TextColor(theme::TEXT)));
                    sbutton(row, "+", SettingsButton::RenderDistanceDelta(1), ButtonStyle::Normal, 46.0, 46.0, 24.0, false, theme::TEXT, None);
                });
                dim(body, "How many chunks around you are drawn. Takes effect after restart.", 13.0);
                dim(body, "More video options (fullscreen, vsync, brightness) will appear here.", 14.0);
            }
            Section::Sound => {
                body.spawn((Text::new(crate::lang::tr("Sound")), TextFont { font_size: 20.0, ..default() }, TextColor(theme::TEXT)));
                for slider in crate::sound::Slider::ALL {
                    stepper(body, &format!("{}: {}%", crate::lang::tr(slider.label()), sound.get(slider)), SettingsButton::VolumeDelta(slider, -10), SettingsButton::VolumeDelta(slider, 10));
                }
                dim(body, "Resource packs and mods can replace any sound: see sounds/ in a pack.", 13.0);
            }
            Section::Saving => {
                body.spawn((Text::new(crate::lang::tr("Saving")), TextFont { font_size: 20.0, ..default() }, TextColor(theme::TEXT)));
                sbutton(body, &autosave_label(&autosave), SettingsButton::ToggleAutosave, ButtonStyle::Choice, 300.0, 42.0, 17.0, autosave.enabled, theme::TEXT, None);
                if autosave.enabled {
                    stepper(body, &autosave_interval_label(&autosave), SettingsButton::AutosaveMinutesDelta(-1), SettingsButton::AutosaveMinutesDelta(1));
                }
                dim(body, "Leaving a world, or closing the game with one open, always saves it.\nAn autosave is an extra snapshot taken in the background while you play, kept only until then: if the game ever crashes, the world is restored from it the next time you open it.", 13.0);
            }
            Section::Language => spawn_language(body, &languages, &view),
            Section::Packs => crate::pack_panel::spawn_pack_panel(body, crate::pack_panel::PackScope::Global, false, &pack_selection, &[], &installed_packs, None),
            Section::Mods => {
                body.spawn((Text::new(crate::lang::tr("Mods")), TextFont { font_size: 20.0, ..default() }, TextColor(theme::TEXT)));
                let storage = store.load_graphics_settings().mod_storage;
                sbutton(body, &mod_storage_label(storage), SettingsButton::ToggleDefaultModStorage, ButtonStyle::Normal, 440.0, 44.0, 16.0, false, theme::TEXT, None);
                dim(body, "Game folder: one copy of each mod, shared by every world (remove it and worlds using it won't open).\nIn the world: each world gets its own copy, so it can have mods the game doesn't.", 13.0);
            }
            Section::Controls => spawn_controls(body, &view, &bindings, &capture, &notes, &options, &pad_state),
        });
    });
}

/// The group containing the language in use, if that group has regional variants (it starts open).
fn open_group_of(languages: &crate::lang::Languages) -> Option<String> {
    languages.catalog.groups().into_iter().find(|g| g.members.len() > 1 && g.members.contains(&languages.code)).map(|g| g.name)
}

/// "Español (México)  -  100%": a language and how much of the game is translated into it.
pub(crate) fn language_label(languages: &crate::lang::Languages, code: &str) -> String {
    format!("{}  -  {}%", languages.display_name(code), languages.catalog.percent(code))
}

fn spawn_language(body: &mut ChildSpawnerCommands, languages: &crate::lang::Languages, view: &SettingsView) {
    body.spawn((Text::new(crate::lang::tr("Language")), TextFont { font_size: 20.0, ..default() }, TextColor(theme::TEXT)));
    dim(body, "Commands such as /give stay in English in every language. The percentage is how much of the game is translated.", 13.0);
    body.spawn((
        KeybindList,
        ScrollPosition { offset_x: 0.0, offset_y: view.scroll },
        Node {
            width: Val::Percent(100.0),
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(4.0),
            overflow: Overflow::scroll_y(),
            ..default()
        },
    ))
    .with_children(|list| {
        for group in languages.catalog.groups() {
            if let [only] = group.members.as_slice() {
                sbutton(list, &language_label(languages, only), SettingsButton::PickLanguage(only.clone()), ButtonStyle::Choice, 440.0, 36.0, 15.0, *only == languages.code, theme::TEXT, None);
                continue;
            }
            let open = view.lang_open.as_deref() == Some(group.name.as_str());
            let holds_current = group.members.contains(&languages.code);
            let label = format!("{} {}", if open { "v" } else { ">" }, languages.display_group(&group.name));
            sbutton(list, &label, SettingsButton::LangGroup(group.name.clone()), ButtonStyle::Choice, 440.0, 36.0, 15.0, holds_current && !open, theme::TEXT, None);
            if open {
                for code in &group.members {
                    sbutton(list, &language_label(languages, code), SettingsButton::PickLanguage(code.clone()), ButtonStyle::Choice, 400.0, 34.0, 14.0, *code == languages.code, theme::TEXT, None);
                }
            }
        }
        for note in &languages.notes {
            list.spawn((Text::new(format!("{note}")), TextFont { font_size: 12.0, ..default() }, TextColor(theme::WARN), Node { max_width: Val::Px(420.0), ..default() }));
        }
    });
}

fn spawn_controls(body: &mut ChildSpawnerCommands, view: &SettingsView, bindings: &Bindings, capture: &Capture, notes: &ControlNotes, options: &ControlOptions, pad: &PadState) {
    body.spawn(Node { column_gap: Val::Px(6.0), ..default() }).with_children(|row| {
        for (tab, label) in [(ControlsTab::Keybinds, "Keybinds"), (ControlsTab::Options, "Options")] {
            let clash = tab == ControlsTab::Keybinds && conflicts_anywhere(bindings) > 0;
            sbutton(row, label, SettingsButton::ControlsTab(tab), ButtonStyle::Choice, 130.0, 32.0, 15.0, tab == view.controls_tab, if clash { RED } else { theme::TEXT }, clash.then_some(RED));
        }
    });
    match view.controls_tab {
        ControlsTab::Keybinds => spawn_keybinds(body, view, bindings, capture, notes, pad),
        ControlsTab::Options => {
            body.spawn((Text::new(crate::lang::tr("Flight (Creative mode)")), TextFont { font_size: 18.0, ..default() }, TextColor(theme::ACCENT), Node { margin: UiRect::top(Val::Px(10.0)), ..default() }));
            body.spawn(Node { column_gap: Val::Px(10.0), ..default() }).with_children(|row| {
                for mode in FlightMode::ALL {
                    sbutton(row, mode.label(), SettingsButton::SetFlightMode(mode), ButtonStyle::Choice, 210.0, 42.0, 16.0, mode == options.flight_mode, theme::TEXT, None);
                }
            });
            dim(body, options.flight_mode.description(), 14.0);
            body.spawn((Text::new(crate::lang::tr("Controller sticks")), TextFont { font_size: 18.0, ..default() }, TextColor(theme::ACCENT), Node { margin: UiRect::top(Val::Px(14.0)), ..default() }));
            stepper(body, &crate::lang::tr_args("Look speed: {n}%", &[("n", &options.look_speed_pct.to_string())]), SettingsButton::LookSpeedDelta(-25), SettingsButton::LookSpeedDelta(25));
            stepper(body, &crate::lang::tr_args("Dead zone: {n}%", &[("n", &options.deadzone_pct.to_string())]), SettingsButton::DeadzoneDelta(-5), SettingsButton::DeadzoneDelta(5));
            sbutton(body, if options.invert_look_y { "Invert look (Y): On" } else { "Invert look (Y): Off" }, SettingsButton::ToggleInvertLook, ButtonStyle::Choice, 260.0, 38.0, 16.0, options.invert_look_y, theme::TEXT, None);
            dim(body, "Dead zone is how far a stick must move before it counts - raise it if the view drifts on its own.", 13.0);
        }
    }
}

fn spawn_keybinds(body: &mut ChildSpawnerCommands, view: &SettingsView, bindings: &Bindings, capture: &Capture, notes: &ControlNotes, pad: &PadState) {
    let device = view.device;
    // Which device these bindings are for.
    body.spawn(Node { column_gap: Val::Px(6.0), ..default() }).with_children(|row| {
        for d in Device::ALL {
            let clash = !bindings.conflicting_on(d).is_empty();
            sbutton(row, d.label(), SettingsButton::SetDevice(d), ButtonStyle::Choice, 190.0, 30.0, 14.0, d == device, if clash { RED } else { theme::TEXT }, clash.then_some(RED));
        }
    });
    // Search box + clear + reset all.
    body.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(8.0), width: Val::Percent(100.0), ..default() }).with_children(|row| {
        row.spawn((
            Node {
                flex_grow: 1.0,
                height: Val::Px(36.0),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(16.0)),
                ..default()
            },
            Skinned::new(if capture.active() { Element::Field } else { Element::FieldFocus }),
        ))
        .with_children(|b| {
            let (text, color) = if view.search.is_empty() && capture.active() {
                (crate::lang::tr("Search"), TEXT_DIM)
            } else if view.search.is_empty() {
                (crate::lang::tr_args("Search...  (try {prefix}:a)_", &[("prefix", if device == Device::Keyboard { "key" } else { "btn" })]), TEXT_DIM)
            } else if capture.active() {
                (view.search.clone(), theme::TEXT)
            } else {
                (format!("{}_", view.search), theme::TEXT)
            };
            b.spawn((Text::new(text), TextFont { font_size: 16.0, ..default() }, TextColor(color)));
        });
        if !view.search.is_empty() {
            sbutton(row, "Clear", SettingsButton::ClearSearch, ButtonStyle::Normal, 72.0, 36.0, 14.0, false, theme::TEXT, None);
        }
        sbutton(row, if device == Device::Keyboard { "Reset all" } else { "Reset all" }, SettingsButton::ResetAll, ButtonStyle::Normal, 112.0, 36.0, 14.0, false, theme::TEXT, None);
    });
    match device {
        Device::Keyboard => dim(body, "key:a = what uses A  -  key:ctrl+left = combos  -  is:conflict, is:unbound, is:changed", 12.0),
        Device::Controller => dim(
            body,
            if pad.connected {
                "btn:a = what uses A  -  btn:dpad  -  is:conflict, is:unbound, is:changed  -  the left stick moves and the right looks"
            } else {
                "No controller detected - plug one in to try it. You can still set buttons now.  -  btn:a, is:conflict"
            },
            12.0,
        ),
    }

    if let Some(summary) = conflict_summary_on(bindings, device) {
        body.spawn((Text::new(summary), TextFont { font_size: 14.0, ..default() }, TextColor(RED)));
    }
    if let Some(id) = &capture.target {
        let label = bindings.def(id).map(|d| d.label.as_str()).unwrap_or(id.as_str());
        body.spawn((
            Text::new(match device {
                Device::Keyboard => crate::lang::tr_args("Binding \"{label}\": press a key, click, or a combo like Ctrl+Alt+A. Release Ctrl/Alt/Shift alone to bind it by itself. Esc cancels.", &[("label", &crate::lang::tr(label))]),
                Device::Controller => crate::lang::tr_args("Binding \"{label}\": press a controller button. Esc on the keyboard cancels.", &[("label", &crate::lang::tr(label))]),
            }),
            TextFont { font_size: 14.0, ..default() },
            TextColor(YELLOW),
            Node { max_width: Val::Px(700.0), ..default() },
        ));
    }

    let rows = keybind_rows_on(bindings, &view.search, device);
    let conflicting = bindings.conflicting_on(device);
    body.spawn((
        KeybindList,
        ScrollPosition { offset_x: 0.0, offset_y: view.scroll },
        Node {
            width: Val::Percent(100.0),
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(2.0),
            overflow: Overflow::scroll_y(),
            padding: UiRect::right(Val::Px(8.0)),
            ..default()
        },
    ))
    .with_children(|list| {
        if rows.is_empty() {
            dim(list, "No controls match.", 15.0);
        }
        for (category, ids) in rows {
            list.spawn((Text::new(crate::lang::tr(&category)), TextFont { font_size: 17.0, ..default() }, TextColor(theme::ACCENT), Node { margin: UiRect::new(Val::Px(0.0), Val::Px(0.0), Val::Px(10.0), Val::Px(4.0)), ..default() }));
            for (n, id) in ids.into_iter().enumerate() {
                let Some(def) = bindings.def(&id) else { continue };
                let clash = conflicting.contains(&id);
                let listening = capture.target.as_deref() == Some(id.as_str());
                list.spawn((
                    Node { flex_direction: FlexDirection::Column, padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)), ..default() },
                    BackgroundColor(if n % 2 == 0 { Color::srgba(0.0, 0.0, 0.0, 0.22) } else { Color::NONE }),
                ))
                .with_children(|entry| {
                    entry.spawn(Node { align_items: AlignItems::Center, justify_content: JustifyContent::SpaceBetween, column_gap: Val::Px(8.0), ..default() }).with_children(|row| {
                        row.spawn((Text::new(crate::lang::tr(&def.label)), TextFont { font_size: 16.0, ..default() }, TextColor(if clash { RED } else { theme::TEXT })));
                        row.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(6.0), ..default() }).with_children(|buttons| {
                            let shown = match device {
                                Device::Keyboard => bindings.get(&id).map(|c| c.display()),
                                Device::Controller => bindings.pad_get(&id).map(|b| b.label().to_string()),
                            };
                            let (text, color) = match (listening, shown) {
                                (true, _) => (if device == Device::Keyboard { "Press a key..." } else { "Press a button..." }.to_string(), YELLOW),
                                (false, Some(shown)) => (shown, if clash { RED } else { theme::TEXT }),
                                (false, None) => ("Unbound".to_string(), TEXT_DIM),
                            };
                            let accent = if listening { Some(YELLOW) } else if clash { Some(RED) } else { None };
                            let glyph = (device == Device::Controller && !listening).then(|| bindings.pad_get(&id)).flatten();
                            theme::button(buttons, SettingsButton::Rebind(id.clone()), &text, ButtonOpts::new(ButtonStyle::Key).size(200.0, 32.0).font(15.0).text(color).accent(accent).glyph(glyph));
                            let is_default = if device == Device::Keyboard { bindings.is_default(&id) } else { bindings.pad_is_default(&id) };
                            let changed = if is_default { TEXT_DIM } else { theme::TEXT };
                            sbutton(buttons, "Reset", SettingsButton::Reset(id.clone()), ButtonStyle::Normal, 70.0, 32.0, 13.0, false, changed, None);
                            let is_bound = if device == Device::Keyboard { bindings.get(&id).is_some() } else { bindings.pad_get(&id).is_some() };
                            let bound = if is_bound { theme::TEXT } else { TEXT_DIM };
                            sbutton(buttons, "Unbind", SettingsButton::Unbind(id.clone()), ButtonStyle::Normal, 104.0, 32.0, 13.0, false, bound, None);
                        });
                    });
                    if let Some(note) = also_used_by_on(bindings, &id, device) {
                        entry.spawn((Text::new(note), TextFont { font_size: 12.0, ..default() }, TextColor(RED)));
                    }
                    if let Some(command) = &def.command {
                        entry.spawn((Text::new(crate::lang::tr_args("runs {command}", &[("command", command)])), TextFont { font_size: 12.0, ..default() }, TextColor(TEXT_DIM)));
                    }
                });
            }
        }
    });
    for note in &notes.0 {
        body.spawn((Text::new(crate::lang::tr_args("Mod control skipped: {note}", &[("note", note)])), TextFont { font_size: 12.0, ..default() }, TextColor(theme::WARN), Node { max_width: Val::Px(700.0), ..default() }));
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Typing goes into the search box while the Keybinds section is open. Typed
/// text is read from the keyboard events (not key codes), same as every other
/// text field here. Ignored - and the events thrown away - while a key is being
/// captured, so the key being bound never also lands in the search.
fn search_input(mut events: EventReader<KeyboardInput>, capture: Res<Capture>, mut view: ResMut<SettingsView>) {
    if view.section != Section::Controls || view.controls_tab != ControlsTab::Keybinds || capture.active() {
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
    mut options: ResMut<ControlOptions>,
    mut sound: ResMut<crate::sound::SoundOptions>,
    mut autosave: ResMut<crate::save::AutosaveOptions>,
    mut languages: ResMut<crate::lang::Languages>,
    installed_mods: Res<crate::mods::InstalledMods>,
    mut installed_packs: ResMut<crate::packs::InstalledPacks>,
    pack_folder: Res<crate::packs::PackFolder>,
    store: Res<SaveStore>,
) {
    for (interaction, button) in &interactions {
        if *interaction != Interaction::Pressed || capture.active() {
            continue;
        }
        match button.clone() {
            SettingsButton::Section(section) => {
                if section == Section::Packs {
                    installed_packs.0 = crate::packs::scan_packs(&pack_folder.0);
                }
                if section == Section::Language {
                    // A mod may have been added since the game started.
                    languages.reload(&crate::blocks::find_data_dir("lang"), &installed_mods.0);
                    view.lang_open = open_group_of(&languages);
                }
                view.section = section;
                view.scroll = 0.0;
            }
            SettingsButton::ControlsTab(tab) => {
                view.controls_tab = tab;
                view.scroll = 0.0;
            }
            SettingsButton::SetDevice(device) => {
                view.device = device;
                view.scroll = 0.0;
            }
            SettingsButton::SetFlightMode(mode) => {
                options.flight_mode = mode;
                let _ = store.save_control_options(&options);
            }
            SettingsButton::LookSpeedDelta(d) => {
                options.nudge_look_speed(d);
                let _ = store.save_control_options(&options);
            }
            SettingsButton::DeadzoneDelta(d) => {
                options.nudge_deadzone(d);
                let _ = store.save_control_options(&options);
            }
            SettingsButton::VolumeDelta(slider, d) => {
                sound.nudge(slider, d);
                let _ = store.save_sound_options(&sound);
            }
            SettingsButton::LangGroup(name) => {
                view.lang_open = if view.lang_open.as_deref() == Some(name.as_str()) { None } else { Some(name) };
            }
            SettingsButton::PickLanguage(code) => {
                if languages.select(&code) {
                    let _ = store.save_language(&code);
                }
            }
            SettingsButton::ToggleAutosave => {
                autosave.enabled = !autosave.enabled;
                let _ = store.save_autosave_options(&autosave);
            }
            SettingsButton::AutosaveMinutesDelta(d) => {
                autosave.nudge_minutes(d);
                let _ = store.save_autosave_options(&autosave);
            }
            SettingsButton::ToggleInvertLook => {
                options.invert_look_y = !options.invert_look_y;
                let _ = store.save_control_options(&options);
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
            SettingsButton::Rebind(id) => match view.device {
                Device::Keyboard => capture.start(id),
                Device::Controller => capture.start_pad(id),
            },
            SettingsButton::Unbind(id) => {
                match view.device {
                    Device::Keyboard => bindings.set(id, None),
                    Device::Controller => bindings.pad_set(id, None),
                }
                save_bindings(&store, &bindings);
            }
            SettingsButton::Reset(id) => {
                match view.device {
                    Device::Keyboard => bindings.reset(id),
                    Device::Controller => bindings.pad_reset(id),
                }
                save_bindings(&store, &bindings);
            }
            SettingsButton::ResetAll => {
                match view.device {
                    Device::Keyboard => bindings.reset_all_keyboard(),
                    Device::Controller => bindings.reset_all_pads(),
                }
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
    pad: Option<Res<PadState>>,
) {
    let Some(target) = capture.target.clone() else { return };
    if capture.pad {
        let state = pad.as_deref().cloned().unwrap_or_default();
        match capture.bypass_change_detection().step_pad(&keys, &state) {
            Some(PadCaptureEvent::Chosen(button)) => {
                bindings.pad_set(target, Some(button));
                save_bindings(&store, &bindings);
                capture.set_changed();
            }
            Some(PadCaptureEvent::Cancelled) => capture.set_changed(),
            None => {}
        }
        return;
    }
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
        app.init_resource::<crate::sound::SoundOptions>().init_resource::<crate::save::AutosaveOptions>().init_resource::<crate::lang::Languages>().init_resource::<crate::mods::InstalledMods>().init_resource::<crate::packs::PackFolder>();
        if !app.is_plugin_added::<crate::pack_panel::PackPanelPlugin>() {
            app.add_plugins(crate::pack_panel::PackPanelPlugin);
        }
        app.init_resource::<crate::packs::PackSelection>().init_resource::<crate::packs::InstalledPacks>();
        if !app.is_plugin_added::<theme::ThemePlugin>() {
            app.add_plugins(theme::ThemePlugin);
        }
        app.init_resource::<SettingsView>()
            .init_resource::<Capture>()
            .add_systems(OnEnter(AppState::Settings), setup_settings)
            .add_systems(OnExit(AppState::Settings), despawn_all::<SettingsRoot>)
            .add_systems(
                Update,
                (
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
        assert_eq!(tab_label(Section::Controls, &b), "Controls (2 in conflict)");
        assert_eq!(tab_label(Section::Rendering, &b), "Rendering", "only the Controls tab warns");
        assert_eq!(tab_label(Section::Controls, &Bindings::default()), "Controls");
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

        fn click_pack(&mut self, button: crate::pack_panel::PackButton) {
            let mut q = self.app.world_mut().query::<(Entity, &crate::pack_panel::PackButton)>();
            let entity = q.iter(self.app.world()).find(|(_, b)| **b == button).map(|(e, _)| e).unwrap_or_else(|| panic!("no {button:?} on screen"));
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
    fn volume_buttons_change_and_save_the_sliders() {
        let mut s = screen();
        s.click(SettingsButton::Section(Section::Sound));
        for _ in 0..3 {
            s.click(SettingsButton::VolumeDelta(crate::sound::Slider::Blocks, -10));
        }
        assert_eq!(s.app.world().resource::<crate::sound::SoundOptions>().blocks, 70);
        let saved = s.app.world().resource::<SaveStore>().load_sound_options();
        assert_eq!(saved.blocks, 70, "saved to disk");
        assert!(s.texts().iter().any(|t| t == "Blocks: 70%"));
    }

    #[test]
    fn the_language_list_groups_variants_shows_percentages_and_picks_and_saves() {
        let mut s = screen();
        // A small catalog: English, a two-variant group and a lone language, one of them half translated.
        let files = |code: &str, json: &str| crate::lang::parse_file(code, json).unwrap().0;
        let catalog = crate::lang::Catalog::build(vec![
            files("en", r#"{"A": "a", "B": "b"}"#),
            files("es-ES", r#"{"_meta": {"name": "Español (España)", "group": "Español"}, "A": "x", "B": "y"}"#),
            files("es-MX", r#"{"_meta": {"name": "Español (México)", "group": "Español", "fallback": "es-ES"}}"#),
            files("hi", r#"{"_meta": {"name": "Hindi"}, "A": "x"}"#),
        ]);
        s.app.world_mut().resource_mut::<crate::lang::Languages>().catalog = std::sync::Arc::new(catalog);
        // Opening the tab by clicking re-reads the real files; open it directly so the test's catalog stays.
        s.app.world_mut().resource_mut::<SettingsView>().section = Section::Language;
        s.app.update();
        s.app.update();
        let texts = s.texts();
        assert!(texts.iter().any(|t| t == "English (United States)  -  100%"), "English is listed directly: {texts:?}");
        assert!(texts.iter().any(|t| t == "Hindi  -  50%"), "a lone language shows its own percentage");
        assert!(texts.iter().any(|t| t == "> Español"), "variants sit under one heading");
        assert!(!texts.iter().any(|t| t.starts_with("Español (")), "collapsed: the variants are not listed yet");
        // Opening the heading lists both variants, with what they inherit counted.
        s.click(SettingsButton::LangGroup("Español".into()));
        let texts = s.texts();
        assert!(texts.iter().any(|t| t == "v Español"));
        assert!(texts.iter().any(|t| t == "Español (España)  -  100%"));
        assert!(texts.iter().any(|t| t == "Español (México)  -  100%"), "inherits from España");
        // Clicking it again closes it.
        s.click(SettingsButton::LangGroup("Español".into()));
        assert!(!s.texts().iter().any(|t| t.starts_with("Español (")));
        // Picking a language chooses it and saves it.
        s.click(SettingsButton::LangGroup("Español".into()));
        s.click(SettingsButton::PickLanguage("es-ES".into()));
        assert_eq!(s.app.world().resource::<crate::lang::Languages>().code, "es-ES");
        assert_eq!(s.app.world().resource::<SaveStore>().load_language(), "es-ES");
    }

    #[test]
    fn each_language_is_listed_in_its_own_name_and_in_the_one_in_use() {
        let files = |code: &str, json: &str| crate::lang::parse_file(code, json).unwrap().0;
        let catalog = crate::lang::Catalog::build(vec![
            files("en", r#"{"lang.name.en": "English (United States)", "lang.name.es-ES": "Spanish (Spain)", "lang.group.Español": "Spanish"}"#),
            files("es-ES", r#"{"_meta": {"name": "Español (España)", "group": "Español"}, "lang.name.en": "Inglés (Estados Unidos)", "lang.name.es-ES": "Español (España)", "lang.group.Español": "Español"}"#),
            files("tlh", r#"{"_meta": {"name": "tlhIngan Hol"}}"#),
        ]);
        let mut languages = crate::lang::Languages { catalog: std::sync::Arc::new(catalog), ..Default::default() };
        // English in use: native name, then English's name for it.
        assert_eq!(languages.display_name("es-ES"), "Español (España) / Spanish (Spain)");
        assert_eq!(languages.display_group("Español"), "Español / Spanish");
        assert_eq!(languages.display_name("en"), "English (United States)", "the same in both: shown once");
        assert_eq!(languages.display_name("tlh"), "tlhIngan Hol", "a language nobody has named is just its own name");
        // Spanish in use: English keeps its own name first, then Spanish's word for it.
        languages.select("es-ES");
        assert_eq!(languages.display_name("en"), "English (United States) / Inglés (Estados Unidos)");
        assert_eq!(languages.display_name("es-ES"), "Español (España)", "Spanish in Spanish: once");
        assert_eq!(languages.display_group("Español"), "Español");
    }

    #[test]
    fn the_saving_section_toggles_autosave_and_sets_the_interval_and_saves_both() {
        let mut s = screen();
        s.click(SettingsButton::Section(Section::Saving));
        assert!(s.texts().iter().any(|t| t == "Autosave: On"));
        assert!(s.texts().iter().any(|t| t == "Every 5 minutes"));
        for _ in 0..2 {
            s.click(SettingsButton::AutosaveMinutesDelta(1));
        }
        assert!(s.texts().iter().any(|t| t == "Every 7 minutes"));
        for _ in 0..9 {
            s.click(SettingsButton::AutosaveMinutesDelta(-1));
        }
        assert_eq!(s.app.world().resource::<crate::save::AutosaveOptions>().minutes, 1);
        assert!(s.texts().iter().any(|t| t == "Every 1 minute"));
        s.click(SettingsButton::ToggleAutosave);
        let saved = s.app.world().resource::<SaveStore>().load_autosave_options();
        assert_eq!(saved, crate::save::AutosaveOptions { enabled: false, minutes: 1 }, "saved to disk");
        assert!(s.texts().iter().any(|t| t == "Autosave: Off"));
        assert!(!s.texts().iter().any(|t| t.starts_with("Every ")), "no interval to set while it is off");
    }

    #[test]
    fn the_packs_section_enables_orders_and_saves_packs() {
        let mut s = screen();
        s.app.world_mut().resource_mut::<crate::packs::InstalledPacks>().0 = ["a", "b"]
            .iter()
            .map(|f| crate::packs::PackInfo { folder: f.to_string(), name: f.to_string(), description: String::new(), path: f.into(), warning: None })
            .collect();
        s.click(SettingsButton::Section(Section::Packs));
        s.app.world_mut().resource_mut::<crate::packs::InstalledPacks>().0 = ["a", "b"]
            .iter()
            .map(|f| crate::packs::PackInfo { folder: f.to_string(), name: f.to_string(), description: String::new(), path: f.into(), warning: None })
            .collect();
        s.app.update();
        s.click_pack(crate::pack_panel::PackButton::Toggle("a".into()));
        s.click_pack(crate::pack_panel::PackButton::Toggle("b".into()));
        assert_eq!(s.app.world().resource::<crate::packs::PackSelection>().enabled, ["b", "a"], "newest goes on top");
        s.click_pack(crate::pack_panel::PackButton::Shift("a".into(), true));
        assert_eq!(s.app.world().resource::<crate::packs::PackSelection>().enabled, ["a", "b"]);
        assert_eq!(s.app.world().resource::<SaveStore>().load_pack_selection().enabled, ["a", "b"], "saved");
        s.click_pack(crate::pack_panel::PackButton::Toggle("a".into()));
        assert_eq!(s.app.world().resource::<crate::packs::PackSelection>().enabled, ["b"]);
    }

    #[test]
    fn each_section_shows_its_own_body() {
        let mut s = screen();
        s.click(SettingsButton::Section(Section::Rendering));
        assert!(s.texts().iter().any(|t| t.starts_with("Render distance")));
        s.click(SettingsButton::Section(Section::Sound));
        assert!(s.texts().iter().any(|t| t.starts_with("Master volume")));
        s.click(SettingsButton::Section(Section::Mods));
        assert!(s.texts().iter().any(|t| t.starts_with("New worlds keep mods")));
        s.click(SettingsButton::Section(Section::Controls));
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
        s.click(SettingsButton::Section(Section::Controls));
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
        s.click(SettingsButton::Section(Section::Controls));
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
        s.click(SettingsButton::Section(Section::Controls));
        s.click(SettingsButton::Rebind(id(Action::Jump)));
        s.click(SettingsButton::Section(Section::Sound));
        assert_eq!(s.app.world().resource::<SettingsView>().section, Section::Controls);
        s.click(SettingsButton::Reset(id(Action::Chat)));
        assert!(s.app.world().resource::<Capture>().active(), "still waiting for the key");
    }

    #[test]
    fn unbind_and_reset_work_and_reset_all_restores_everything() {
        let mut s = screen();
        s.click(SettingsButton::Section(Section::Controls));
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
        s.click(SettingsButton::Section(Section::Controls));
        s.app.world_mut().resource_mut::<Bindings>().set(Action::Chat, Chord::parse("E"));
        s.app.update();
        let texts = s.texts();
        assert!(texts.iter().any(|t| t == "Also used by: Open inventory"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "Also used by: Open chat"));
        assert!(texts.iter().any(|t| t == "Controls (2 in conflict)"), "the tab warns too");
        // The red is real colour on the rows' text, not just wording.
        let mut q = s.app.world_mut().query::<(&Text, &TextColor)>();
        let red_labels: Vec<_> = q.iter(s.app.world()).filter(|(_, c)| c.0 == RED).map(|(t, _)| t.0.clone()).collect();
        assert!(red_labels.iter().any(|t| t == "Open chat") && red_labels.iter().any(|t| t == "Open inventory"), "{red_labels:?}");
    }

    #[test]
    fn typing_searches_and_key_queries_find_a_key() {
        let mut s = screen();
        s.click(SettingsButton::Section(Section::Controls));
        s.type_text("key:a");
        assert_eq!(s.app.world().resource::<SettingsView>().search, "key:a");
        assert_eq!(s.rebind_buttons(), vec![id(Action::Left)], "only the control on A is listed");
        s.click(SettingsButton::ClearSearch);
        assert_eq!(s.rebind_buttons().len(), Action::ALL.len());
        s.type_text("hotbar");
        assert_eq!(s.rebind_buttons().len(), 11, "nine slots plus previous/next");
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
        s.click(SettingsButton::Section(Section::Controls));
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
        s.click(SettingsButton::Section(Section::Controls));
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
        s.click(SettingsButton::Section(Section::Controls));
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
        s.click(SettingsButton::Section(Section::Controls));
        assert!(!s.rebind_buttons().contains(&"weather:noon".to_string()), "gone from the screen");
        // Any later change (here: another rebind) saves the file again; the removed mod's key must still be in it.
        s.click(SettingsButton::Unbind("chat".into()));
        let saved = s.app.world().resource::<SaveStore>().load_keybinds();
        assert_eq!(saved["weather:noon"], "F9", "taking a mod out must not cost the player their key for it");
    }


    // ---- the Controls options (flight mode) ---------------------------------

    #[test]
    fn controls_opens_on_the_keybinds_and_has_an_options_half() {
        let mut s = screen();
        s.click(SettingsButton::Section(Section::Controls));
        assert_eq!(s.app.world().resource::<SettingsView>().controls_tab, ControlsTab::Keybinds);
        assert!(s.texts().iter().any(|t| t == "Controls"), "the section tab is called Controls");
        assert!(!s.texts().iter().any(|t| t == "Keybinds (0 in conflict)"));
        assert_eq!(s.rebind_buttons().len(), Action::ALL.len());
        s.click(SettingsButton::ControlsTab(ControlsTab::Options));
        assert!(s.rebind_buttons().is_empty(), "the options half has no key list");
        let texts = s.texts();
        assert!(texts.iter().any(|t| t == "Flight (Creative mode)"));
        assert!(texts.iter().any(|t| t == "Toggle key") && texts.iter().any(|t| t == "Double-tap jump"));
        assert!(texts.iter().any(|t| t == FlightMode::Toggle.description()), "the current choice is explained");
        s.click(SettingsButton::ControlsTab(ControlsTab::Keybinds));
        assert_eq!(s.rebind_buttons().len(), Action::ALL.len(), "and back");
    }

    #[test]
    fn choosing_double_tap_jump_changes_the_resource_explains_it_and_saves_it() {
        let mut s = screen();
        s.click(SettingsButton::Section(Section::Controls));
        s.click(SettingsButton::ControlsTab(ControlsTab::Options));
        assert_eq!(s.app.world().resource::<ControlOptions>().flight_mode, FlightMode::Toggle, "toggle is the default");
        s.click(SettingsButton::SetFlightMode(FlightMode::DoubleTapJump));
        assert_eq!(s.app.world().resource::<ControlOptions>().flight_mode, FlightMode::DoubleTapJump);
        assert!(s.texts().iter().any(|t| t == FlightMode::DoubleTapJump.description()));
        assert_eq!(s.app.world().resource::<SaveStore>().load_control_options().flight_mode, FlightMode::DoubleTapJump, "written to disk");
        s.click(SettingsButton::SetFlightMode(FlightMode::Toggle));
        assert_eq!(s.app.world().resource::<SaveStore>().load_control_options().flight_mode, FlightMode::Toggle);
    }

    #[test]
    fn typing_on_the_options_half_does_not_go_into_the_hidden_search() {
        let mut s = screen();
        s.click(SettingsButton::Section(Section::Controls));
        s.click(SettingsButton::ControlsTab(ControlsTab::Options));
        s.type_text("abc");
        assert_eq!(s.app.world().resource::<SettingsView>().search, "");
    }

    #[test]
    fn the_sections_are_the_ones_asked_for() {
        let labels: Vec<_> = Section::ALL.iter().map(|s| s.label()).collect();
        assert_eq!(labels, ["Player", "Rendering", "Sound", "Saving", "Language", "Controls", "Packs", "Mods"]);
    }

    #[test]
    fn mod_storage_has_a_label_for_each_choice() {
        assert!(mod_storage_label(ModStorage::Instance).contains("game folder"));
        assert!(mod_storage_label(ModStorage::World).contains("in the world"));
    }

    #[test]
    fn the_device_switch_shows_controller_buttons_and_rebinds_them_separately() {
        use crate::gamepad::PadButton;
        let mut s = screen();
        s.click(SettingsButton::Section(Section::Controls));
        assert!(s.texts().iter().any(|t| t == "Space"), "starts on the keyboard");
        s.click(SettingsButton::SetDevice(Device::Controller));
        let texts = s.texts();
        assert!(texts.iter().any(|t| t == "A (Cross)"), "Jump shows its controller button: {texts:?}");
        assert!(!texts.iter().any(|t| t == "Space"), "and not its key");

        s.click(SettingsButton::Rebind(id(Action::Jump)));
        assert!(s.app.world().resource::<Capture>().pad, "waiting for a button, not a key");
        assert!(s.texts().iter().any(|t| t == "Press a button..."));
        // A real controller entity, so the path through `read_pads` is exercised too.
        let mut gamepad = Gamepad::default();
        gamepad.digital_mut().press(GamepadButton::West);
        s.app.world_mut().spawn(gamepad);
        s.app.update();
        assert_eq!(s.bindings().pad_get(Action::Jump), Some(PadButton::West));
        assert_eq!(s.bindings().get(Action::Jump), crate::keybinds::Chord::parse("Space"), "the key is untouched");
        assert_eq!(s.app.world().resource::<SaveStore>().load_keybinds()["@pad:jump"], "west", "saved");
        assert!(s.texts().iter().any(|t| t == "X (Square)"));

        s.click(SettingsButton::Unbind(id(Action::Jump)));
        assert_eq!(s.bindings().pad_get(Action::Jump), None);
        s.click(SettingsButton::Reset(id(Action::Jump)));
        assert_eq!(s.bindings().pad_get(Action::Jump), Some(PadButton::South));
    }

    #[test]
    fn a_controller_clash_is_flagged_on_the_controller_tab_and_the_section_label() {
        use crate::gamepad::PadButton;
        let mut s = screen();
        s.app.world_mut().resource_mut::<Bindings>().pad_set(Action::Sneak, Some(PadButton::South));
        s.click(SettingsButton::Section(Section::Controls));
        assert!(s.texts().iter().any(|t| t.starts_with("Controls (2 in conflict)")), "counted even while looking at the keyboard");
        s.click(SettingsButton::SetDevice(Device::Controller));
        assert!(s.texts().iter().any(|t| t.contains("share a button")));
        assert!(s.texts().iter().any(|t| t.starts_with("Also used by")));
    }

    #[test]
    fn stick_tuning_steps_within_its_range_shows_and_saves() {
        let mut s = screen();
        s.click(SettingsButton::Section(Section::Controls));
        s.click(SettingsButton::ControlsTab(ControlsTab::Options));
        assert!(s.texts().iter().any(|t| t == "Look speed: 100%") && s.texts().iter().any(|t| t == "Dead zone: 20%"));
        s.click(SettingsButton::LookSpeedDelta(25));
        s.click(SettingsButton::DeadzoneDelta(-5));
        s.click(SettingsButton::ToggleInvertLook);
        let o = *s.app.world().resource::<ControlOptions>();
        assert_eq!((o.look_speed_pct, o.deadzone_pct, o.invert_look_y), (125, 15, true));
        assert!(s.texts().iter().any(|t| t == "Look speed: 125%") && s.texts().iter().any(|t| t == "Invert look (Y): On"));
        let saved = s.app.world().resource::<SaveStore>().load_control_options();
        assert_eq!(saved, o, "written to disk");
        for _ in 0..40 {
            s.click(SettingsButton::LookSpeedDelta(-25));
            s.click(SettingsButton::DeadzoneDelta(5));
        }
        let o = *s.app.world().resource::<ControlOptions>();
        assert_eq!((o.look_speed_pct, o.deadzone_pct), (ControlOptions::LOOK_SPEED_RANGE.0, ControlOptions::DEADZONE_RANGE.1), "stops at the ends");
    }
}
