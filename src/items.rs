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

use crate::blocks::{BlockTables, ItemStack, Tables};
use crate::item_registry::ItemRegistry;
use crate::chat::ChatState;
use crate::interact::Hotbar;
use crate::inventory::{Inventory, InventoryState};
use crate::light::LightCell;
use crate::player::{cursor_grabbed, Player, HALF_W, HEIGHT};
use crate::render::{bucket_to_mesh, ChunkMaterials};
use crate::state::{AppState, DeathState, PauseState};
use crate::world::{BlockSetEvent, ChunkMap};

/// Side length of the drawn cube, in blocks.
pub const ITEM_SIZE: f32 = 0.25;
/// Side of a plain item's flat picture - bigger than a block item's cube,
/// since it's only one pixel thick.
const SPRITE_SIZE: f32 = 0.45;
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

/// Joules of effort behind a click-throw: the same for everything, so a
/// lighter thing leaves your hand faster ([`throw_speed`]).
pub const THROW_ENERGY: f32 = 8.0;
/// Air resistance, in kg/m: deceleration is `AIR_RESISTANCE / mass * speed^2`,
/// so a light pebble is slowed far more than a heavy block.
const AIR_RESISTANCE: f32 = 0.002;
/// Damage per joule of kinetic energy at the moment of impact. A pebble
/// thrown at a target a few blocks away (about 6 joules by then) does about 1.5.
pub const DAMAGE_PER_JOULE: f32 = 0.25;

/// How fast a thing of `mass` kilograms leaves the hand: `THROW_ENERGY`
/// worth of kinetic energy, `v = sqrt(2E / m)`.
pub fn throw_speed(mass: f32) -> f32 {
    (2.0 * THROW_ENERGY / mass.max(1e-3)).sqrt()
}

/// Damage from a thing of `mass` hitting at `speed`: proportional to its
/// kinetic energy, so speed counts for more than mass.
pub fn impact_damage(mass: f32, speed: f32) -> f32 {
    DAMAGE_PER_JOULE * 0.5 * mass * speed * speed
}

/// One step of a thrown thing in flight: gravity and air resistance (heavier
/// things are slowed less). Returns the solid cell it ran into, if it did -
/// `pos` is then left where it was, in the open cell before it.
pub fn step_projectile(
    pos: &mut Vec3,
    vel: &mut Vec3,
    mass: f32,
    dt: f32,
    solid: impl Fn(IVec3) -> bool,
) -> Option<IVec3> {
    let speed = vel.length();
    let drag = AIR_RESISTANCE / mass.max(1e-3) * speed;
    *vel += (Vec3::new(0.0, GRAVITY, 0.0) - *vel * drag) * dt;
    let next = *pos + *vel * dt;
    let cell = next.floor().as_ivec3();
    if solid(cell) {
        return Some(cell);
    }
    *pos = next;
    None
}

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
    /// `Some(mass)` while it's a thrown projectile still in flight (see
    /// [`step_projectile`]); it lands as an ordinary item when it hits
    /// something.
    pub thrown_mass: Option<f32>,
    /// The light its mesh was last built with - rebuilt when the cell's light
    /// changes (`refresh_item_meshes`).
    lit: Option<LightCell>,
    mesh: Option<Handle<Mesh>>,
}

