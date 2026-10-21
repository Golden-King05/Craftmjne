//! Dropped items: what a block leaves behind when it's mined in Survival,
//! what Q throws out of the hotbar, and what a player's inventory scatters
//! into on death.
//!
//! Each one is an entity with a [`DroppedItem`] - a small spinning cube of the
//! block's own textures, drawn through the chunk material
//! (`mesher::cube_mesh`) and lit by whichever cell it's in. It falls, settles
//! on the ground, floats up in water, merges with nearby stacks of the same
//! block, is picked up by walking into it (hotbar first, then storage - see
//! [`give`]), and despawns after [`DESPAWN_SECONDS`]. Items are saved with
//! the world (`save::DroppedItemSave`, written by `world::write_save`).
//!
//! The physics is a cut-down copy of the player's: axis-separated AABB
//! moves against the voxel grid. It's a pure function ([`step_item`]) over
//! "is this cell solid / wet" closures, tested without an app.

use bevy::prelude::*;

use crate::blocks::{BlockRegistry, BlockTables, ItemStack, Tables};
use crate::chat::ChatState;
use crate::interact::Hotbar;
use crate::inventory::{Inventory, InventoryState};
use crate::light::LightCell;
use crate::player::{cursor_grabbed, Player, HALF_W, HEIGHT};
use crate::render::{bucket_to_mesh, ChunkMaterials};
use crate::state::{AppState, DeathState, PauseState};
use crate::world::ChunkMap;

/// Side length of the drawn cube, in blocks.
pub const ITEM_SIZE: f32 = 0.25;
/// Half the collision box's side. The box is the drawn cube, so an item
/// rests with its cube sitting on the ground.
const HALF: f32 = ITEM_SIZE / 2.0;
const GRAVITY: f32 = -18.0;
const MAX_FALL: f32 = -30.0;
/// Fraction of horizontal speed lost per second in the air - enough that a
/// throw arcs down rather than flying on like a bullet.
const AIR_DRAG: f32 = 0.8;
/// The longest physics step taken at once; a slow frame is split into
/// several, so an item can't tunnel through a floor.
const MAX_STEP: f32 = 1.0 / 60.0;
/// How long an item lies around before vanishing (Minecraft's five minutes).
pub const DESPAWN_SECONDS: f32 = 300.0;
/// How close (in blocks, from the player's body) an item has to be to be
/// picked up.
const PICKUP_RADIUS: f32 = 1.2;
/// Before this, a freshly spawned item can't be picked up - long enough to
/// see a mined block's drop pop out, and for a thrown one to clear you.
pub const MINED_PICKUP_DELAY: f32 = 0.4;
pub const THROWN_PICKUP_DELAY: f32 = 1.5;
/// Stacks of the same block closer than this merge into one.
const MERGE_RADIUS: f32 = 0.9;
const THROW_SPEED: f32 = 6.0;

#[derive(Component)]
pub struct DroppedItem {
    pub stack: ItemStack,
    /// The collision box's centre. The entity's `Transform` adds the bob and
    /// spin on top, so it isn't the position to save or test against.
    pub pos: Vec3,
    pub vel: Vec3,
    /// Seconds since it was dropped, carried across saves so an item can't
    /// be kept forever by reloading.
    pub age: f32,
    pub pickup_delay: f32,
    /// The light its mesh was last built with - rebuilt when the cell's light
    /// changes (`refresh_item_meshes`).
    lit: Option<LightCell>,
    mesh: Option<Handle<Mesh>>,
}

impl DroppedItem {
    pub fn new(stack: ItemStack, pos: Vec3, vel: Vec3, pickup_delay: f32) -> Self {
        Self { stack, pos, vel, age: 0.0, pickup_delay, lit: None, mesh: None }
    }
}

/// Spawns a dropped item. Written by mining, Q, and death; read by
/// `spawn_drops`.
#[derive(Event, Clone, Copy)]
pub struct SpawnDrop {
    pub pos: Vec3,
    pub stack: ItemStack,
    pub vel: Vec3,
    pub pickup_delay: f32,
}

/// Items read from the save by `world::enter_world`, waiting for
/// `spawn_loaded_items` to turn them into entities. A resource rather than
/// `SpawnDrop` events so the world plugin doesn't need this one to load.
#[derive(Resource, Default)]
pub struct LoadedItems(pub Vec<(ItemStack, Vec3, f32)>);

