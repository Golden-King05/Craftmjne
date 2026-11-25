//! The look of every menu, in one place: palette, buttons, panels, the title, and
//! a pixel-art night-sky backdrop built from plain UI nodes (no image assets).
//!
//! Screens build their layout from these pieces instead of picking colours
//! themselves, so changing the look of the whole game's menus is a change here.
//!
//! **Buttons** carry a [`ThemedButton`] (a [`ButtonStyle`], whether it is selected,
//! and an optional accent colour that replaces the style's border - used for the red
//! of a conflicting key or the yellow of one being rebound). One system
//! ([`themed_button_visuals`]) paints them from that plus the `Interaction`, using the
//! pure [`button_colors`], so a screen never repaints a button by hand: it flips
//! `selected` and the theme does the rest. Because the button's colours come from the
//! component rather than from where it was spawned, a screen can be rebuilt from
//! scratch (the spawn-on-change pattern) without losing hover or selection.
//!
//! **Backdrop.** [`spawn_backdrop`] draws banded sky, stars and three layers of blocky
//! hills as absolutely-positioned nodes behind whatever else the screen holds. It is
//! deterministic (a seed, not a random call), so a screen looks the same every time.

use bevy::prelude::*;

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

pub const TEXT: Color = Color::srgb(0.93, 0.95, 0.98);
pub const TEXT_DIM: Color = Color::srgb(0.60, 0.65, 0.75);
pub const ACCENT: Color = Color::srgb(0.40, 0.85, 0.55);
pub const WARN: Color = Color::srgb(0.97, 0.72, 0.38);
pub const ERROR: Color = Color::srgb(1.0, 0.38, 0.38);

pub const PANEL_BG: Color = Color::srgba(0.06, 0.07, 0.11, 0.97);
/// Opaque on purpose: Bevy blends UI in linear space, where a faint white overlay
/// (alpha 0.05) comes out as a *much* lighter grey than it reads in sRGB.
pub const PANEL_BORDER: Color = Color::srgb(0.22, 0.25, 0.33);
pub const CARD_BG: Color = Color::srgb(0.095, 0.105, 0.145);
pub const CARD_BG_ALT: Color = Color::srgb(0.125, 0.14, 0.19);
pub const FIELD_BG: Color = Color::srgba(0.0, 0.0, 0.0, 0.40);
/// What the game shows behind a menu drawn over the running world.
pub const OVERLAY: Color = Color::srgba(0.02, 0.03, 0.06, 0.70);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ButtonStyle {
    /// An ordinary action.
    Normal,
    /// The thing you probably want: Create, Play, Apply.
    Primary,
    /// Leaves or destroys: Quit.
    Danger,
    /// A tab or a choice among several; shows `selected`.
    Choice,
    /// A text box you click to type in.
    Field,
    /// A key cap: shows what a control is bound to.
    Key,
}

/// `(background, border)` for a button in this state. Pure, so the look can be tested
/// (selected is distinguishable, hover is distinguishable, pressed is darker...).
pub fn button_colors(style: ButtonStyle, interaction: Interaction, selected: bool, accent: Option<Color>) -> (Color, Color) {
    let hovered = interaction == Interaction::Hovered;
    let pressed = interaction == Interaction::Pressed;
    let (bg, border) = match style {
        ButtonStyle::Normal | ButtonStyle::Choice | ButtonStyle::Key => {
            let bg = match (selected, pressed, hovered) {
                (_, true, _) => Color::srgb(0.11, 0.13, 0.18),
                (true, _, true) => Color::srgb(0.17, 0.32, 0.25),
                (true, _, _) => Color::srgb(0.13, 0.25, 0.20),
                (_, _, true) => Color::srgb(0.23, 0.27, 0.36),
                _ => Color::srgb(0.15, 0.17, 0.23),
            };
            let border = if selected || hovered || pressed { ACCENT } else { Color::srgb(0.27, 0.30, 0.39) };
            (bg, border)
        }
        ButtonStyle::Primary => {
            let bg = match (pressed, hovered) {
                (true, _) => Color::srgb(0.13, 0.36, 0.22),
                (_, true) => Color::srgb(0.24, 0.58, 0.36),
                _ => Color::srgb(0.19, 0.46, 0.29),
            };
            (bg, Color::srgb(0.45, 0.90, 0.60))
        }
        ButtonStyle::Danger => {
            let bg = match (pressed, hovered) {
                (true, _) => Color::srgb(0.30, 0.10, 0.10),
                (_, true) => Color::srgb(0.46, 0.15, 0.15),
                _ => Color::srgb(0.17, 0.17, 0.22),
            };
            (bg, if hovered || pressed { ERROR } else { Color::srgb(0.27, 0.30, 0.39) })
        }
        ButtonStyle::Field => {
            let border = if selected { ACCENT } else if hovered { Color::srgb(0.45, 0.50, 0.62) } else { Color::srgb(0.27, 0.30, 0.39) };
            (FIELD_BG, border)
        }
    };
    // An accent replaces the border (a conflict stays red whether hovered or not).
    (bg, accent.unwrap_or(border))
}

