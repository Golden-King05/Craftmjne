//! Rebindable controls: every thing the player can *do* is an [`Action`], each
//! bound to one [`Chord`] (a key or mouse button plus optional Ctrl/Alt/Shift),
//! and game systems ask [`Actions`] "is Jump held?" instead of "is Space held?".
//!
//! The pieces, all pure and unit-tested except the system glue at the bottom:
//! - [`Chord`] - `Ctrl+Alt+A`, `Ctrl+Left Click`, a lone `Shift`. Parsed from and
//!   shown as text, so a settings file and the search box share one format.
//! - [`Bindings`] - the action -> chord map, its conflict check, and the text
//!   [`search`] behind the Keybinds screen (`jump`, `key:a`, `key:ctrl+left`,
//!   `is:conflict`).
//! - [`Capture`] - the "press the new combo now" state machine.
//!
//! **Modifiers.** A chord with no modifiers still fires while Ctrl/Alt/Shift are
//! held (walking while sprinting), but yields to a *more specific* chord on the
//! same input that is also held: with `Q` = drop one and `Ctrl+Q` = drop stack,
//! Ctrl+Q drops the stack and does not also drop one. That is the only
//! disambiguation; two actions with the *identical* chord are a conflict, shown
//! in red, and both fire.
//!
//! **Not bindable on purpose:** menu navigation (Esc closes an open screen,
//! Enter/Tab/Backspace in text boxes) and the inventory's Shift-click, which are
//! conventions of the screens themselves rather than game actions.
//!
//! Only the *differences from the defaults* are saved (`keybinds.json`), so a
//! default changed in a later version still reaches players who never touched it.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::gamepad::{move_keys, PadButton, PadState};
use crate::save::SaveStore;

// ---------------------------------------------------------------------------
// Inputs and names
// ---------------------------------------------------------------------------

/// What a chord is built on: a keyboard key or a mouse button.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Input {
    Key(KeyCode),
    Mouse(MouseButton),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Modifier {
    Ctrl,
    Alt,
    Shift,
}

/// `(key, name shown to the player, extra spellings accepted when typing)`.
/// The one table both directions of naming come from. Modifier keys are listed
/// by their *left* variant; the right one is folded into it by [`canon_key`].
const KEYS: &[(KeyCode, &str, &[&str])] = &[
    (KeyCode::KeyA, "A", &[]),
    (KeyCode::KeyB, "B", &[]),
    (KeyCode::KeyC, "C", &[]),
    (KeyCode::KeyD, "D", &[]),
    (KeyCode::KeyE, "E", &[]),
    (KeyCode::KeyF, "F", &[]),
    (KeyCode::KeyG, "G", &[]),
    (KeyCode::KeyH, "H", &[]),
    (KeyCode::KeyI, "I", &[]),
    (KeyCode::KeyJ, "J", &[]),
    (KeyCode::KeyK, "K", &[]),
    (KeyCode::KeyL, "L", &[]),
    (KeyCode::KeyM, "M", &[]),
    (KeyCode::KeyN, "N", &[]),
    (KeyCode::KeyO, "O", &[]),
    (KeyCode::KeyP, "P", &[]),
    (KeyCode::KeyQ, "Q", &[]),
    (KeyCode::KeyR, "R", &[]),
    (KeyCode::KeyS, "S", &[]),
    (KeyCode::KeyT, "T", &[]),
    (KeyCode::KeyU, "U", &[]),
    (KeyCode::KeyV, "V", &[]),
    (KeyCode::KeyW, "W", &[]),
    (KeyCode::KeyX, "X", &[]),
    (KeyCode::KeyY, "Y", &[]),
    (KeyCode::KeyZ, "Z", &[]),
    (KeyCode::Digit0, "0", &[]),
    (KeyCode::Digit1, "1", &[]),
    (KeyCode::Digit2, "2", &[]),
    (KeyCode::Digit3, "3", &[]),
    (KeyCode::Digit4, "4", &[]),
    (KeyCode::Digit5, "5", &[]),
    (KeyCode::Digit6, "6", &[]),
    (KeyCode::Digit7, "7", &[]),
    (KeyCode::Digit8, "8", &[]),
    (KeyCode::Digit9, "9", &[]),
    (KeyCode::F1, "F1", &[]),
    (KeyCode::F2, "F2", &[]),
    (KeyCode::F3, "F3", &[]),
    (KeyCode::F4, "F4", &[]),
    (KeyCode::F5, "F5", &[]),
    (KeyCode::F6, "F6", &[]),
    (KeyCode::F7, "F7", &[]),
    (KeyCode::F8, "F8", &[]),
    (KeyCode::F9, "F9", &[]),
    (KeyCode::F10, "F10", &[]),
    (KeyCode::F11, "F11", &[]),
    (KeyCode::F12, "F12", &[]),
    (KeyCode::Numpad0, "Num 0", &["num0", "numpad0"]),
    (KeyCode::Numpad1, "Num 1", &["num1", "numpad1"]),
    (KeyCode::Numpad2, "Num 2", &["num2", "numpad2"]),
    (KeyCode::Numpad3, "Num 3", &["num3", "numpad3"]),
    (KeyCode::Numpad4, "Num 4", &["num4", "numpad4"]),
    (KeyCode::Numpad5, "Num 5", &["num5", "numpad5"]),
    (KeyCode::Numpad6, "Num 6", &["num6", "numpad6"]),
    (KeyCode::Numpad7, "Num 7", &["num7", "numpad7"]),
    (KeyCode::Numpad8, "Num 8", &["num8", "numpad8"]),
    (KeyCode::Numpad9, "Num 9", &["num9", "numpad9"]),
    (KeyCode::ArrowUp, "Up Arrow", &["arrowup"]),
    (KeyCode::ArrowDown, "Down Arrow", &["arrowdown"]),
    (KeyCode::ArrowLeft, "Left Arrow", &["arrowleft"]),
    (KeyCode::ArrowRight, "Right Arrow", &["arrowright"]),
    (KeyCode::Space, "Space", &["spacebar"]),
    (KeyCode::Enter, "Enter", &["return"]),
    (KeyCode::Tab, "Tab", &[]),
    (KeyCode::Escape, "Esc", &["escape"]),
    (KeyCode::Backspace, "Backspace", &[]),
    (KeyCode::Delete, "Delete", &["del"]),
    (KeyCode::Insert, "Insert", &["ins"]),
    (KeyCode::Home, "Home", &[]),
    (KeyCode::End, "End", &[]),
    (KeyCode::PageUp, "Page Up", &["pageup", "pgup"]),
    (KeyCode::PageDown, "Page Down", &["pagedown", "pgdn"]),
    (KeyCode::Minus, "-", &["minus"]),
    (KeyCode::Equal, "=", &["equal", "equals"]),
    (KeyCode::BracketLeft, "[", &["bracketleft"]),
    (KeyCode::BracketRight, "]", &["bracketright"]),
    (KeyCode::Semicolon, ";", &["semicolon"]),
    (KeyCode::Quote, "'", &["quote"]),
    (KeyCode::Comma, ",", &["comma"]),
    (KeyCode::Period, ".", &["period", "dot"]),
    (KeyCode::Slash, "/", &["slash"]),
    (KeyCode::Backslash, "\\", &["backslash"]),
    (KeyCode::Backquote, "`", &["backquote", "grave"]),
    (KeyCode::CapsLock, "Caps Lock", &["capslock", "caps"]),
    (KeyCode::ControlLeft, "Ctrl", &["control", "ctl"]),
    (KeyCode::ShiftLeft, "Shift", &[]),
    (KeyCode::AltLeft, "Alt", &["option"]),
];

const MOUSE: &[(MouseButton, &str, &[&str])] = &[
    (MouseButton::Left, "Left Click", &["lmb", "mouse1", "leftclick"]),
    (MouseButton::Right, "Right Click", &["rmb", "mouse2", "rightclick"]),
    (MouseButton::Middle, "Middle Click", &["mmb", "mouse3", "middleclick"]),
    (MouseButton::Back, "Mouse 4", &["mouse4", "back"]),
    (MouseButton::Forward, "Mouse 5", &["mouse5", "forward"]),
];

/// Right Ctrl is Left Ctrl as far as bindings go (and likewise Shift, Alt).
pub fn canon_key(key: KeyCode) -> KeyCode {
    match key {
        KeyCode::ControlRight => KeyCode::ControlLeft,
        KeyCode::ShiftRight => KeyCode::ShiftLeft,
        KeyCode::AltRight => KeyCode::AltLeft,
        other => other,
    }
}

/// Which modifier a key is, if it is one.
pub fn modifier_of(key: KeyCode) -> Option<Modifier> {
    match canon_key(key) {
        KeyCode::ControlLeft => Some(Modifier::Ctrl),
        KeyCode::AltLeft => Some(Modifier::Alt),
        KeyCode::ShiftLeft => Some(Modifier::Shift),
        _ => None,
    }
}

/// The name shown for a key, or `None` for keys that can't be bound.
pub fn key_name(key: KeyCode) -> Option<&'static str> {
    let key = canon_key(key);
    KEYS.iter().find(|(k, _, _)| *k == key).map(|(_, name, _)| *name)
}

pub fn mouse_name(button: MouseButton) -> Option<&'static str> {
    MOUSE.iter().find(|(b, _, _)| *b == button).map(|(_, name, _)| *name)
}

/// A key or mouse button from what a player typed (case-insensitive, accepts
/// the aliases in the tables).
pub fn parse_input(text: &str) -> Option<Input> {
    let want = text.trim().to_lowercase();
    if want.is_empty() {
        return None;
    }
    let hit = |name: &str, aliases: &[&str]| name.to_lowercase() == want || aliases.iter().any(|a| *a == want);
    if let Some((k, _, _)) = KEYS.iter().find(|(_, n, a)| hit(n, a)) {
        return Some(Input::Key(*k));
    }
    MOUSE.iter().find(|(_, n, a)| hit(n, a)).map(|(b, _, _)| Input::Mouse(*b))
}

fn input_name(input: Input) -> &'static str {
    match input {
        Input::Key(k) => key_name(k).unwrap_or("?"),
        Input::Mouse(b) => mouse_name(b).unwrap_or("?"),
    }
}

// ---------------------------------------------------------------------------
// Chords
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub input: Input,
}

impl Chord {
    /// Normalizes: right-hand modifier keys become left-hand ones, and a chord
    /// whose input *is* a modifier doesn't also list it as a held modifier
    /// (`Shift` on its own, never "Shift+Shift").
    pub fn new(ctrl: bool, alt: bool, shift: bool, input: Input) -> Self {
        let input = match input {
            Input::Key(k) => Input::Key(canon_key(k)),
            m => m,
        };
        let (mut ctrl, mut alt, mut shift) = (ctrl, alt, shift);
        if let Input::Key(k) = input {
            match modifier_of(k) {
                Some(Modifier::Ctrl) => ctrl = false,
                Some(Modifier::Alt) => alt = false,
                Some(Modifier::Shift) => shift = false,
                None => {}
            }
        }
        Self { ctrl, alt, shift, input }
    }