/// Puts as much of `stack` as fits into the hotbar, then storage: first
/// topping up stacks of the same block, then filling empty slots, hotbar
/// before storage in both passes. Returns how many didn't fit.
pub fn give(hotbar: &mut [ItemStack], storage: &mut [ItemStack], stack: ItemStack, max_stack: u32) -> u32 {
    if stack.is_empty() {
        return 0;
    }
    let max_stack = max_stack.max(1);
    let mut left = stack.count;
    for slot in hotbar.iter_mut().chain(storage.iter_mut()) {
        if left == 0 {
            break;
        }
        if !slot.is_empty() && slot.id == stack.id && slot.count < max_stack {
            let moved = left.min(max_stack - slot.count);
            slot.count += moved;
            left -= moved;
        }
    }
    for slot in hotbar.iter_mut().chain(storage.iter_mut()) {
        if left == 0 {
            break;
        }
        if slot.is_empty() {
            let moved = left.min(max_stack);
            *slot = ItemStack { id: stack.id, count: moved };
            left -= moved;
        }
    }
    left
}

/// Moves one axis and pushes back out of any solid cell the box ended up
/// in. Returns whether it hit something.
fn move_axis(pos: &mut Vec3, axis: usize, delta: f32, solid: &impl Fn(IVec3) -> bool) -> bool {
    if delta == 0.0 {
        return false;
    }
    pos[axis] += delta;
    let min = (*pos - Vec3::splat(HALF)).floor().as_ivec3();
    let max = (*pos + Vec3::splat(HALF)).floor().as_ivec3();
    let mut bound: Option<i32> = None;
    for y in min.y..=max.y {
        for z in min.z..=max.z {
            for x in min.x..=max.x {
                if !solid(IVec3::new(x, y, z)) {
                    continue;
                }
                let v = [x, y, z][axis];
                bound = Some(match bound {
                    Some(b) if delta > 0.0 => b.min(v),
                    Some(b) => b.max(v + 1),
                    None if delta > 0.0 => v,
                    None => v + 1,
                });
            }
        }
    }
    let Some(bound) = bound else { return false };
    pos[axis] = if delta > 0.0 { bound as f32 - HALF - 1e-3 } else { bound as f32 + HALF + 1e-3 };
    true
}

/// One physics step for an item with its box centred at `pos`. Returns
/// whether it's resting on the ground.
pub fn step_item(
    pos: &mut Vec3,
    vel: &mut Vec3,
    dt: f32,
    solid: impl Fn(IVec3) -> bool,
    wet: impl Fn(IVec3) -> bool,
) -> bool {
    let cell = pos.floor().as_ivec3();
    // Buried (a block placed or fallen onto it): rise out rather than stay
    // stuck inside it, the way Minecraft nudges items free.
    if solid(cell) {
        *vel = Vec3::ZERO;
        pos.y += 4.0 * dt;
        return false;
    }
    if wet(cell) {
        // Float up to the surface, slowly, with the current damped out.
        vel.y += (1.2 - vel.y) * (4.0 * dt).min(1.0);
        let drag = (3.0 * dt).min(1.0);
        vel.x -= vel.x * drag;
        vel.z -= vel.z * drag;
    } else {
        vel.y = (vel.y + GRAVITY * dt).max(MAX_FALL);
        let drag = (AIR_DRAG * dt).min(1.0);
        vel.x -= vel.x * drag;
        vel.z -= vel.z * drag;
    }
    if move_axis(pos, 0, vel.x * dt, &solid) {
        vel.x = 0.0;
    }
    if move_axis(pos, 2, vel.z * dt, &solid) {
        vel.z = 0.0;
    }
    let falling = vel.y <= 0.0;
    let on_ground = move_axis(pos, 1, vel.y * dt, &solid) && falling;
    if on_ground {
        vel.y = 0.0;
        // Ground friction stops a sliding item within a fraction of a second.
        let friction = (8.0 * dt).min(1.0);
        vel.x -= vel.x * friction;
        vel.z -= vel.z * friction;
    } else if vel.y > 0.0 && solid((*pos + Vec3::Y * (HALF + 1e-2)).floor().as_ivec3()) {
        vel.y = 0.0; // hit a ceiling
    }
    on_ground
}

/// Whether the player standing at `feet` is close enough to pick up an item
/// whose box is centred at `item`: within `PICKUP_RADIUS` of the player's
/// box.
pub fn in_pickup_range(feet: Vec3, item: Vec3) -> bool {
    let nearest = Vec3::new(
        item.x.clamp(feet.x - HALF_W, feet.x + HALF_W),
        item.y.clamp(feet.y, feet.y + HEIGHT),
        item.z.clamp(feet.z - HALF_W, feet.z + HALF_W),
    );
    nearest.distance(item) <= PICKUP_RADIUS
}

