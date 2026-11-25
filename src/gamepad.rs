//! Controller input: which buttons exist, what is held right now, and the two sticks.
//!
//! Bevy (through `bevy_gilrs`) puts every connected controller in the world as a
//! [`Gamepad`] entity. The game never asks those directly; [`read_pads`] folds all of
//! them into one [`PadState`] a frame, and everything else - controls, movement, menu
//! navigation - reads that. That keeps the logic testable (a test just fills a
//! `PadState`) and means two controllers plugged in act as one.
//!
//! [`PadButton`] is the game's own button list (rather than Bevy's `GamepadButton`)
//! so a button has a stable saved name, a friendly label, and a fixed set of choices
//! the Keybinds screen can offer. Sticks are not rebindable buttons: the left stick
//! moves, the right looks; see [`move_keys`] and [`look_delta`].

use std::collections::HashSet;

use bevy::prelude::*;

/// A controller button a control can be bound to.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum PadButton {
    South,
    East,
    West,
    North,
    LeftBumper,
    RightBumper,
    LeftTrigger,
    RightTrigger,
    Select,
    Start,
    LeftStick,
    RightStick,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
}

impl PadButton {
    pub const ALL: [PadButton; 16] = [
        PadButton::South,
        PadButton::East,
        PadButton::West,
        PadButton::North,
        PadButton::LeftBumper,
        PadButton::RightBumper,
        PadButton::LeftTrigger,
        PadButton::RightTrigger,
        PadButton::Select,
        PadButton::Start,
        PadButton::LeftStick,
        PadButton::RightStick,
        PadButton::DPadUp,
        PadButton::DPadDown,
        PadButton::DPadLeft,
        PadButton::DPadRight,
    ];

    /// The saved spelling; never rename one.
    pub fn id(self) -> &'static str {
        match self {
            PadButton::South => "south",
            PadButton::East => "east",
            PadButton::West => "west",
            PadButton::North => "north",
            PadButton::LeftBumper => "left_bumper",
            PadButton::RightBumper => "right_bumper",
            PadButton::LeftTrigger => "left_trigger",
            PadButton::RightTrigger => "right_trigger",
            PadButton::Select => "select",
            PadButton::Start => "start",
            PadButton::LeftStick => "left_stick",
            PadButton::RightStick => "right_stick",
            PadButton::DPadUp => "dpad_up",
            PadButton::DPadDown => "dpad_down",
            PadButton::DPadLeft => "dpad_left",
            PadButton::DPadRight => "dpad_right",
        }
    }

    /// What the Keybinds screen shows. Face buttons use Xbox letters, with the
    /// PlayStation shapes in brackets since the same positions mean different things.
    pub fn label(self) -> &'static str {
        match self {
            PadButton::South => "A (Cross)",
            PadButton::East => "B (Circle)",
            PadButton::West => "X (Square)",
            PadButton::North => "Y (Triangle)",
            PadButton::LeftBumper => "Left Bumper",
            PadButton::RightBumper => "Right Bumper",
            PadButton::LeftTrigger => "Left Trigger",
            PadButton::RightTrigger => "Right Trigger",
            PadButton::Select => "Back / Select",
            PadButton::Start => "Start",
            PadButton::LeftStick => "Left Stick Click",
            PadButton::RightStick => "Right Stick Click",
            PadButton::DPadUp => "D-Pad Up",
            PadButton::DPadDown => "D-Pad Down",
            PadButton::DPadLeft => "D-Pad Left",
            PadButton::DPadRight => "D-Pad Right",
        }
    }

    /// Reads a saved id or a label, any case, ignoring spaces/dashes/underscores
    /// (`left bumper`, `LeftBumper`, `LB`, `a`).
    pub fn parse(text: &str) -> Option<PadButton> {
        let norm = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
        let want = norm(text);
        if want.is_empty() {
            return None;
        }
        let short = |b: PadButton| match b {
            PadButton::South => &["a", "cross"][..],
            PadButton::East => &["b", "circle"],
            PadButton::West => &["x", "square"],
            PadButton::North => &["y", "triangle"],
            PadButton::LeftBumper => &["lb", "l1"],
            PadButton::RightBumper => &["rb", "r1"],
            PadButton::LeftTrigger => &["lt", "l2"],
            PadButton::RightTrigger => &["rt", "r2"],
            PadButton::Select => &["back", "share"],
            PadButton::Start => &["options", "menu"],
            PadButton::LeftStick => &["l3", "lstick"],
            PadButton::RightStick => &["r3", "rstick"],
            _ => &[],
        };
        PadButton::ALL.into_iter().find(|b| norm(b.id()) == want || norm(b.label()) == want || short(*b).iter().any(|s| *s == want))
    }

    pub fn to_bevy(self) -> GamepadButton {
        match self {
            PadButton::South => GamepadButton::South,
            PadButton::East => GamepadButton::East,
            PadButton::West => GamepadButton::West,
            PadButton::North => GamepadButton::North,
            PadButton::LeftBumper => GamepadButton::LeftTrigger,
            PadButton::RightBumper => GamepadButton::RightTrigger,
            PadButton::LeftTrigger => GamepadButton::LeftTrigger2,
            PadButton::RightTrigger => GamepadButton::RightTrigger2,
            PadButton::Select => GamepadButton::Select,
            PadButton::Start => GamepadButton::Start,
            PadButton::LeftStick => GamepadButton::LeftThumb,
            PadButton::RightStick => GamepadButton::RightThumb,
            PadButton::DPadUp => GamepadButton::DPadUp,
            PadButton::DPadDown => GamepadButton::DPadDown,
            PadButton::DPadLeft => GamepadButton::DPadLeft,
            PadButton::DPadRight => GamepadButton::DPadRight,
        }
    }
}

