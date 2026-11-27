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
    BlockId, BlockRegistry, BlockTables, ItemStack, Rotation, Shape, AIR, AXIS_X, AXIS_Y, AXIS_Z,
};
use crate::chat::ChatState;
use crate::config::WORLD_HEIGHT;
use crate::crafting::RecipeBook;
use crate::inventory::{open_inventory, InventoryState};
use crate::keybinds::{Action, Actions};
use crate::item_registry::ItemRegistry;
use bevy::ecs::system::SystemParam;
use crate::items::{impact_damage_scaled, throw_speed_with, ProjectileHit, SpawnDrop, SpawnProjectile, MINED_PICKUP_DELAY};
use crate::light::{LightCell, MAX_LIGHT};
use crate::player::{cursor_grabbed, Player};
use crate::render::{bucket_to_mesh, ChunkMaterials};
use crate::save::GameMode;
use crate::state::{AppState, DeathState, PauseState};
use crate::damage::{DamageEvent, DamageSource};
use crate::tools::{harvests, mining_seconds};
use crate::world::{Atlas, BlockSetEvent, ChunkMap};

const REACH: f32 = 6.0;
/// Seconds between hits while the button is held.
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

/// What a ray test says about one cell.
pub enum CellHit {
    Miss,
    /// The whole cell is hit (the ray's entry face is the hit face).
    Full,
    /// Something inside the cell is hit, `t` along the ray, on the face with this `normal`
    /// (a mod's model shape - see [`ray_boxes`]).
    Inside { t: f32, normal: IVec3 },
}

/// Voxel raycast (Amanatides & Woo DDA). Steps exactly through the grid cells
/// along the ray — no float sampling, no missed corners.
pub fn raycast_voxel(origin: Vec3, dir: Vec3, max_dist: f32, mut hit_test: impl FnMut(IVec3) -> bool) -> Option<RayHit> {
    raycast_voxel_with(origin, dir, max_dist, |c| if hit_test(c) { CellHit::Full } else { CellHit::Miss })
}

