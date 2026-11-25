//! The look of every menu, in one place: palette, buttons, panels, the title and the
//! backdrop. The textures themselves live in `gui_skin.rs` (rustic wood, stone and
//! leather, replaceable by a texture pack or a mod); this module decides *which*
//! texture each piece of a screen uses.
//!
//! Screens build their layout from these pieces instead of picking colours or
//! images themselves, so restyling every menu is a change here (or a new skin).
//!
//! **Buttons** carry a [`ThemedButton`] (a [`ButtonStyle`], whether it is selected,
//! and an optional accent tint - used for the red of a conflicting key or the yellow
//! of one being rebound). One system ([`themed_button_visuals`]) picks the skin
//! element from that plus the `Interaction`, using the pure [`button_element`], so a
//! screen never repaints a button by hand: it flips `selected` and the theme does the
//! rest. Because the look comes from the component rather than from where it was
//! spawned, a screen can be rebuilt from scratch (the spawn-on-change pattern)
//! without losing hover or selection.

use bevy::prelude::*;

use crate::gui_skin::{apply_skin, Element, GuiSkin, GuiSkinPlugin, Skinned};

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

/// Cream, like lettering on a wooden sign.
pub const TEXT: Color = Color::srgb(0.97, 0.93, 0.82);
pub const TEXT_DIM: Color = Color::srgb(0.80, 0.72, 0.58);
/// Lantern gold.
pub const ACCENT: Color = Color::srgb(0.93, 0.75, 0.33);
pub const WARN: Color = Color::srgb(0.97, 0.72, 0.38);
pub const ERROR: Color = Color::srgb(1.0, 0.45, 0.40);
/// Dark text shadow, like carved lettering.
pub const TEXT_SHADOW: Color = Color::srgba(0.05, 0.03, 0.01, 0.85);
/// What the game shows behind a menu drawn over the running world.
pub const OVERLAY: Color = Color::srgba(0.04, 0.025, 0.015, 0.66);

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

/// Which skin element a button shows in this state. Pure, so the look can be tested
/// (selected is distinguishable, hover is distinguishable, pressed is its own texture...).
pub fn button_element(style: ButtonStyle, interaction: Interaction, selected: bool) -> Element {
    let hovered = interaction == Interaction::Hovered;
    let pressed = interaction == Interaction::Pressed;
    match style {
        ButtonStyle::Normal | ButtonStyle::Choice | ButtonStyle::Key => match (pressed, selected, hovered) {
            (true, _, _) => Element::ButtonPressed,
            (_, true, _) => Element::ButtonSelected,
            (_, _, true) => Element::ButtonHover,
            _ => Element::Button,
        },
        ButtonStyle::Primary => match (pressed, hovered) {
            (true, _) => Element::ButtonPrimaryPressed,
            (_, true) => Element::ButtonPrimaryHover,
            _ => Element::ButtonPrimary,
        },
        ButtonStyle::Danger => match (pressed, hovered) {
            (true, _) => Element::ButtonDangerPressed,
            (_, true) => Element::ButtonDangerHover,
            _ => Element::ButtonDanger,
        },
        ButtonStyle::Field => {
            if selected { Element::FieldFocus } else { Element::Field }
        }
    }
}

/// A button's tint: the accent when it has one (a conflict stays red whether hovered or
/// not), otherwise none. The skin is multiplied by it.
pub fn button_tint(accent: Option<Color>) -> Color {
    accent.unwrap_or(Color::WHITE)
}

#[derive(Component, Clone, Copy, Debug)]
pub struct ThemedButton {
    pub style: ButtonStyle,
    pub selected: bool,
    pub accent: Option<Color>,
}

/// Gives every [`ThemedButton`] the texture for its current state.
pub fn themed_button_visuals(
    skin: Option<Res<GuiSkin>>,
    mut buttons: Query<(Entity, &Interaction, &ThemedButton, Option<&mut ImageNode>)>,
    mut commands: Commands,
) {
    let Some(skin) = skin else { return };
    for (entity, interaction, themed, existing) in &mut buttons {
        let element = button_element(themed.style, *interaction, themed.selected);
        let tint = button_tint(themed.accent);
        let Some(handle) = skin.handle(element) else { continue };
        match existing {
            Some(mut node) => {
                if node.image != handle || node.color != tint {
                    if let Some(new) = skin.node(element, tint) {
                        *node = new;
                    }
                }
            }
            None => {
                if let Some(new) = skin.node(element, tint) {
                    commands.entity(entity).try_insert(new);
                }
            }
        }
    }
}

pub struct ThemePlugin;

impl Plugin for ThemePlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<GuiSkinPlugin>() {
            app.add_plugins(GuiSkinPlugin);
        }
        app.add_systems(Update, themed_button_visuals.after(apply_skin));
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

/// Carved-lettering shadow for a piece of text.
pub fn shadow() -> TextShadow {
    TextShadow { offset: Vec2::splat(2.0), color: TEXT_SHADOW }
}