    pub fn key(key: KeyCode) -> Self {
        Self::new(false, false, false, Input::Key(key))
    }

    pub fn mouse(button: MouseButton) -> Self {
        Self::new(false, false, false, Input::Mouse(button))
    }

    pub fn with_ctrl(mut self) -> Self {
        self.ctrl = true;
        Self::new(self.ctrl, self.alt, self.shift, self.input)
    }

    /// `Ctrl+Alt+Shift+Input`, e.g. `Ctrl+Left Click`.
    pub fn display(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        parts.push(input_name(self.input));
        parts.join("+")
    }

    /// The inverse of [`Chord::display`] (case-insensitive, spaces around `+` ok).
    pub fn parse(text: &str) -> Option<Self> {
        let parts: Vec<&str> = text.split('+').map(str::trim).collect();
        let (last, mods) = parts.split_last()?;
        let input = parse_input(last)?;
        let (mut ctrl, mut alt, mut shift) = (false, false, false);
        for m in mods {
            match parse_input(m) {
                Some(Input::Key(k)) => match modifier_of(k)? {
                    Modifier::Ctrl => ctrl = true,
                    Modifier::Alt => alt = true,
                    Modifier::Shift => shift = true,
                },
                _ => return None,
            }
        }
        Some(Self::new(ctrl, alt, shift, input))
    }

    fn needs(&self, m: Modifier) -> bool {
        match m {
            Modifier::Ctrl => self.ctrl,
            Modifier::Alt => self.alt,
            Modifier::Shift => self.shift,
        }
    }

    fn mods_held(&self, keys: &ButtonInput<KeyCode>) -> bool {
        [Modifier::Ctrl, Modifier::Alt, Modifier::Shift].into_iter().all(|m| !self.needs(m) || modifier_down(keys, m))
    }

    /// The input is down and every required modifier is held (extra ones may be).
    pub fn held(&self, keys: &ButtonInput<KeyCode>, mouse: &ButtonInput<MouseButton>) -> bool {
        self.mods_held(keys)
            && match self.input {
                Input::Key(k) => key_down(keys, k),
                Input::Mouse(b) => mouse.pressed(b),
            }
    }

    /// The input went down this frame with every required modifier already held.
    pub fn just_pressed(&self, keys: &ButtonInput<KeyCode>, mouse: &ButtonInput<MouseButton>) -> bool {
        self.mods_held(keys)
            && match self.input {
                Input::Key(k) => key_just_pressed(keys, k),
                Input::Mouse(b) => mouse.just_pressed(b),
            }
    }

    /// Requires everything `other` does, and more.
    fn more_specific_than(&self, other: &Chord) -> bool {
        let mods = [Modifier::Ctrl, Modifier::Alt, Modifier::Shift];
        mods.iter().all(|m| !other.needs(*m) || self.needs(*m)) && mods.iter().any(|m| self.needs(*m) && !other.needs(*m))
    }
}

fn modifier_down(keys: &ButtonInput<KeyCode>, m: Modifier) -> bool {
    match m {
        Modifier::Ctrl => keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight),
        Modifier::Alt => keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight),
        Modifier::Shift => keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight),
    }
}

fn mirror(key: KeyCode) -> KeyCode {
    match key {
        KeyCode::ControlLeft => KeyCode::ControlRight,
        KeyCode::ShiftLeft => KeyCode::ShiftRight,
        KeyCode::AltLeft => KeyCode::AltRight,
        other => other,
    }
}

fn key_down(keys: &ButtonInput<KeyCode>, key: KeyCode) -> bool {
    keys.pressed(key) || keys.pressed(mirror(key))
}

fn key_just_pressed(keys: &ButtonInput<KeyCode>, key: KeyCode) -> bool {
    keys.just_pressed(key) || keys.just_pressed(mirror(key))
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    Movement,
    Gameplay,
    Inventory,
    Interface,
}

impl Category {
    pub const ALL: [Category; 4] = [Category::Movement, Category::Gameplay, Category::Inventory, Category::Interface];

    pub fn name(self) -> &'static str {
        match self {
            Category::Movement => "Movement",
            Category::Gameplay => "Gameplay",
            Category::Inventory => "Inventory",
            Category::Interface => "Interface",
        }
    }
}

fn key(k: KeyCode) -> Option<Chord> {
    Some(Chord::key(k))
}

fn mouse(b: MouseButton) -> Option<Chord> {
    Some(Chord::mouse(b))
}

fn ctrl_key(k: KeyCode) -> Option<Chord> {
    Some(Chord::key(k).with_ctrl())
}

/// One row per action: `Variant => (saved id, label, category, default chord)`.
/// Adding a control is one line here plus asking [`Actions`] about it where it
/// is used; the Keybinds screen, search, conflicts and saving all follow.
macro_rules! actions {
    ($($variant:ident => ($id:expr, $label:expr, $cat:expr, $default:expr)),+ $(,)?) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
        pub enum Action { $($variant),+ }

        impl Action {
            pub const ALL: &'static [Action] = &[$(Action::$variant),+];

            /// Stable name used in `keybinds.json`; never rename one.
            pub fn id(self) -> &'static str {
                match self { $(Action::$variant => $id),+ }
            }

            pub fn label(self) -> &'static str {
                match self { $(Action::$variant => $label),+ }
            }

            pub fn category(self) -> Category {
                match self { $(Action::$variant => $cat),+ }
            }

            pub fn default_chord(self) -> Option<Chord> {
                match self { $(Action::$variant => $default),+ }
            }
        }
    };
}

actions! {
    Forward => ("forward", "Move forward", Category::Movement, key(KeyCode::KeyW)),
    Back => ("back", "Move backward", Category::Movement, key(KeyCode::KeyS)),
    Left => ("left", "Move left", Category::Movement, key(KeyCode::KeyA)),
    Right => ("right", "Move right", Category::Movement, key(KeyCode::KeyD)),
    Jump => ("jump", "Jump / swim up / fly up", Category::Movement, key(KeyCode::Space)),
    Sneak => ("sneak", "Sneak / fly down", Category::Movement, key(KeyCode::ShiftLeft)),
    Sprint => ("sprint", "Sprint", Category::Movement, key(KeyCode::ControlLeft)),
    ToggleFly => ("toggle_fly", "Toggle flying (Creative)", Category::Movement, key(KeyCode::KeyF)),
    Attack => ("attack", "Attack / break block", Category::Gameplay, mouse(MouseButton::Left)),
    UseItem => ("use", "Place block / use", Category::Gameplay, mouse(MouseButton::Right)),
    PickBlock => ("pick_block", "Pick block (Creative)", Category::Gameplay, mouse(MouseButton::Middle)),
    HotbarPrev => ("hotbar_prev", "Previous slot", Category::Inventory, None),
    HotbarNext => ("hotbar_next", "Next slot", Category::Inventory, None),
    DropItem => ("drop_item", "Drop one item", Category::Inventory, key(KeyCode::KeyQ)),
    DropStack => ("drop_stack", "Drop the whole stack", Category::Inventory, ctrl_key(KeyCode::KeyQ)),
    Inventory => ("inventory", "Open inventory", Category::Inventory, key(KeyCode::KeyE)),
    Hotbar1 => ("hotbar_1", "Hotbar slot 1", Category::Inventory, key(KeyCode::Digit1)),
    Hotbar2 => ("hotbar_2", "Hotbar slot 2", Category::Inventory, key(KeyCode::Digit2)),
    Hotbar3 => ("hotbar_3", "Hotbar slot 3", Category::Inventory, key(KeyCode::Digit3)),
    Hotbar4 => ("hotbar_4", "Hotbar slot 4", Category::Inventory, key(KeyCode::Digit4)),
    Hotbar5 => ("hotbar_5", "Hotbar slot 5", Category::Inventory, key(KeyCode::Digit5)),
    Hotbar6 => ("hotbar_6", "Hotbar slot 6", Category::Inventory, key(KeyCode::Digit6)),
    Hotbar7 => ("hotbar_7", "Hotbar slot 7", Category::Inventory, key(KeyCode::Digit7)),
    Hotbar8 => ("hotbar_8", "Hotbar slot 8", Category::Inventory, key(KeyCode::Digit8)),
    Hotbar9 => ("hotbar_9", "Hotbar slot 9", Category::Inventory, key(KeyCode::Digit9)),
    Chat => ("chat", "Open chat", Category::Interface, key(KeyCode::KeyT)),
    Command => ("command", "Open chat with /", Category::Interface, key(KeyCode::Slash)),
    DebugOverlay => ("debug_overlay", "Debug overlay", Category::Interface, key(KeyCode::F3)),
    Pause => ("pause", "Pause menu", Category::Interface, key(KeyCode::Escape)),
}

impl Action {
    /// What each control starts bound to on a controller. The sticks are not here: the
    /// left one moves and the right one looks whatever is bound (see `gamepad.rs`).
    pub fn default_pad(self) -> Option<PadButton> {
        use PadButton as P;
        match self {
            Action::Jump => Some(P::South),
            Action::Sneak => Some(P::East),
            Action::Sprint => Some(P::LeftStick),
            Action::ToggleFly => Some(P::Select),
            Action::Attack => Some(P::RightTrigger),
            Action::UseItem => Some(P::LeftTrigger),
            Action::PickBlock => Some(P::West),
            Action::DropItem => Some(P::DPadDown),
            Action::Inventory => Some(P::North),
            Action::HotbarPrev => Some(P::LeftBumper),
            Action::HotbarNext => Some(P::RightBumper),
            Action::Pause => Some(P::Start),
            _ => None,
        }
    }

    pub fn from_id(id: &str) -> Option<Action> {
        Action::ALL.iter().copied().find(|a| a.id() == id)
    }

    /// The nine hotbar actions in slot order.
    pub const HOTBAR: [Action; 9] = [
        Action::Hotbar1,
        Action::Hotbar2,
        Action::Hotbar3,
        Action::Hotbar4,
        Action::Hotbar5,
        Action::Hotbar6,
        Action::Hotbar7,
        Action::Hotbar8,
        Action::Hotbar9,
    ];
}

// ---------------------------------------------------------------------------
// Controls and bindings
// ---------------------------------------------------------------------------

/// Anything that names a control: a built-in [`Action`] or a plain id string
/// (`"granite_pack:dig_deep"`). Lets every [`Bindings`] and [`Actions`] method take
/// either, so the game's own code keeps saying `Action::Jump` while a mod or
/// plugin says `"mymod:thing"`.
pub trait Control {
    fn control_id(&self) -> &str;
}

impl Control for Action {
    fn control_id(&self) -> &str {
        self.id()
    }
}

impl Control for str {
    fn control_id(&self) -> &str {
        self
    }
}

impl Control for String {
    fn control_id(&self) -> &str {
        self
    }
}

impl<T: Control + ?Sized> Control for &T {
    fn control_id(&self) -> &str {
        (**self).control_id()
    }
}