/// What every connected controller is doing this frame, merged.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub struct PadState {
    pub pressed: HashSet<PadButton>,
    pub just_pressed: HashSet<PadButton>,
    pub left: Vec2,
    pub right: Vec2,
    /// A controller is plugged in (used to decide whether to mention it in the UI).
    pub connected: bool,
}

impl PadState {
    pub fn held(&self, b: PadButton) -> bool {
        self.pressed.contains(&b)
    }
    pub fn started(&self, b: PadButton) -> bool {
        self.just_pressed.contains(&b)
    }
}

/// Folds all [`Gamepad`]s into the [`PadState`] resource.
pub fn read_pads(pads: Query<&Gamepad>, mut state: ResMut<PadState>) {
    let mut next = PadState { connected: false, ..default() };
    for pad in &pads {
        next.connected = true;
        for b in PadButton::ALL {
            if pad.pressed(b.to_bevy()) {
                next.pressed.insert(b);
            }
            if pad.just_pressed(b.to_bevy()) {
                next.just_pressed.insert(b);
            }
        }
        // The stick furthest from centre wins, so a second idle pad can't cancel the first.
        let (l, r) = (pad.left_stick(), pad.right_stick());
        if l.length() > next.left.length() {
            next.left = l;
        }
        if r.length() > next.right.length() {
            next.right = r;
        }
    }
    if *state != next {
        *state = next;
    }
}

/// Below this a stick counts as centred (cheap sticks drift).
pub const DEADZONE: f32 = 0.2;
/// How far the left stick must be pushed before it counts as a held movement key.
pub const MOVE_THRESHOLD: f32 = 0.5;
/// Look speed at full deflection, radians a second.
pub const LOOK_SPEED: f32 = 2.6;

/// A stick with the dead zone removed and the remaining range rescaled to 0..1, so
/// look speed ramps up smoothly from nothing instead of jumping at the edge.
pub fn apply_deadzone(v: Vec2) -> Vec2 {
    let len = v.length();
    if len <= DEADZONE {
        return Vec2::ZERO;
    }
    v / len * ((len - DEADZONE) / (1.0 - DEADZONE)).min(1.0)
}

