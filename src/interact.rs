//! Block targeting (voxel DDA raycast), mining, placing, block picking,
//! and hotbar selection.
//!
//! Survival mining takes time: hold the left button on a block for
//! [`mining_seconds`] of its `hardness` - shorter with the right tool in hand
//! (`tools.rs`) - and it breaks, dropping whatever its `drops` says
//! (`items.rs` spawns the item), unless it needs a tool you don't have. Cracks spread across it while
//! you mine (`atlas::destroy_stage_name` tiles on a cube drawn just outside
//! the block), and looking away or letting go starts it over. A block with
//! `breakable: false` (bedrock) can't be mined at all, in either mode.
//! Creative breaks instantly and drops nothing.

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;

use crate::atlas::{destroy_stage_name, DESTROY_STAGES};
use crate::blocks::{
    BlockId, BlockRegistry, BlockTables, ItemStack, Rotation, Shape, AIR, AXIS_X, AXIS_Y, AXIS_Z, FLUID_SOURCE,
};
use crate::chat::ChatState;
use crate::config::WORLD_HEIGHT;
use crate::crafting::RecipeBook;
use crate::inventory::{open_inventory, InventoryState};
use crate::item_registry::ItemRegistry;
use bevy::ecs::system::SystemParam;
use crate::items::{SpawnDrop, MINED_PICKUP_DELAY};
use crate::light::{LightCell, MAX_LIGHT};
use crate::player::{cursor_grabbed, Player};
use crate::render::{bucket_to_mesh, ChunkMaterials};
use crate::save::GameMode;
use crate::state::{AppState, DeathState, PauseState};
use crate::particles::DamageNumberEvent;
use crate::tools::{harvests, mining_seconds};
use crate::world::{Atlas, BlockSetEvent, ChunkMap};

const REACH: f32 = 6.0;
/// Seconds between hits while the button is held.
const ATTACK_COOLDOWN: f32 = 0.45;
const ACTION_REPEAT: f32 = 0.22; // seconds between repeats while a button is held
/// After a block breaks, the next one starts after this pause, so holding
/// the button on instant blocks (torches) doesn't clear a whole row in a
/// blink.
const MINE_COOLDOWN: f32 = 0.15;

/// The block being mined in Survival, and how far along it is.
#[derive(Resource, Default)]
pub struct Mining {
    /// The cell being mined, and the block that was in it when mining
    /// started - if either changes, mining starts over.
    target: Option<(IVec3, BlockId)>,
    /// How far through `target` it is, `0..1`. A fraction rather than
    /// seconds, so changing tool partway through (a different speed) carries
    /// the work done over instead of letting a long, slow block finish the
    /// instant you swap to a fast tool.
    progress: f32,
    cooldown: f32,
}

impl Mining {
    /// Advances mining by `dt` with the button `held` on `target` (`None`
    /// when not aiming at a breakable block), where the block takes
    /// `seconds` to mine. Returns `true` on the frame it breaks.
    pub fn advance(&mut self, target: Option<(IVec3, BlockId)>, held: bool, dt: f32, seconds: f32) -> bool {
        self.cooldown = (self.cooldown - dt).max(0.0);
        if !held || target.is_none() {
            self.target = None;
            self.progress = 0.0;
            return false;
        }
        if self.target != target {
            self.target = target;
            self.progress = 0.0;
        }
        if self.cooldown > 0.0 {
            return false;
        }
        self.progress += if seconds > 0.0 { dt / seconds } else { 1.0 };
        if self.progress < 1.0 {
            return false;
        }
        self.target = None;
        self.progress = 0.0;
        self.cooldown = MINE_COOLDOWN;
        true
    }

    /// Which crack stage to draw (`0..DESTROY_STAGES`), or `None` when
    /// nothing's being mined.
    pub fn crack_stage(&self) -> Option<usize> {
        self.target?;
        if self.progress <= 0.0 {
            return None;
        }
        let fraction = self.progress.clamp(0.0, 1.0);
        Some(((fraction * DESTROY_STAGES as f32) as usize).min(DESTROY_STAGES - 1))
    }

    fn cell(&self) -> Option<IVec3> {
        self.target.map(|(pos, _)| pos)
    }
}