/// Which mod a control came from.
#[derive(Clone, PartialEq, Debug)]
pub struct ControlSource {
    /// The mod's declared id (the prefix of the control's id).
    pub mod_id: String,
    pub mod_name: String,
    /// The mod's folder - what tells whether the current world uses it.
    pub path: std::path::PathBuf,
}

/// One entry in the Keybinds screen: what it's called, where it's listed, what it
/// starts bound to, and optionally a chat command it runs.
#[derive(Clone, PartialEq, Debug)]
pub struct ControlDef {
    /// Unique. The game's own are bare (`jump`); a mod's are `<mod id>:<name>`.
    pub id: String,
    pub label: String,
    /// The heading it's listed under.
    pub category: String,
    pub default: Option<Chord>,
    /// What it starts bound to on a controller (`None`: nothing).
    pub default_pad: Option<PadButton>,
    /// A chat command (`/time set day`, with or without the slash) to run when the
    /// control is pressed in a world. Without one the control only exists for code
    /// that asks about it (`Actions::just_pressed("mymod:thing")`).
    pub command: Option<String>,
    pub source: Option<ControlSource>,
}

impl ControlDef {
    pub fn new(id: impl Into<String>, label: impl Into<String>, category: impl Into<String>) -> Self {
        Self { id: id.into(), label: label.into(), category: category.into(), default: None, default_pad: None, command: None, source: None }
    }

    pub fn with_default(mut self, chord: Chord) -> Self {
        self.default = Some(chord);
        self
    }

    pub fn with_default_pad(mut self, button: PadButton) -> Self {
        self.default_pad = Some(button);
        self
    }

    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self
    }
}

/// Every control that exists and what each is bound to right now.
///
/// Starts with the game's own ([`Action`]); a mod's `keybinds/*.json` or a plugin's
/// code adds more with [`Bindings::register`]. A saved change for a control that
/// isn't registered *right now* (its mod was removed) is parked, not dropped, and
/// applied if the control ever comes back - otherwise taking a mod out for a while
/// would silently throw away the player's keys for it.
#[derive(Resource, Clone, PartialEq, Debug)]
pub struct Bindings {
    defs: Vec<ControlDef>,
    chords: HashMap<String, Option<Chord>>,
    /// The controller button of each control (separate from the keyboard chord).
    pads: HashMap<String, Option<PadButton>>,
    /// Saved changes (`id -> chord text`) waiting for their control to exist.
    pending: BTreeMap<String, String>,
}

fn builtin_defs() -> Vec<ControlDef> {
    Action::ALL
        .iter()
        .map(|a| ControlDef {
            id: a.id().to_string(),
            label: a.label().to_string(),
            category: a.category().name().to_string(),
            default: a.default_chord(),
            default_pad: a.default_pad(),
            command: None,
            source: None,
        })
        .collect()
}

impl Default for Bindings {
    fn default() -> Self {
        let defs = builtin_defs();
        let chords = defs.iter().map(|d| (d.id.clone(), d.default)).collect();
        let pads = defs.iter().map(|d| (d.id.clone(), d.default_pad)).collect();
        Self { defs, chords, pads, pending: BTreeMap::new() }
    }
}

/// `none` or a chord; `None` when the text is neither.
fn parse_saved(text: &str) -> Option<Option<Chord>> {
    if text.trim().eq_ignore_ascii_case("none") {
        Some(None)
    } else {
        Chord::parse(text).map(Some)
    }
}

/// The key a controller binding is parked/saved under. `@` can't appear in a control id
/// (ids are letters, digits, `_`, `-` and one `:`), so it never collides with a keyboard entry.
fn pad_key(id: &str) -> String {
    format!("@pad:{id}")
}

/// `none` or a button name; `None` when the text is neither.
fn parse_saved_pad(text: &str) -> Option<Option<PadButton>> {
    if text.trim().eq_ignore_ascii_case("none") {
        Some(None)
    } else {
        PadButton::parse(text).map(Some)
    }
}

/// Which device the Keybinds screen is editing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Device {
    #[default]
    Keyboard,
    Controller,
}

impl Device {
    pub const ALL: [Device; 2] = [Device::Keyboard, Device::Controller];

    pub fn label(self) -> &'static str {
        match self {
            Device::Keyboard => "Keyboard & Mouse",
            Device::Controller => "Controller",
        }
    }
}

impl Bindings {
    pub fn defs(&self) -> &[ControlDef] {
        &self.defs
    }

    pub fn def(&self, id: &str) -> Option<&ControlDef> {
        self.defs.iter().find(|d| d.id == id)
    }

    /// Headings in the order they first appear: the game's own, then each mod's.
    pub fn categories(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for d in &self.defs {
            if !out.contains(&d.category) {
                out.push(d.category.clone());
            }
        }
        out
    }

    /// Adds a control. Returns `false`, changing nothing, if its id is empty or
    /// already taken - the first to claim an id keeps it, so a mod can never take
    /// over one of the game's own controls. If a saved change was waiting for this
    /// id, it is applied.
    pub fn register(&mut self, def: ControlDef) -> bool {
        if def.id.is_empty() || self.chords.contains_key(&def.id) {
            return false;
        }
        let chord = self.pending.remove(&def.id).and_then(|text| parse_saved(&text)).unwrap_or(def.default);
        self.chords.insert(def.id.clone(), chord);
        let pad = self.pending.remove(&pad_key(&def.id)).and_then(|text| parse_saved_pad(&text)).unwrap_or(def.default_pad);
        self.pads.insert(def.id.clone(), pad);
        self.defs.push(def);
        true
    }

    /// The controls that came from mods, in order.
    pub fn mod_defs(&self) -> Vec<ControlDef> {
        self.defs.iter().filter(|d| d.source.is_some()).cloned().collect()
    }

    /// Replaces every mod-provided control with this set (used when the installed
    /// mods change). A changed key of a control that is going away is parked.
    /// Returns the ids that were refused (already taken).
    pub fn set_mod_controls(&mut self, new: Vec<ControlDef>) -> Vec<String> {
        let old: Vec<String> = self.defs.iter().filter(|d| d.source.is_some()).map(|d| d.id.clone()).collect();
        for id in &old {
            if !self.is_default(id.as_str()) {
                let text = self.get(id.as_str()).map(|c| c.display()).unwrap_or_else(|| "none".to_string());
                self.pending.insert(id.clone(), text);
            }
            self.chords.remove(id);
            if !self.pad_is_default(id.as_str()) {
                let text = self.pad_get(id.as_str()).map(|b| b.id().to_string()).unwrap_or_else(|| "none".to_string());
                self.pending.insert(pad_key(id), text);
            }
            self.pads.remove(id);
        }
        self.defs.retain(|d| d.source.is_none());
        new.into_iter().filter_map(|d| { let id = d.id.clone(); (!self.register(d)).then_some(id) }).collect()
    }

    pub fn get(&self, control: impl Control) -> Option<Chord> {
        self.chords.get(control.control_id()).copied().flatten()
    }

    /// Binds (or, with `None`, unbinds) a control. Ignored for an id that isn't registered.
    pub fn set(&mut self, control: impl Control, chord: Option<Chord>) {
        if let Some(slot) = self.chords.get_mut(control.control_id()) {
            *slot = chord;
        }
    }

    pub fn default_of(&self, control: impl Control) -> Option<Chord> {
        self.def(control.control_id()).and_then(|d| d.default)
    }

    pub fn reset(&mut self, control: impl Control) {
        let id = control.control_id();
        let default = self.default_of(id);
        self.set(id, default);
    }

    /// Every control back to its default, including any parked change for a mod
    /// that isn't installed ("reset all" means all).
    pub fn reset_all(&mut self) {
        for d in &self.defs {
            self.chords.insert(d.id.clone(), d.default);
            self.pads.insert(d.id.clone(), d.default_pad);
        }
        self.pending.clear();
    }

    pub fn is_default(&self, control: impl Control) -> bool {
        let id = control.control_id();
        self.get(id) == self.default_of(id)
    }

    /// Every other control bound to exactly this one's chord.
    pub fn conflicts_with(&self, control: impl Control) -> Vec<String> {
        let id = control.control_id();
        let Some(chord) = self.get(id) else { return Vec::new() };
        self.defs.iter().filter(|d| d.id != id && self.get(d.id.as_str()) == Some(chord)).map(|d| d.id.clone()).collect()
    }

    /// Every control that shares its chord with at least one other.
    pub fn conflicting(&self) -> HashSet<String> {
        self.defs.iter().filter(|d| !self.conflicts_with(d.id.as_str()).is_empty()).map(|d| d.id.clone()).collect()
    }

    // ---- controller bindings ------------------------------------------------

    pub fn pad_get(&self, control: impl Control) -> Option<PadButton> {
        self.pads.get(control.control_id()).copied().flatten()
    }

    /// Binds (or, with `None`, unbinds) a control's controller button.
    pub fn pad_set(&mut self, control: impl Control, button: Option<PadButton>) {
        if let Some(slot) = self.pads.get_mut(control.control_id()) {
            *slot = button;
        }
    }

    pub fn pad_default_of(&self, control: impl Control) -> Option<PadButton> {
        self.def(control.control_id()).and_then(|d| d.default_pad)
    }

    pub fn pad_reset(&mut self, control: impl Control) {
        let id = control.control_id();
        let default = self.pad_default_of(id);
        self.pad_set(id, default);
    }

    pub fn pad_is_default(&self, control: impl Control) -> bool {
        let id = control.control_id();
        self.pad_get(id) == self.pad_default_of(id)
    }

    /// Every controller button back to its default, leaving the keyboard alone.
    pub fn reset_all_pads(&mut self) {
        for d in &self.defs {
            self.pads.insert(d.id.clone(), d.default_pad);
        }
        self.pending.retain(|k, _| !k.starts_with("@pad:"));
    }

    /// Every keyboard chord back to its default, leaving the controller alone.
    pub fn reset_all_keyboard(&mut self) {
        for d in &self.defs {
            self.chords.insert(d.id.clone(), d.default);
        }
        self.pending.retain(|k, _| k.starts_with("@pad:"));
    }

    /// Every other control on exactly this one's controller button.
    pub fn pad_conflicts_with(&self, control: impl Control) -> Vec<String> {
        let id = control.control_id();
        let Some(button) = self.pad_get(id) else { return Vec::new() };
        self.defs.iter().filter(|d| d.id != id && self.pad_get(d.id.as_str()) == Some(button)).map(|d| d.id.clone()).collect()
    }

    pub fn pad_conflicting(&self) -> HashSet<String> {
        self.defs.iter().filter(|d| !self.pad_conflicts_with(d.id.as_str()).is_empty()).map(|d| d.id.clone()).collect()
    }

    /// Conflicts for whichever device is being edited.
    pub fn conflicts_on(&self, device: Device, control: impl Control) -> Vec<String> {
        match device {
            Device::Keyboard => self.conflicts_with(control),
            Device::Controller => self.pad_conflicts_with(control),
        }
    }

    pub fn conflicting_on(&self, device: Device) -> HashSet<String> {
        match device {
            Device::Keyboard => self.conflicting(),
            Device::Controller => self.pad_conflicting(),
        }
    }

