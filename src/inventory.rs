//! In-world inventory screen. Press E to open/close it (Escape also closes
//! it, taking priority over the pause menu), or right-click a crafting
//! station. Survival shows the hotbar plus three rows of personal storage -
//! both start empty in a new world and fill up as you pick up what you mine
//! (`items.rs`); both are saved with the world. Creative shows the same
//! view *or* a scrollable list of every item, switched with a button above
//! the grid; clicking an item there gives a full stack of it in the selected
//! hotbar slot. `Inventory` is one resource shared by both modes, so
//! switching modes (including mid-session via `/mode`) never touches its
//! contents. Hovering any occupied slot shows the item's name.
//!
//! **Moving items.** Click a slot to pick its stack up onto the cursor
//! ([`HeldStack`]); click another slot to put it down - into an empty slot,
//! onto a stack of the same item (as much as fits), or swapping with a
//! different one. Shift-click moves a stack straight across: from the
//! hotbar into storage, or from storage into the hotbar ([`quick_move`]).
//! Closing the screen with something on the cursor puts it back in your
//! inventory, dropping whatever doesn't fit.
//!
//! **Crafting** (`crafting.rs`) is the panel on the right: every recipe
//! you've discovered that can be made by hand or at a station within reach.
//! Click to craft one, shift-click to craft as many as you can.

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, PrimaryWindow};

use crate::chat::ChatState;
use crate::crafting::{craft, stations_near, times_craftable, KnownItems, NearbyStations, RecipeBook};
use crate::interact::Hotbar;
use crate::item_registry::{ItemId, ItemRegistry, ItemStack, NO_ITEM};
use crate::items::{give, SpawnDrop, THROWN_PICKUP_DELAY};
use crate::player::Player;
use crate::save::{GameMode, InventorySave, SlotSave};
use crate::state::{AppState, DeathState, PauseState};
use crate::ui::ItemIcons;
use crate::world::ChunkMap;

/// Personal storage beyond the hotbar: three rows of `STORAGE_ROW_WIDTH`,
/// Minecraft's classic layout. Just constants for now - "how much inventory
/// space" is meant to be easy to make configurable later without touching
/// anything else here.
pub const STORAGE_ROW_WIDTH: usize = 9;
pub const STORAGE_ROWS: usize = 3;
pub const INVENTORY_SIZE: usize = STORAGE_ROW_WIDTH * STORAGE_ROWS;

#[derive(Resource)]
pub struct Inventory {
    pub slots: Vec<ItemStack>,
}

impl Default for Inventory {
    fn default() -> Self {
        Self { slots: vec![ItemStack::EMPTY; INVENTORY_SIZE] }
    }
}

/// The stack picked up onto the cursor while the inventory screen is open.
#[derive(Resource, Default)]
pub struct HeldStack(pub ItemStack);

/// Which slot a click landed on.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotRef {
    Hotbar(usize),
    Storage(usize),
}

/// A plain click on a slot with `held` on the cursor: pick the slot's stack
/// up, put the held one down, add as much as fits onto the same item, or
/// swap two different ones. `max_stack` is for the held item.
pub fn click_slot(held: &mut ItemStack, slot: &mut ItemStack, max_stack: u32) {
    if held.is_empty() {
        *held = std::mem::take(slot);
    } else if slot.is_empty() {
        *slot = std::mem::take(held);
    } else if slot.id == held.id {
        let moved = held.count.min(max_stack.saturating_sub(slot.count));
        slot.count += moved;
        held.count -= moved;
        if held.count == 0 {
            *held = ItemStack::EMPTY;
        }
    } else {
        std::mem::swap(held, slot);
    }
}

/// Shift-click: moves a stack from the hotbar into storage, or from storage
/// into the hotbar - topping up stacks of the same item first, then the
/// first empty slots. What doesn't fit stays where it was.
pub fn quick_move(from: SlotRef, hotbar: &mut [ItemStack], storage: &mut [ItemStack], max_stack: impl Fn(ItemId) -> u32) {
    let (source, dest) = match from {
        SlotRef::Hotbar(i) => (&mut hotbar[i], storage),
        SlotRef::Storage(i) => (&mut storage[i], hotbar),
    };
    if source.is_empty() {
        return;
    }
    let stack = std::mem::take(source);
    let left = give(dest, &mut [], stack, max_stack(stack.id));
    if left > 0 {
        *source = ItemStack { id: stack.id, count: left };
    }
}

