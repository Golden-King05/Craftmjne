//! In-world inventory screen. Press E to open/close it (Escape also closes
//! it, taking priority over the pause menu). Survival shows the hotbar plus
//! a second row of personal storage - both start completely empty, since
//! there's no block-pickup-on-break yet (a natural next step; see README).
//! Creative shows a scrollable list of every registered block instead;
//! clicking one puts it in the currently selected hotbar slot. Hovering any
//! occupied slot shows the block's name, Minecraft-tooltip style.

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, PrimaryWindow};

use crate::blocks::{BlockId, BlockRegistry, BlockTables, AIR};
use crate::chat::ChatState;
use crate::interact::Hotbar;
use crate::render::AtlasImage;
use crate::save::GameMode;
use crate::state::{AppState, PauseState};
use crate::ui::tile_rect;

/// Personal storage beyond the hotbar: three rows of `STORAGE_ROW_WIDTH`,
/// Minecraft's classic layout. Just constants for now - "how much inventory
/// space" is meant to be easy to make configurable later without touching
/// anything else here.
pub const STORAGE_ROW_WIDTH: usize = 9;
pub const STORAGE_ROWS: usize = 3;
pub const INVENTORY_SIZE: usize = STORAGE_ROW_WIDTH * STORAGE_ROWS;

#[derive(Resource)]
pub struct Inventory {
    pub slots: Vec<BlockId>,
}

impl Default for Inventory {
    fn default() -> Self {
        Self { slots: vec![AIR; INVENTORY_SIZE] }
    }
}

#[derive(Resource, Default)]
pub struct InventoryState {
    pub open: bool,
    was_grabbed: bool,
    tooltip: Option<String>,
}

fn close(inv: &mut InventoryState, window: &mut Window) {
    inv.open = false;
    inv.tooltip = None;
    if inv.was_grabbed {
        window.cursor_options.grab_mode = CursorGrabMode::Locked;
        window.cursor_options.visible = false;
    }
}

#[derive(Component)]
struct InventoryRoot;
#[derive(Component)]
struct SlotBlock(BlockId);
/// Marks a creative block-list cell specifically (as opposed to a survival
/// hotbar/storage slot, which shows the same `SlotBlock` component but isn't
/// clickable yet).
#[derive(Component)]
struct CreativeCell;
#[derive(Component)]
struct CreativeListRoot;
#[derive(Component)]
struct TooltipText;

fn reset_on_enter(mut inv: ResMut<InventoryState>) {
    inv.open = false;
    inv.tooltip = None;
}

fn despawn_inventory_ui(mut commands: Commands, roots: Query<Entity, With<InventoryRoot>>) {
    for e in &roots {
        commands.entity(e).despawn();
    }
}

/// E opens the screen (unless chat or the pause menu is up) and frees the
/// cursor; closing restores whatever grab state it found, same pattern as
/// `chat::toggle_chat`.
fn toggle_inventory(
    keys: Res<ButtonInput<KeyCode>>,
    chat: Res<ChatState>,
    paused: Res<PauseState>,
    mut inv: ResMut<InventoryState>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if chat.open || paused.open || !keys.just_pressed(KeyCode::KeyE) {
        return;
    }
    let Ok(mut window) = windows.single_mut() else { return };
    if inv.open {
        close(&mut inv, &mut window);
    } else {
        inv.was_grabbed = window.cursor_options.grab_mode != CursorGrabMode::None;
        inv.open = true;
        window.cursor_options.grab_mode = CursorGrabMode::None;
        window.cursor_options.visible = true;
    }
}

fn inventory_escape(
    keys: Res<ButtonInput<KeyCode>>,
    mut inv: ResMut<InventoryState>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if !inv.open || !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    let Ok(mut window) = windows.single_mut() else { return };
    close(&mut inv, &mut window);
}

