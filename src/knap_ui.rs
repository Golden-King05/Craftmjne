//! The knapping screen (see `knapping.rs` for the rules, `crafting.rs` for
//! the recipe side). Clicking a knapping recipe in the inventory's crafting
//! panel opens it on top of the inventory.
//!
//! - **Left click** strikes: a circle of force around the cell, hardest at
//!   its centre. Cells on the stone's edge are safe to strike; deep ones can
//!   crack.
//! - **Right click** taps: breaks just the one cell, can't crack, but you
//!   only get [`MAX_TAPS`] per stone.
//! - **Done** takes the stone as it is; the tool arrives at the quality the
//!   shape earned (shown by name, never as a number). **Cancel** / Escape
//!   leaves.
//!
//! The pebble is used up by the first strike or tap, so closing the screen
//! before touching it costs nothing (it was never taken); closing after loses the stone. The haft
//! and binding are used up when you press Done. Nothing here is saved: a
//! knap in progress is simply abandoned if the game closes.

use bevy::prelude::*;
use bevy::ui::{FocusPolicy, GlobalZIndex};

use crate::keybinds::{Action, Actions};
use crate::crafting::{consume_knap_stone, finish_knapping, RecipeBook};
use crate::interact::Hotbar;
use crate::inventory::{Inventory, InventoryState};
use crate::item_registry::ItemRegistry;
use crate::items::{SpawnDrop, THROWN_PICKUP_DELAY};
use crate::knapping::{Outcome, Outline, Session, FLAW_INTEGRITY, GRID, MAX_TAPS};
use crate::player::Player;
use crate::state::AppState;

/// A knap in progress.
pub struct Active {
    pub recipe: usize,
    pub session: Session,
    /// The stone has been used up (something was struck).
    pub started: bool,
    pub message: String,
}

#[derive(Resource, Default)]
pub struct KnapState {
    pub active: Option<Active>,
}

impl KnapState {
    pub fn is_open(&self) -> bool {
        self.active.is_some()
    }

    /// Opens the screen for `recipe` with a fresh stone.
    pub fn open(&mut self, recipe: usize, outline: Outline) {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1);
        self.active = Some(Active { recipe, session: Session::new(outline, seed), started: false, message: String::new() });
    }
}

#[derive(Component)]
pub(crate) struct KnapRoot;
#[derive(Component)]
pub(crate) struct KnapCell(usize, usize);
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KnapButton {
    Done,
    Cancel,
}

const CELL: f32 = 24.0;

/// What a cell looks like. Shows only what a player could see: the outline
/// to aim for, and whether the stone is solid, weakened or flawed - never
/// the hidden toughness.
fn cell_color(session: &Session, x: usize, y: usize) -> Color {
    let wanted = session.outline.contains(x, y);
    if !session.stone.present(x, y) {
        return if wanted { Color::srgba(0.85, 0.25, 0.2, 0.35) } else { Color::srgba(0.0, 0.0, 0.0, 0.0) };
    }
    let integrity = session.stone.integrity(x, y);
    let base = if integrity <= FLAW_INTEGRITY {
        [0.30, 0.22, 0.20]
    } else if integrity < 50 {
        [0.50, 0.50, 0.52]
    } else {
        [0.64, 0.64, 0.66]
    };
    if wanted {
        Color::srgb(base[0] + 0.12, base[1] + 0.16, base[2] + 0.06)
    } else {
        Color::srgb(base[0] * 0.8, base[1] * 0.8, base[2] * 0.85)
    }
}

fn reset_on_exit(mut knap: ResMut<KnapState>) {
    knap.active = None;
}

/// With the inventory gone (death, leaving the world), a knap is abandoned.
pub(crate) fn abandon_when_inventory_closes(inv: Res<InventoryState>, mut knap: ResMut<KnapState>) {
    if !inv.open && knap.is_open() {
        knap.active = None;
    }
}

