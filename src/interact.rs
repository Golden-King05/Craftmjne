//! Block targeting (voxel DDA raycast), mining, placing, block picking,
//! and hotbar selection.
//!
//! Survival mining takes time: hold the left button on a block for
//! [`mining_seconds`] of its `hardness` and it breaks, dropping whatever its
//! `drops` says (`items.rs` spawns the item). Cracks spread across it while
//! you mine (`atlas::destroy_stage_name` tiles on a cube drawn just outside
//! the block), and looking away or letting go starts it over. A block with
//! `breakable: false` (bedrock) can't be mined at all, in either mode.
//! Creative breaks instantly and drops nothing.

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;

use crate::atlas::{destroy_stage_name, DESTROY_STAGES};
use crate::blocks::{BlockId, BlockRegistry, BlockTables, ItemStack, Rotation, AIR, AXIS_X, AXIS_Y, AXIS_Z, FLUID_SOURCE};
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
use crate::world::{Atlas, BlockSetEvent, ChunkMap};

const REACH: f32 = 6.0;
const ACTION_REPEAT: f32 = 0.22; // seconds between repeats while a button is held
/// Seconds of bare-handed mining per point of a block's `hardness`.
pub const SECONDS_PER_HARDNESS: f32 = 1.5;
/// After a block breaks, the next one starts after this pause, so holding
/// the button on instant blocks (torches) doesn't clear a whole row in a
/// blink.
const MINE_COOLDOWN: f32 = 0.15;

/// How long a block of this hardness takes to mine by hand.
pub fn mining_seconds(hardness: f32) -> f32 {
    hardness.max(0.0) * SECONDS_PER_HARDNESS
}

/// The block being mined in Survival, and how far along it is.
#[derive(Resource, Default)]
pub struct Mining {
    /// The cell being mined, and the block that was in it when mining
    /// started - if either changes, mining starts over.
    target: Option<(IVec3, BlockId)>,
    /// Seconds spent on `target` so far.
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
        self.progress += dt;
        if self.progress < seconds {
            return false;
        }
        self.target = None;
        self.progress = 0.0;
        self.cooldown = MINE_COOLDOWN;
        true
    }

    /// Which crack stage to draw (`0..DESTROY_STAGES`), or `None` when
    /// nothing's being mined.
    pub fn crack_stage(&self, seconds: f32) -> Option<usize> {
        self.target?;
        if seconds <= 0.0 || self.progress <= 0.0 {
            return None;
        }
        let fraction = (self.progress / seconds).clamp(0.0, 1.0);
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
        gizmos.cuboid(
            Transform::from_translation(hit.pos.as_vec3() + Vec3::splat(0.5))
                .with_scale(Vec3::splat(1.002)),
            Color::srgba(0.05, 0.05, 0.05, 0.9),
        );
    }

    let dt = time.delta_secs();
    timers.break_t -= dt;
    timers.place_t -= dt;
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
    let aimed = map.get_block(hit.pos);
    let breakable = registry.def(aimed).breakable;
    if *mode == GameMode::Creative {
        if mouse.pressed(MouseButton::Left) && timers.break_t <= 0.0 {
            if breakable {
                if let Some(prev) = map.set_block(hit.pos, AIR) {
                    block_events.write(BlockSetEvent { pos: hit.pos, id: AIR, prev, axis: AXIS_Y });
                }
            }
            timers.break_t = ACTION_REPEAT;
        }
    } else {
        let seconds = mining_seconds(registry.def(aimed).hardness);
        let mined = breakable.then_some((hit.pos, aimed));
        if mining.advance(mined, mouse.pressed(MouseButton::Left), dt, seconds) {
            if let Some(prev) = map.set_block(hit.pos, AIR) {
                block_events.write(BlockSetEvent { pos: hit.pos, id: AIR, prev, axis: AXIS_Y });
                timers.breaks = timers.breaks.wrapping_add(1);
                if let Some(stack) = content.items.drop_of(registry, prev, drop_roll(hit.pos, timers.breaks)) {
                    drops.write(mined_drop(hit.pos, stack));
                }
            }
        }
    }

    // Place (right click / hold).
    // Right-clicking a crafting station opens it instead (`open_station`);
    // hold shift to place a block against one.
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
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
            let blocked = registry.def(id).solid && player.intersects_block(place_pos);
            if replaceable && !blocked {
                if let Some(prev) = map.set_block(place_pos, id) {
                    if registry.def(id).fluid {
                        // A player-placed fluid always starts as a
                        // permanent source, not whatever level that cell's
                        // fluid array last held (e.g. a dried flowing cell).
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
                hotbar.slots[sel] = ItemStack { id, count: content.items.max_stack(id) };
            }
        }
    }
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
    map: Res<ChunkMap>,
    registry: Res<BlockRegistry>,
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
    let shown = mining.cell().and_then(|pos| {
        let seconds = mining_seconds(registry.def(map.get_block(pos)).hardness);
        mining.crack_stage(seconds).map(|stage| (pos, stage))
    });
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
                (select_slot, interact, open_station, sync_crack_overlay)
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
        let secs = mining_seconds(1.5); // stone: 2.25s
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
        assert_eq!(m.crack_stage(secs), Some(4));
        m.advance(other, true, dt, secs);
        assert_eq!(m.crack_stage(secs), Some(0), "switching blocks restarts");
        // Letting go also restarts.
        m.advance(other, false, dt, secs);
        assert_eq!(m.crack_stage(secs), None);
        // And so does the block in the cell changing under you.
        for _ in 0..20 {
            m.advance(block, true, dt, secs);
        }
        m.advance(Some((IVec3::new(1, 2, 3), 6)), true, dt, secs);
        assert!(m.crack_stage(secs).unwrap() < 1);
    }

    #[test]
    fn instant_blocks_break_at_once_but_not_all_in_one_frame() {
        let dt = 0.01;
        let mut m = Mining::default();
        let mut broken = 0;
        for i in 0..100 {
            // A fresh torch in front of you every frame (the last one broke).
            if m.advance(Some((IVec3::new(i, 0, 0), 9)), true, dt, mining_seconds(0.0)) {
                broken += 1;
            }
        }
        assert!((5..=8).contains(&broken), "{broken} torches in one second");
    }

    #[test]
    fn harder_blocks_take_longer() {
        assert!(mining_seconds(0.5) < mining_seconds(1.5));
        assert_eq!(mining_seconds(0.0), 0.0);
        assert_eq!(mining_seconds(-1.0), 0.0);
    }

    #[test]
    fn raycast_respects_max_distance_and_inside_start() {
        assert!(raycast_voxel(Vec3::splat(0.5), Vec3::X, 3.0, |c| c.x == 5).is_none());
        // starting inside a hit cell yields None (no face to place against)
        assert!(raycast_voxel(Vec3::splat(0.5), Vec3::X, 3.0, |_| true).is_none());
    }
}