#[derive(Component, Clone, Copy, Debug)]
pub struct ThemedButton {
    pub style: ButtonStyle,
    pub selected: bool,
    pub accent: Option<Color>,
}

/// Paints every [`ThemedButton`] whenever its interaction or its own state changes.
pub fn themed_button_visuals(
    mut buttons: Query<(&Interaction, &ThemedButton, &mut BackgroundColor, &mut BorderColor), Or<(Changed<Interaction>, Changed<ThemedButton>)>>,
) {
    for (interaction, themed, mut bg, mut border) in &mut buttons {
        let (b, o) = button_colors(themed.style, *interaction, themed.selected, themed.accent);
        *bg = BackgroundColor(b);
        *border = BorderColor(o);
    }
}

pub struct ThemePlugin;

impl Plugin for ThemePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, themed_button_visuals);
    }
}

// ---------------------------------------------------------------------------
// Building blocks
// ---------------------------------------------------------------------------

/// How a button looks and how big it is.
#[derive(Clone, Copy, Debug)]
pub struct ButtonOpts {
    pub style: ButtonStyle,
    pub width: f32,
    pub height: f32,
    pub font: f32,
    pub selected: bool,
    pub text: Color,
    pub accent: Option<Color>,
}

impl ButtonOpts {
    pub fn new(style: ButtonStyle) -> Self {
        Self { style, width: 320.0, height: 46.0, font: 19.0, selected: false, text: TEXT, accent: None }
    }

    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    pub fn font(mut self, font: f32) -> Self {
        self.font = font;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn text(mut self, color: Color) -> Self {
        self.text = color;
        self
    }

    pub fn accent(mut self, color: Option<Color>) -> Self {
        self.accent = color;
        self
    }
}

/// Spawns a themed button carrying `action` (a screen's own marker component).
pub fn button(parent: &mut ChildSpawnerCommands, action: impl Bundle, label: &str, o: ButtonOpts) {
    let (bg, border) = button_colors(o.style, Interaction::None, o.selected, o.accent);
    parent
        .spawn((
            Button,
            action,
            ThemedButton { style: o.style, selected: o.selected, accent: o.accent },
            Node {
                width: Val::Px(o.width),
                height: Val::Px(o.height),
                border: UiRect::all(Val::Px(2.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::horizontal(Val::Px(8.0)),
                ..default()
            },
            BackgroundColor(bg),
            BorderColor(border),
        ))
        .with_children(|b| {
            b.spawn((Text::new(label), TextFont { font_size: o.font, ..default() }, TextColor(o.text)));
        });
}

/// A full-screen column, centred, that a screen's content goes in.
pub fn screen_root() -> impl Bundle {
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

/// A full-screen dim layer for a menu drawn over the running game (pause, death).
pub fn overlay_root() -> impl Bundle {
    (
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            row_gap: Val::Px(14.0),
            ..default()
        },
        BackgroundColor(OVERLAY),
    )
}

/// A bordered panel of the given width: the box a screen's controls sit in.
pub fn panel(width: f32) -> impl Bundle {
    (
        Node {
            width: Val::Px(width),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            padding: UiRect::axes(Val::Px(30.0), Val::Px(24.0)),
            row_gap: Val::Px(10.0),
            border: UiRect::all(Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(PANEL_BG),
        BorderColor(PANEL_BORDER),
    )
}

/// A panel's heading: bold-ish title with a short accent bar under it.
pub fn heading(parent: &mut ChildSpawnerCommands, text: &str) {
    heading_with(parent, text, ());
}

/// [`heading`], with extra components on the title text (a marker, so a screen can
/// change its own heading as it moves between modes).
pub fn heading_with(parent: &mut ChildSpawnerCommands, text: &str, extra: impl Bundle) {
    parent
        .spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: Val::Px(8.0), margin: UiRect::bottom(Val::Px(8.0)), ..default() })
        .with_children(|h| {
            h.spawn((Text::new(text), TextFont { font_size: 30.0, ..default() }, TextColor(TEXT), extra));
            h.spawn((Node { width: Val::Px(54.0), height: Val::Px(4.0), ..default() }, BackgroundColor(ACCENT)));
        });
}

/// Small dim helper text.
pub fn hint(parent: &mut ChildSpawnerCommands, text: impl Into<String>, size: f32) {
    parent.spawn((Text::new(text.into()), TextFont { font_size: size, ..default() }, TextColor(TEXT_DIM), Node { max_width: Val::Px(640.0), ..default() }));
}

/// A label above a group of controls ("Game mode").
pub fn label(parent: &mut ChildSpawnerCommands, text: &str) {
    parent.spawn((Text::new(text), TextFont { font_size: 14.0, ..default() }, TextColor(TEXT_DIM), Node { margin: UiRect::top(Val::Px(4.0)), ..default() }));
}

/// A small pill of coloured text, e.g. a world's game mode.
pub fn badge(parent: &mut ChildSpawnerCommands, text: &str, color: Color) {
    parent
        .spawn((
            Node { padding: UiRect::axes(Val::Px(8.0), Val::Px(2.0)), border: UiRect::all(Val::Px(1.0)), ..default() },
            BorderColor(color),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.25)),
        ))
        .with_children(|b| {
            b.spawn((Text::new(text), TextFont { font_size: 12.0, ..default() }, TextColor(color)));
        });
}

/// The game's name, big, with a block-shadow, and a line under it.
pub fn title_block(parent: &mut ChildSpawnerCommands, subtitle: &str) {
    parent.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: Val::Px(6.0), margin: UiRect::bottom(Val::Px(22.0)), ..default() }).with_children(|t| {
        t.spawn(Node::default()).with_children(|name| {
            name.spawn((Text::new("CRAFTMJNE"), TextFont { font_size: 76.0, ..default() }, TextColor(TEXT)));
            name.spawn((
                Text::new("CRAFTMJNE"),
                TextFont { font_size: 76.0, ..default() },
                TextColor(Color::srgb(0.10, 0.45, 0.28)),
                Node { position_type: PositionType::Absolute, left: Val::Px(5.0), top: Val::Px(5.0), ..default() },
                ZIndex(-1),
            ));
        });
        t.spawn((Text::new(subtitle), TextFont { font_size: 16.0, ..default() }, TextColor(TEXT_DIM)));
    });
}