/// Slots as saved: by item id, `None` for empty.
pub fn stacks_to_save(slots: &[ItemStack], registry: &ItemRegistry) -> Vec<Option<SlotSave>> {
    slots.iter().map(|s| stack_to_save(*s, registry)).collect()
}

fn stack_to_save(stack: ItemStack, registry: &ItemRegistry) -> Option<SlotSave> {
    (!stack.is_empty()).then(|| SlotSave { block: registry.def(stack.id).id.clone(), count: stack.count })
}

fn stack_from_save(saved: &SlotSave, registry: &ItemRegistry) -> ItemStack {
    match registry.by_name(&saved.block) {
        Some(id) if id != NO_ITEM && saved.count > 0 => {
            ItemStack { id, count: saved.count.min(registry.max_stack(id)) }
        }
        _ => ItemStack::EMPTY,
    }
}

/// The inverse of [`stacks_to_save`], always exactly `len` slots long. An
/// item that no longer exists (a removed mod) comes back as an empty slot,
/// and a count is clamped to what one slot can hold, so a hand-edited or
/// out-of-date save can't put anything impossible in a slot.
pub fn stacks_from_save(saved: &[Option<SlotSave>], len: usize, registry: &ItemRegistry) -> Vec<ItemStack> {
    let mut slots = vec![ItemStack::EMPTY; len];
    for (slot, saved) in slots.iter_mut().zip(saved) {
        if let Some(saved) = saved {
            *slot = stack_from_save(saved, registry);
        }
    }
    slots
}

pub fn inventory_to_save(hotbar: &Hotbar, inventory: &Inventory, held: &HeldStack, registry: &ItemRegistry) -> InventorySave {
    InventorySave {
        hotbar: stacks_to_save(&hotbar.slots, registry),
        storage: stacks_to_save(&inventory.slots, registry),
        selected: hotbar.selected,
        held: stack_to_save(held.0, registry),
    }
}

/// Restores a saved hotbar and storage, or empties both for a world that
/// has none saved (a new world, or one from before inventories were saved).
/// A stack saved on the cursor goes back into the inventory, since the
/// screen is closed on load; if somehow there's no room, it stays held and
/// returns the next time the screen closes.
pub fn restore_inventory(
    saved: Option<&InventorySave>,
    hotbar: &mut Hotbar,
    inventory: &mut Inventory,
    held: &mut HeldStack,
    registry: &ItemRegistry,
) {
    *hotbar = Hotbar::default();
    *inventory = Inventory::default();
    held.0 = ItemStack::EMPTY;
    let Some(saved) = saved else { return };
    hotbar.slots = stacks_from_save(&saved.hotbar, hotbar.slots.len(), registry);
    inventory.slots = stacks_from_save(&saved.storage, inventory.slots.len(), registry);
    hotbar.selected = saved.selected.min(hotbar.slots.len() - 1);
    if let Some(stack) = saved.held.as_ref().map(|s| stack_from_save(s, registry)).filter(|s| !s.is_empty()) {
        let left = give(&mut hotbar.slots, &mut inventory.slots, stack, registry.max_stack(stack.id));
        held.0 = ItemStack { id: stack.id, count: left };
        if left == 0 {
            held.0 = ItemStack::EMPTY;
        }
    }
}

/// Which panel Creative's inventory screen shows - toggled by
/// `CreativeTabButton`, independent of the survival-only hotbar+storage
/// view (which has no tab, since it's the only thing Survival ever shows).
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum CreativeTab {
    #[default]
    Blocks,
    Inventory,
}

#[derive(Resource, Default)]
pub struct InventoryState {
    pub open: bool,
    was_grabbed: bool,
    tooltip: Option<String>,
    creative_tab: CreativeTab,
}