/// A mined block's drop pops out of the block's middle with a little hop,
/// nudged sideways by an amount that depends only on where it was, so it's
/// deterministic but not the same for every block.
pub fn mined_drop(pos: IVec3, stack: ItemStack) -> SpawnDrop {
    let h = (pos.x.wrapping_mul(73_856_093) ^ pos.y.wrapping_mul(19_349_663) ^ pos.z.wrapping_mul(83_492_791)) as u32;
    let angle = (h % 6283) as f32 / 1000.0;
    SpawnDrop {
        pos: pos.as_vec3() + Vec3::splat(0.5),
        stack,
        vel: Vec3::new(angle.cos() * 0.8, 3.0, angle.sin() * 0.8),
        pickup_delay: MINED_PICKUP_DELAY,
    }
}

/// Which rotation axis a placement's face normal implies - whichever axis
/// its single nonzero component lies on. Minecraft log placement: click a
/// block's top/bottom face (normal on Y) and it stands upright; click a
/// side face (normal on X or Z) and it lies down along that axis instead,
/// end grain facing the face you clicked (see `blocks::Rotation::Log`).
fn axis_from_normal(normal: IVec3) -> u8 {
    if normal.x != 0 {
        AXIS_X
    } else if normal.z != 0 {
        AXIS_Z
    } else {
        AXIS_Y
    }
}

pub struct RayHit {
    pub pos: IVec3,
    pub normal: IVec3,
}

/// Voxel raycast (Amanatides & Woo DDA). Steps exactly through the grid cells
/// along the ray — no float sampling, no missed corners.
pub fn raycast_voxel(
    origin: Vec3,
    dir: Vec3,
    max_dist: f32,
    mut hit_test: impl FnMut(IVec3) -> bool,
) -> Option<RayHit> {
    let mut cell = origin.floor().as_ivec3();
    let step = IVec3::new(
        if dir.x > 0.0 { 1 } else { -1 },
        if dir.y > 0.0 { 1 } else { -1 },
        if dir.z > 0.0 { 1 } else { -1 },
    );
    let t_delta = Vec3::new(
        if dir.x != 0.0 { (1.0 / dir.x).abs() } else { f32::INFINITY },
        if dir.y != 0.0 { (1.0 / dir.y).abs() } else { f32::INFINITY },
        if dir.z != 0.0 { (1.0 / dir.z).abs() } else { f32::INFINITY },
    );
    let bound = |o: f32, c: i32, d: f32| {
        if d > 0.0 { c as f32 + 1.0 - o } else { o - c as f32 }
    };
    let mut t_max = Vec3::new(
        bound(origin.x, cell.x, dir.x) * t_delta.x,
        bound(origin.y, cell.y, dir.y) * t_delta.y,
        bound(origin.z, cell.z, dir.z) * t_delta.z,
    );

    let mut normal = IVec3::ZERO;
    let mut t = 0.0;
    while t <= max_dist {
        if hit_test(cell) {
            if normal == IVec3::ZERO {
                return None; // started inside a block
            }
            return Some(RayHit { pos: cell, normal });
        }
        if t_max.x < t_max.y && t_max.x < t_max.z {
            cell.x += step.x;
            t = t_max.x;
            t_max.x += t_delta.x;
            normal = IVec3::new(-step.x, 0, 0);
        } else if t_max.y < t_max.z {
            cell.y += step.y;
            t = t_max.y;
            t_max.y += t_delta.y;
            normal = IVec3::new(0, -step.y, 0);
        } else {
            cell.z += step.z;
            t = t_max.z;
            t_max.z += t_delta.z;
            normal = IVec3::new(0, 0, -step.z);
        }
    }
    None
}

pub const HOTBAR_SLOTS: usize = 9;

#[derive(Resource)]
pub struct Hotbar {
    pub slots: Vec<ItemStack>,
    pub selected: usize,
}

/// Starts completely empty in both modes (Minecraft-style: nothing is
/// pre-filled). Survival fills it by picking up what you mine (`items.rs`);
/// Creative via the inventory screen's block list
/// (`inventory::handle_creative_click`). `world::enter_world` restores the
/// world's saved hotbar over this.
impl Default for Hotbar {
    fn default() -> Self {
        Self { slots: vec![ItemStack::EMPTY; HOTBAR_SLOTS], selected: 0 }
    }
}

#[derive(Resource, Default)]
pub struct Target(pub Option<RayHit>);

#[derive(Resource, Default)]
struct ActionTimers {
    break_t: f32,
    place_t: f32,
    /// Seconds until the next hit can land.
    attack_t: f32,
    /// Blocks broken this session, mixed into each drop's random roll.
    breaks: u32,
}