/// [`raycast_voxel`] where a cell can be hit only in part: `test` may answer
/// [`CellHit::Inside`] with the exact hit, which ends the ray there (a ray that passes a half
/// block's empty upper half carries on into the cells behind it).
pub fn raycast_voxel_with(origin: Vec3, dir: Vec3, max_dist: f32, mut test: impl FnMut(IVec3) -> CellHit) -> Option<RayHit> {
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
        match test(cell) {
            CellHit::Miss => {}
            CellHit::Full => {
                if normal == IVec3::ZERO {
                    return None; // started inside a block
                }
                return Some(RayHit { pos: cell, normal });
            }
            CellHit::Inside { t: hit_t, normal: hit_normal } => {
                if hit_t <= max_dist {
                    return Some(RayHit { pos: cell, normal: hit_normal });
                }
            }
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

/// Where a ray first meets any of `boxes` (block units, relative to `cell`): the distance along the
/// ray and the face's outward normal. A ray that starts inside a box finds nothing in it, like
/// [`raycast_voxel`]'s "started inside a block".
pub fn ray_boxes(origin: Vec3, dir: Vec3, cell: IVec3, boxes: &[crate::models::CollisionBox]) -> Option<(f32, IVec3)> {
    let base = cell.as_vec3();
    let mut best: Option<(f32, IVec3)> = None;
    for b in boxes {
        let (lo, hi) = (base + Vec3::from(b.min), base + Vec3::from(b.max));
        let (mut t_near, mut t_far) = (f32::NEG_INFINITY, f32::INFINITY);
        let mut normal = IVec3::ZERO;
        let mut missed = false;
        for a in 0..3 {
            let (o, d) = (origin[a], dir[a]);
            if d.abs() < 1e-9 {
                if o < lo[a] || o > hi[a] {
                    missed = true;
                    break;
                }
                continue;
            }
            let (t1, t2) = ((lo[a] - o) / d, (hi[a] - o) / d);
            let (enter, exit, face) = if t1 < t2 { (t1, t2, -1) } else { (t2, t1, 1) };
            if enter > t_near {
                t_near = enter;
                normal = IVec3::ZERO;
                normal[a] = face;
            }
            t_far = t_far.min(exit);
            if t_near > t_far {
                missed = true;
                break;
            }
        }
        if missed || t_near < 0.0 || normal == IVec3::ZERO {
            continue;
        }
        if best.is_none_or(|(t, _)| t_near < t) {
            best = Some((t_near, normal));
        }
    }
    best
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
    act: Actions,
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
    for (i, action) in Action::HOTBAR.iter().enumerate() {
        if act.just_pressed(*action) && i < hotbar.slots.len() {
            hotbar.selected = i;
        }
    }
    let mut step = 0;
    let scroll: f32 = wheel.read().map(|e| e.y).sum();
    if scroll != 0.0 {
        step = if scroll < 0.0 { 1 } else { -1 };
    }
    // Controller bumpers (or any keys bound to these) step the same way.
    if act.just_pressed(Action::HotbarNext) {
        step = 1;
    } else if act.just_pressed(Action::HotbarPrev) {
        step = -1;
    }
    if step != 0 {
        let n = hotbar.slots.len() as i32;
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
            events.write(BlockSetEvent { pos: partner, id: AIR, prev });
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

/// Left click with a throwable item (a pebble) in hand throws one: it leaves
/// the eye along the look direction at the speed its mass allows
/// (`items::throw_speed`). Survival uses one up; Creative's are free.
#[allow(clippy::too_many_arguments)]
fn throw_on_click(
    time: Res<Time>,
    act: Actions,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mode: Res<GameMode>,
    items: Res<ItemRegistry>,
    mut hotbar: ResMut<Hotbar>,
    players: Query<&Player>,
    mut cooldown: Local<f32>,
    mut projectiles: EventWriter<SpawnProjectile>,
) {
    *cooldown -= time.delta_secs();
    if !act.pressed(Action::Attack) || *cooldown > 0.0 || !cursor_grabbed(windows) {
        return;
    }
    let Ok(player) = players.single() else { return };
    let selected = hotbar.selected;
    let held = hotbar.slots[selected];
    if !items.throwable(&held) {
        return;
    }
    let spec = items.throw_spec(held.id);
    *cooldown = spec.cooldown;
    let mass = items.mass(held.id);
    let dir = player.look_dir();
    projectiles.write(SpawnProjectile {
        pos: player.eye() - Vec3::Y * 0.1 + dir * 0.4,
        stack: ItemStack { count: 1, ..held },
        vel: dir * throw_speed_with(mass, spec.energy),
        mass,
        spec,
    });
    if *mode == GameMode::Survival {
        let slot = &mut hotbar.slots[selected];
        slot.count -= 1;
        if slot.count == 0 {
            *slot = ItemStack::EMPTY;
        }
    }
}

/// A thrown thing hit a block: it deals its impact energy as damage there
/// (`damage.rs` decides whether what it hit takes it).
fn projectile_impacts(mut hits: EventReader<ProjectileHit>, mut damage: EventWriter<DamageEvent>) {
    for hit in hits.read() {
        damage.write(DamageEvent {
            cell: hit.cell,
            amount: impact_damage_scaled(hit.mass, hit.speed, hit.damage_scale),
            source: DamageSource::Projectile,
        });
    }
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
    act: Actions,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut map: ResMut<ChunkMap>,
    mut content: Content,
    mode: Res<GameMode>,
    mut hotbar: ResMut<Hotbar>,
    mut target: ResMut<Target>,
    mut timers: Local<ActionTimers>,
    mut mining: ResMut<Mining>,
    players: Query<&Player>,
    mut block_events: EventWriter<BlockSetEvent>,
    mut drops: EventWriter<SpawnDrop>,
    mut damage: EventWriter<DamageEvent>,
    mut gizmos: Gizmos,
) {
    let Ok(player) = players.single() else { return };
    let registry = &content.blocks;

    // Crosshair target.
    target.0 = if player.spawned {
        let (eye, look) = (player.eye(), player.look_dir());
        raycast_voxel_with(eye, look, REACH, |cell| {
            let id = map.get_block(cell);
            if id == AIR || !registry.def(id).selectable {
                return CellHit::Miss;
            }
            // Where a mod's model replaced the block's shape, only that shape can be targeted.
            let boxes = content.tables.as_ref().and_then(|t| t.0.collision_boxes.get(id as usize)).and_then(|b| b.as_ref());
            match boxes {
                Some(boxes) => match ray_boxes(eye, look, cell, boxes) {
                    Some((t, normal)) => CellHit::Inside { t, normal },
                    None => CellHit::Miss,
                },
                None => CellHit::Full,
            }
        })
    } else {
        None
    };

    if let Some(hit) = &target.0 {
        // A thin box around something lying on the ground, a cube otherwise.
        let id = map.get_block(hit.pos);
        let flat = registry.def(id).shape == Shape::Floor;
        let colour = Color::srgba(0.05, 0.05, 0.05, 0.9);
        let boxes = content.tables.as_ref().and_then(|t| t.0.collision_boxes.get(id as usize)).and_then(|b| b.as_ref());
        if let Some(boxes) = boxes {
            // A mod's model replaced the shape: outline exactly what can be hit.
            for b in boxes.iter() {
                let (lo, hi) = (Vec3::from(b.min), Vec3::from(b.max));
                let size = hi - lo + Vec3::splat(0.002);
                gizmos.cuboid(Transform::from_translation(hit.pos.as_vec3() + (lo + hi) * 0.5).with_scale(size), colour);
            }
        } else {
            // A thin box around something lying on the ground, a cube otherwise.
            let (centre, size) = if flat {
                (hit.pos.as_vec3() + Vec3::new(0.5, 0.05, 0.5), Vec3::new(1.002, 0.1, 1.002))
            } else if registry.def(id).shape == Shape::Plant {
                // As tall as the plant, and as wide as its crossed pictures.
                let h = registry.def(id).height;
                (hit.pos.as_vec3() + Vec3::new(0.5, h * 0.5, 0.5), Vec3::new(0.72, h + 0.002, 0.72))
            } else {
                (hit.pos.as_vec3() + Vec3::splat(0.5), Vec3::splat(1.002))
            };
            gizmos.cuboid(Transform::from_translation(centre).with_scale(size), colour);
        }
    }

    let dt = time.delta_secs();
    timers.break_t -= dt;
    timers.place_t -= dt;
    timers.attack_t -= dt;
    if !act.pressed(Action::Attack) {
        timers.break_t = 0.0;
    }
    if !act.pressed(Action::UseItem) {
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
    let shift = act.pressed(Action::Sneak);
    let aimed = map.get_block(hit.pos);
    // Holding something throwable, left click throws it (`throw_on_click`),
    // so it neither mines nor hits.
    let throwing = content.items.throwable(&hotbar.slots[hotbar.selected]);
    let breakable = mineable(registry.def(aimed), shift) && !throwing;
    // A plain click on something hittable hits it, with whatever is in hand.
    if !throwing && registry.def(aimed).hittable && !shift && act.pressed(Action::Attack) && timers.attack_t <= 0.0 {
        timers.attack_t = content.items.attack_cooldown(&hotbar.slots[hotbar.selected]);
        damage.write(DamageEvent {
            cell: hit.pos,
            amount: content.items.attack_damage(&hotbar.slots[hotbar.selected]),
            source: DamageSource::Melee,
        });
    }
    if *mode == GameMode::Creative {
        if act.pressed(Action::Attack) && timers.break_t <= 0.0 {
            if breakable {
                let after = registry.left_behind(aimed);
                if let Some(prev) = map.set_block_keep_level(hit.pos, after) {
                    block_events.write(BlockSetEvent { pos: hit.pos, id: after, prev });
                    break_partner(&mut map, registry, hit.pos, prev, &mut block_events);
                    let ev = crate::sound::resolve_event(&content.sound_set, &registry.def(aimed).sound, "break");
                    content.sounds.write(crate::sound::SoundEvent::at(&ev, hit.pos.as_vec3() + Vec3::splat(0.5)));
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
        if mining.advance(mined, act.pressed(Action::Attack), dt, seconds) {
            // Mining a waterlogged pebble takes the pebble and leaves the
            // water (or the ice) it was sitting in.
            let after = registry.left_behind(aimed);
            if let Some(prev) = map.set_block_keep_level(hit.pos, after) {
                block_events.write(BlockSetEvent { pos: hit.pos, id: after, prev });
                break_partner(&mut map, registry, hit.pos, prev, &mut block_events);
                let ev = crate::sound::resolve_event(&content.sound_set, &registry.def(aimed).sound, "break");
                content.sounds.write(crate::sound::SoundEvent::at(&ev, hit.pos.as_vec3() + Vec3::splat(0.5)));
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
    if act.pressed(Action::UseItem) && timers.place_t <= 0.0 && !on_station {
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
            let unsupported = matches!(registry.def(id).shape, Shape::Floor | Shape::Plant)
                && !registry.def(map.get_block(place_pos - IVec3::Y)).solid;
            // A block two cells tall needs the cell above free too.
            let upper = registry.upper_half(id);
            let upper_pos = place_pos + IVec3::Y;
            let upper_ok = upper_fits(&map, registry, id, place_pos)
                && (upper.is_none()
                    || (!player.intersects_block(upper_pos) && !player.solids.iter().any(|b| b.overlaps_cell(upper_pos))));
            if replaceable && !blocked && !unsupported && upper_ok {
                // A waterlogged block takes the cell's fluid level with it;
                // anything else starts from a cleared state (`set_block`), so
                // a stale flowing level can't make a placed ice block slope.
                let placed = if waterlogged.is_some() {
                    map.set_block_keep_level(place_pos, id)
                } else {
                    map.set_block(place_pos, id)
                };
                if let Some(prev) = placed {
                    if let Some(upper) = upper {
                        if let Some(upper_prev) = map.set_block(upper_pos, upper) {
                            block_events.write(BlockSetEvent { pos: upper_pos, id: upper, prev: upper_prev });
                        }
                    }
                    // A rotating block's orientation follows the face it was
                    // placed against, Minecraft-log-style; everything else
                    // stays at the default (AXIS_Y is also the "doesn't
                    // matter" value the mesher ignores for non-rotating ids).
                    if registry.def(id).rotation != Rotation::None {
                        map.set_axis_raw(place_pos, axis_from_normal(hit.normal));
                    }
                    block_events.write(BlockSetEvent { pos: place_pos, id, prev });
                    let ev = crate::sound::resolve_event(&content.sound_set, &registry.def(id).sound, "place");
                    content.sounds.write(crate::sound::SoundEvent::at(&ev, place_pos.as_vec3() + Vec3::splat(0.5)));
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
    if *mode == GameMode::Creative && act.just_pressed(Action::PickBlock) {
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

/// Breaks off a floor-shaped block or plant (a pebble, a twig, grass) whose support just
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
            events.p1().write(BlockSetEvent { pos: above, id: after, prev });
            // The top half of a tall plant falling with its bottom half
            // drops nothing: the bottom half drops the one item.
            if content.blocks.lower_half(prev).is_none() {
                if let Some(stack) = content.items.drop_of(&content.blocks, prev, 0.0) {
                    drops.write(mined_drop(above, stack));
                }
            }
        }
    }
}

/// `Some` if the block at `pos` lies on the ground (or is a plant's upper half)
/// and nothing solid (or its lower half) is under it any more.
pub fn unsupported_floor(map: &ChunkMap, blocks: &BlockRegistry, pos: IVec3) -> Option<BlockId> {
    let id = map.get_block(pos);
    if !matches!(blocks.def(id).shape, Shape::Floor | Shape::Plant) {
        return None;
    }
    let below = map.get_block(pos - IVec3::Y);
    // The upper half of a tall plant stands on its lower half; anything
    // else on solid ground.
    let held = match blocks.lower_half(id) {
        Some(lower) => below == lower,
        None => blocks.def(below).solid,
    };
    (!held).then_some(id)
}

/// The block, item and recipe lists, as one system parameter.
#[derive(SystemParam)]
pub struct Content<'w> {
    pub blocks: Res<'w, BlockRegistry>,
    pub items: Res<'w, ItemRegistry>,
    pub book: Res<'w, RecipeBook>,
    pub tables: Option<Res<'w, BlockTables>>,
    pub sounds: EventWriter<'w, crate::sound::SoundEvent>,
    pub sound_set: Res<'w, crate::sound::SoundSet>,
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
    act: Actions,
    target: Res<Target>,
    map: Res<ChunkMap>,
    book: Res<RecipeBook>,
    death: Res<DeathState>,
    mut inv: ResMut<InventoryState>,
    mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
) {
    let shift = act.pressed(Action::Sneak);
    if !act.just_pressed(Action::UseItem) || shift || inv.open || death.open {
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
            .add_event::<crate::sound::SoundEvent>()
            .init_resource::<crate::sound::SoundSet>()
            .init_resource::<Hotbar>()
            .init_resource::<Mining>()
            .init_resource::<CrackOverlay>()
            .init_resource::<DeathState>()
            .add_event::<SpawnDrop>()
            .add_systems(OnExit(AppState::InGame), reset_mining)
            .add_systems(
                Update,
                (select_slot, throw_on_click, interact, projectile_impacts, pop_unsupported, open_station, sync_crack_overlay)
                    .chain()
                    .after(crate::player::PlayerSet)
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

#[cfg(test)]
mod tests {
    use crate::models::CollisionBox;

    fn slab() -> Vec<CollisionBox> {
        vec![CollisionBox { min: [0.0, 0.0, 0.0], max: [1.0, 0.5, 1.0] }]
    }

    #[test]
    fn a_ray_over_a_half_block_passes_it_and_one_through_it_hits() {
        let cell = IVec3::new(0, 0, 0);
        let dir = Vec3::X;
        // Above the slab's top (y = 0.5): nothing.
        assert!(ray_boxes(Vec3::new(-2.0, 0.8, 0.5), dir, cell, &slab()).is_none());
        // Through it: hits the west face, 2 blocks along.
        let (t, n) = ray_boxes(Vec3::new(-2.0, 0.3, 0.5), dir, cell, &slab()).unwrap();
        assert!((t - 2.0).abs() < 1e-5 && n == IVec3::new(-1, 0, 0));
        // From above, down onto the top face.
        let (t, n) = ray_boxes(Vec3::new(0.5, 2.0, 0.5), Vec3::NEG_Y, cell, &slab()).unwrap();
        assert!((t - 1.5).abs() < 1e-5 && n == IVec3::Y);
    }

    #[test]
    fn the_voxel_walk_carries_on_past_a_half_blocks_empty_half() {
        let boxes = slab();
        // A slab at x=3 and a full block at x=5; the ray at height 0.8 skips the slab, hits the full block.
        let hit = raycast_voxel_with(Vec3::new(0.5, 0.8, 0.5), Vec3::X, 10.0, |c| match c.x {
            3 => match ray_boxes(Vec3::new(0.5, 0.8, 0.5), Vec3::X, c, &boxes) {
                Some((t, normal)) => CellHit::Inside { t, normal },
                None => CellHit::Miss,
            },
            5 => CellHit::Full,
            _ => CellHit::Miss,
        })
        .unwrap();
        assert_eq!(hit.pos, IVec3::new(5, 0, 0));
        assert_eq!(hit.normal, IVec3::new(-1, 0, 0));
        // At height 0.3 the slab itself is hit.
        let hit = raycast_voxel_with(Vec3::new(0.5, 0.3, 0.5), Vec3::X, 10.0, |c| match c.x {
            3 => match ray_boxes(Vec3::new(0.5, 0.3, 0.5), Vec3::X, c, &boxes) {
                Some((t, normal)) => CellHit::Inside { t, normal },
                None => CellHit::Miss,
            },
            5 => CellHit::Full,
            _ => CellHit::Miss,
        })
        .unwrap();
        assert_eq!(hit.pos, IVec3::new(3, 0, 0));
    }

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
    fn a_plant_needs_solid_ground_and_a_tall_ones_top_half_needs_its_bottom_half() {
        let (mut map, reg) = dummy_world();
        let (stone, short, medium, top) = (reg.id("stone"), reg.id("grass_short"), reg.id("grass_medium"), reg.id("grass_medium_top"));
        let at = IVec3::new(4, 20, 4);
        map.set_block(at - IVec3::Y, stone);
        map.set_block(at, medium);
        map.set_block(at + IVec3::Y, top);
        assert_eq!(unsupported_floor(&map, &reg, at), None);
        assert_eq!(unsupported_floor(&map, &reg, at + IVec3::Y), None, "stands on its bottom half");
        // The ground goes: the bottom half is loose, and then so is the top.
        map.set_block(at - IVec3::Y, AIR);
        assert_eq!(unsupported_floor(&map, &reg, at), Some(medium));
        map.set_block(at, AIR);
        assert_eq!(unsupported_floor(&map, &reg, at + IVec3::Y), Some(top));
        // A short plant on a plant is not on the ground.
        map.set_block(at, short);
        map.set_block(at + IVec3::Y, short);
        assert_eq!(unsupported_floor(&map, &reg, at + IVec3::Y), Some(short));
    }
}