/// Opens the screen, freeing the cursor - from E, or from right-clicking a
/// crafting station (`interact.rs`).
pub fn open_inventory(inv: &mut InventoryState, window: &mut Window) {
    inv.was_grabbed = window.cursor_options.grab_mode != CursorGrabMode::None;
    inv.open = true;
    window.cursor_options.grab_mode = CursorGrabMode::None;
    window.cursor_options.visible = true;
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
/// The item a slot shows, for its tooltip.
#[derive(Component)]
struct SlotItem(ItemId);
/// A cell in Creative's item list.
#[derive(Component)]
struct CreativeCell;
#[derive(Component)]
struct CreativeListRoot;
/// Switches Creative's inventory screen between the block list and the
/// hotbar+storage view (see `CreativeTab`).
#[derive(Component)]
struct CreativeTabButton;
#[derive(Component)]
struct TooltipText;
/// The held stack, drawn under the cursor.
#[derive(Component)]
struct HeldIcon;
/// A row in the crafting panel: the recipe's index in `RecipeBook`.
#[derive(Component)]
struct CraftButton(usize);

const SLOT_BG: Color = Color::srgba(0.0, 0.0, 0.0, 0.4);

fn reset_on_enter(mut inv: ResMut<InventoryState>) {
    inv.open = false;
    inv.tooltip = None;
    inv.creative_tab = CreativeTab::default();
}

fn despawn_inventory_ui(mut commands: Commands, roots: Query<Entity, With<InventoryRoot>>) {
    for e in &roots {
        commands.entity(e).despawn();
    }
}

/// E opens the screen (unless chat, the pause menu or the death screen is
/// up) and frees the cursor; closing restores whatever grab state it found,
/// same pattern as `chat::toggle_chat`.
fn toggle_inventory(
    keys: Res<ButtonInput<KeyCode>>,
    chat: Res<ChatState>,
    paused: Res<PauseState>,
    death: Res<DeathState>,
    mut inv: ResMut<InventoryState>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if chat.open || paused.open || death.open || !keys.just_pressed(KeyCode::KeyE) {
        return;
    }
    let Ok(mut window) = windows.single_mut() else { return };
    if inv.open {
        close(&mut inv, &mut window);
    } else {
        open_inventory(&mut inv, &mut window);
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

/// With the screen closed (however that happened), a stack left on the
/// cursor goes back into the inventory; whatever doesn't fit is dropped at
/// the player's feet.
fn return_held_stack(
    inv: Res<InventoryState>,
    registry: Res<ItemRegistry>,
    mut held: ResMut<HeldStack>,
    mut hotbar: ResMut<Hotbar>,
    mut inventory: ResMut<Inventory>,
    players: Query<&Player>,
    mut drops: EventWriter<SpawnDrop>,
) {
    if inv.open || held.0.is_empty() {
        return;
    }
    let stack = std::mem::take(&mut held.0);
    let left = give(&mut hotbar.slots, &mut inventory.slots, stack, registry.max_stack(stack.id));
    if left > 0 {
        if let Ok(player) = players.single() {
            drops.write(SpawnDrop {
                pos: player.eye() - Vec3::Y * 0.3 + player.look_dir() * 0.4,
                stack: ItemStack { id: stack.id, count: left },
                vel: player.look_dir() * 2.0,
                pickup_delay: THROWN_PICKUP_DELAY,
            });
        }
    }
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

fn click_slots(
    keys: Res<ButtonInput<KeyCode>>,
    registry: Res<ItemRegistry>,
    mut held: ResMut<HeldStack>,
    mut hotbar: ResMut<Hotbar>,
    mut inventory: ResMut<Inventory>,
    clicks: Query<(&Interaction, &SlotRef), Changed<Interaction>>,
) {
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    for (interaction, &slot) in &clicks {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if shift && held.0.is_empty() {
            quick_move(slot, &mut hotbar.slots, &mut inventory.slots, |id| registry.max_stack(id));
            continue;
        }
        let target = match slot {
            SlotRef::Hotbar(i) => &mut hotbar.slots[i],
            SlotRef::Storage(i) => &mut inventory.slots[i],
        };
        let max = registry.max_stack(if held.0.is_empty() { target.id } else { held.0.id });
        click_slot(&mut held.0, target, max);
    }
}

fn handle_creative_click(
    registry: Res<ItemRegistry>,
    mut hotbar: ResMut<Hotbar>,
    clicks: Query<(&Interaction, &SlotItem), (Changed<Interaction>, With<CreativeCell>)>,
) {
    for (interaction, item) in &clicks {
        if *interaction == Interaction::Pressed {
            let selected = hotbar.selected;
            hotbar.slots[selected] = ItemStack { id: item.0, count: registry.max_stack(item.0) };
        }
    }
}

/// Crafts the clicked recipe once, or as many times as possible with shift.
#[allow(clippy::too_many_arguments)]
fn craft_clicks(
    keys: Res<ButtonInput<KeyCode>>,
    book: Res<RecipeBook>,
    registry: Res<ItemRegistry>,
    mut hotbar: ResMut<Hotbar>,
    mut inventory: ResMut<Inventory>,
    players: Query<&Player>,
    mut drops: EventWriter<SpawnDrop>,
    clicks: Query<(&Interaction, &CraftButton), Changed<Interaction>>,
) {
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    for (interaction, button) in &clicks {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let recipe = &book.recipes[button.0];
        let max = registry.max_stack(recipe.output.id);
        let times = if shift { times_craftable(recipe, &hotbar.slots, &inventory.slots) } else { 1 };
        let mut overflow = 0;
        for _ in 0..times {
            match craft(recipe, &mut hotbar.slots, &mut inventory.slots, max) {
                Some(left) => overflow += left,
                None => break,
            }
        }
        if overflow > 0 {
            if let Ok(player) = players.single() {
                drops.write(SpawnDrop {
                    pos: player.pos + Vec3::Y * 0.5,
                    stack: ItemStack { id: recipe.output.id, count: overflow },
                    vel: Vec3::ZERO,
                    pickup_delay: THROWN_PICKUP_DELAY,
                });
            }
        }
    }
}

/// While the screen is open, which stations are within reach - written only
/// when it changes, since a change rebuilds the screen.
fn track_nearby_stations(
    inv: Res<InventoryState>,
    map: Res<ChunkMap>,
    book: Res<RecipeBook>,
    players: Query<&Player>,
    mut nearby: ResMut<NearbyStations>,
) {
    if !inv.open {
        return;
    }
    let Ok(player) = players.single() else { return };
    let found = NearbyStations(stations_near(&map, &book, player.eye().floor().as_ivec3()));
    if *nearby != found {
        *nearby = found;
    }
}

fn slot_hover_visuals(
    mut cells: Query<(&Interaction, &mut BackgroundColor), (Changed<Interaction>, With<SlotItem>)>,
) {
    for (interaction, mut bg) in &mut cells {
        *bg = BackgroundColor(match interaction {
            Interaction::Pressed => Color::srgba(1.0, 1.0, 1.0, 0.35),
            Interaction::Hovered => Color::srgba(1.0, 1.0, 1.0, 0.22),
            Interaction::None => SLOT_BG,
        });
    }
}

fn button_hover(
    mut buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (Changed<Interaction>, Or<(With<CreativeTabButton>, With<CraftButton>)>),
    >,
) {
    for (interaction, mut bg) in &mut buttons {
        let base = bg.0.alpha().min(0.15);
        *bg = BackgroundColor(match interaction {
            Interaction::Pressed => Color::srgba(1.0, 1.0, 1.0, 0.35),
            Interaction::Hovered => Color::srgba(1.0, 1.0, 1.0, 0.25),
            Interaction::None => Color::srgba(1.0, 1.0, 1.0, base.max(0.06)),
        });
    }
}

fn track_hovered_item(
    registry: Res<ItemRegistry>,
    mut inv: ResMut<InventoryState>,
    slots: Query<(&Interaction, &SlotItem)>,
) {
    if !inv.open {
        return;
    }
    let hovered = slots.iter().find_map(|(interaction, slot)| {
        (slot.0 != NO_ITEM && matches!(interaction, Interaction::Hovered | Interaction::Pressed))
            .then(|| registry.def(slot.0).name.clone())
    });
    if inv.tooltip != hovered {
        inv.tooltip = hovered;
    }
}

/// An item icon of `size` pixels, with a count badge above 1.
fn spawn_stack(parent: &mut ChildSpawnerCommands, icons: &ItemIcons, stack: ItemStack, size: f32) {
    let Some(icon) = (!stack.is_empty()).then(|| icons.icon(stack.id)).flatten() else { return };
    parent.spawn((icon, Node { width: Val::Px(size), height: Val::Px(size), ..default() }));
    if stack.count > 1 {
        parent.spawn((
            Text::new(stack.count.to_string()),
            TextFont { font_size: 12.0, ..default() },
            TextColor(Color::WHITE),
            Node { position_type: PositionType::Absolute, right: Val::Px(3.0), bottom: Val::Px(1.0), ..default() },
        ));
    }
}

fn spawn_slot_row(
    parent: &mut ChildSpawnerCommands,
    icons: &ItemIcons,
    slots: impl Iterator<Item = (SlotRef, ItemStack)>,
) {
    parent
        .spawn(Node { column_gap: Val::Px(4.0), ..default() })
        .with_children(|row| {
            for (slot, stack) in slots {
                row.spawn((
                    Button,
                    slot,
                    SlotItem(stack.id),
                    Node {
                        width: Val::Px(46.0),
                        height: Val::Px(46.0),
                        border: UiRect::all(Val::Px(2.0)),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BorderColor(Color::srgba(1.0, 1.0, 1.0, 0.35)),
                    BackgroundColor(SLOT_BG),
                ))
                .with_children(|cell| spawn_stack(cell, icons, stack, 34.0));
            }
        });
}

/// The hotbar+storage view shared by Survival (its only screen) and
/// Creative's "Inventory" tab - one `Inventory` resource either way, so
/// contents survive a mode switch untouched.
fn spawn_hotbar_and_storage(panel: &mut ChildSpawnerCommands, icons: &ItemIcons, hotbar: &Hotbar, inventory: &Inventory) {
    let label = |panel: &mut ChildSpawnerCommands, text: &str| {
        panel.spawn((
            Text::new(text),
            TextFont { font_size: 12.0, ..default() },
            TextColor(Color::srgba(1.0, 1.0, 1.0, 0.6)),
        ));
    };
    label(panel, "Storage");
    for (row, slots) in inventory.slots.chunks(STORAGE_ROW_WIDTH).enumerate() {
        let start = row * STORAGE_ROW_WIDTH;
        let slots = slots.iter().enumerate().map(|(i, s)| (SlotRef::Storage(start + i), *s));
        spawn_slot_row(panel, icons, slots);
    }
    label(panel, "Hotbar");
    spawn_slot_row(panel, icons, hotbar.slots.iter().enumerate().map(|(i, s)| (SlotRef::Hotbar(i), *s)));
}

/// The crafting panel: discovered recipes usable here, craftable ones lit.
#[allow(clippy::too_many_arguments)]
fn spawn_crafting_panel(
    parent: &mut ChildSpawnerCommands,
    icons: &ItemIcons,
    book: &RecipeBook,
    known: &KnownItems,
    nearby: &NearbyStations,
    hotbar: &Hotbar,
    inventory: &Inventory,
) {
    let where_text = {
        let mut names: Vec<String> = nearby.0.iter().map(|&b| icons.blocks.def(b).name.clone()).collect();
        names.sort();
        if names.is_empty() { "By hand".to_string() } else { format!("By hand + {}", names.join(", ")) }
    };
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(20.0)),
                row_gap: Val::Px(6.0),
                width: Val::Px(320.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.08, 0.09, 0.12, 0.92)),
        ))
        .with_children(|panel| {
            panel.spawn((Text::new("Crafting"), TextFont { font_size: 24.0, ..default() }, TextColor(Color::WHITE)));
            panel.spawn((
                Text::new(where_text),
                TextFont { font_size: 12.0, ..default() },
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.6)),
            ));
            let mut any = false;
            for (index, recipe) in book.visible(&nearby.0, &known.0) {
                any = true;
                let craftable = times_craftable(recipe, &hotbar.slots, &inventory.slots) > 0;
                let dim = if craftable { 1.0 } else { 0.45 };
                panel
                    .spawn((
                        Button,
                        CraftButton(index),
                        Node {
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(8.0),
                            padding: UiRect::all(Val::Px(4.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, if craftable { 0.12 } else { 0.04 })),
                    ))
                    .with_children(|row| {
                        row.spawn(Node {
                            width: Val::Px(40.0),
                            height: Val::Px(40.0),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            ..default()
                        })
                        .with_children(|cell| spawn_stack(cell, icons, recipe.output, 32.0));
                        row.spawn(Node { flex_direction: FlexDirection::Column, ..default() }).with_children(|text| {
                            text.spawn((
                                Text::new(icons.name(recipe.output.id)),
                                TextFont { font_size: 15.0, ..default() },
                                TextColor(Color::srgba(1.0, 1.0, 1.0, dim)),
                            ));
                            text.spawn(Node { column_gap: Val::Px(6.0), ..default() }).with_children(|inputs| {
                                for input in &recipe.inputs {
                                    let have = crate::crafting::count_of(
                                        hotbar.slots.iter().chain(&inventory.slots),
                                        input.id,
                                    );
                                    let color = if have >= input.count {
                                        Color::srgba(0.75, 0.95, 0.75, dim)
                                    } else {
                                        Color::srgba(1.0, 0.55, 0.5, 0.9)
                                    };
                                    inputs.spawn((
                                        Text::new(format!("{} {}", input.count, icons.name(input.id))),
                                        TextFont { font_size: 12.0, ..default() },
                                        TextColor(color),
                                    ));
                                }
                            });
                        });
                    });
            }
            if !any {
                panel.spawn((
                    Text::new("Nothing yet - pick things up to discover what you can make."),
                    TextFont { font_size: 13.0, ..default() },
                    TextColor(Color::srgba(1.0, 1.0, 1.0, 0.6)),
                ));
            }
        });
}