    pub fn pad_pressed(&self, control: impl Control, pad: &PadState) -> bool {
        self.pad_get(control).is_some_and(|b| pad.held(b))
    }

    pub fn pad_just_pressed(&self, control: impl Control, pad: &PadState) -> bool {
        self.pad_get(control).is_some_and(|b| pad.started(b))
    }

    /// A more specific chord on the same input that is held right now wins.
    fn shadowed(&self, id: &str, chord: Chord, keys: &ButtonInput<KeyCode>, mouse: &ButtonInput<MouseButton>) -> bool {
        self.defs.iter().any(|d| {
            d.id != id
                && self
                    .get(d.id.as_str())
                    .is_some_and(|c| c.input == chord.input && c.more_specific_than(&chord) && c.held(keys, mouse))
        })
    }

    /// Is the control held down?
    pub fn pressed(&self, control: impl Control, keys: &ButtonInput<KeyCode>, mouse: &ButtonInput<MouseButton>) -> bool {
        let id = control.control_id();
        self.get(id).is_some_and(|c| c.held(keys, mouse) && !self.shadowed(id, c, keys, mouse))
    }

    /// Did it start this frame?
    pub fn just_pressed(&self, control: impl Control, keys: &ButtonInput<KeyCode>, mouse: &ButtonInput<MouseButton>) -> bool {
        let id = control.control_id();
        self.get(id).is_some_and(|c| c.just_pressed(keys, mouse) && !self.shadowed(id, c, keys, mouse))
    }

    /// The movement actions expressed as the fixed keys `Player::step` reads, so
    /// the physics doesn't need to know about bindings at all.
    pub fn movement_input(&self, keys: &ButtonInput<KeyCode>, mouse: &ButtonInput<MouseButton>) -> ButtonInput<KeyCode> {
        let mut out = ButtonInput::<KeyCode>::default();
        for (action, key) in [
            (Action::Forward, KeyCode::KeyW),
            (Action::Back, KeyCode::KeyS),
            (Action::Left, KeyCode::KeyA),
            (Action::Right, KeyCode::KeyD),
            (Action::Jump, KeyCode::Space),
            (Action::Sneak, KeyCode::ShiftLeft),
            (Action::Sprint, KeyCode::ControlLeft),
        ] {
            if self.pressed(action, keys, mouse) {
                out.press(key);
            }
        }
        out
    }

    /// [`Bindings::movement_input`] plus the controller: bound buttons, and the left
    /// stick held past its threshold counts as W/A/S/D.
    pub fn movement_input_with_pad(&self, keys: &ButtonInput<KeyCode>, mouse: &ButtonInput<MouseButton>, pad: &PadState) -> ButtonInput<KeyCode> {
        let mut out = self.movement_input(keys, mouse);
        for (action, key) in [
            (Action::Forward, KeyCode::KeyW),
            (Action::Back, KeyCode::KeyS),
            (Action::Left, KeyCode::KeyA),
            (Action::Right, KeyCode::KeyD),
            (Action::Jump, KeyCode::Space),
            (Action::Sneak, KeyCode::ShiftLeft),
            (Action::Sprint, KeyCode::ControlLeft),
        ] {
            if self.pad_pressed(action, pad) {
                out.press(key);
            }
        }
        for (held, key) in move_keys(pad.left).into_iter().zip([KeyCode::KeyW, KeyCode::KeyS, KeyCode::KeyA, KeyCode::KeyD]) {
            if held {
                out.press(key);
            }
        }
        out
    }

    /// What differs from the defaults, as `id -> chord text` (`none` = unbound),
    /// including parked changes for controls that aren't registered right now.
    pub fn overrides(&self) -> BTreeMap<String, String> {
        let mut out = self.pending.clone();
        for d in &self.defs {
            if !self.is_default(d.id.as_str()) {
                out.insert(d.id.clone(), self.get(d.id.as_str()).map(|c| c.display()).unwrap_or_else(|| "none".to_string()));
            }
            if !self.pad_is_default(d.id.as_str()) {
                out.insert(pad_key(&d.id), self.pad_get(d.id.as_str()).map(|b| b.id().to_string()).unwrap_or_else(|| "none".to_string()));
            }
        }
        out
    }

    /// The game's own controls with a saved set of changes on top. A change for a
    /// control that doesn't exist yet is parked for when it does; an unreadable
    /// chord for one that does is ignored. Never an error: a file from a newer or
    /// older version, or a hand edit, must not stop the game starting.
    pub fn from_overrides(saved: &BTreeMap<String, String>) -> Self {
        let mut bindings = Bindings::default();
        bindings.apply_saved(saved);
        bindings
    }