// ---------------------------------------------------------------------------
// Backdrop
// ---------------------------------------------------------------------------

/// A cheap, repeatable pseudo-random number from a seed and an index.
fn noise(seed: u32, i: u32) -> f32 {
    let mut x = seed.wrapping_add(i.wrapping_mul(0x9E37_79B9));
    x ^= x >> 15;
    x = x.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 13;
    x = x.wrapping_mul(0xC2B2_AE35);
    x ^= x >> 16;
    (x & 0xFFFF) as f32 / 65535.0
}

/// Heights (in whole 8-px steps, so the hills look like stacked blocks) of one layer.
pub fn hill_heights(seed: u32, columns: usize, base: f32, amplitude: f32) -> Vec<f32> {
    (0..columns)
        .map(|i| {
            let x = i as f32;
            let wave = (x * 0.21 + noise(seed, 1) * 6.0).sin() * 0.55 + (x * 0.07 + noise(seed, 2) * 6.0).sin() * 0.45;
            let jitter = (noise(seed, 100 + i as u32) - 0.5) * 0.25;
            let h = base + (wave + jitter) * amplitude;
            (h / 8.0).round().max(1.0) * 8.0
        })
        .collect()
}

const COLUMNS: usize = 56;

/// Sky, stars and hills, as absolutely-positioned children. Spawn this *first* in a
/// screen's root so everything else draws over it.
pub fn spawn_backdrop(root: &mut ChildSpawnerCommands, seed: u32) {
    // Sky: horizontal bands from deep navy at the top to a faint glow at the horizon.
    let bands = 12;
    for i in 0..bands {
        let t = i as f32 / (bands - 1) as f32;
        let color = Color::srgb(0.035 + 0.14 * t * t, 0.05 + 0.17 * t * t, 0.11 + 0.28 * t);
        root.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                top: Val::Percent(100.0 * i as f32 / bands as f32),
                height: Val::Percent(100.0 / bands as f32 + 0.5),
                ..default()
            },
            BackgroundColor(color),
        ));
    }
    // Stars, in the upper half.
    for i in 0..44u32 {
        let size = if noise(seed, 300 + i) > 0.82 { 4.0 } else { 2.0 };
        let alpha = 0.35 + 0.55 * noise(seed, 400 + i);
        root.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(100.0 * noise(seed, 200 + i)),
                top: Val::Percent(52.0 * noise(seed, 250 + i)),
                width: Val::Px(size),
                height: Val::Px(size),
                ..default()
            },
            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, alpha)),
        ));
    }
    // Hills: far to near, each a row of columns of blocky height.
    let layers: [(f32, f32, Color); 3] = [
        (190.0, 70.0, Color::srgb(0.12, 0.17, 0.30)),
        (130.0, 55.0, Color::srgb(0.08, 0.19, 0.22)),
        (70.0, 40.0, Color::srgb(0.05, 0.13, 0.13)),
    ];
    for (n, (base, amp, color)) in layers.into_iter().enumerate() {
        let heights = hill_heights(seed.wrapping_add(n as u32 * 7919), COLUMNS, base, amp);
        root.spawn(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            bottom: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Px(base + amp + 16.0),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::FlexEnd,
            ..default()
        })
        .with_children(|row| {
            for h in heights {
                row.spawn((Node { width: Val::Percent(100.0 / COLUMNS as f32 + 0.05), height: Val::Px(h), ..default() }, BackgroundColor(color)));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bg(style: ButtonStyle, i: Interaction, selected: bool) -> Color {
        button_colors(style, i, selected, None).0
    }

    #[test]
    fn hover_press_and_selected_all_look_different_from_idle() {
        for style in [ButtonStyle::Normal, ButtonStyle::Primary, ButtonStyle::Danger, ButtonStyle::Choice] {
            let idle = bg(style, Interaction::None, false);
            assert_ne!(idle, bg(style, Interaction::Hovered, false), "{style:?} hover");
            assert_ne!(idle, bg(style, Interaction::Pressed, false), "{style:?} pressed");
            assert_ne!(bg(style, Interaction::Hovered, false), bg(style, Interaction::Pressed, false), "{style:?} hover vs press");
        }
        let (idle, idle_border) = button_colors(ButtonStyle::Choice, Interaction::None, false, None);
        let (sel, sel_border) = button_colors(ButtonStyle::Choice, Interaction::None, true, None);
        assert_ne!(idle, sel);
        assert_ne!(idle_border, sel_border);
        assert_eq!(sel_border, ACCENT, "a selected choice is outlined in the accent colour");
    }

    #[test]
    fn an_accent_replaces_the_border_in_every_state_so_a_conflict_stays_red() {
        for i in [Interaction::None, Interaction::Hovered, Interaction::Pressed] {
            for selected in [false, true] {
                assert_eq!(button_colors(ButtonStyle::Key, i, selected, Some(ERROR)).1, ERROR);
            }
        }
    }

    #[test]
    fn a_text_field_shows_focus_with_its_border_not_its_fill() {
        let (a, ab) = button_colors(ButtonStyle::Field, Interaction::None, false, None);
        let (b, bb) = button_colors(ButtonStyle::Field, Interaction::None, true, None);
        assert_eq!(a, b);
        assert_ne!(ab, bb);
    }

    #[test]
    fn the_backdrop_is_deterministic_and_the_hills_are_blocky_and_in_range() {
        let a = hill_heights(7, COLUMNS, 130.0, 55.0);
        assert_eq!(a, hill_heights(7, COLUMNS, 130.0, 55.0), "same seed, same hills");
        assert_ne!(a, hill_heights(8, COLUMNS, 130.0, 55.0));
        assert_eq!(a.len(), COLUMNS);
        for h in &a {
            assert_eq!(h % 8.0, 0.0, "heights are whole 8-px blocks");
            assert!(*h >= 8.0 && *h <= 130.0 + 55.0 * 1.2 + 8.0, "{h}");
        }
        assert!(a.iter().any(|h| *h != a[0]), "not flat");
    }

    #[test]
    fn noise_stays_in_the_unit_range() {
        for i in 0..1000 {
            let n = noise(3, i);
            assert!((0.0..=1.0).contains(&n));
        }
    }
}