/// Resources the inventory screen is drawn from - rebuilt when any changes.
#[derive(bevy::ecs::system::SystemParam)]
struct ScreenState<'w> {
    inv: Res<'w, InventoryState>,
    hotbar: Res<'w, Hotbar>,
    inventory: Res<'w, Inventory>,
    held: Res<'w, HeldStack>,
    book: Res<'w, RecipeBook>,
    known: Res<'w, KnownItems>,
    nearby: Res<'w, NearbyStations>,
    mode: Res<'w, GameMode>,
}

impl ScreenState<'_> {
    fn changed(&self) -> bool {
        self.inv.is_changed()
            || self.hotbar.is_changed()
            || self.inventory.is_changed()
            || self.held.is_changed()
            || self.known.is_changed()
            || self.nearby.is_changed()
            || self.mode.is_changed()
    }
}

fn sync_inventory_screen(
    mut commands: Commands,
    state: ScreenState,
    icons: ItemIcons,
    roots: Query<Entity, With<InventoryRoot>>,
) {
    if !state.inv.open {
        for e in &roots {
            commands.entity(e).despawn();
        }
        return;
    }
    if !icons.ready() || (!state.changed() && !roots.is_empty()) {
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
                column_gap: Val::Px(12.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        ))
        .with_children(|root| {
            let show_crafting = *state.mode == GameMode::Survival || state.inv.creative_tab == CreativeTab::Inventory;
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
            .with_children(|panel| match *state.mode {
                GameMode::Survival => {
                    panel.spawn((Text::new("Inventory"), TextFont { font_size: 24.0, ..default() }, TextColor(Color::WHITE)));
                    spawn_hotbar_and_storage(panel, &icons, &state.hotbar, &state.inventory);
                }
                GameMode::Creative => {
                    let title = match state.inv.creative_tab {
                        CreativeTab::Blocks => "Items",
                        CreativeTab::Inventory => "Inventory",
                    };
                    panel.spawn((Text::new(title), TextFont { font_size: 24.0, ..default() }, TextColor(Color::WHITE)));
                    let tab_label = match state.inv.creative_tab {
                        CreativeTab::Blocks => "Show Inventory",
                        CreativeTab::Inventory => "Show Items",
                    };
                    panel
                        .spawn((
                            Button,
                            CreativeTabButton,
                            Node { padding: UiRect::axes(Val::Px(14.0), Val::Px(6.0)), ..default() },
                            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.15)),
                        ))
                        .with_children(|b| {
                            b.spawn((Text::new(tab_label), TextFont { font_size: 14.0, ..default() }, TextColor(Color::WHITE)));
                        });
                    match state.inv.creative_tab {
                        CreativeTab::Blocks => {
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
                                    for id in 1..icons.items.defs.len() as ItemId {
                                        grid.spawn((
                                            Button,
                                            SlotItem(id),
                                            CreativeCell,
                                            Node {
                                                width: Val::Px(46.0),
                                                height: Val::Px(46.0),
                                                margin: UiRect::all(Val::Px(2.0)),
                                                align_items: AlignItems::Center,
                                                justify_content: JustifyContent::Center,
                                                ..default()
                                            },
                                            BackgroundColor(SLOT_BG),
                                        ))
                                        .with_children(|cell| {
                                            spawn_stack(cell, &icons, ItemStack { id, count: 1 }, 34.0)
                                        });
                                    }
                                });
                        }
                        CreativeTab::Inventory => {
                            spawn_hotbar_and_storage(panel, &icons, &state.hotbar, &state.inventory);
                        }
                    }
                }
            });
            if show_crafting {
                spawn_crafting_panel(root, &icons, &state.book, &state.known, &state.nearby, &state.hotbar, &state.inventory);
            }

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
            root.spawn((
                HeldIcon,
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Px(40.0),
                    height: Val::Px(40.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                // Clicks go to the slot underneath, not the icon.
                bevy::ui::FocusPolicy::Pass,
            ))
            .with_children(|cell| spawn_stack(cell, &icons, state.held.0, 34.0));
        });
}