    /// Applies saved changes to what is registered and parks the rest.
    pub fn apply_saved(&mut self, saved: &BTreeMap<String, String>) {
        for (id, text) in saved {
            if let Some(control) = id.strip_prefix("@pad:") {
                if self.pads.contains_key(control) {
                    if let Some(button) = parse_saved_pad(text) {
                        self.pads.insert(control.to_string(), button);
                    }
                } else {
                    self.pending.insert(id.clone(), text.clone());
                }
            } else if self.chords.contains_key(id) {
                if let Some(chord) = parse_saved(text) {
                    self.chords.insert(id.clone(), chord);
                }
            } else {
                self.pending.insert(id.clone(), text.clone());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

/// Does the chord involve this token? `a` means the A key (exactly - not Alt);
/// `ctrl` means Ctrl as a modifier *or* as the key itself; a token that isn't a
/// whole key name falls back to a prefix of one (`spa` finds Space, `lef` finds
/// both Left Arrow and Left Click).
fn chord_has(chord: &Chord, token: &str) -> bool {
    if let Some(input) = parse_input(token) {
        return match input {
            Input::Mouse(b) => chord.input == Input::Mouse(b),
            Input::Key(k) => {
                chord.input == input
                    || modifier_of(k).is_some_and(|m| chord.needs(m))
            }
        };
    }
    if token.len() < 2 {
        return false;
    }
    let mut names: Vec<String> = vec![input_name(chord.input).to_lowercase()];
    for (on, name) in [(chord.ctrl, "ctrl"), (chord.alt, "alt"), (chord.shift, "shift")] {
        if on {
            names.push(name.to_string());
        }
    }
    names.iter().any(|n| n.starts_with(token))
}

/// One `+`-separated part of a key query. A whole name with a space in it
/// (`left click`) is tried first; failing that the words are separate keys
/// (`q ctrl`).
fn segment_has(chord: &Chord, segment: &str) -> bool {
    chord_has(chord, segment) || (segment.contains(char::is_whitespace) && segment.split_whitespace().all(|w| chord_has(chord, w)))
}

/// The actions matching a search, in the screen's order. Forms:
/// - `key:a`, `key:ctrl+left`, `key:ctrl alt`: bound to a chord using *all* of those keys
/// - `key:` alone: everything that is bound
/// - `is:conflict`, `is:unbound`, `is:changed`
/// - anything else: every word must appear in the action's name, category or id
pub fn search(bindings: &Bindings, query: &str) -> Vec<String> {
    search_on(bindings, query, Device::Keyboard)
}

/// [`search`] for whichever device is being edited. On a controller `key:` (or `btn:`)
/// takes one button - `key:a`, `btn:right bumper`, or a prefix like `btn:dpad`.
pub fn search_on(bindings: &Bindings, query: &str, device: Device) -> Vec<String> {
    if device == Device::Controller {
        return search_pad(bindings, query);
    }
    let q = query.trim().to_lowercase();
    let all = bindings.defs().iter().map(|d| d.id.as_str());
    if let Some(rest) = q.strip_prefix("key:") {
        let tokens: Vec<&str> = rest.split('+').map(str::trim).filter(|t| !t.is_empty()).collect();
        return all
            .filter(|a| bindings.get(*a).is_some_and(|c| tokens.iter().all(|t| segment_has(&c, t))))
            .map(str::to_string)
            .collect();
    }
    if let Some(rest) = q.strip_prefix("is:") {
        let conflicting = bindings.conflicting();
        return match rest.trim() {
            "conflict" | "conflicts" => all.filter(|a| conflicting.contains(*a)).map(str::to_string).collect(),
            "unbound" | "none" => all.filter(|a| bindings.get(*a).is_none()).map(str::to_string).collect(),
            "changed" | "custom" => all.filter(|a| !bindings.is_default(*a)).map(str::to_string).collect(),
            _ => Vec::new(),
        };
    }
    search_words(bindings, &q)
}

/// Every word must appear in the control's name, category, id or mod name.
fn search_words(bindings: &Bindings, q: &str) -> Vec<String> {
    let words: Vec<&str> = q.split_whitespace().collect();
    bindings
        .defs()
        .iter()
        .filter(|d| {
            let from = d.source.as_ref().map(|s| s.mod_name.as_str()).unwrap_or("");
            let hay = format!("{} {} {} {from}", d.label, d.category, d.id).to_lowercase();
            words.iter().all(|w| hay.contains(w))
        })
        .map(|d| d.id.clone())
        .collect()
}

fn pad_matches(button: PadButton, token: &str) -> bool {
    if let Some(b) = PadButton::parse(token) {
        return b == button;
    }
    token.len() >= 2 && {
        let t: String = token.chars().filter(|c| c.is_alphanumeric()).collect();
        let norm = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
        norm(button.id()).starts_with(&t) || norm(button.label()).starts_with(&t)
    }
}

fn search_pad(bindings: &Bindings, query: &str) -> Vec<String> {
    let q = query.trim().to_lowercase();
    let all = bindings.defs().iter().map(|d| d.id.as_str());
    for prefix in ["key:", "btn:", "button:"] {
        if let Some(rest) = q.strip_prefix(prefix) {
            let token = rest.trim();
            return all.filter(|a| bindings.pad_get(*a).is_some_and(|b| token.is_empty() || pad_matches(b, token))).map(str::to_string).collect();
        }
    }
    if let Some(rest) = q.strip_prefix("is:") {
        let conflicting = bindings.pad_conflicting();
        return match rest.trim() {
            "conflict" | "conflicts" => all.filter(|a| conflicting.contains(*a)).map(str::to_string).collect(),
            "unbound" | "none" => all.filter(|a| bindings.pad_get(*a).is_none()).map(str::to_string).collect(),
            "changed" | "custom" => all.filter(|a| !bindings.pad_is_default(*a)).map(str::to_string).collect(),
            _ => Vec::new(),
        };
    }
    search_words(bindings, &q)
}

// ---------------------------------------------------------------------------
// Capturing a new binding
// ---------------------------------------------------------------------------

/// What happened while waiting for a new combo.
#[derive(Debug, PartialEq, Eq)]
pub enum CaptureEvent {
    Cancelled,
    Chosen(Chord),
}

/// "Press the new combo now". Resolved by [`Capture::step`] once a frame.
#[derive(Resource, Default)]
pub struct Capture {
    pub target: Option<String>,
    /// Frames still to ignore: the click that pressed "Rebind" is itself a mouse
    /// press this frame and must not become the binding.
    warmup: u8,
    /// A modifier pressed with nothing else yet. Releasing it alone binds the
    /// modifier by itself (`Shift` to sneak); pressing anything else first makes
    /// it part of a combo instead.
    pending: Option<KeyCode>,
    /// Waiting for a controller button rather than a key.
    pub pad: bool,
}

/// What happened while waiting for a controller button.
#[derive(Debug, PartialEq, Eq)]
pub enum PadCaptureEvent {
    Cancelled,
    Chosen(PadButton),
}

impl Capture {
    pub fn start(&mut self, control: impl Control) {
        self.target = Some(control.control_id().to_string());
        self.warmup = 1;
        self.pending = None;
        self.pad = false;
    }

    /// Wait for a controller button instead.
    pub fn start_pad(&mut self, control: impl Control) {
        self.start(control);
        self.pad = true;
    }

    /// The controller version of [`Capture::step`]: the first button pressed is the binding.
    /// Escape on the keyboard cancels (a controller user can't get stuck: any button binds).
    pub fn step_pad(&mut self, keys: &ButtonInput<KeyCode>, pad: &PadState) -> Option<PadCaptureEvent> {
        self.target.as_ref()?;
        if self.warmup > 0 {
            self.warmup -= 1;
            return None;
        }
        let done = |c: &mut Self, e| {
            c.target = None;
            c.pad = false;
            Some(e)
        };
        if keys.just_pressed(KeyCode::Escape) {
            return done(self, PadCaptureEvent::Cancelled);
        }
        // Lowest button first so two pressed in one frame resolve the same way every time.
        let first = PadButton::ALL.into_iter().find(|b| pad.started(*b));
        first.and_then(|b| done(self, PadCaptureEvent::Chosen(b)))
    }

    pub fn active(&self) -> bool {
        self.target.is_some()
    }

    fn finish(&mut self, event: CaptureEvent) -> Option<CaptureEvent> {
        self.target = None;
        self.pad = false;
        self.pending = None;
        Some(event)
    }

    /// Esc cancels. A key or mouse button binds with whatever modifiers are held.
    /// A modifier on its own binds when released without anything else.
    pub fn step(&mut self, keys: &ButtonInput<KeyCode>, mouse: &ButtonInput<MouseButton>) -> Option<CaptureEvent> {
        self.target.as_ref()?;
        if self.warmup > 0 {
            self.warmup -= 1;
            return None;
        }
        if keys.just_pressed(KeyCode::Escape) {
            return self.finish(CaptureEvent::Cancelled);
        }
        let (ctrl, alt, shift) = (modifier_down(keys, Modifier::Ctrl), modifier_down(keys, Modifier::Alt), modifier_down(keys, Modifier::Shift));
        for k in keys.get_just_pressed() {
            if modifier_of(*k).is_some() {
                self.pending = Some(canon_key(*k));
            } else if key_name(*k).is_some() {
                return self.finish(CaptureEvent::Chosen(Chord::new(ctrl, alt, shift, Input::Key(*k))));
            }
        }
        for (button, _, _) in MOUSE {
            if mouse.just_pressed(*button) {
                return self.finish(CaptureEvent::Chosen(Chord::new(ctrl, alt, shift, Input::Mouse(*button))));
            }
        }
        if let Some(pending) = self.pending {
            if keys.just_released(pending) || keys.just_released(mirror(pending)) {
                // Whatever else is still held becomes the modifiers of the combo.
                let held = |m: Modifier| modifier_down(keys, m) && modifier_of(pending) != Some(m);
                let chord = Chord::new(held(Modifier::Ctrl), held(Modifier::Alt), held(Modifier::Shift), Input::Key(pending));
                return self.finish(CaptureEvent::Chosen(chord));
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// The handle game systems use
// ---------------------------------------------------------------------------

fn default_bindings() -> &'static Bindings {
    static DEFAULT: OnceLock<Bindings> = OnceLock::new();
    DEFAULT.get_or_init(Bindings::default)
}

fn no_mouse() -> &'static ButtonInput<MouseButton> {
    static NONE: OnceLock<ButtonInput<MouseButton>> = OnceLock::new();
    NONE.get_or_init(ButtonInput::default)
}

/// Ask "is this action held / did it start this frame" without knowing which key
/// it is bound to. Falls back to the default bindings (and no mouse) when those
/// resources aren't there, so a test app with only part of the game still runs.
#[derive(SystemParam)]
pub struct Actions<'w> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    mouse: Option<Res<'w, ButtonInput<MouseButton>>>,
    bindings: Option<Res<'w, Bindings>>,
    pad: Option<Res<'w, PadState>>,
}

fn no_pad() -> &'static PadState {
    static NONE: OnceLock<PadState> = OnceLock::new();
    NONE.get_or_init(PadState::default)
}

impl Actions<'_> {
    fn pad_state(&self) -> &PadState {
        self.pad.as_deref().unwrap_or_else(|| no_pad())
    }

    fn parts(&self) -> (&Bindings, &ButtonInput<KeyCode>, &ButtonInput<MouseButton>) {
        (
            self.bindings.as_deref().unwrap_or_else(|| default_bindings()),
            &self.keys,
            self.mouse.as_deref().unwrap_or_else(|| no_mouse()),
        )
    }

    /// Held on the keyboard/mouse *or* the controller.
    pub fn pressed(&self, control: impl Control) -> bool {
        let (b, k, m) = self.parts();
        let id = control.control_id();
        b.pressed(id, k, m) || b.pad_pressed(id, self.pad_state())
    }

    pub fn just_pressed(&self, control: impl Control) -> bool {
        let (b, k, m) = self.parts();
        let id = control.control_id();
        b.just_pressed(id, k, m) || b.pad_just_pressed(id, self.pad_state())
    }

    /// The fixed-key view of the movement actions (see [`Bindings::movement_input`]).
    pub fn movement_input(&self) -> ButtonInput<KeyCode> {
        let (b, k, m) = self.parts();
        b.movement_input_with_pad(k, m, self.pad_state())
    }
}

// ---------------------------------------------------------------------------
// Control options (how controls behave, as opposed to which keys)
// ---------------------------------------------------------------------------

/// How flying is switched on and off in Creative.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlightMode {
    /// Press the Toggle flying control (F) to start and stop.
    #[default]
    Toggle,
    /// Double-tap Jump to start flying; double-tap it again to stop. The Toggle
    /// flying control does nothing in this mode.
    DoubleTapJump,
}

impl FlightMode {
    pub const ALL: [FlightMode; 2] = [FlightMode::Toggle, FlightMode::DoubleTapJump];

    pub fn label(self) -> &'static str {
        match self {
            FlightMode::Toggle => "Toggle key",
            FlightMode::DoubleTapJump => "Double-tap jump",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            FlightMode::Toggle => "Press the \"Toggle flying\" key (F by default) to start flying, and again to stop.",
            FlightMode::DoubleTapJump => "Double-tap Jump to start flying, and double-tap it again to stop. The \"Toggle flying\" key does nothing in this mode.",
        }
    }
}

/// Settings about how controls behave. Saved next to the keybinds, so (like them)
/// they belong to a launcher instance. Missing or unreadable reads as the defaults.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct ControlOptions {
    #[serde(default)]
    pub flight_mode: FlightMode,
    /// Right-stick look speed as a percentage of the default (whole numbers keep this
    /// struct `Eq`, and a percentage is how it is shown).
    #[serde(default = "default_look_speed_pct")]
    pub look_speed_pct: u32,
    /// How far a stick must move before it counts, as a percentage of its range.
    #[serde(default = "default_deadzone_pct")]
    pub deadzone_pct: u32,
    /// Push the right stick up to look down.
    #[serde(default)]
    pub invert_look_y: bool,
}

fn default_look_speed_pct() -> u32 {
    100
}

fn default_deadzone_pct() -> u32 {
    20
}

impl Default for ControlOptions {
    fn default() -> Self {
        Self { flight_mode: FlightMode::default(), look_speed_pct: 100, deadzone_pct: 20, invert_look_y: false }
    }
}

impl ControlOptions {
    pub const LOOK_SPEED_RANGE: (u32, u32) = (25, 300);
    pub const DEADZONE_RANGE: (u32, u32) = (5, 50);

    /// Look speed as a multiplier, kept in range even if the file was edited by hand.
    pub fn look_speed(&self) -> f32 {
        self.look_speed_pct.clamp(Self::LOOK_SPEED_RANGE.0, Self::LOOK_SPEED_RANGE.1) as f32 / 100.0
    }

    /// The stick dead zone as a fraction of the stick's range.
    pub fn deadzone(&self) -> f32 {
        self.deadzone_pct.clamp(Self::DEADZONE_RANGE.0, Self::DEADZONE_RANGE.1) as f32 / 100.0
    }

    /// Steps look speed by `delta` percent, staying in range.
    pub fn nudge_look_speed(&mut self, delta: i32) {
        self.look_speed_pct = (self.look_speed_pct as i32 + delta).clamp(Self::LOOK_SPEED_RANGE.0 as i32, Self::LOOK_SPEED_RANGE.1 as i32) as u32;
    }

    pub fn nudge_deadzone(&mut self, delta: i32) {
        self.deadzone_pct = (self.deadzone_pct as i32 + delta).clamp(Self::DEADZONE_RANGE.0 as i32, Self::DEADZONE_RANGE.1 as i32) as u32;
    }
}

/// How close together two Jump presses have to be to count as a double-tap.
pub const DOUBLE_TAP_WINDOW: f32 = 0.3;

/// Remembers the last Jump press so a second one soon after can be recognised.
#[derive(Default, Debug)]
pub struct DoubleTap {
    last: Option<f32>,
}

impl DoubleTap {
    /// Registers a press at time `now` (seconds). `true` when it is the second of a
    /// double-tap. A double-tap uses both presses up, so three quick taps are one
    /// double-tap and a fresh first tap, not two double-taps.
    pub fn tap(&mut self, now: f32) -> bool {
        match self.last {
            Some(prev) if now - prev <= DOUBLE_TAP_WINDOW => {
                self.last = None;
                true
            }
            _ => {
                self.last = Some(now);
                false
            }
        }
    }
}