/// Applied unconditionally to the (single) creative list while it exists -
/// nothing else is visible/scrollable at the same time, so there's no need
/// to hit-test which panel is under the cursor first.
fn scroll_creative_list(
    mut wheel: EventReader<MouseWheel>,
    mut lists: Query<&mut ScrollPosition, With<CreativeListRoot>>,
) {
    let dy: f32 = wheel.read().map(|e| e.y).sum();
    if dy == 0.0 {
        return;
    }
    for mut pos in &mut lists {
        pos.offset_y -= dy * 24.0;
    }
}

fn handle_creative_click(
    mut hotbar: ResMut<Hotbar>,
    clicks: Query<(&Interaction, &SlotBlock), (Changed<Interaction>, With<CreativeCell>)>,
) {
    for (interaction, slot) in &clicks {
        if *interaction == Interaction::Pressed {
            let selected = hotbar.selected;
            hotbar.slots[selected] = slot.0;
        }
    }
}

fn slot_hover_visuals(
    mut cells: Query<(&Interaction, &mut BackgroundColor), (Changed<Interaction>, With<SlotBlock>)>,
) {
    for (interaction, mut bg) in &mut cells {
        *bg = BackgroundColor(match interaction {
            Interaction::Pressed => Color::srgba(1.0, 1.0, 1.0, 0.35),
            Interaction::Hovered => Color::srgba(1.0, 1.0, 1.0, 0.22),
            Interaction::None => Color::srgba(0.0, 0.0, 0.0, 0.4),
        });
    }
}

fn track_hovered_block(
    registry: Res<BlockRegistry>,
    mut inv: ResMut<InventoryState>,
    slots: Query<(&Interaction, &SlotBlock)>,
) {
    if !inv.open {
        return;
    }
    let hovered = slots.iter().find_map(|(interaction, slot)| {
        if slot.0 != AIR && matches!(interaction, Interaction::Hovered | Interaction::Pressed) {
            Some(registry.def(slot.0).name.clone())
        } else {
            None
        }
    });
    if inv.tooltip != hovered {
        inv.tooltip = hovered;
    }
}

fn spawn_slot_row(parent: &mut ChildSpawnerCommands, tables: &BlockTables, atlas: &AtlasImage, slots: impl Iterator<Item = BlockId>) {
    parent
        .spawn(Node { column_gap: Val::Px(4.0), ..default() })
        .with_children(|row| {
            for id in slots {
                row.spawn((
                    Button,
                    SlotBlock(id),
                    Node {
                        width: Val::Px(46.0),
                        height: Val::Px(46.0),
                        border: UiRect::all(Val::Px(2.0)),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BorderColor(Color::srgba(1.0, 1.0, 1.0, 0.35)),
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.4)),
                ))
                .with_children(|cell| {
                    if id != AIR {
                        let tile = tables.0.tiles[id as usize * 6];
                        cell.spawn((
                            ImageNode { image: atlas.0.clone(), rect: Some(tile_rect(tile)), ..default() },
                            Node { width: Val::Px(34.0), height: Val::Px(34.0), ..default() },
                        ));
                    }
                });
            }
        });
}