fn sync_cursor_ui(
    inv: Res<InventoryState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut tooltips: Query<(&mut Text, &mut Node, &mut Visibility), (With<TooltipText>, Without<HeldIcon>)>,
    mut held_icons: Query<&mut Node, (With<HeldIcon>, Without<TooltipText>)>,
) {
    let cursor = windows.single().ok().and_then(|w| w.cursor_position());
    if let (Ok(mut node), Some(cursor)) = (held_icons.single_mut(), cursor) {
        node.left = Val::Px(cursor.x - 20.0);
        node.top = Val::Px(cursor.y - 20.0);
    }
    let Ok((mut text, mut node, mut vis)) = tooltips.single_mut() else { return };
    let Some(name) = &inv.tooltip else {
        *vis = Visibility::Hidden;
        return;
    };
    *vis = Visibility::Visible;
    if text.0 != *name {
        text.0 = name.clone();
    }
    if let Some(cursor) = cursor {
        node.left = Val::Px(cursor.x + 16.0);
        node.top = Val::Px(cursor.y + 16.0);
    }
}

fn toggle_creative_tab(
    mut inv: ResMut<InventoryState>,
    clicks: Query<&Interaction, (Changed<Interaction>, With<CreativeTabButton>)>,
) {
    for interaction in &clicks {
        if *interaction == Interaction::Pressed {
            inv.creative_tab = match inv.creative_tab {
                CreativeTab::Blocks => CreativeTab::Inventory,
                CreativeTab::Inventory => CreativeTab::Blocks,
            };
        }
    }
}