/// Skips entirely while chat, the pause menu, or the inventory screen is
/// open, so typing a digit or scrolling doesn't also swap the selected
/// hotbar slot (or, with the inventory open, scroll its block list instead).
fn select_slot(
    keys: Res<ButtonInput<KeyCode>>,
    mut wheel: EventReader<MouseWheel>,
    chat: Res<ChatState>,
    paused: Res<PauseState>,
    inventory: Res<InventoryState>,
    death: Res<DeathState>,
    mut hotbar: ResMut<Hotbar>,
) {
    if chat.open || paused.open || inventory.open || death.open {
        wheel.clear();
        return;
    }
    const DIGITS: [KeyCode; 9] = [
        KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5,
        KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9,
    ];
    for (i, key) in DIGITS.iter().enumerate() {
        if keys.just_pressed(*key) && i < hotbar.slots.len() {
            hotbar.selected = i;
        }
    }
    let scroll: f32 = wheel.read().map(|e| e.y).sum();
    if scroll != 0.0 {
        let n = hotbar.slots.len() as i32;
        let step = if scroll < 0.0 { 1 } else { -1 };
        hotbar.selected = ((hotbar.selected as i32 + step).rem_euclid(n)) as usize;
    }
}

/// When half of a two-cell-tall block (the training dummy) is broken, the
/// other half goes too - without a drop of its own, since the half that was
/// mined already dropped the item.
fn break_partner(
    map: &mut ChunkMap,
    registry: &BlockRegistry,
    pos: IVec3,
    broken: BlockId,
    events: &mut EventWriter<BlockSetEvent>,
) {
    if let Some((partner, _)) = partner_of(map, registry, pos, broken) {
        if let Some(prev) = map.set_block(partner, AIR) {
            events.write(BlockSetEvent { pos: partner, id: AIR, prev, axis: AXIS_Y });
        }
    }
}

/// The other half of a two-cell-tall block of which `broken` stood at
/// `pos`: its cell and block, but only if that cell really still holds it.
fn partner_of(map: &ChunkMap, registry: &BlockRegistry, pos: IVec3, broken: BlockId) -> Option<(IVec3, BlockId)> {
    let (cell, block) = if let Some(upper) = registry.upper_half(broken) {
        (pos + IVec3::Y, upper)
    } else if let Some(lower) = registry.lower_half(broken) {
        (pos - IVec3::Y, lower)
    } else {
        return None;
    };
    (map.get_block(cell) == block).then_some((cell, block))
}

/// The feet of whatever a block hit belongs to: the cell itself, or for the
/// upper half of a two-cell-tall block the cell below it, centred.
fn hit_origin(registry: &BlockRegistry, pos: IVec3, block: BlockId) -> Vec3 {
    let base = if registry.lower_half(block).is_some() { pos - IVec3::Y } else { pos };
    Vec3::new(base.x as f32 + 0.5, base.y as f32, base.z as f32 + 0.5)
}

/// Whether a block can be mined right now: breakable at all, and, for one
/// that `requires_crouch` (the dummy), only while crouching.
fn mineable(def: &crate::blocks::BlockDef, crouching: bool) -> bool {
    def.breakable && (crouching || !def.requires_crouch)
}

/// Whether a block two cells tall (or an ordinary one, trivially) has room
/// for its upper half above `place_pos`: free, or replaceable, and in the world.
fn upper_fits(map: &ChunkMap, registry: &BlockRegistry, id: BlockId, place_pos: IVec3) -> bool {
    registry.upper_half(id).is_none_or(|_| {
        let above = place_pos + IVec3::Y;
        above.y < WORLD_HEIGHT && {
            let existing = map.get_block(above);
            existing == AIR || registry.def(existing).replaceable
        }
    })
}