#[allow(clippy::too_many_arguments)]
fn sync_inventory_screen(
    mut commands: Commands,
    inv: Res<InventoryState>,
    hotbar: Res<Hotbar>,
    inventory: Res<Inventory>,
    mode: Res<GameMode>,
    registry: Res<BlockRegistry>,
    tables: Option<Res<BlockTables>>,
    atlas: Option<Res<AtlasImage>>,
    roots: Query<Entity, With<InventoryRoot>>,
) {
    if !inv.open {
        for e in &roots {
            commands.entity(e).despawn();
        }
        return;
    }
    let (Some(tables), Some(atlas)) = (tables, atlas) else { return };
    if !inv.is_changed() && !hotbar.is_changed() && !inventory.is_changed() {
        return;
    }
    for e in &roots {
        commands.entity(e).despawn();
    }

    commands
        .spawn((
            InventoryRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    padding: UiRect::all(Val::Px(20.0)),
                    row_gap: Val::Px(12.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.08, 0.09, 0.12, 0.92)),
            ))
            .with_children(|panel| match *mode {
                GameMode::Survival => {
                    panel.spawn((Text::new("Inventory"), TextFont { font_size: 24.0, ..default() }, TextColor(Color::WHITE)));
                    panel.spawn((
                        Text::new("Hotbar"),
                        TextFont { font_size: 12.0, ..default() },
                        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.6)),
                    ));
                    spawn_slot_row(panel, &tables, &atlas, hotbar.slots.iter().copied());
                    panel.spawn((
                        Text::new("Storage"),
                        TextFont { font_size: 12.0, ..default() },
                        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.6)),
                    ));
                    for row in inventory.slots.chunks(STORAGE_ROW_WIDTH) {
                        spawn_slot_row(panel, &tables, &atlas, row.iter().copied());
                    }
                }
                GameMode::Creative => {
                    panel.spawn((Text::new("Blocks"), TextFont { font_size: 24.0, ..default() }, TextColor(Color::WHITE)));
                    panel
                        .spawn((
                            CreativeListRoot,
                            ScrollPosition::default(),
                            Node {
                                width: Val::Px(9.0 * 50.0),
                                height: Val::Px(400.0),
                                flex_direction: FlexDirection::Row,
                                flex_wrap: FlexWrap::Wrap,
                                align_content: AlignContent::FlexStart,
                                overflow: Overflow::scroll_y(),
                                ..default()
                            },
                        ))
                        .with_children(|grid| {
                            for (id, _def) in registry.defs.iter().enumerate().skip(1) {
                                let id = id as BlockId;
                                let tile = tables.0.tiles[id as usize * 6];
                                grid.spawn((
                                    Button,
                                    SlotBlock(id),
                                    CreativeCell,
                                    Node {
                                        width: Val::Px(46.0),
                                        height: Val::Px(46.0),
                                        margin: UiRect::all(Val::Px(2.0)),
                                        align_items: AlignItems::Center,
                                        justify_content: JustifyContent::Center,
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.4)),
                                ))
                                .with_children(|cell| {
                                    cell.spawn((
                                        ImageNode { image: atlas.0.clone(), rect: Some(tile_rect(tile)), ..default() },
                                        Node { width: Val::Px(34.0), height: Val::Px(34.0), ..default() },
                                    ));
                                });
                            }
                        });
                }
            });

            root.spawn((
                TooltipText,
                Text::new(""),
                TextFont { font_size: 14.0, ..default() },
                TextColor(Color::WHITE),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.85)),
                Visibility::Hidden,
            ));
        });
}

fn sync_tooltip_ui(
    inv: Res<InventoryState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut tooltips: Query<(&mut Text, &mut Node, &mut Visibility), With<TooltipText>>,
) {
    let Ok((mut text, mut node, mut vis)) = tooltips.single_mut() else { return };
    let Some(name) = &inv.tooltip else {
        *vis = Visibility::Hidden;
        return;
    };
    *vis = Visibility::Visible;
    text.0 = name.clone();
    if let Ok(window) = windows.single() {
        if let Some(cursor) = window.cursor_position() {
            node.left = Val::Px(cursor.x + 16.0);
            node.top = Val::Px(cursor.y + 16.0);
        }
    }
}

pub struct InventoryPlugin;

impl Plugin for InventoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Inventory>()
            .init_resource::<InventoryState>()
            .add_systems(OnEnter(AppState::InGame), reset_on_enter)
            .add_systems(OnExit(AppState::InGame), despawn_inventory_ui)
            .add_systems(
                Update,
                (
                    toggle_inventory,
                    inventory_escape,
                    scroll_creative_list,
                    handle_creative_click,
                    slot_hover_visuals,
                    track_hovered_block,
                    sync_inventory_screen,
                    sync_tooltip_ui,
                )
                    .chain()
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_inventory_is_empty() {
        let inv = Inventory::default();
        assert_eq!(inv.slots.len(), INVENTORY_SIZE);
        assert!(inv.slots.iter().all(|&id| id == AIR));
    }
}