/// Rings the knap cell the controller/keyboard has highlighted (the mouse gets the plain hover).
pub(crate) fn knap_focus_ring(knap: Res<KnapState>, focus: Option<Res<crate::menu_nav::MenuFocus>>, mut cells: Query<(Entity, &KnapCell, &mut BorderColor)>) {
    let Some(active) = knap.active.as_ref() else { return };
    let keyed = focus.and_then(|f| f.entity);
    for (e, &KnapCell(x, y), mut border) in &mut cells {
        let want = if keyed == Some(e) {
            Color::WHITE
        } else if active.session.outline.contains(x, y) {
            Color::srgba(0.95, 0.85, 0.3, 0.8)
        } else {
            Color::srgba(1.0, 1.0, 1.0, 0.05)
        };
        if border.0 != want {
            border.0 = want;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn knap_input(
    keys: Res<ButtonInput<KeyCode>>,
    pad: Option<Res<crate::gamepad::PadState>>,
    focus: Option<Res<crate::menu_nav::MenuFocus>>,
    act: Actions,
    book: Res<RecipeBook>,
    registry: Res<ItemRegistry>,
    mut knap: ResMut<KnapState>,
    mut hotbar: ResMut<Hotbar>,
    mut inventory: ResMut<Inventory>,
    players: Query<&Player>,
    mut drops: EventWriter<SpawnDrop>,
    cells: Query<(Entity, &Interaction, &KnapCell)>,
    buttons: Query<(&Interaction, &KnapButton), Changed<Interaction>>,
) {
    if !knap.is_open() {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) || pad.as_deref().is_some_and(|p| p.started(crate::gamepad::PadButton::East)) {
        knap.active = None;
        return;
    }

    for (interaction, button) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match button {
            KnapButton::Cancel => knap.active = None,
            KnapButton::Done => {
                let Some(active) = knap.active.as_mut() else { return };
                if !active.started {
                    knap.active = None;
                    return;
                }
                let recipe = &book.recipes[active.recipe];
                let quality = active.session.quality();
                let max = registry.max_stack(recipe.output.id);
                match finish_knapping(recipe, quality, &mut hotbar.slots, &mut inventory.slots, max) {
                    Some(left) => {
                        if left > 0 {
                            if let Ok(player) = players.single() {
                                drops.write(SpawnDrop {
                                    pos: player.pos + Vec3::Y * 0.5,
                                    stack: crate::item_registry::ItemStack { count: left, quality, ..recipe.output },
                                    vel: Vec3::ZERO,
                                    pickup_delay: THROWN_PICKUP_DELAY,
                                });
                            }
                        }
                        knap.active = None;
                    }
                    None => active.message = crate::lang::tr("You no longer have the haft and binding."),
                }
                return;
            }
        }
    }

    // The cell under the mouse, or the one the controller/keyboard has highlighted. On a
    // controller A strikes and X taps, as well as the bound triggers.
    let keyed = focus.and_then(|f| f.entity);
    let pad_pressed = |b| pad.as_deref().is_some_and(|p| p.started(b));
    let keyed_cell = cells.iter().any(|(e, ..)| keyed == Some(e));
    let tap = act.just_pressed(Action::UseItem) || (keyed_cell && pad_pressed(crate::gamepad::PadButton::West));
    let strike = act.just_pressed(Action::Attack) || (keyed_cell && pad_pressed(crate::gamepad::PadButton::South));
    if !(strike || tap) {
        return;
    }
    let Some((_, _, &KnapCell(x, y))) = cells.iter().find(|(e, i, _)| **i != Interaction::None || keyed == Some(*e)) else { return };
    let Some(active) = knap.active.as_mut() else { return };
    let recipe = &book.recipes[active.recipe];
    if !active.session.stone.present(x, y) || (tap && active.session.taps_left() == 0) {
        return;
    }
    if !active.started {
        if !consume_knap_stone(recipe, &mut hotbar.slots, &mut inventory.slots) {
            active.message = crate::lang::tr("You no longer have the stone.");
            return;
        }
        active.started = true;
    }
    let outcome = if tap { active.session.tap(x, y) } else { active.session.strike(x, y) };
    if let Some(outcome) = outcome {
        active.message = describe(outcome);
    }
}

fn describe(outcome: Outcome) -> String {
    if outcome.broken > 0 {
        crate::lang::tr("The stone cracks through!")
    } else if outcome.arrested {
        crate::lang::tr("A crack runs into the stone and stops, leaving a flaw.")
    } else if outcome.cracked {
        crate::lang::tr("A crack starts to run...")
    } else if outcome.fallen > 0 {
        crate::lang::tr("A piece falls away.")
    } else {
        String::new()
    }
}

pub(crate) fn sync_knap_screen(
    mut commands: Commands,
    knap: Res<KnapState>,
    book: Res<RecipeBook>,
    registry: Res<ItemRegistry>,
    roots: Query<Entity, With<KnapRoot>>,
) {
    if !knap.is_changed() && !roots.is_empty() {
        return;
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
    let Some(active) = &knap.active else { return };
    let recipe = &book.recipes[active.recipe];
    let session = &active.session;
    let rarity = registry.rarity(session.quality());
    let [r, g, b] = rarity.color;
    let tool = registry.def(recipe.output.id).display();

    commands
        .spawn((
            KnapRoot,
            GlobalZIndex(20),
            // Swallow clicks so the inventory underneath can't be touched.
            FocusPolicy::Block,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(crate::ui_theme::OVERLAY),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(Val::Px(38.0), Val::Px(32.0)),
                    row_gap: Val::Px(10.0),
                    ..default()
                },
                crate::gui_skin::Skinned::new(crate::gui_skin::Element::Panel),
            ))
            .with_children(|panel| {
                let text = |s: String, size: f32, color: Color| {
                    (Text::new(crate::lang::tr(&s)), TextFont { font_size: size, ..default() }, TextColor(color))
                };
                panel.spawn(text(crate::lang::tr_args("Knapping: {tool}", &[("tool", &tool)]), 24.0, Color::WHITE));
                panel.spawn(text(
                    crate::lang::tr("Left click strikes (deep hits can crack). Right click taps one cell."),
                    13.0,
                    Color::srgb(0.7, 0.7, 0.75),
                ));
                panel
                    .spawn(Node {
                        flex_direction: FlexDirection::Column,
                        border: UiRect::all(Val::Px(2.0)),
                        ..default()
                    })
                    .insert(BorderColor(Color::srgb(0.3, 0.3, 0.35)))
                    .with_children(|grid| {
                        for y in 0..GRID {
                            grid.spawn(Node { flex_direction: FlexDirection::Row, ..default() }).with_children(|row| {
                                for x in 0..GRID {
                                    let wanted = session.outline.contains(x, y);
                                    row.spawn((
                                        Button,
                                        KnapCell(x, y),
                                        Node {
                                            width: Val::Px(CELL),
                                            height: Val::Px(CELL),
                                            border: UiRect::all(Val::Px(1.0)),
                                            ..default()
                                        },
                                        BackgroundColor(cell_color(session, x, y)),
                                        BorderColor(if wanted {
                                            Color::srgba(0.95, 0.85, 0.3, 0.8)
                                        } else {
                                            Color::srgba(1.0, 1.0, 1.0, 0.05)
                                        }),
                                    ));
                                }
                            });
                        }
                    });
                panel.spawn(text(
                    crate::lang::tr_args("Quality: {rarity}", &[("rarity", &rarity.display())]),
                    20.0,
                    Color::srgb(r, g, b),
                ));
                panel.spawn(text(
                    crate::lang::tr_args("Taps left: {n} of {max}", &[("n", &session.taps_left().to_string()), ("max", &MAX_TAPS.to_string())]),
                    14.0,
                    Color::WHITE,
                ));
                let hint = if !active.message.is_empty() {
                    active.message.clone()
                } else if active.started {
                    crate::lang::tr("Closing now loses the stone.")
                } else {
                    crate::lang::tr("The stone is used up by your first strike.")
                };
                panel.spawn(text(hint, 14.0, Color::srgb(0.9, 0.8, 0.5)));
                panel.spawn(Node { column_gap: Val::Px(12.0), ..default() }).with_children(|row| {
                    for (label, kind) in [("Done", KnapButton::Done), ("Cancel", KnapButton::Cancel)] {
                        row.spawn((
                            Button,
                            kind,
                            Node { padding: UiRect::axes(Val::Px(24.0), Val::Px(10.0)), ..default() },
                            crate::ui_theme::ThemedButton { style: crate::ui_theme::ButtonStyle::Normal, selected: false, accent: None },
                        ))
                        .with_children(|b| {
                            b.spawn(text(label.into(), 16.0, Color::WHITE));
                        });
                    }
                });
            });
        });
}

pub struct KnapUiPlugin;

impl Plugin for KnapUiPlugin {
    fn build(&self, app: &mut App) {
        // The systems themselves are scheduled inside `InventoryPlugin`'s
        // chain, after its own Escape/E handling has seen the screen's state.
        app.init_resource::<KnapState>().add_systems(OnExit(AppState::InGame), reset_on_exit);
    }
}