#[allow(clippy::too_many_arguments)]
fn interact(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut map: ResMut<ChunkMap>,
    content: Content,
    mode: Res<GameMode>,
    mut hotbar: ResMut<Hotbar>,
    mut target: ResMut<Target>,
    mut timers: Local<ActionTimers>,
    mut mining: ResMut<Mining>,
    players: Query<&Player>,
    mut block_events: EventWriter<BlockSetEvent>,
    mut drops: EventWriter<SpawnDrop>,
    mut damage_numbers: EventWriter<DamageNumberEvent>,
    mut gizmos: Gizmos,
) {
    let Ok(player) = players.single() else { return };
    let registry = &content.blocks;

    // Crosshair target.
    target.0 = if player.spawned {
        raycast_voxel(player.eye(), player.look_dir(), REACH, |cell| {
            let id = map.get_block(cell);
            id != AIR && registry.def(id).selectable
        })
    } else {
        None
    };

    if let Some(hit) = &target.0 {
        // A thin box around something lying on the ground, a cube otherwise.
        let flat = registry.def(map.get_block(hit.pos)).shape == Shape::Floor;
        let (centre, size) = if flat {
            (hit.pos.as_vec3() + Vec3::new(0.5, 0.05, 0.5), Vec3::new(1.002, 0.1, 1.002))
        } else {
            (hit.pos.as_vec3() + Vec3::splat(0.5), Vec3::splat(1.002))
        };
        gizmos.cuboid(Transform::from_translation(centre).with_scale(size), Color::srgba(0.05, 0.05, 0.05, 0.9));
    }

    let dt = time.delta_secs();
    timers.break_t -= dt;
    timers.place_t -= dt;
    timers.attack_t -= dt;
    if !mouse.pressed(MouseButton::Left) {
        timers.break_t = 0.0;
    }
    if !mouse.pressed(MouseButton::Right) {
        timers.place_t = 0.0;
    }
    if !cursor_grabbed(windows) {
        mining.advance(None, false, dt, 0.0);
        return;
    }
    let Some(hit) = &target.0 else {
        mining.advance(None, false, dt, 0.0);
        return;
    };

    // Break (left click / hold). Creative is instant and repeats while
    // held; Survival mines for a while and drops what the block drops.
    // A block that `requires_crouch` (the training dummy) ignores a plain
    // click: hold Shift to mine it.
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let aimed = map.get_block(hit.pos);
    let breakable = mineable(registry.def(aimed), shift);
    // A plain click on something hittable hits it, with whatever is in hand.
    if registry.def(aimed).hittable && !shift && mouse.pressed(MouseButton::Left) && timers.attack_t <= 0.0 {
        timers.attack_t = ATTACK_COOLDOWN;
        damage_numbers.write(DamageNumberEvent {
            origin: hit_origin(registry, hit.pos, aimed),
            amount: content.items.attack_damage(&hotbar.slots[hotbar.selected]),
        });
    }
    if *mode == GameMode::Creative {
        if mouse.pressed(MouseButton::Left) && timers.break_t <= 0.0 {
            if breakable {
                let after = registry.left_behind(aimed);
                if let Some(prev) = map.set_block(hit.pos, after) {
                    block_events.write(BlockSetEvent { pos: hit.pos, id: after, prev, axis: AXIS_Y });
                    break_partner(&mut map, registry, hit.pos, prev, &mut block_events);
                }
            }
            timers.break_t = ACTION_REPEAT;
        }
    } else {
        // What's in hand sets the speed (when it's this block's tool) and
        // whether the block drops anything (when it needs one).
        let def = registry.def(aimed);
        let in_hand = content.items.held_tool(&hotbar.slots[hotbar.selected]);
        let seconds = mining_seconds(def, in_hand);
        let mined = breakable.then_some((hit.pos, aimed));
        if mining.advance(mined, mouse.pressed(MouseButton::Left), dt, seconds) {
            // Mining a waterlogged pebble takes the pebble and leaves the
            // water (or the ice) it was sitting in.
            let after = registry.left_behind(aimed);
            if let Some(prev) = map.set_block(hit.pos, after) {
                block_events.write(BlockSetEvent { pos: hit.pos, id: after, prev, axis: AXIS_Y });
                break_partner(&mut map, registry, hit.pos, prev, &mut block_events);
                timers.breaks = timers.breaks.wrapping_add(1);
                if harvests(def, in_hand) {
                    if let Some(stack) = content.items.drop_of(registry, prev, drop_roll(hit.pos, timers.breaks)) {
                        drops.write(mined_drop(hit.pos, stack));
                    }
                }
            }
        }
    }

    // Place (right click / hold).
    // Right-clicking a crafting station opens it instead (`open_station`);
    // hold shift to place a block against one.
    let on_station = content.book.is_station(aimed) && !shift;
    if mouse.pressed(MouseButton::Right) && timers.place_t <= 0.0 && !on_station {
        let place_pos = hit.pos + hit.normal;
        // An empty hand, or an item that isn't a block (a stick), places
        // nothing.
        let held = hotbar.slots[hotbar.selected];
        let block = (!held.is_empty()).then(|| content.items.block_of(held.id)).flatten();
        if let Some(id) = block.filter(|_| place_pos.y >= 0 && place_pos.y < WORLD_HEIGHT) {
            let existing = map.get_block(place_pos);
            let replaceable = existing == AIR || registry.def(existing).replaceable;
            // A waterloggable block set down in a fluid takes the fluid in
            // with it, as a `block@fluid` variant, instead of displacing it.
            let held_fluid = registry.def(existing).fluid.then(|| registry.def(existing).layered.map_or(existing, |l| l.layer));
            let waterlogged = held_fluid.and_then(|fluid| registry.variant(id, fluid));
            let id = waterlogged.unwrap_or(id);
            let blocked = registry.def(id).solid
                && (player.intersects_block(place_pos) || player.solids.iter().any(|b| b.overlaps_cell(place_pos)));
            // Something that lies on the ground needs ground under it.
            let unsupported = registry.def(id).shape == Shape::Floor
                && !registry.def(map.get_block(place_pos - IVec3::Y)).solid;
            // A block two cells tall needs the cell above free too.
            let upper = registry.upper_half(id);
            let upper_pos = place_pos + IVec3::Y;
            let upper_ok = upper_fits(&map, registry, id, place_pos)
                && (upper.is_none()
                    || (!player.intersects_block(upper_pos) && !player.solids.iter().any(|b| b.overlaps_cell(upper_pos))));
            if replaceable && !blocked && !unsupported && upper_ok {
                if let Some(prev) = map.set_block(place_pos, id) {
                    if let Some(upper) = upper {
                        if let Some(upper_prev) = map.set_block(upper_pos, upper) {
                            block_events.write(BlockSetEvent { pos: upper_pos, id: upper, prev: upper_prev, axis: AXIS_Y });
                        }
                    }
                    if registry.def(id).fluid && waterlogged.is_none() {
                        // A player-placed fluid always starts as a
                        // permanent source, not whatever level that cell's
                        // fluid array last held (e.g. a dried flowing cell).
                        map.set_fluid_level_raw(place_pos, FLUID_SOURCE);
                    } else if waterlogged.is_none() {
                        // Anything else starts at the default level too, so a
                        // stale flowing level left by earlier water can't
                        // make a placed block of ice slope.
                        map.set_fluid_level_raw(place_pos, FLUID_SOURCE);
                    }
                    // A rotating block's orientation follows the face it was
                    // placed against, Minecraft-log-style; everything else
                    // stays at the default (AXIS_Y is also the "doesn't
                    // matter" value the mesher ignores for non-rotating ids).
                    let axis = if registry.def(id).rotation != Rotation::None {
                        let axis = axis_from_normal(hit.normal);
                        map.set_axis_raw(place_pos, axis);
                        axis
                    } else {
                        AXIS_Y
                    };
                    block_events.write(BlockSetEvent { pos: place_pos, id, prev, axis });
                    // Survival consumes from the stack (clearing the slot
                    // once it's empty); Creative's stacks never deplete -
                    // its "free items" already come from being able to
                    // refill a slot from the block list at will.
                    if *mode == GameMode::Survival {
                        let selected = hotbar.selected;
                        let stack = &mut hotbar.slots[selected];
                        stack.count -= 1;
                        if stack.count == 0 {
                            *stack = ItemStack::EMPTY;
                        }
                    }
                }
            }
        }
        timers.place_t = ACTION_REPEAT;
    }

    // Pick block (middle click): put the targeted block in the current slot.
    // Creative-only for now - Survival has no free items, and picking a
    // block you don't already have would defeat starting with nothing.
    // `item: false` blocks (air, and anything similarly hidden) can't be
    // picked either - there's nothing to hold.
    if *mode == GameMode::Creative && mouse.just_pressed(MouseButton::Middle) {
        if let Some(id) = content.items.of_block(map.get_block(hit.pos)) {
            if let Some(existing) = hotbar.slots.iter().position(|s| !s.is_empty() && s.id == id) {
                hotbar.selected = existing;
            } else {
                let sel = hotbar.selected;
                hotbar.slots[sel] = content.items.stack(id, content.items.max_stack(id));
            }
        }
    }
}