impl ControlOptions {
    /// Should flying be switched this frame? In `Toggle` mode that is the Toggle
    /// flying control; in `DoubleTapJump` mode it is a double-tap of Jump. The
    /// other way of asking is ignored, so the two modes never both fire.
    pub fn wants_fly_toggle(&self, toggle_pressed: bool, jump_pressed: bool, now: f32, tap: &mut DoubleTap) -> bool {
        match self.flight_mode {
            FlightMode::Toggle => toggle_pressed,
            FlightMode::DoubleTapJump => jump_pressed && tap.tap(now),
        }
    }
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

/// Loads the saved bindings (if a `SaveStore` is present) and provides the
/// resources the settings screen edits. The editing itself is in `settings`.
pub struct KeybindPlugin;

impl Plugin for KeybindPlugin {
    fn build(&self, app: &mut App) {
        let bindings = app
            .world()
            .get_resource::<SaveStore>()
            .map(|store| Bindings::from_overrides(&store.load_keybinds()))
            .unwrap_or_default();
        let options = app.world().get_resource::<SaveStore>().map(|store| store.load_control_options()).unwrap_or_default();
        app.insert_resource(bindings).insert_resource(options).init_resource::<Capture>();
        if !app.is_plugin_added::<crate::gamepad::GamepadPlugin>() {
            app.add_plugins(crate::gamepad::GamepadPlugin);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(actions: &[Action]) -> Vec<String> {
        actions.iter().map(|a| a.id().to_string()).collect()
    }

    fn input(keys: &[KeyCode], mouse_buttons: &[MouseButton]) -> (ButtonInput<KeyCode>, ButtonInput<MouseButton>) {
        let mut k = ButtonInput::<KeyCode>::default();
        let mut m = ButtonInput::<MouseButton>::default();
        for key in keys {
            k.press(*key);
        }
        for b in mouse_buttons {
            m.press(*b);
        }
        (k, m)
    }

    #[test]
    fn every_key_name_parses_back_to_its_key() {
        for (key, name, aliases) in KEYS {
            assert_eq!(parse_input(name), Some(Input::Key(*key)), "{name}");
            assert_eq!(parse_input(&name.to_lowercase()), Some(Input::Key(*key)), "{name} lowercase");
            for alias in *aliases {
                assert_eq!(parse_input(alias), Some(Input::Key(*key)), "alias {alias}");
            }
        }
        for (button, name, aliases) in MOUSE {
            assert_eq!(parse_input(name), Some(Input::Mouse(*button)), "{name}");
            for alias in *aliases {
                assert_eq!(parse_input(alias), Some(Input::Mouse(*button)), "alias {alias}");
            }
        }
    }

    #[test]
    fn no_two_inputs_share_a_name_or_alias() {
        let mut seen = HashSet::new();
        for (_, name, aliases) in KEYS.iter().map(|(k, n, a)| (format!("{k:?}"), *n, *a)).chain(MOUSE.iter().map(|(b, n, a)| (format!("{b:?}"), *n, *a))) {
            for spelling in std::iter::once(name).chain(aliases.iter().copied()) {
                assert!(seen.insert(spelling.to_lowercase()), "{spelling} names two inputs");
            }
            assert!(!name.contains('+'), "'+' would break chord parsing: {name}");
        }
    }

    #[test]
    fn chords_round_trip_through_text() {
        for text in ["Ctrl+Alt+A", "Ctrl+Left Click", "Shift", "Ctrl+Shift", "Alt+F4", "Space", "Ctrl+/"] {
            let chord = Chord::parse(text).unwrap_or_else(|| panic!("{text} should parse"));
            assert_eq!(chord.display(), text);
            assert_eq!(Chord::parse(&chord.display()), Some(chord));
        }
        assert_eq!(Chord::parse("control + ALT + a"), Chord::parse("Ctrl+Alt+A"), "case and spacing don't matter");
        for bad in ["", "Ctrl+", "+A", "Ctrl+Nope", "A+B", "Left Click+A", "Ctrl+Ctrl+"] {
            assert_eq!(Chord::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_modifier_key_is_never_also_its_own_modifier() {
        let chord = Chord::new(false, false, true, Input::Key(KeyCode::ShiftRight));
        assert_eq!(chord, Chord::key(KeyCode::ShiftLeft));
        assert_eq!(chord.display(), "Shift");
        assert_eq!(Chord::new(true, false, false, Input::Key(KeyCode::ShiftLeft)).display(), "Ctrl+Shift");
    }

    #[test]
    fn action_ids_are_unique_and_every_default_survives_a_save() {
        let mut ids = HashSet::new();
        for a in Action::ALL {
            assert!(ids.insert(a.id()), "{} used twice", a.id());
            assert_eq!(Action::from_id(a.id()), Some(*a));
            if let Some(chord) = a.default_chord() {
                assert_eq!(Chord::parse(&chord.display()), Some(chord), "{}", a.id());
            }
        }
        assert!(Bindings::default().overrides().is_empty(), "the defaults are not 'changes'");
    }

    #[test]
    fn the_defaults_do_not_conflict() {
        assert!(Bindings::default().conflicting().is_empty(), "{:?}", Bindings::default().conflicting());
    }

    #[test]
    fn rebinding_moves_the_action_and_frees_the_old_key() {
        let mut b = Bindings::default();
        b.set(Action::Jump, Chord::parse("Ctrl+Alt+J"));
        let (keys, mouse) = input(&[KeyCode::Space], &[]);
        assert!(!b.pressed(Action::Jump, &keys, &mouse), "the old key no longer jumps");
        let (keys, mouse) = input(&[KeyCode::ControlLeft, KeyCode::AltRight, KeyCode::KeyJ], &[]);
        assert!(b.pressed(Action::Jump, &keys, &mouse), "right-hand modifiers count too");
        let (keys, mouse) = input(&[KeyCode::ControlLeft, KeyCode::KeyJ], &[]);
        assert!(!b.pressed(Action::Jump, &keys, &mouse), "a missing modifier is not enough");
    }

    #[test]
    fn a_plain_chord_still_fires_while_modifiers_are_held() {
        let b = Bindings::default();
        let (keys, mouse) = input(&[KeyCode::KeyW, KeyCode::ControlLeft, KeyCode::ShiftLeft], &[]);
        assert!(b.pressed(Action::Forward, &keys, &mouse), "walking forward while sprinting");
        assert!(b.pressed(Action::Sprint, &keys, &mouse));
    }

    #[test]
    fn the_more_specific_chord_on_one_input_wins() {
        let b = Bindings::default();
        let (mut keys, mouse) = input(&[KeyCode::KeyQ], &[]);
        assert!(b.just_pressed(Action::DropItem, &keys, &mouse));
        assert!(!b.just_pressed(Action::DropStack, &keys, &mouse));
        keys.release(KeyCode::KeyQ);
        keys.clear();
        keys.press(KeyCode::ControlLeft);
        keys.press(KeyCode::KeyQ);
        assert!(b.just_pressed(Action::DropStack, &keys, &mouse), "Ctrl+Q drops the stack");
        assert!(!b.just_pressed(Action::DropItem, &keys, &mouse), "and does not also drop one");
    }

    #[test]
    fn combos_work_on_mouse_buttons() {
        let mut b = Bindings::default();
        b.set(Action::PickBlock, Chord::parse("Ctrl+Left Click"));
        let (keys, mouse) = input(&[], &[MouseButton::Left]);
        assert!(!b.pressed(Action::PickBlock, &keys, &mouse), "a bare click is not Ctrl+click");
        assert!(b.pressed(Action::Attack, &keys, &mouse));
        let (keys, mouse) = input(&[KeyCode::ControlLeft], &[MouseButton::Left]);
        assert!(b.pressed(Action::PickBlock, &keys, &mouse));
        assert!(!b.pressed(Action::Attack, &keys, &mouse), "the plain click action yields to Ctrl+click");
    }

    #[test]
    fn identical_chords_conflict_and_a_mere_modifier_difference_does_not() {
        let mut b = Bindings::default();
        b.set(Action::Chat, Chord::parse("E"));
        assert_eq!(b.conflicts_with(Action::Chat), ids(&[Action::Inventory]));
        assert_eq!(b.conflicts_with(Action::Inventory), ids(&[Action::Chat]));
        assert_eq!(b.conflicting(), ids(&[Action::Chat, Action::Inventory]).into_iter().collect::<HashSet<_>>());
        b.set(Action::Chat, Chord::parse("Ctrl+E"));
        assert!(b.conflicting().is_empty(), "Ctrl+E and E are different chords");
        b.set(Action::Chat, None);
        b.set(Action::Command, None);
        assert!(b.conflicting().is_empty(), "two unbound actions are not a conflict");
    }

    #[test]
    fn movement_actions_become_the_keys_the_physics_reads() {
        let mut b = Bindings::default();
        b.set(Action::Forward, Chord::parse("Up Arrow"));
        b.set(Action::Jump, Chord::parse("Ctrl+Alt+J"));
        let (keys, mouse) = input(&[KeyCode::ArrowUp, KeyCode::ControlLeft, KeyCode::AltLeft, KeyCode::KeyJ], &[]);
        let out = b.movement_input(&keys, &mouse);
        assert!(out.pressed(KeyCode::KeyW), "forward, however it is bound");
        assert!(out.pressed(KeyCode::Space), "jump, however it is bound");
        assert!(out.pressed(KeyCode::ControlLeft), "sprint is still plain Ctrl");
        assert!(!out.pressed(KeyCode::KeyS));
        let (keys, mouse) = input(&[KeyCode::KeyW], &[]);
        assert!(!b.movement_input(&keys, &mouse).pressed(KeyCode::KeyW), "W no longer moves forward");
    }

    #[test]
    fn only_changes_are_saved_and_unreadable_entries_are_skipped() {
        let mut b = Bindings::default();
        b.set(Action::Jump, Chord::parse("Ctrl+Alt+J"));
        b.set(Action::Chat, None);
        let saved = b.overrides();
        assert_eq!(saved.len(), 2);
        assert_eq!(saved["jump"], "Ctrl+Alt+J");
        assert_eq!(saved["chat"], "none");
        assert_eq!(Bindings::from_overrides(&saved), b);

        let mut junk = saved.clone();
        junk.insert("no_such_action".into(), "A".into());
        junk.insert("forward".into(), "Ctrl+Nope".into());
        let loaded = Bindings::from_overrides(&junk);
        assert_eq!(loaded.get(Action::Forward), Action::Forward.default_chord(), "an unreadable chord for a real control is ignored");
        assert_eq!(loaded.get(Action::Jump), b.get(Action::Jump));
        assert_eq!(loaded.get(Action::Chat), None);
        assert_eq!(loaded.overrides()["no_such_action"], "A", "an unknown control's key is kept for when it exists (a mod that is only absent for now)");
        assert!(!loaded.overrides().contains_key("forward"));
    }

    #[test]
    fn plain_search_matches_names_categories_and_words_together() {
        let b = Bindings::default();
        assert_eq!(search(&b, "jump"), ids(&[Action::Jump]));
        assert_eq!(search(&b, "HOTBAR").len(), 11, "nine slots plus previous/next (matched by id)");
        assert_eq!(search(&b, "hotbar 3"), ids(&[Action::Hotbar3]));
        assert!(search(&b, "movement").contains(&Action::Sprint.id().to_string()));
        assert!(search(&b, "zzz").is_empty());
        assert_eq!(search(&b, "").len(), Action::ALL.len(), "an empty box shows everything");
    }

    #[test]
    fn key_search_finds_what_is_bound_to_a_key() {
        let b = Bindings::default();
        assert_eq!(search(&b, "key:a"), ids(&[Action::Left]), "A, not Alt or anything starting with a");
        assert_eq!(search(&b, "key:space"), ids(&[Action::Jump]));
        assert_eq!(search(&b, "KEY:Q"), ids(&[Action::DropItem, Action::DropStack]), "Q alone and Ctrl+Q both use Q");
        assert_eq!(search(&b, "key:ctrl"), ids(&[Action::Sprint, Action::DropStack]), "Ctrl as a key and as a modifier");
        assert_eq!(search(&b, "key:ctrl+q"), ids(&[Action::DropStack]), "every named key must be in the chord");
        assert_eq!(search(&b, "key:q ctrl"), ids(&[Action::DropStack]), "spaces work too");
        assert_eq!(search(&b, "key:left click"), ids(&[Action::Attack]));
        assert_eq!(search(&b, "key:1"), ids(&[Action::Hotbar1]));
        assert!(search(&b, "key:zzz").is_empty());
        assert_eq!(search(&b, "key:").len(), Action::ALL.len() - 2, "everything but the two scroll-only hotbar controls is bound");
    }

    #[test]
    fn a_half_typed_key_name_still_finds_it() {
        let b = Bindings::default();
        assert_eq!(search(&b, "key:spa"), ids(&[Action::Jump]));
        assert_eq!(search(&b, "key:esc"), ids(&[Action::Pause]));
        assert!(search(&b, "key:s").contains(&Action::Back.id().to_string()), "one letter is an exact key, not a prefix");
        assert!(!search(&b, "key:s").contains(&Action::Sneak.id().to_string()), "so S does not also find Shift");
    }

    #[test]
    fn the_search_follows_a_rebind_and_the_special_filters_work() {
        let mut b = Bindings::default();
        b.set(Action::Chat, Chord::parse("E"));
        b.set(Action::Command, None);
        assert_eq!(search(&b, "key:e"), ids(&[Action::Inventory, Action::Chat]));
        assert_eq!(search(&b, "is:conflict"), ids(&[Action::Inventory, Action::Chat]));
        assert_eq!(search(&b, "is:unbound"), ids(&[Action::HotbarPrev, Action::HotbarNext, Action::Command]), "previous/next slot are on the wheel and the bumpers, not a key");
        assert_eq!(search(&b, "is:changed"), ids(&[Action::Chat, Action::Command]));
        assert!(search(&b, "is:wat").is_empty());
        assert!(!search(&b, "key:/").contains(&Action::Command.id().to_string()), "unbound means not found by key");
    }

    fn frame(keys: &mut ButtonInput<KeyCode>, mouse: &mut ButtonInput<MouseButton>) {
        keys.clear();
        mouse.clear();
    }

    #[test]
    fn capturing_a_key_a_combo_and_a_click() {
        let mut cap = Capture::default();
        let (mut keys, mut mouse) = input(&[], &[]);
        cap.start(Action::Jump);
        assert_eq!(cap.step(&keys, &mouse), None, "the first frame is ignored");

        keys.press(KeyCode::KeyJ);
        assert_eq!(cap.step(&keys, &mouse), Some(CaptureEvent::Chosen(Chord::key(KeyCode::KeyJ))));
        assert!(!cap.active());
        assert_eq!(cap.step(&keys, &mouse), None, "nothing happens once it has finished");

        frame(&mut keys, &mut mouse);
        keys.release(KeyCode::KeyJ);
        cap.start(Action::Jump);
        cap.step(&keys, &mouse);
        frame(&mut keys, &mut mouse);
        keys.press(KeyCode::ControlRight);
        assert_eq!(cap.step(&keys, &mouse), None, "a lone modifier waits to see what follows");
        frame(&mut keys, &mut mouse);
        keys.press(KeyCode::AltLeft);
        cap.step(&keys, &mouse);
        frame(&mut keys, &mut mouse);
        keys.press(KeyCode::KeyA);
        assert_eq!(cap.step(&keys, &mouse), Some(CaptureEvent::Chosen(Chord::parse("Ctrl+Alt+A").unwrap())));

        let (mut keys, mut mouse) = input(&[KeyCode::ControlLeft], &[]);
        cap.start(Action::Attack);
        cap.step(&keys, &mouse);
        frame(&mut keys, &mut mouse);
        mouse.press(MouseButton::Left);
        assert_eq!(cap.step(&keys, &mouse), Some(CaptureEvent::Chosen(Chord::parse("Ctrl+Left Click").unwrap())));
    }

    #[test]
    fn the_click_that_started_a_capture_is_not_the_binding() {
        let mut cap = Capture::default();
        let (mut keys, mut mouse) = input(&[], &[MouseButton::Left]);
        cap.start(Action::Jump);
        assert_eq!(cap.step(&keys, &mouse), None, "the Rebind click is still 'just pressed' this frame");
        frame(&mut keys, &mut mouse);
        assert_eq!(cap.step(&keys, &mouse), None);
        assert!(cap.active(), "and it keeps waiting");
    }

    #[test]
    fn a_modifier_alone_binds_when_released_and_escape_cancels() {
        let mut cap = Capture::default();
        let (mut keys, mut mouse) = input(&[], &[]);
        cap.start(Action::Sneak);
        cap.step(&keys, &mouse);
        frame(&mut keys, &mut mouse);
        keys.press(KeyCode::ShiftRight);
        assert_eq!(cap.step(&keys, &mouse), None);
        frame(&mut keys, &mut mouse);
        keys.release(KeyCode::ShiftRight);
        assert_eq!(cap.step(&keys, &mouse), Some(CaptureEvent::Chosen(Chord::key(KeyCode::ShiftLeft))));

        frame(&mut keys, &mut mouse);
        cap.start(Action::Sneak);
        cap.step(&keys, &mouse);
        frame(&mut keys, &mut mouse);
        keys.press(KeyCode::Escape);
        assert_eq!(cap.step(&keys, &mouse), Some(CaptureEvent::Cancelled));
        assert!(!cap.active());
    }

    #[test]
    fn a_held_modifier_plus_another_modifier_becomes_a_combo_chord() {
        let mut cap = Capture::default();
        let (mut keys, mut mouse) = input(&[], &[]);
        cap.start(Action::Sneak);
        cap.step(&keys, &mouse);
        frame(&mut keys, &mut mouse);
        keys.press(KeyCode::ControlLeft);
        cap.step(&keys, &mouse);
        frame(&mut keys, &mut mouse);
        keys.press(KeyCode::ShiftLeft);
        cap.step(&keys, &mouse);
        frame(&mut keys, &mut mouse);
        keys.release(KeyCode::ShiftLeft);
        assert_eq!(cap.step(&keys, &mouse), Some(CaptureEvent::Chosen(Chord::parse("Ctrl+Shift").unwrap())));
    }

    #[derive(Resource, Default)]
    struct Seen {
        jump: bool,
        jump_edge: bool,
        attack: bool,
        move_w: bool,
    }

    fn probe(act: Actions, mut seen: ResMut<Seen>) {
        seen.jump = act.pressed(Action::Jump);
        seen.jump_edge = act.just_pressed(Action::Jump);
        seen.attack = act.pressed(Action::Attack);
        seen.move_w = act.movement_input().pressed(KeyCode::KeyW);
    }

    fn probe_app(with_bindings: bool, with_mouse: bool) -> App {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>().init_resource::<Seen>();
        if with_mouse {
            app.init_resource::<ButtonInput<MouseButton>>();
        }
        if with_bindings {
            app.init_resource::<Bindings>();
        }
        app.add_systems(Update, probe);
        app
    }

    #[test]
    fn the_actions_handle_follows_a_rebind_inside_a_running_system() {
        let mut app = probe_app(true, true);
        app.world_mut().resource_mut::<Bindings>().set(Action::Jump, Chord::parse("Ctrl+J"));
        app.world_mut().resource_mut::<Bindings>().set(Action::Forward, Chord::parse("Up Arrow"));
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Space);
        app.update();
        assert!(!app.world().resource::<Seen>().jump, "Space no longer jumps");
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(KeyCode::Space);
            keys.clear();
            keys.press(KeyCode::ControlLeft);
            keys.press(KeyCode::KeyJ);
            keys.press(KeyCode::ArrowUp);
        }
        app.update();
        let seen = app.world().resource::<Seen>();
        assert!(seen.jump && seen.jump_edge, "Ctrl+J jumps, and the edge is seen on its first frame");
        assert!(seen.move_w, "the rebound Forward still reaches the physics as W");
    }

    #[test]
    fn a_partial_app_falls_back_to_the_defaults() {
        let mut app = probe_app(false, false);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Space);
        app.update();
        let seen = app.world().resource::<Seen>();
        assert!(seen.jump, "default Space with no Bindings resource");
        assert!(!seen.attack, "no mouse resource reads as nothing pressed");

        let mut app = probe_app(true, true);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        assert!(app.world().resource::<Seen>().attack);
    }

    #[test]
    fn a_double_tap_needs_two_presses_close_together_and_uses_both_up() {
        let mut t = DoubleTap::default();
        assert!(!t.tap(0.0), "the first tap starts the clock");
        assert!(t.tap(0.2), "a second within the window is a double-tap");
        assert!(!t.tap(0.25), "which used both up: this is a new first tap");
        assert!(t.tap(0.4));

        let mut slow = DoubleTap::default();
        assert!(!slow.tap(0.0));
        assert!(!slow.tap(0.5), "too slow is two single taps");
        assert!(slow.tap(0.6), "but that second one started a new clock");
    }

    #[test]
    fn toggle_mode_flies_on_the_key_and_ignores_jump_taps() {
        let o = ControlOptions { flight_mode: FlightMode::Toggle, ..ControlOptions::default() };
        let mut t = DoubleTap::default();
        assert!(o.wants_fly_toggle(true, false, 0.0, &mut t));
        assert!(!o.wants_fly_toggle(false, true, 0.1, &mut t));
        assert!(!o.wants_fly_toggle(false, true, 0.2, &mut t), "double-tapping jump does nothing in toggle mode");
    }

    #[test]
    fn double_tap_mode_flies_on_a_jump_double_tap_and_ignores_the_key() {
        let o = ControlOptions { flight_mode: FlightMode::DoubleTapJump, ..ControlOptions::default() };
        let mut t = DoubleTap::default();
        assert!(!o.wants_fly_toggle(true, false, 0.0, &mut t), "the toggle key does nothing in this mode");
        assert!(!o.wants_fly_toggle(false, true, 1.0, &mut t), "one tap is just a jump");
        assert!(o.wants_fly_toggle(false, true, 1.2, &mut t), "two start flying");
        assert!(!o.wants_fly_toggle(false, true, 2.0, &mut t));
        assert!(o.wants_fly_toggle(false, true, 2.2, &mut t), "and two more stop it");
        assert!(!o.wants_fly_toggle(false, false, 2.3, &mut t), "no press, no toggle");
    }

    #[test]
    fn control_options_default_and_survive_a_save_with_or_without_the_field() {
        assert_eq!(ControlOptions::default().flight_mode, FlightMode::Toggle);
        let saved = serde_json::to_string(&ControlOptions { flight_mode: FlightMode::DoubleTapJump, ..ControlOptions::default() }).unwrap();
        assert_eq!(saved, r#"{"flight_mode":"double_tap_jump","look_speed_pct":100,"deadzone_pct":20,"invert_look_y":false}"#);
        assert_eq!(serde_json::from_str::<ControlOptions>(&saved).unwrap().flight_mode, FlightMode::DoubleTapJump);
        let old = serde_json::from_str::<ControlOptions>(r#"{"flight_mode":"double_tap_jump"}"#).unwrap();
        assert_eq!((old.look_speed_pct, old.deadzone_pct, old.invert_look_y), (100, 20, false), "a file from before stick tuning gets the defaults, not zeros");
        let wild = ControlOptions { look_speed_pct: 99999, deadzone_pct: 0, ..ControlOptions::default() };
        assert_eq!((wild.look_speed(), wild.deadzone()), (3.0, 0.05), "a hand-edited file is clamped on use");
        assert_eq!(serde_json::from_str::<ControlOptions>("{}").unwrap(), ControlOptions::default(), "a file from before the option existed");
        assert!(serde_json::from_str::<ControlOptions>("{\"flight_mode\": \"nope\"}").is_err(), "load_control_options turns that into the default");
    }

    // ---- controller bindings -------------------------------------------------

    fn pad_with(buttons: &[PadButton]) -> PadState {
        let mut p = PadState::default();
        for b in buttons {
            p.pressed.insert(*b);
            p.just_pressed.insert(*b);
        }
        p
    }

    #[test]
    fn the_default_controller_layout_has_no_clashes_and_covers_the_essentials() {
        let b = Bindings::default();
        assert!(b.pad_conflicting().is_empty(), "{:?}", b.pad_conflicting());
        for (a, button) in [(Action::Jump, PadButton::South), (Action::Attack, PadButton::RightTrigger), (Action::UseItem, PadButton::LeftTrigger), (Action::Pause, PadButton::Start), (Action::Inventory, PadButton::North)] {
            assert_eq!(b.pad_get(a), Some(button), "{}", a.id());
        }
        // The sticks move and look; those four actions have no default button.
        for a in [Action::Forward, Action::Back, Action::Left, Action::Right] {
            assert_eq!(b.pad_get(a), None);
        }
    }

    #[test]
    fn a_controller_binding_is_independent_of_the_keyboard_one() {
        let mut b = Bindings::default();
        b.pad_set(Action::Jump, Some(PadButton::East));
        assert_eq!(b.pad_get(Action::Jump), Some(PadButton::East));
        assert_eq!(b.get(Action::Jump), Chord::parse("Space"), "the keyboard binding did not move");
        assert!(!b.pad_is_default(Action::Jump) && b.is_default(Action::Jump));
        b.pad_reset(Action::Jump);
        assert!(b.pad_is_default(Action::Jump));
        b.set(Action::Jump, None);
        assert_eq!(b.pad_get(Action::Jump), Some(PadButton::South), "unbinding the key leaves the button");
    }

    #[test]
    fn two_controls_on_one_button_are_a_controller_conflict_only() {
        let mut b = Bindings::default();
        b.pad_set(Action::Sneak, Some(PadButton::South)); // Jump's button
        let clash = b.pad_conflicting();
        assert!(clash.contains("jump") && clash.contains("sneak"));
        assert!(b.conflicting().is_empty(), "the keyboard side is untouched");
        assert_eq!(b.conflicts_on(Device::Controller, Action::Jump), vec!["sneak".to_string()]);
        assert!(b.conflicts_on(Device::Keyboard, Action::Jump).is_empty());
    }

    #[test]
    fn controller_changes_save_with_a_prefix_and_load_back_without_touching_the_keyboard() {
        let mut b = Bindings::default();
        b.pad_set(Action::Jump, Some(PadButton::East));
        b.pad_set(Action::Attack, None);
        b.set(Action::Chat, Chord::parse("Ctrl+C"));
        let saved = b.overrides();
        assert_eq!(saved["@pad:jump"], "east");
        assert_eq!(saved["@pad:attack"], "none");
        assert_eq!(saved["chat"], "Ctrl+C");
        assert_eq!(saved.len(), 3, "defaults are not saved");
        assert_eq!(Bindings::from_overrides(&saved), b);
        // An unreadable button is ignored rather than breaking the load.
        let mut junk = saved.clone();
        junk.insert("@pad:jump".into(), "nope".into());
        assert_eq!(Bindings::from_overrides(&junk).pad_get(Action::Jump), Some(PadButton::South));
    }

    #[test]
    fn a_saved_button_for_a_control_that_isnt_there_waits_for_it() {
        let mut saved = BTreeMap::new();
        saved.insert("@pad:mymod:zap".to_string(), "west".to_string());
        let mut b = Bindings::from_overrides(&saved);
        assert_eq!(b.overrides().get("@pad:mymod:zap").map(String::as_str), Some("west"), "parked, so saving doesn't lose it");
        assert!(b.register(ControlDef::new("mymod:zap", "Zap", "My Mod").with_default_pad(PadButton::North)));
        assert_eq!(b.pad_get("mymod:zap"), Some(PadButton::West), "the player's choice beats the mod's default");
        // Removing the mod again parks the change instead of dropping it.
        b.set_mod_controls(Vec::new());
        // (the control wasn't from a mod source, so it stays; emulate a mod one)
        let mut with_source = ControlDef::new("m:q", "Q", "M").with_default_pad(PadButton::North);
        with_source.source = Some(ControlSource { mod_id: "m".into(), mod_name: "M".into(), path: ".".into() });
        assert!(b.register(with_source));
        b.pad_set("m:q", Some(PadButton::East));
        b.set_mod_controls(Vec::new());
        assert_eq!(b.overrides().get("@pad:m:q").map(String::as_str), Some("east"));
    }

    #[test]
    fn reset_all_is_per_device_and_both_together() {
        let mut b = Bindings::default();
        b.set(Action::Jump, Chord::parse("J"));
        b.pad_set(Action::Jump, Some(PadButton::East));
        let mut only_pads = b.clone();
        only_pads.reset_all_pads();
        assert!(only_pads.pad_is_default(Action::Jump) && !only_pads.is_default(Action::Jump));
        let mut only_keys = b.clone();
        only_keys.reset_all_keyboard();
        assert!(only_keys.is_default(Action::Jump) && !only_keys.pad_is_default(Action::Jump));
        b.reset_all();
        assert_eq!(b, Bindings::default());
    }

    #[test]
    fn buttons_and_the_left_stick_become_the_fixed_movement_keys() {
        let b = Bindings::default();
        let (keys, mouse) = input(&[], &[]);
        let mut pad = pad_with(&[PadButton::South]);
        let out = b.movement_input_with_pad(&keys, &mouse, &pad);
        assert!(out.pressed(KeyCode::Space), "A jumps");
        pad.left = Vec2::new(0.0, 1.0);
        let out = b.movement_input_with_pad(&keys, &mouse, &pad);
        assert!(out.pressed(KeyCode::KeyW) && !out.pressed(KeyCode::KeyS), "stick up walks forward");
        pad.left = Vec2::new(-0.9, -0.9);
        let out = b.movement_input_with_pad(&keys, &mouse, &pad);
        assert!(out.pressed(KeyCode::KeyS) && out.pressed(KeyCode::KeyA));
        // Rebinding Jump to another button moves it.
        let mut b = b;
        b.pad_set(Action::Jump, Some(PadButton::West));
        assert!(!b.movement_input_with_pad(&keys, &mouse, &pad_with(&[PadButton::South])).pressed(KeyCode::Space));
        assert!(b.movement_input_with_pad(&keys, &mouse, &pad_with(&[PadButton::West])).pressed(KeyCode::Space));
    }

    #[test]
    fn capturing_a_button_waits_a_frame_takes_the_first_press_and_escape_cancels() {
        let keys = ButtonInput::<KeyCode>::default();
        let mut c = Capture::default();
        c.start_pad("jump");
        assert!(c.pad && c.active());
        // The click on "Rebind" is a frame old: a button held through it must not bind.
        assert_eq!(c.step_pad(&keys, &pad_with(&[PadButton::East])), None, "warm-up frame");
        assert_eq!(c.step_pad(&keys, &PadState::default()), None, "still waiting");
        assert_eq!(c.step_pad(&keys, &pad_with(&[PadButton::East])), Some(PadCaptureEvent::Chosen(PadButton::East)));
        assert!(!c.active() && !c.pad, "done");
        c.start_pad("jump");
        c.step_pad(&keys, &PadState::default());
        let mut esc = ButtonInput::<KeyCode>::default();
        esc.press(KeyCode::Escape);
        assert_eq!(c.step_pad(&esc, &PadState::default()), Some(PadCaptureEvent::Cancelled));
        // The keyboard capture is a different mode and clears the flag.
        c.start("jump");
        assert!(!c.pad);
    }

    #[test]
    fn controller_search_finds_by_button_and_by_state() {
        let mut b = Bindings::default();
        assert_eq!(search_on(&b, "btn:a", Device::Controller), vec!["jump".to_string()]);
        assert_eq!(search_on(&b, "key:south", Device::Controller), vec!["jump".to_string()], "key: works too");
        assert!(search_on(&b, "btn:dpad", Device::Controller).contains(&"drop_item".to_string()), "a prefix finds the D-pad");
        assert!(search_on(&b, "is:conflict", Device::Controller).is_empty());
        b.pad_set(Action::Sneak, Some(PadButton::South));
        let mut clash = search_on(&b, "is:conflict", Device::Controller);
        clash.sort();
        assert_eq!(clash, vec!["jump".to_string(), "sneak".to_string()]);
        assert!(search_on(&b, "is:unbound", Device::Controller).contains(&"forward".to_string()));
        assert!(search_on(&b, "is:changed", Device::Controller).contains(&"sneak".to_string()));
        assert_eq!(search_on(&b, "hotbar prev", Device::Controller), vec!["hotbar_prev".to_string()], "plain words work on both devices");
        assert!(search(&b, "btn:a").is_empty(), "the keyboard search doesn't know btn:");
    }

    /// `Actions` is what the game systems actually ask; it must see the controller too.
    #[test]
    fn actions_answer_for_the_controller_as_well_as_the_keyboard() {
        #[derive(Resource, Default)]
        struct Seen {
            jump_held: bool,
            jump_started: bool,
            fwd: bool,
        }
        fn probe(act: Actions, mut seen: ResMut<Seen>) {
            seen.jump_held = act.pressed(Action::Jump);
            seen.jump_started = act.just_pressed(Action::Jump);
            seen.fwd = act.movement_input().pressed(KeyCode::KeyW);
        }
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>().init_resource::<Seen>().add_systems(Update, probe);
        app.update();
        assert!(!app.world().resource::<Seen>().jump_held);
        // No PadState resource at all behaves as "nothing pressed" (headless apps).
        app.insert_resource(pad_with(&[PadButton::South]));
        app.update();
        let seen = app.world().resource::<Seen>();
        assert!(seen.jump_held && seen.jump_started, "A is Jump's default button");
        let mut p = PadState::default();
        p.left = Vec2::new(0.0, 1.0);
        app.insert_resource(p);
        app.update();
        assert!(app.world().resource::<Seen>().fwd, "left stick up is forward");
        assert!(!app.world().resource::<Seen>().jump_held);
    }
}