pub struct InventoryPlugin;

impl Plugin for InventoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Inventory>()
            .init_resource::<InventoryState>()
            .init_resource::<HeldStack>()
            .init_resource::<KnownItems>()
            .init_resource::<NearbyStations>()
            .init_resource::<DeathState>()
            .add_event::<SpawnDrop>()
            .add_systems(OnEnter(AppState::InGame), reset_on_enter)
            .add_systems(OnExit(AppState::InGame), despawn_inventory_ui)
            .add_systems(
                Update,
                (
                    toggle_inventory,
                    inventory_escape,
                    return_held_stack,
                    scroll_creative_list,
                    click_slots,
                    handle_creative_click,
                    craft_clicks,
                    toggle_creative_tab,
                    track_nearby_stations,
                    slot_hover_visuals,
                    button_hover,
                    track_hovered_item,
                    sync_inventory_screen,
                    sync_cursor_ui,
                )
                    .chain()
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(id: ItemId, count: u32) -> ItemStack {
        ItemStack { id, count }
    }

    #[test]
    fn clicking_picks_up_puts_down_merges_and_swaps() {
        let mut held = ItemStack::EMPTY;
        let mut a = stack(3, 10);
        let mut b = ItemStack::EMPTY;
        click_slot(&mut held, &mut a, 64);
        assert_eq!((held, a), (stack(3, 10), ItemStack::EMPTY), "picked up");
        click_slot(&mut held, &mut b, 64);
        assert_eq!((held, b), (ItemStack::EMPTY, stack(3, 10)), "put down");

        let mut held = stack(3, 60);
        let mut c = stack(3, 10);
        click_slot(&mut held, &mut c, 64);
        assert_eq!((held, c), (stack(3, 6), stack(3, 64)), "merged as much as fits");

        let mut held = stack(5, 2);
        let mut d = stack(3, 7);
        click_slot(&mut held, &mut d, 64);
        assert_eq!((held, d), (stack(3, 7), stack(5, 2)), "swapped");

        let mut held = ItemStack::EMPTY;
        let mut e = ItemStack::EMPTY;
        click_slot(&mut held, &mut e, 64);
        assert!(held.is_empty() && e.is_empty(), "empty on empty does nothing");
    }

    #[test]
    fn shift_click_moves_between_hotbar_and_storage() {
        let mut hotbar = vec![stack(3, 10), stack(4, 1), ItemStack::EMPTY];
        let mut storage = vec![ItemStack::EMPTY, stack(3, 60), ItemStack::EMPTY];
        quick_move(SlotRef::Hotbar(0), &mut hotbar, &mut storage, |_| 64);
        assert_eq!(storage[1], stack(3, 64), "topped up the matching stack first");
        assert_eq!(storage[0], stack(3, 6), "then the first empty slot");
        assert!(hotbar[0].is_empty());

        quick_move(SlotRef::Storage(1), &mut hotbar, &mut storage, |_| 64);
        assert_eq!(hotbar[0], stack(3, 64), "storage -> hotbar's first empty slot");
        assert!(storage[1].is_empty());
        assert_eq!(hotbar[1], stack(4, 1), "an unrelated hotbar stack is untouched");
    }

    #[test]
    fn shift_click_into_a_full_area_leaves_the_rest_where_it_was() {
        let mut hotbar = vec![stack(3, 10)];
        let mut storage = vec![stack(3, 60), stack(9, 1)];
        quick_move(SlotRef::Hotbar(0), &mut hotbar, &mut storage, |_| 64);
        assert_eq!(storage[0], stack(3, 64));
        assert_eq!(hotbar[0], stack(3, 6), "the 6 that didn't fit stay put");
    }

    #[test]
    fn slots_round_trip_through_the_save_and_bad_entries_come_back_empty() {
        let blocks = crate::blocks::BlockRegistry::with_defaults();
        let registry = ItemRegistry::with_defaults(&blocks);
        let dirt = registry.id("dirt");
        let stick = registry.id("stick");
        let slots = vec![stack(dirt, 12), ItemStack::EMPTY, stack(stick, 1)];
        let saved = stacks_to_save(&slots, &registry);
        assert_eq!(saved[1], None);
        assert_eq!(stacks_from_save(&saved, 3, &registry), slots);

        let odd = vec![
            Some(SlotSave { block: "no_such_item".into(), count: 3 }),
            Some(SlotSave { block: "dirt".into(), count: 100_000 }),
            Some(SlotSave { block: "air".into(), count: 3 }),
            Some(SlotSave { block: "dirt".into(), count: 4 }),
        ];
        let back = stacks_from_save(&odd, 3, &registry);
        assert_eq!(back.len(), 3, "a longer saved list is cut to the slots that exist");
        assert!(back[0].is_empty(), "unknown item");
        assert_eq!(back[1], stack(dirt, registry.max_stack(dirt)), "count clamped");
        assert!(back[2].is_empty(), "air isn't an item");
    }

    #[test]
    fn a_stack_saved_on_the_cursor_comes_back_into_the_inventory() {
        let blocks = crate::blocks::BlockRegistry::with_defaults();
        let registry = ItemRegistry::with_defaults(&blocks);
        let saved = InventorySave {
            hotbar: vec![],
            storage: vec![],
            selected: 0,
            held: Some(SlotSave { block: "stick".into(), count: 5 }),
        };
        let (mut hotbar, mut inventory, mut held) = (Hotbar::default(), Inventory::default(), HeldStack::default());
        restore_inventory(Some(&saved), &mut hotbar, &mut inventory, &mut held, &registry);
        assert!(held.0.is_empty());
        assert_eq!(hotbar.slots[0], stack(registry.id("stick"), 5));
    }

    #[test]
    fn default_inventory_is_empty() {
        let inv = Inventory::default();
        assert_eq!(inv.slots.len(), INVENTORY_SIZE);
        assert!(inv.slots.iter().all(|s| s.is_empty()));
    }
}