/// Breaks off a floor-shaped block (a pebble, a twig) whose support just
/// went - the block under it was mined, or replaced by something it can't
/// rest on - dropping it as an item, the way it would drop if mined.
fn pop_unsupported(
    mut events: ParamSet<(EventReader<BlockSetEvent>, EventWriter<BlockSetEvent>)>,
    mut map: ResMut<ChunkMap>,
    content: Content,
    mut drops: EventWriter<SpawnDrop>,
) {
    let changed: Vec<IVec3> = events.p0().read().map(|e| e.pos).collect();
    for pos in changed {
        let above = pos + IVec3::Y;
        let after = content.blocks.left_behind(map.get_block(above));
        if let Some(prev) = unsupported_floor(&map, &content.blocks, above).and_then(|_| map.set_block(above, after)) {
            events.p1().write(BlockSetEvent { pos: above, id: after, prev, axis: AXIS_Y });
            if let Some(stack) = content.items.drop_of(&content.blocks, prev, 0.0) {
                drops.write(mined_drop(above, stack));
            }
        }
    }
}

/// `Some` if the block at `pos` lies on the ground and nothing solid is
/// under it any more.
pub fn unsupported_floor(map: &ChunkMap, blocks: &BlockRegistry, pos: IVec3) -> Option<BlockId> {
    let id = map.get_block(pos);
    (blocks.def(id).shape == Shape::Floor && !blocks.def(map.get_block(pos - IVec3::Y)).solid).then_some(id)
}