/// Which of the four movement keys the left stick is holding down: `[forward, back, left, right]`.
pub fn move_keys(left: Vec2) -> [bool; 4] {
    [left.y > MOVE_THRESHOLD, left.y < -MOVE_THRESHOLD, left.x < -MOVE_THRESHOLD, left.x > MOVE_THRESHOLD]
}

/// `(yaw, pitch)` change this frame from the right stick. Pushing right turns right
/// (yaw decreases, as for the mouse); pushing up looks up.
pub fn look_delta(right: Vec2, dt: f32) -> (f32, f32) {
    let v = apply_deadzone(right);
    (-v.x * LOOK_SPEED * dt, v.y * LOOK_SPEED * dt)
}

pub struct GamepadPlugin;

impl Plugin for GamepadPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PadState>().add_systems(PreUpdate, read_pads.after(bevy::input::InputSystem));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_button_name_parses_back_and_none_clash() {
        let mut seen = HashSet::new();
        for b in PadButton::ALL {
            assert_eq!(PadButton::parse(b.id()), Some(b), "{}", b.id());
            assert_eq!(PadButton::parse(b.label()), Some(b), "{}", b.label());
            assert!(seen.insert(b.id()), "{} listed twice", b.id());
        }
        for (text, b) in [("A", PadButton::South), ("lb", PadButton::LeftBumper), ("Right Trigger", PadButton::RightTrigger), ("d-pad up", PadButton::DPadUp), ("START", PadButton::Start), ("circle", PadButton::East)] {
            assert_eq!(PadButton::parse(text), Some(b), "{text}");
        }
        for bad in ["", "  ", "nope", "a+b"] {
            assert_eq!(PadButton::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn every_button_maps_to_a_different_bevy_button() {
        let set: HashSet<_> = PadButton::ALL.iter().map(|b| b.to_bevy()).collect();
        assert_eq!(set.len(), PadButton::ALL.len());
    }

    #[test]
    fn the_dead_zone_swallows_drift_and_rescales_the_rest() {
        assert_eq!(apply_deadzone(Vec2::new(0.1, -0.15)), Vec2::ZERO);
        let full = apply_deadzone(Vec2::new(1.0, 0.0));
        assert!((full.x - 1.0).abs() < 1e-5 && full.y == 0.0);
        let edge = apply_deadzone(Vec2::new(DEADZONE + 0.001, 0.0));
        assert!(edge.x > 0.0 && edge.x < 0.01, "ramps up from nothing, not a jump: {edge}");
        assert!(apply_deadzone(Vec2::new(5.0, 0.0)).length() <= 1.0 + 1e-5, "never above full");
    }

    #[test]
    fn the_left_stick_holds_movement_keys_only_past_the_threshold() {
        assert_eq!(move_keys(Vec2::new(0.0, 0.4)), [false; 4]);
        assert_eq!(move_keys(Vec2::new(0.0, 0.9)), [true, false, false, false]);
        assert_eq!(move_keys(Vec2::new(-0.8, -0.8)), [false, true, true, false], "diagonals hold two");
        assert_eq!(move_keys(Vec2::new(0.8, 0.0)), [false, false, false, true]);
    }

    #[test]
    fn the_right_stick_turns_and_tilts_in_the_expected_directions() {
        let (yaw, pitch) = look_delta(Vec2::new(1.0, 0.0), 1.0);
        assert!(yaw < 0.0 && pitch == 0.0, "right turns right, like the mouse");
        let (yaw, pitch) = look_delta(Vec2::new(0.0, 1.0), 1.0);
        assert!(pitch > 0.0 && yaw == 0.0, "up looks up");
        assert_eq!(look_delta(Vec2::new(0.1, 0.1), 1.0), (0.0, 0.0));
        let (a, _) = look_delta(Vec2::new(1.0, 0.0), 0.5);
        let (b, _) = look_delta(Vec2::new(1.0, 0.0), 1.0);
        assert!((a * 2.0 - b).abs() < 1e-6, "scales with time, not frame rate");
    }
}