fn spawn_drops(
    mut commands: Commands,
    mut events: EventReader<SpawnDrop>,
    mut loaded: ResMut<LoadedItems>,
) {
    for e in events.read() {
        if !e.stack.is_empty() {
            commands.spawn(item_bundle(DroppedItem::new(e.stack, e.pos, e.vel, e.pickup_delay)));
        }
    }
    for (stack, pos, age) in loaded.0.drain(..) {
        if !stack.is_empty() {
            let mut item = DroppedItem::new(stack, pos, Vec3::ZERO, 0.0);
            item.age = age;
            commands.spawn(item_bundle(item));
        }
    }
}

fn item_bundle(item: DroppedItem) -> impl Bundle {
    (Transform::from_translation(item.pos), Visibility::default(), item)
}

fn simulate_items(
    mut commands: Commands,
    time: Res<Time>,
    map: Res<ChunkMap>,
    tables: Option<Res<BlockTables>>,
    paused: Res<PauseState>,
    mut items: Query<(Entity, &mut DroppedItem)>,
) {
    if paused.open {
        return;
    }
    let Some(tables) = tables else { return };
    let tables: &Tables = &tables.0;
    let dt = time.delta_secs().min(0.25);
    for (entity, mut item) in &mut items {
        item.age += dt;
        item.pickup_delay = (item.pickup_delay - dt).max(0.0);
        if item.age >= DESPAWN_SECONDS || item.pos.y < -16.0 {
            commands.entity(entity).despawn();
            continue;
        }
        // Frozen where its chunk isn't loaded, rather than falling through
        // terrain that isn't there (it's saved and comes back either way).
        if !map.is_loaded(item.pos.floor().as_ivec3()) {
            continue;
        }
        let solid = |c: IVec3| map.is_solid(tables, c);
        let wet = |c: IVec3| tables.fluid[map.get_block(c) as usize];
        let item = &mut *item;
        let mut left = dt;
        while left > 0.0 {
            let step = left.min(MAX_STEP);
            step_item(&mut item.pos, &mut item.vel, step, solid, wet);
            left -= step;
        }
    }
}