/// The block, item and recipe lists, as one system parameter.
#[derive(SystemParam)]
pub struct Content<'w> {
    pub blocks: Res<'w, BlockRegistry>,
    pub items: Res<'w, ItemRegistry>,
    pub book: Res<'w, RecipeBook>,
}

/// A number in `0..1` for a block's drop chance, different for every break
/// (`breaks` counts them) yet reproducible in tests.
fn drop_roll(pos: IVec3, breaks: u32) -> f32 {
    let seed = (pos.x.wrapping_mul(73_856_093) ^ pos.y.wrapping_mul(19_349_663) ^ pos.z.wrapping_mul(83_492_791))
        as u32
        ^ breaks.wrapping_mul(0x9E37_79B9);
    crate::noise::mulberry32(seed)()
}

/// Right-clicking a crafting station (without shift) opens the inventory
/// screen, where its recipes are listed while it's within reach.
#[allow(clippy::too_many_arguments)]
fn open_station(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    target: Res<Target>,
    map: Res<ChunkMap>,
    book: Res<RecipeBook>,
    death: Res<DeathState>,
    mut inv: ResMut<InventoryState>,
    mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
) {
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if !mouse.just_pressed(MouseButton::Right) || shift || inv.open || death.open {
        return;
    }
    let Some(hit) = &target.0 else { return };
    if !book.is_station(map.get_block(hit.pos)) {
        return;
    }
    let Ok(mut window) = windows.single_mut() else { return };
    if window.cursor_options.grab_mode == bevy::window::CursorGrabMode::None {
        return; // another overlay has the cursor
    }
    open_inventory(&mut inv, &mut window);
}

/// The crack overlay on the block being mined: one cube entity, moved to
/// whichever block is being mined and given the mesh for the current stage.
#[derive(Resource, Default)]
struct CrackOverlay {
    entity: Option<Entity>,
    /// One mesh per stage, built the first time it's needed.
    meshes: Vec<Handle<Mesh>>,
}

#[allow(clippy::too_many_arguments)]
fn sync_crack_overlay(
    mut commands: Commands,
    mining: Res<Mining>,
    tables: Option<Res<BlockTables>>,
    atlas: Option<Res<Atlas>>,
    materials: Option<Res<ChunkMaterials>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut overlay: ResMut<CrackOverlay>,
    mut cubes: Query<(&mut Transform, &mut Mesh3d, &mut Visibility)>,
) {
    let (Some(tables), Some(atlas), Some(materials)) = (tables, atlas, materials) else { return };
    if overlay.meshes.is_empty() {
        // Drawn fully lit: the cracks are near-black, so they read as dark
        // lines in any light, and a dim crack would just disappear.
        let lit = LightCell { block: [MAX_LIGHT; 3], sky: [MAX_LIGHT; 3] };
        overlay.meshes = (0..DESTROY_STAGES)
            .map(|stage| {
                let tile = atlas.0.indices[&destroy_stage_name(stage)];
                meshes.add(bucket_to_mesh(crate::mesher::cube_mesh(&tables.0, 1.004, lit, |_| tile)))
            })
            .collect();
    }
    let shown = mining.cell().and_then(|pos| mining.crack_stage().map(|stage| (pos, stage)));
    let entity = match overlay.entity {
        Some(e) => e,
        None => {
            let e = commands
                .spawn((
                    Mesh3d(overlay.meshes[0].clone()),
                    MeshMaterial3d(materials.solid.clone()),
                    Transform::default(),
                    Visibility::Hidden,
                ))
                .id();
            overlay.entity = Some(e);
            e
        }
    };
    let Ok((mut transform, mut mesh, mut visibility)) = cubes.get_mut(entity) else { return };
    match shown {
        Some((pos, stage)) => {
            transform.translation = pos.as_vec3() + Vec3::splat(0.5);
            if mesh.0 != overlay.meshes[stage] {
                mesh.0 = overlay.meshes[stage].clone();
            }
            *visibility = Visibility::Visible;
        }
        None => *visibility = Visibility::Hidden,
    }
}