/// Spawns a themed button carrying `action` (a screen's own marker component).
pub fn button(parent: &mut ChildSpawnerCommands, action: impl Bundle, label: &str, o: ButtonOpts) {
    parent
        .spawn((
            Button,
            action,
            ThemedButton { style: o.style, selected: o.selected, accent: o.accent },
            Node {
                width: Val::Px(o.width),
                height: Val::Px(o.height),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::horizontal(Val::Px(14.0)),
                ..default()
            },
        ))
        .with_children(|b| {
            b.spawn((Text::new(label), TextFont { font_size: o.font, ..default() }, TextColor(o.text), shadow()));
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

/// A framed oak board of the given width: the box a screen's controls sit on.
pub fn panel(width: f32) -> impl Bundle {
    (
        Node {
            width: Val::Px(width),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            padding: UiRect::axes(Val::Px(38.0), Val::Px(34.0)),
            row_gap: Val::Px(10.0),
            ..default()
        },
        Skinned::new(Element::Panel),
    )
}

/// A leather patch: list entries and the body of a settings page.
pub fn card_skin(alt: bool) -> Skinned {
    Skinned::new(if alt { Element::CardAlt } else { Element::Card })
}

/// A panel's heading: carved title with a short plank under it.
pub fn heading(parent: &mut ChildSpawnerCommands, text: &str) {
    heading_with(parent, text, ());
}

/// [`heading`], with extra components on the title text (a marker, so a screen can
/// change its own heading as it moves between modes).
pub fn heading_with(parent: &mut ChildSpawnerCommands, text: &str, extra: impl Bundle) {
    parent
        .spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: Val::Px(8.0), margin: UiRect::bottom(Val::Px(8.0)), ..default() })
        .with_children(|h| {
            h.spawn((Text::new(text), TextFont { font_size: 30.0, ..default() }, TextColor(TEXT), shadow(), extra));
            h.spawn((Node { width: Val::Px(84.0), height: Val::Px(12.0), ..default() }, Skinned::new(Element::Plank)));
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
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
        ))
        .with_children(|b| {
            b.spawn((Text::new(text), TextFont { font_size: 12.0, ..default() }, TextColor(color)));
        });
}

/// The game's name, carved in big letters on a hanging oak sign.
pub fn title_block(parent: &mut ChildSpawnerCommands, subtitle: &str) {
    parent.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: Val::Px(10.0), margin: UiRect::bottom(Val::Px(22.0)), ..default() }).with_children(|t| {
        t.spawn((Node { padding: UiRect::axes(Val::Px(44.0), Val::Px(26.0)), ..default() }, Skinned::new(Element::Panel))).with_children(|sign| {
            sign.spawn((Text::new("CRAFTMJNE"), TextFont { font_size: 72.0, ..default() }, TextColor(ACCENT), TextShadow { offset: Vec2::splat(5.0), color: TEXT_SHADOW }));
        });
        t.spawn((Text::new(subtitle), TextFont { font_size: 17.0, ..default() }, TextColor(TEXT), shadow()));
    });
}

// ---------------------------------------------------------------------------
// Backdrop
// ---------------------------------------------------------------------------

/// The tiled dirt that fills the screen behind a menu, darkened. Spawn this *first*
/// in a screen's root so everything else draws over it. (`_seed` is kept so each
/// screen can still ask for its own variation; the dirt tile is the same everywhere.)
pub fn spawn_backdrop(root: &mut ChildSpawnerCommands, _seed: u32) {
    root.spawn((
        Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
        Skinned::new(Element::Background).tint(Color::srgb(0.50, 0.46, 0.44)),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_press_and_selected_all_pick_a_different_texture_from_idle() {
        for style in [ButtonStyle::Normal, ButtonStyle::Primary, ButtonStyle::Danger, ButtonStyle::Choice] {
            let idle = button_element(style, Interaction::None, false);
            assert_ne!(idle, button_element(style, Interaction::Hovered, false), "{style:?} hover");
            assert_ne!(idle, button_element(style, Interaction::Pressed, false), "{style:?} pressed");
            assert_ne!(button_element(style, Interaction::Hovered, false), button_element(style, Interaction::Pressed, false), "{style:?} hover vs press");
        }
        assert_ne!(button_element(ButtonStyle::Choice, Interaction::None, true), button_element(ButtonStyle::Choice, Interaction::None, false));
    }

    #[test]
    fn an_accent_tints_in_every_state_so_a_conflict_stays_red() {
        assert_eq!(button_tint(Some(ERROR)), ERROR);
        assert_eq!(button_tint(None), Color::WHITE);
    }

    #[test]
    fn a_text_field_shows_focus_with_its_own_texture() {
        assert_eq!(button_element(ButtonStyle::Field, Interaction::None, false), Element::Field);
        assert_eq!(button_element(ButtonStyle::Field, Interaction::None, true), Element::FieldFocus);
        assert_eq!(button_element(ButtonStyle::Field, Interaction::Hovered, false), Element::Field);
    }

    #[test]
    fn every_button_element_exists_in_the_skin() {
        for style in [ButtonStyle::Normal, ButtonStyle::Primary, ButtonStyle::Danger, ButtonStyle::Choice, ButtonStyle::Field, ButtonStyle::Key] {
            for i in [Interaction::None, Interaction::Hovered, Interaction::Pressed] {
                for sel in [false, true] {
                    assert!(Element::ALL.contains(&button_element(style, i, sel)));
                }
            }
        }
    }
}