/// Merges stacks of the same block lying close together, so mining a pile
/// of dirt leaves one item rather than dozens. Checked twice a second.
fn merge_items(
    mut commands: Commands,
    time: Res<Time>,
    mut timer: Local<f32>,
    registry: Res<BlockRegistry>,
    mut items: Query<(Entity, &mut DroppedItem)>,
) {
    *timer += time.delta_secs();
    if *timer < 0.5 {
        return;
    }
    *timer = 0.0;
    let mut all: Vec<(Entity, ItemStack, Vec3)> = items.iter().map(|(e, i)| (e, i.stack, i.pos)).collect();
    for i in 0..all.len() {
        for j in (i + 1)..all.len() {
            let (a, b) = (all[i], all[j]);
            if a.1.is_empty() || b.1.is_empty() || a.1.id != b.1.id || a.2.distance(b.2) > MERGE_RADIUS {
                continue;
            }
            let max = registry.def(a.1.id).max_stack.max(1);
            let moved = b.1.count.min(max.saturating_sub(a.1.count));
            if moved == 0 {
                continue;
            }
            all[i].1.count += moved;
            all[j].1.count -= moved;
        }
    }
    for (entity, stack, _) in all {
        let Ok((_, mut item)) = items.get_mut(entity) else { continue };
        if stack.count == 0 {
            commands.entity(entity).despawn();
        } else if item.stack != stack {
            item.stack = stack;
            item.mesh = None; // same block, but keeps the visuals honest if that changes
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn pick_up_items(
    mut commands: Commands,
    registry: Res<BlockRegistry>,
    death: Res<DeathState>,
    mut hotbar: ResMut<Hotbar>,
    mut inventory: ResMut<Inventory>,
    players: Query<&Player>,
    mut items: Query<(Entity, &mut DroppedItem)>,
) {
    let Ok(player) = players.single() else { return };
    if death.open || !player.spawned {
        return;
    }
    for (entity, mut item) in &mut items {
        if item.pickup_delay > 0.0 || !in_pickup_range(player.pos, item.pos) {
            continue;
        }
        let max = registry.def(item.stack.id).max_stack;
        let left = give(&mut hotbar.slots, &mut inventory.slots, item.stack, max);
        if left == 0 {
            commands.entity(entity).despawn();
        } else if left != item.stack.count {
            item.stack.count = left;
        }
    }
}

/// Q drops one of the selected hotbar item, Ctrl+Q the whole stack, thrown
/// the way you're looking.
#[allow(clippy::too_many_arguments)]
fn throw_selected(
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    chat: Res<ChatState>,
    inventory: Res<InventoryState>,
    death: Res<DeathState>,
    mut hotbar: ResMut<Hotbar>,
    players: Query<&Player>,
    mut drops: EventWriter<SpawnDrop>,
) {
    if !keys.just_pressed(KeyCode::KeyQ) || chat.open || inventory.open || death.open || !cursor_grabbed(windows) {
        return;
    }
    let Ok(player) = players.single() else { return };
    let selected = hotbar.selected;
    let slot = &mut hotbar.slots[selected];
    if slot.is_empty() {
        return;
    }
    let whole = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let count = if whole { slot.count } else { 1 };
    let thrown = ItemStack { id: slot.id, count };
    slot.count -= count;
    if slot.count == 0 {
        *slot = ItemStack::EMPTY;
    }
    let dir = player.look_dir();
    drops.write(SpawnDrop {
        pos: player.eye() - Vec3::Y * 0.3 + dir * 0.4,
        stack: thrown,
        vel: dir * THROW_SPEED + Vec3::Y * 1.5,
        pickup_delay: THROWN_PICKUP_DELAY,
    });
}

/// Builds or rebuilds each item's cube when it's new or the light where it
/// lies has changed, and moves the drawn cube to its position with a gentle
/// bob and spin.
fn refresh_item_meshes(
    mut commands: Commands,
    time: Res<Time>,
    map: Res<ChunkMap>,
    tables: Option<Res<BlockTables>>,
    materials: Option<Res<ChunkMaterials>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut items: Query<(Entity, &mut DroppedItem, &mut Transform)>,
) {
    let (Some(tables), Some(materials)) = (tables, materials) else { return };
    let t = time.elapsed_secs();
    for (entity, mut item, mut transform) in &mut items {
        let light = map.get_light(item.pos.floor().as_ivec3());
        if item.lit != Some(light) || item.mesh.is_none() {
            let id = item.stack.id;
            let bucket = crate::mesher::cube_mesh(&tables.0, ITEM_SIZE, light, |f| tables.0.tiles[id as usize * 6 + f]);
            let mesh = bucket_to_mesh(bucket);
            match &item.mesh {
                Some(handle) => {
                    meshes.insert(handle, mesh);
                }
                None => {
                    let handle = meshes.add(mesh);
                    commands.entity(entity).insert((Mesh3d(handle.clone()), MeshMaterial3d(materials.solid.clone())));
                    item.mesh = Some(handle);
                }
            }
            item.lit = Some(light);
        }
        // Each item spins at its own phase so a pile doesn't turn in step.
        let phase = (item.pos.x * 3.1 + item.pos.z * 1.7).fract() * std::f32::consts::TAU;
        let bob = ((t * 2.0 + phase).sin() * 0.5 + 0.5) * 0.08;
        transform.translation = item.pos + Vec3::Y * bob;
        transform.rotation = Quat::from_rotation_y(t * 1.2 + phase);
    }
}

/// Leaving a world despawns its items - they're saved first by
/// `world::exit_world`, which this runs after.
fn despawn_items(mut commands: Commands, items: Query<Entity, With<DroppedItem>>) {
    for e in &items {
        commands.entity(e).despawn();
    }
}

pub struct ItemsPlugin;

impl Plugin for ItemsPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<SpawnDrop>()
            .init_resource::<LoadedItems>()
            .init_resource::<DeathState>()
            .init_resource::<Hotbar>()
            .init_resource::<Inventory>()
            .add_systems(OnExit(AppState::InGame), despawn_items.after(crate::world::WorldSaveSet))
            .add_systems(
                Update,
                (spawn_drops, throw_selected, simulate_items, merge_items, pick_up_items, refresh_item_meshes)
                    .chain()
                    .after(crate::player::PlayerSet)
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::AIR;

    fn stack(id: u16, count: u32) -> ItemStack {
        ItemStack { id, count }
    }

    #[test]
    fn giving_tops_up_matching_stacks_before_using_empty_slots() {
        let mut hotbar = vec![ItemStack::EMPTY; 3];
        let mut storage = vec![ItemStack::EMPTY; 3];
        hotbar[1] = stack(5, 60);
        storage[0] = stack(5, 10);
        let left = give(&mut hotbar, &mut storage, stack(5, 10), 64);
        assert_eq!(left, 0);
        assert_eq!(hotbar[1], stack(5, 64), "the hotbar stack fills first");
        assert_eq!(storage[0], stack(5, 16), "then the storage one");
        assert!(hotbar[0].is_empty() && hotbar[2].is_empty(), "no new slot needed");
    }

    #[test]
    fn giving_fills_empty_hotbar_slots_before_storage_and_reports_overflow() {
        let mut hotbar = vec![stack(1, 1), ItemStack::EMPTY];
        let mut storage = vec![ItemStack::EMPTY];
        let left = give(&mut hotbar, &mut storage, stack(7, 150), 64);
        assert_eq!(hotbar[1], stack(7, 64));
        assert_eq!(storage[0], stack(7, 64));
        assert_eq!(left, 22);
        assert_eq!(hotbar[0], stack(1, 1), "an unrelated stack is untouched");
        assert_eq!(give(&mut hotbar, &mut storage, ItemStack::EMPTY, 64), 0);
    }

    fn floor_at(y: i32) -> impl Fn(IVec3) -> bool {
        move |c: IVec3| c.y <= y
    }

    #[test]
    fn an_item_falls_and_comes_to_rest_on_the_ground() {
        let (mut pos, mut vel) = (Vec3::new(0.5, 20.0, 0.5), Vec3::ZERO);
        let mut on_ground = false;
        for _ in 0..600 {
            on_ground = step_item(&mut pos, &mut vel, MAX_STEP, floor_at(10), |_| false);
        }
        assert!(on_ground);
        assert!((pos.y - (11.0 + HALF)).abs() < 0.01, "resting at y = {}", pos.y);
        assert!(vel.length() < 1e-3, "still moving: {vel:?}");
    }

    #[test]
    fn an_item_sliding_on_the_ground_stops_soon() {
        let (mut pos, mut vel) = (Vec3::new(0.5, 11.0 + HALF + 0.01, 0.5), Vec3::new(3.0, 0.0, 0.0));
        for _ in 0..600 {
            step_item(&mut pos, &mut vel, MAX_STEP, floor_at(10), |_| false);
        }
        assert!(vel.length() < 1e-3, "still sliding: {vel:?}");
        assert!(pos.x > 0.6 && pos.x < 1.5, "slid a little, not forever: x = {}", pos.x);
    }

    #[test]
    fn an_item_in_water_floats_up() {
        let (mut pos, mut vel) = (Vec3::new(0.5, 12.5, 0.5), Vec3::ZERO);
        let wet = |c: IVec3| (11..=15).contains(&c.y);
        for _ in 0..120 {
            step_item(&mut pos, &mut vel, MAX_STEP, floor_at(10), wet);
        }
        assert!(pos.y > 13.5, "sank or stayed put: y = {}", pos.y);
    }

    #[test]
    fn a_buried_item_rises_out_of_the_block() {
        let (mut pos, mut vel) = (Vec3::new(0.5, 10.5, 0.5), Vec3::ZERO);
        for _ in 0..240 {
            step_item(&mut pos, &mut vel, MAX_STEP, floor_at(10), |_| false);
        }
        assert!(pos.y >= 11.0 + HALF - 0.01, "still buried: y = {}", pos.y);
    }

    #[test]
    fn a_thrown_item_stops_at_a_wall() {
        let (mut pos, mut vel) = (Vec3::new(0.5, 11.2, 0.5), Vec3::new(20.0, 0.0, 0.0));
        let solid = |c: IVec3| c.y <= 10 || c.x >= 3;
        for _ in 0..120 {
            step_item(&mut pos, &mut vel, MAX_STEP, solid, |_| false);
        }
        assert!(pos.x <= 3.0 - HALF + 1e-3, "went through the wall: x = {}", pos.x);
    }

    #[test]
    fn pickup_range_is_measured_from_the_players_body() {
        let feet = Vec3::new(0.0, 10.0, 0.0);
        assert!(in_pickup_range(feet, Vec3::new(0.0, 10.1, 1.0)), "at your feet");
        assert!(in_pickup_range(feet, Vec3::new(0.0, 12.5, 0.0)), "just above your head");
        assert!(!in_pickup_range(feet, Vec3::new(2.0, 10.1, 0.0)), "two blocks away");
        assert!(!in_pickup_range(feet, Vec3::new(0.0, 8.0, 0.0)), "two blocks below");
    }

    #[test]
    fn an_empty_drop_is_never_given() {
        let mut hotbar = vec![ItemStack::EMPTY];
        assert_eq!(give(&mut hotbar, &mut [], stack(AIR, 5), 64), 0);
        assert!(hotbar[0].is_empty());
    }
}