fn reset_mining(mut commands: Commands, mut mining: ResMut<Mining>, mut overlay: ResMut<CrackOverlay>) {
    *mining = Mining::default();
    if let Some(e) = overlay.entity.take() {
        commands.entity(e).despawn();
    }
}

pub struct InteractPlugin;

impl Plugin for InteractPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Target>()
            .init_resource::<Hotbar>()
            .init_resource::<Mining>()
            .init_resource::<CrackOverlay>()
            .init_resource::<DeathState>()
            .add_event::<SpawnDrop>()
            .add_systems(OnExit(AppState::InGame), reset_mining)
            .add_systems(
                Update,
                (select_slot, interact, pop_unsupported, open_station, sync_crack_overlay)
                    .chain()
                    .after(crate::player::PlayerSet)
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raycast_hits_the_expected_cell_and_face() {
        // Block at (5, 0, 0); ray along +x from origin height 0.5.
        let hit = raycast_voxel(
            Vec3::new(0.5, 0.5, 0.5),
            Vec3::X,
            10.0,
            |c| c == IVec3::new(5, 0, 0),
        )
        .expect("hit");
        assert_eq!(hit.pos, IVec3::new(5, 0, 0));
        assert_eq!(hit.normal, IVec3::new(-1, 0, 0)); // entered from -x side

        // Diagonal ray down onto a floor: normal must be +y.
        let hit = raycast_voxel(
            Vec3::new(0.5, 5.5, 0.5),
            Vec3::new(0.4, -1.0, 0.3).normalize(),
            20.0,
            |c| c.y < 0,
        )
        .expect("hit floor");
        assert_eq!(hit.normal, IVec3::Y);
        assert_eq!(hit.pos.y, -1);
    }

    #[test]
    fn mining_takes_the_blocks_time_and_restarts_when_interrupted() {
        let block = Some((IVec3::new(1, 2, 3), 5));
        let other = Some((IVec3::new(1, 2, 4), 5));
        let dt = 0.05;
        let mut m = Mining::default();
        let secs = 2.25; // seconds, say
        let mut frames = 0;
        while !m.advance(block, true, dt, secs) {
            frames += 1;
            assert!(frames < 1000);
        }
        assert!(((frames + 1) as f32 * dt - secs).abs() < dt + 1e-4, "took {} frames", frames + 1);

        // Halfway, then look at another block: back to zero.
        let mut m = Mining::default();
        for _ in 0..20 {
            assert!(!m.advance(block, true, dt, secs));
        }
        assert_eq!(m.crack_stage(), Some(4));
        m.advance(other, true, dt, secs);
        assert_eq!(m.crack_stage(), Some(0), "switching blocks restarts");
        // Letting go also restarts.
        m.advance(other, false, dt, secs);
        assert_eq!(m.crack_stage(), None);
        // And so does the block in the cell changing under you.
        for _ in 0..20 {
            m.advance(block, true, dt, secs);
        }
        m.advance(Some((IVec3::new(1, 2, 3), 6)), true, dt, secs);
        assert!(m.crack_stage().unwrap() < 1);
    }

    #[test]
    fn instant_blocks_break_at_once_but_not_all_in_one_frame() {
        let dt = 0.01;
        let mut m = Mining::default();
        let mut broken = 0;
        for i in 0..100 {
            // A fresh torch in front of you every frame (the last one broke).
            if m.advance(Some((IVec3::new(i, 0, 0), 9)), true, dt, 0.0) {
                broken += 1;
            }
        }
        assert!((5..=8).contains(&broken), "{broken} torches in one second");
    }

    #[test]
    fn a_pebble_with_nothing_under_it_is_unsupported() {
        let blocks = BlockRegistry::with_defaults();
        let (stone, pebble) = (blocks.id("stone"), blocks.id("pebble"));
        let mut map = ChunkMap::default();
        map.chunks.insert(
            IVec2::ZERO,
            crate::world::Chunk {
                blocks: Some(vec![AIR; crate::config::CS * crate::config::CS * crate::config::H]),
                ..Default::default()
            },
        );
        let at = IVec3::new(4, 20, 4);
        map.set_block(at - IVec3::Y, stone);
        map.set_block(at, pebble);
        assert_eq!(unsupported_floor(&map, &blocks, at), None, "resting on stone");
        map.set_block(at - IVec3::Y, AIR);
        assert_eq!(unsupported_floor(&map, &blocks, at), Some(pebble), "the stone's gone");
        map.set_block(at, stone);
        assert_eq!(unsupported_floor(&map, &blocks, at), None, "a cube doesn't need support");
    }

    #[test]
    fn swapping_to_a_faster_tool_partway_carries_the_work_done_over() {
        let block = Some((IVec3::new(1, 2, 3), 5));
        let dt = 0.1;
        let mut m = Mining::default();
        // Half way through a 60-second block by hand...
        for _ in 0..300 {
            assert!(!m.advance(block, true, dt, 60.0));
        }
        assert!(matches!(m.crack_stage(), Some(4 | 5)), "{:?}", m.crack_stage());
        // ...then a tool that does it in 3 seconds: half is left, not none.
        let mut frames = 0;
        while !m.advance(block, true, dt, 3.0) {
            frames += 1;
            assert!(frames < 100);
        }
        assert!((13..=17).contains(&frames), "took {frames} frames of 0.1s for the second half");
    }

    #[test]
    fn raycast_respects_max_distance_and_inside_start() {
        assert!(raycast_voxel(Vec3::splat(0.5), Vec3::X, 3.0, |c| c.x == 5).is_none());
        // starting inside a hit cell yields None (no face to place against)
        assert!(raycast_voxel(Vec3::splat(0.5), Vec3::X, 3.0, |_| true).is_none());
    }

    fn dummy_world() -> (ChunkMap, BlockRegistry) {
        let reg = BlockRegistry::with_defaults();
        let mut map = ChunkMap::default();
        map.chunks.insert(
            IVec2::ZERO,
            crate::world::Chunk { blocks: Some(vec![0; crate::config::CS * crate::config::CS * crate::config::H]), ..Default::default() },
        );
        (map, reg)
    }

    #[test]
    fn a_crouch_only_block_is_mined_only_while_crouching() {
        let reg = BlockRegistry::with_defaults();
        let dummy = reg.def(reg.id("training_dummy"));
        assert!(!mineable(dummy, false), "a plain left click leaves it alone");
        assert!(mineable(dummy, true));
        assert!(mineable(reg.def(reg.id("stone")), false), "ordinary blocks don't care");
        assert!(!mineable(reg.def(reg.id("bedrock")), true), "unbreakable stays unbreakable");
    }

    #[test]
    fn breaking_either_half_of_the_dummy_finds_the_other() {
        let (mut map, reg) = dummy_world();
        let (lower, upper) = (reg.id("training_dummy"), reg.id("training_dummy_top"));
        let base = IVec3::new(4, 10, 4);
        map.set_block(base, lower);
        map.set_block(base + IVec3::Y, upper);
        assert_eq!(partner_of(&map, &reg, base, lower), Some((base + IVec3::Y, upper)));
        assert_eq!(partner_of(&map, &reg, base + IVec3::Y, upper), Some((base, lower)));
        // An ordinary block has no partner, and a half whose partner is gone finds nothing.
        assert_eq!(partner_of(&map, &reg, base, reg.id("stone")), None);
        map.set_block(base + IVec3::Y, AIR);
        assert_eq!(partner_of(&map, &reg, base, lower), None);
    }

    #[test]
    fn the_dummy_needs_the_cell_above_free() {
        let (mut map, reg) = dummy_world();
        let (lower, stone) = (reg.id("training_dummy"), reg.id("stone"));
        let at = IVec3::new(4, 10, 4);
        assert!(upper_fits(&map, &reg, lower, at));
        map.set_block(at + IVec3::Y, stone);
        assert!(!upper_fits(&map, &reg, lower, at), "a block overhead leaves no room");
        assert!(upper_fits(&map, &reg, stone, at), "a one-cell block doesn't care");
        assert!(!upper_fits(&map, &reg, lower, IVec3::new(4, WORLD_HEIGHT - 1, 4)), "nor does the top of the world");
    }

    #[test]
    fn a_hit_on_either_half_shows_its_number_at_the_dummys_feet() {
        let reg = BlockRegistry::with_defaults();
        let (lower, upper) = (reg.id("training_dummy"), reg.id("training_dummy_top"));
        assert!(reg.def(lower).hittable && reg.def(upper).hittable && !reg.def(reg.id("stone")).hittable);
        assert_eq!(hit_origin(&reg, IVec3::new(4, 10, -3), lower), Vec3::new(4.5, 10.0, -2.5));
        assert_eq!(hit_origin(&reg, IVec3::new(4, 11, -3), upper), Vec3::new(4.5, 10.0, -2.5));
    }
}