impl DroppedItem {
    pub fn new(stack: ItemStack, pos: Vec3, vel: Vec3, pickup_delay: f32) -> Self {
        Self { stack, pos, vel, age: 0.0, pickup_delay, thrown_mass: None, lit: None, mesh: None }
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

/// Something was thrown (left click with a throwable item).
#[derive(Event, Clone, Copy)]
pub struct SpawnProjectile {
    pub pos: Vec3,
    pub stack: ItemStack,
    pub vel: Vec3,
    pub mass: f32,
}

/// A thrown thing hit the solid cell `cell`.
#[derive(Event, Clone, Copy, Debug)]
pub struct ProjectileHit {
    pub cell: IVec3,
    pub speed: f32,
    pub mass: f32,
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
        if !slot.is_empty() && slot.same_item(&stack) && slot.count < max_stack {
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
            *slot = ItemStack { count: moved, ..stack };
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
    mut thrown: EventReader<SpawnProjectile>,
    mut loaded: ResMut<LoadedItems>,
) {
    for e in events.read() {
        if !e.stack.is_empty() {
            commands.spawn(item_bundle(DroppedItem::new(e.stack, e.pos, e.vel, e.pickup_delay)));
        }
    }
    for e in thrown.read() {
        if !e.stack.is_empty() {
            let mut item = DroppedItem::new(e.stack, e.pos, e.vel, 0.0);
            item.thrown_mass = Some(e.mass);
            commands.spawn(item_bundle(item));
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
    mut map: ResMut<ChunkMap>,
    tables: Option<Res<BlockTables>>,
    paused: Res<PauseState>,
    mut hits: EventWriter<ProjectileHit>,
    mut block_events: EventWriter<BlockSetEvent>,
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
        let item = &mut *item;
        let mut left = dt;
        while left > 0.0 {
            let step = left.min(MAX_STEP);
            if let Some(mass) = item.thrown_mass {
                let hit = {
                    let m = &*map;
                    step_projectile(&mut item.pos, &mut item.vel, mass, step, |c| m.is_solid(tables, c))
                };
                if let Some(cell) = hit {
                    let id = map.get_block(cell);
                    let speed = item.vel.length();
                    if let Some(kept) = shatter_outcome(
                        speed,
                        tables.shatter_speed[id as usize],
                        tables.shatter_keep[id as usize],
                    ) {
                        // Breaks the pane and carries on, much slower.
                        map.set_block(cell, crate::blocks::AIR);
                        block_events.write(BlockSetEvent {
                            pos: cell,
                            id: crate::blocks::AIR,
                            prev: id,
                            axis: crate::blocks::AXIS_Y,
                        });
                        item.vel *= kept / speed;
                    } else {
                        hits.write(ProjectileHit { cell, speed, mass });
                        // Lands where it hit, as an ordinary item that can be picked up again.
                        item.thrown_mass = None;
                        item.vel *= 0.1;
                        item.pickup_delay = item.pickup_delay.max(0.5);
                    }
                }
                left -= step;
                continue;
            }
            let m = &*map;
            step_item(&mut item.pos, &mut item.vel, step, |c| m.is_solid(tables, c), |c| m.is_wet(tables, c));
            left -= step;
        }
    }
}

/// Speed a projectile has left after hitting a block that shatters at
/// `threshold` m/s and lets `keep` of the speed through, or `None` if it's
/// too slow to break it (or the block doesn't shatter).
pub fn shatter_outcome(speed: f32, threshold: f32, keep: f32) -> Option<f32> {
    (speed >= threshold).then_some(speed * keep)
}

/// Merges stacks of the same block lying close together, so mining a pile
/// of dirt leaves one item rather than dozens. Checked twice a second.
fn merge_items(
    mut commands: Commands,
    time: Res<Time>,
    mut timer: Local<f32>,
    registry: Res<ItemRegistry>,
    mut items: Query<(Entity, &mut DroppedItem)>,
) {
    *timer += time.delta_secs();
    if *timer < 0.5 {
        return;
    }
    *timer = 0.0;
    let mut all: Vec<(Entity, ItemStack, Vec3)> =
        items.iter().filter(|(_, i)| i.thrown_mass.is_none()).map(|(e, i)| (e, i.stack, i.pos)).collect();
    for i in 0..all.len() {
        for j in (i + 1)..all.len() {
            let (a, b) = (all[i], all[j]);
            if a.1.is_empty() || b.1.is_empty() || a.1.id != b.1.id || a.2.distance(b.2) > MERGE_RADIUS {
                continue;
            }
            let max = registry.max_stack(a.1.id);
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
    registry: Res<ItemRegistry>,
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
        if item.thrown_mass.is_some() || item.pickup_delay > 0.0 || !in_pickup_range(player.pos, item.pos) {
            continue;
        }
        let max = registry.max_stack(item.stack.id);
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
    let thrown = ItemStack { count, ..*slot };
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
    registry: Res<ItemRegistry>,
    materials: Option<Res<ChunkMaterials>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut items: Query<(Entity, &mut DroppedItem, &mut Transform)>,
) {
    let (Some(tables), Some(materials)) = (tables, materials) else { return };
    let t = time.elapsed_secs();
    for (entity, mut item, mut transform) in &mut items {
        let light = map.get_light(item.pos.floor().as_ivec3());
        if item.lit != Some(light) || item.mesh.is_none() {
            // A block item is a little cube of the block; a plain item (a
            // stick) is its flat picture, spinning like a card.
            let bucket = match registry.block_of(item.stack.id) {
                // Something that lies flat (a pebble) drops as its picture.
                Some(block) if tables.0.floor[block as usize] => crate::mesher::sprite_mesh(
                    &tables.0,
                    SPRITE_SIZE,
                    light,
                    tables.0.tiles[block as usize * 6 + 2],
                ),
                Some(block) => {
                    crate::mesher::cube_mesh(&tables.0, ITEM_SIZE, light, |f| tables.0.tiles[block as usize * 6 + f])
                }
                None => crate::mesher::sprite_mesh(&tables.0, SPRITE_SIZE, light, registry.tile(item.stack.id)),
            };
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
            .add_event::<SpawnProjectile>()
            .add_event::<ProjectileHit>()
            .init_resource::<LoadedItems>()
            .init_resource::<DeathState>()
            .init_resource::<Hotbar>()
            .init_resource::<Inventory>()
            .init_resource::<crate::inventory::HeldStack>()
            .init_resource::<crate::crafting::KnownItems>()
            .add_systems(OnExit(AppState::InGame), despawn_items.after(crate::world::WorldSaveSet))
            .add_systems(
                Update,
                (
                    spawn_drops,
                    throw_selected,
                    simulate_items,
                    merge_items,
                    pick_up_items,
                    crate::crafting::learn_items,
                    refresh_item_meshes,
                )
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

    #[test]
    fn shattering_needs_enough_speed_and_costs_most_of_it() {
        assert_eq!(shatter_outcome(10.0, 5.0, 0.5), Some(5.0));
        assert_eq!(shatter_outcome(5.0, 5.0, 0.5), Some(2.5));
        assert_eq!(shatter_outcome(4.9, 5.0, 0.5), None);
        // A block that doesn't shatter never does.
        assert_eq!(shatter_outcome(1e6, f32::INFINITY, 1.0), None);
    }

    #[test]
    fn a_pebble_breaks_two_panes_of_glass_but_not_a_third() {
        let registry = crate::blocks::BlockRegistry::with_defaults();
        let def = registry.def(registry.id("glass"));
        let (at, keep) = (def.shatters_at.expect("glass shatters"), def.shatter_keep);
        let mut speed = throw_speed(0.05);
        let mut broken = 0;
        while let Some(left) = shatter_outcome(speed, at, keep) {
            speed = left;
            broken += 1;
        }
        assert_eq!(broken, 2, "a throw gets through two panes and stalls on the third");
    }

    fn stack(id: u16, count: u32) -> ItemStack {
        ItemStack::new(id, count)
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

    /// Throws something of `mass` level from (0.5, 40, 0.5) along +x at a
    /// wall of solid cells from x = `wall`; returns the impact's height
    /// above the throw and its speed.
    fn throw_at_wall(mass: f32, wall: i32) -> (f32, f32) {
        let mut pos = Vec3::new(0.5, 40.0, 0.5);
        let mut vel = Vec3::X * throw_speed(mass);
        for _ in 0..20_000 {
            if step_projectile(&mut pos, &mut vel, mass, 1.0 / 120.0, |c| c.x >= wall).is_some() {
                return (pos.y - 40.0, vel.length());
            }
        }
        panic!("never hit the wall");
    }

    const PEBBLE: f32 = 0.05;

    #[test]
    fn a_pebble_thrown_at_something_a_few_blocks_away_does_about_one_and_a_half() {
        for wall in [4, 5, 6] {
            let (_, speed) = throw_at_wall(PEBBLE, wall);
            let damage = impact_damage(PEBBLE, speed);
            assert!((1.3..=1.7).contains(&damage), "{damage} at {wall} blocks (speed {speed})");
        }
    }

    #[test]
    fn air_slows_a_light_thing_much_more_than_a_heavy_one() {
        // Launch speed against speed on arrival, compared along the first few
        // blocks where gravity hasn't yet sped it up again.
        let lost = |mass: f32| 1.0 - throw_at_wall(mass, 5).1 / throw_speed(mass);
        assert!(lost(PEBBLE) > 0.1, "a pebble slows noticeably: {}", lost(PEBBLE));
        assert!(lost(PEBBLE) > lost(0.5) * 5.0, "pebble {} vs heavy {}", lost(PEBBLE), lost(0.5));
    }

    #[test]
    fn a_long_lob_that_falls_a_long_way_hits_harder_not_softer() {
        // Honest physics: what falls gains speed, and damage is its energy.
        let near = impact_damage(PEBBLE, throw_at_wall(PEBBLE, 5).1);
        let lob = impact_damage(PEBBLE, throw_at_wall(PEBBLE, 24).1);
        assert!(lob > near * 0.9, "{lob} vs {near}");
    }

    #[test]
    fn lighter_things_are_thrown_faster_and_heavier_ones_drop_sooner() {
        assert!(throw_speed(PEBBLE) > throw_speed(0.5) && throw_speed(0.5) > throw_speed(5.0));
        // Same distance, same aim: the heavy thing is slow, hangs in the air
        // longer, and has fallen much further by the time it arrives.
        let (pebble_drop, _) = throw_at_wall(PEBBLE, 6);
        let (heavy_drop, _) = throw_at_wall(2.0, 6);
        assert!(pebble_drop < 0.0 && heavy_drop < pebble_drop * 5.0, "pebble {pebble_drop}, heavy {heavy_drop}");
    }

    #[test]
    fn damage_follows_kinetic_energy_so_speed_counts_for_more_than_mass() {
        // Twice as fast is four times the damage; twice as heavy only two.
        let base = impact_damage(0.05, 10.0);
        assert!((impact_damage(0.05, 20.0) / base - 4.0).abs() < 1e-4);
        assert!((impact_damage(0.10, 10.0) / base - 2.0).abs() < 1e-4);
        assert_eq!(impact_damage(0.05, 0.0), 0.0);
    }

    #[test]
    fn a_thrown_thing_stops_in_the_open_cell_before_what_it_hit() {
        let mut pos = Vec3::new(0.5, 40.0, 0.5);
        let mut vel = Vec3::X * 10.0;
        let cell = loop {
            if let Some(c) = step_projectile(&mut pos, &mut vel, 0.05, 1.0 / 60.0, |c| c.x >= 3) {
                break c;
            }
        };
        assert_eq!(cell.x, 3);
        assert!(pos.x < 3.0, "pos {pos}");
    }
}
