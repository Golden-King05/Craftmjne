//! Voxel lighting: per-cell colored block light plus propagated sky light.
//!
//! Two independent quantities live in every cell ([`LightCell`]):
//!
//! - **Block light** — three channels (R, G, B), each `0..=`[`MAX_LIGHT`],
//!   emitted by blocks whose definition sets a `light` (see `blocks.rs`'s
//!   `LightSpec`; `blocks/torch.json` is the built-in example). Colored
//!   because the channels propagate *independently*: a pure-red lamp's red
//!   reaches its full range while its green/blue die out immediately, so two
//!   differently-colored lights blend correctly where they overlap instead
//!   of one flatly overwriting the other.
//! - **Sky light** — three channels, each `0..=`[`MAX_LIGHT`], "how much of
//!   the open sky reaches this cell, and in what proportion of colors".
//!   Three rather than one because the media light crosses absorb colors
//!   unequally (`Tables::transmission`): water leaves blue nearly intact
//!   while eating red, so a pool floor is lit blue-green at noon rather than
//!   merely darker - something a single channel could never express, since
//!   scaling one number can only dim.
//!
//!   Deliberately *not* stored pre-multiplied by the time of day: these are
//!   ratios of the sky's own current color, so the day/night cycle can scale
//!   and tint them every frame with no relighting or remeshing at all.
//!
//! Both quantities are stored in finer units than the `0..=`[`MAX_LEVEL`]
//! scale block files are authored in, so that a percentage transmission has
//! somewhere to land instead of rounding to nothing - see [`MAX_LIGHT`].
//!
//! Both are baked into vertex colors by `mesher.rs` and combined in
//! `chunk.wgsl` as `max(block_light, sky_light * sky_color)` — Minecraft's
//! rule, per channel, so a torch is invisible in full daylight and takes
//! over completely at night. The sky's *color* is a uniform written by
//! `sky.rs` (`render::ChunkMaterialParams::sky_light`, an RGB value rather
//! than the plain scalar it was before this module existed), which is what
//! makes a red moon actually cast red light over the whole world - the
//! "more general" colored light source, as opposed to a per-block one.
//!
//! ## How light gets computed
//!
//! Exactly the budgeted-queue + pure-recompute shape the fluid simulation
//! established (`world.rs`'s `FluidQueue`/`recompute_cell`, and the note in
//! CLAUDE.md saying to reuse it for light): [`LightQueue`] holds cells whose
//! value might be stale, a bounded number are popped per frame, and
//! [`recompute_light_cell`] re-derives one cell from its 6 neighbours,
//! enqueuing more work only where something actually changed.
//!
//! The one rule with any subtlety is the same never-downgrade rule fluids
//! needed: a cell accepts a candidate only if it's at least as good as what
//! it already holds, otherwise it drops straight to zero rather than
//! settling for the worse value (see [`relax`]), and re-queues itself so it
//! can pull again once its neighbours have finished collapsing. That pair is
//! what makes *removing* a light work - it's the relaxation-shaped
//! equivalent of the two-pass "clear the region, then refill from whatever
//! still has a real supply" BFS voxel engines usually hand-write.
//!
//! Newly generated chunks don't start from nothing: `terrain.rs` fills in
//! the straight-down sky column during generation (no neighbour information
//! needed, and it's the part that covers everything outdoors), so a chunk's
//! very first mesh is already correctly lit above ground. [`seed_new_chunk`]
//! then queues only what that pass *can't* know - cave and overhang cells,
//! light emitters, and both sides of each cross-chunk seam.
//!
//! ## Not persisted, on purpose
//!
//! Light is a pure function of the block grid, so `save.rs` stores none of
//! it - loading a world recomputes it exactly. That's a real difference from
//! `Chunk::fluid_level`, which *is* saved (see CLAUDE.md): fluid spread is
//! path-dependent, so re-deriving it on load could legitimately converge
//! somewhere other than where the live simulation was. Light has a single
//! well-defined fixpoint (per channel, the max over all paths from every
//! emitter), so re-deriving it can't drift.

use bevy::prelude::*;
use std::collections::{HashSet, VecDeque};

use crate::blocks::{BlockTables, Tables};
use crate::config::{CHUNK_SIZE, WORLD_HEIGHT};
use crate::render::ChunkMaterials;
use crate::state::AppState;
use crate::world::{BlockSetEvent, ChunkMap, ChunkPipelineSet};

/// The authored light scale. `blocks/*.json` writes `light.level` in
/// `0..=MAX_LEVEL`, which is the scale a person reasonably thinks in ("a
/// torch is 14 out of 16") and the range light travels in blocks.
///
/// Storage uses a finer scale ([`MAX_LIGHT`]) so that *fractional* effects -
/// a pane of glass absorbing 2% of what passes through it - accumulate
/// instead of rounding away to nothing. See [`Tables::transmission`].
///
/// [`Tables::transmission`]: crate::blocks::Tables::transmission
pub const MAX_LEVEL: u32 = 16;

/// The brightest a *stored* light channel can be.
///
/// Deliberately finer-grained than [`MAX_LEVEL`]: light is stored in
/// 1/16th-of-a-level units so a percentage transmission has somewhere to
/// land. On the old 0..=16 scale, "glass blocks 2%" rounded to zero and a
/// hundred panes of glass in a row still blocked nothing at all, while
/// anything that *did* round up to 1 was indistinguishable from dense
/// leaves. The extra 4 bits of headroom is what makes transmission a
/// continuous knob rather than a four-position switch.
pub const MAX_LIGHT: u8 = 255;

/// What one block of travel costs, in [`MAX_LIGHT`] units. Chosen so light
/// still reaches exactly [`MAX_LEVEL`] blocks from a full-strength emitter,
/// identical to the range before storage got finer.
pub const LEVEL_STEP: u8 = 16;

/// The faint non-directional floor every surface gets regardless of any real
/// light reaching it, so a sealed cave reads as "very dark" rather than an
/// unnavigable pure black. Baked into the block-light channel by `mesher.rs`
/// (it's a constant, so there's nothing for the day/night uniform to do with
/// it) and still modulated by face shading and ambient occlusion, so shape
/// stays readable down there instead of flattening into one grey.
pub const AMBIENT_LIGHT: f32 = 0.055;

/// The 6 directions light travels. Index order matters: [`NEIGHBOR_UP`]
/// names the one the sky light's "straight down at full strength" rule
/// treats specially (light arriving *from* above is light heading down).
pub const LIGHT_NEIGHBORS: [IVec3; 6] =
    [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z, IVec3::Y, IVec3::NEG_Y];
const NEIGHBOR_UP: usize = 4;

/// One cell's stored light. 6 bytes, held in a `Vec` parallel to
/// `Chunk::blocks` exactly like `fluid_level`/`axis` (see CLAUDE.md's note on
/// per-cell dynamic state).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LightCell {
    /// Per-channel colored block light, `0..=MAX_LIGHT`.
    pub block: [u8; 3],
    /// Per-channel sky light, `0..=MAX_LIGHT`: how much open sky reaches
    /// here, and *in what proportion of colors*.
    ///
    /// Three channels rather than one because the media light passes through
    /// absorb colors unequally - a pool of water leaves the blue channel
    /// nearly intact while eating the red, so the pool floor is lit blue-green
    /// even at noon. Storing one scalar could only make that floor *darker*,
    /// never bluer.
    ///
    /// Still not premultiplied by the time of day: these are ratios of the
    /// sky's own current color, which `sky.rs` supplies as a per-frame
    /// uniform. So this describes the world's shape and the media in it (it
    /// changes only when blocks do), while dawn, dusk and a red moon stay a
    /// uniform write with no relighting or remeshing.
    pub sky: [u8; 3],
}

impl LightCell {
    pub const DARK: LightCell = LightCell { block: [0; 3], sky: [0; 3] };
    /// What sits above the top of the world: nothing blocking the sky.
    pub const OPEN_SKY: LightCell = LightCell { block: [0; 3], sky: [MAX_LIGHT; 3] };
}

/// The relaxation rule, applied per channel: take `candidate` when it's at
/// least as good as what's already there, otherwise drop straight to zero
/// rather than settling for the worse value.
///
/// Accepting a worse-but-still-positive value is what makes a pull-based
/// propagation fail to terminate - cell A derives a slightly dimmer value
/// from B, which then derives a dimmer one from A, forever (the classic
/// "Dijkstra has no termination proof once you remove an edge or a source"
/// problem, hit once already by the fluid simulation). Dropping to zero is
/// monotonic - a channel only empties once - and any neighbour still holding
/// a genuine supply pushes light straight back in on a later pass.
pub fn relax(current: u8, candidate: u8) -> u8 {
    if candidate >= current {
        candidate
    } else {
        0
    }
}

/// Scales `value` by a `0..=255` transmission fraction (`255` = perfectly
/// clear).
///
/// The `255` case returns `value` untouched rather than going through the
/// arithmetic, so a clear medium is *exactly* a no-op - the general formula
/// collapsing to the old behavior for the overwhelming majority of blocks
/// that don't absorb anything, in the same shape as `mesher.rs`'s
/// `rotated_tile` (CLAUDE.md's note on gating a per-instance variation).
fn attenuate(value: u8, transmit: u8) -> u8 {
    if transmit == u8::MAX {
        return value;
    }
    ((value as u32 * transmit as u32 + 127) / 255) as u8
}

/// What one cell would offer a neighbour in direction `toward_down`
/// (`true` only for the cell directly below it), given the transmission of
/// the medium being offered *into*.
///
/// Two separate effects, applied in this order:
///
/// 1. **Distance.** Every channel loses [`LEVEL_STEP`], except sky light at
///    full strength travelling downward, which falls undiminished so a deep
///    shaft stays lit all the way to the bottom.
/// 2. **Absorption.** Whatever survives is scaled by the receiving cell's
///    per-channel transmission. `transmit` belongs to the cell being lit,
///    not the one doing the lighting, which is what makes stacking compose:
///    light crossing glass and then water is multiplied by each in turn, so
///    the pair genuinely differs from either alone.
///
/// Absorption comes *after* the straight-down rule on purpose. Applying it
/// first would let full-strength sky light pass through any depth of water
/// unattenuated, since the rule keys on the value still being `MAX_LIGHT`.
fn offered(from: LightCell, toward_down: bool, transmit: [u8; 3]) -> LightCell {
    let mut out = LightCell::DARK;
    for c in 0..3 {
        out.block[c] = attenuate(from.block[c].saturating_sub(LEVEL_STEP), transmit[c]);
        let travelled = if toward_down && from.sky[c] == MAX_LIGHT {
            MAX_LIGHT
        } else {
            from.sky[c].saturating_sub(LEVEL_STEP)
        };
        out.sky[c] = attenuate(travelled, transmit[c]);
    }
    out
}

/// Recomputes one cell from its 6 neighbours and writes it back if it
/// changed, enqueuing whatever else that invalidates.
///
/// `touched` collects the chunks whose meshes will need rebuilding, flushed
/// in one go by [`flush_touched`] rather than per-write - a single torch
/// rewrites hundreds of cells as its light converges, and remeshing on each
/// of those would be pure waste.
pub fn recompute_light_cell(
    map: &mut ChunkMap,
    tables: &Tables,
    pos: IVec3,
    queue: &mut VecDeque<IVec3>,
    touched: &mut HashSet<IVec2>,
) {
    if pos.y < 0 || pos.y >= WORLD_HEIGHT {
        return;
    }
    let (id, current) = map.get_block_and_light(pos);

    // The best value anything currently offers this cell. An opaque block
    // holds only whatever it emits itself: it neither receives light nor
    // lets any through, so a torch buried in a wall correctly lights only
    // the sides that are actually exposed.
    let mut candidate = LightCell { block: tables.light[id as usize], sky: [0; 3] };
    if !tables.opaque[id as usize] {
        // This cell's own medium is what attenuates everything arriving in
        // it, so the transmission is looked up once here rather than per
        // neighbour.
        let transmit = tables.transmission[id as usize];
        for (i, d) in LIGHT_NEIGHBORS.iter().enumerate() {
            let offer = offered(map.get_light(pos + *d), i == NEIGHBOR_UP, transmit);
            for c in 0..3 {
                candidate.block[c] = candidate.block[c].max(offer.block[c]);
                candidate.sky[c] = candidate.sky[c].max(offer.sky[c]);
            }
        }
    }

    let next = LightCell {
        block: [
            relax(current.block[0], candidate.block[0]),
            relax(current.block[1], candidate.block[1]),
            relax(current.block[2], candidate.block[2]),
        ],
        sky: [
            relax(current.sky[0], candidate.sky[0]),
            relax(current.sky[1], candidate.sky[1]),
            relax(current.sky[2], candidate.sky[2]),
        ],
    };
    if next == current || !map.set_light(pos, next) {
        // Nothing changed, or this cell belongs to a chunk that isn't loaded
        // - in which case the value went nowhere and treating it as a change
        // would re-enqueue these same neighbours on every visit, forever.
        return;
    }
    touch_chunks(touched, pos);
    queue.extend(LIGHT_NEIGHBORS.iter().map(|&d| pos + d));

    // A channel that *emptied* rather than improved leaves this cell holding
    // a provisional zero: `relax` refused a worse-but-real candidate, and
    // that candidate may well be the correct answer once the cells feeding
    // it have finished collapsing. Re-queue this cell so it pulls again.
    //
    // Without this, removing one of two nearby torches leaves a permanent
    // black hole: the cells around the removed torch drop to zero and
    // enqueue their neighbours, but a neighbour still lit by the *surviving*
    // torch recomputes to no change and so never re-enqueues them, and
    // nothing else ever will. (Re-queueing here rather than having settled
    // cells scan for dimmer neighbours matters a lot for cost - a "push"
    // rule fires on every visit of every lit cell, which regenerates work
    // faster than the queue drains and never terminates. This fires only on
    // an actual drop, and each drop lands the cell on a strictly lower value
    // than it held before, so there can only be `MAX_LIGHT` of them.)
    if (0..3).any(|c| next.block[c] < current.block[c] || next.sky[c] < current.sky[c]) {
        queue.push_back(pos);
    }
}

/// Records the chunk a changed cell belongs to, plus any chunk whose padded
/// meshing shell includes it - a cell on a chunk edge is part of up to three
/// neighbours' shells, and their vertex lighting samples it. Mirrors
/// `ChunkMap::touch_borders`.
fn touch_chunks(touched: &mut HashSet<IVec2>, pos: IVec3) {
    let coord = IVec2::new(pos.x.div_euclid(CHUNK_SIZE), pos.z.div_euclid(CHUNK_SIZE));
    let lx = pos.x.rem_euclid(CHUNK_SIZE);
    let lz = pos.z.rem_euclid(CHUNK_SIZE);
    let dxs: &[i32] = if lx == 0 { &[-1, 0] } else if lx == CHUNK_SIZE - 1 { &[0, 1] } else { &[0] };
    let dzs: &[i32] = if lz == 0 { &[-1, 0] } else if lz == CHUNK_SIZE - 1 { &[0, 1] } else { &[0] };
    for &dx in dxs {
        for &dz in dzs {
            touched.insert(coord + IVec2::new(dx, dz));
        }
    }
}

/// Marks every chunk whose light changed for a remesh, in one batch.
fn flush_touched(map: &mut ChunkMap, touched: &mut HashSet<IVec2>) {
    if touched.is_empty() {
        return;
    }
    for coord in touched.drain() {
        if let Some(chunk) = map.chunks.get_mut(&coord) {
            chunk.version += 1;
            chunk.dirty = true;
        }
    }
    map.needs_scan = true;
}

/// Cells awaiting a light recompute, plus the chunks whose meshes that will
/// invalidate. Same role as `world.rs`'s `FluidQueue`, with the batched
/// remesh bookkeeping the fluid sim doesn't need (fluid writes go straight
/// through `set_fluid_cell`, which dirties as it goes).
#[derive(Resource, Default)]
pub struct LightQueue {
    queue: VecDeque<IVec3>,
    touched: HashSet<IVec2>,
    /// Frames the pending remesh flush has been held back waiting for the
    /// queue to drain - bounds how stale a chunk's lighting can look while
    /// something keeps the queue permanently non-empty (heavy chunk
    /// streaming, mostly).
    frames_pending: u32,
}

impl LightQueue {
    pub fn push(&mut self, pos: IVec3) {
        self.queue.push_back(pos);
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

/// How many cells to relax per frame. Each one costs ~7 chunk lookups, so
/// this is the knob trading light-settling latency against frame time;
/// placing a single torch touches roughly a thousand cells, well inside one
/// frame's worth.
const LIGHT_BUDGET_PER_FRAME: usize = 8192;
/// Flush pending remeshes after this many frames even if the queue still
/// hasn't drained.
const LIGHT_FLUSH_MAX_FRAMES: u32 = 6;

/// Queues every changed block plus its 6 neighbours - covers "a light source
/// appeared/vanished here" and "something started/stopped blocking light
/// next door" in one rule, exactly like `enqueue_fluid_updates`.
fn enqueue_light_updates(mut events: EventReader<BlockSetEvent>, mut lights: ResMut<LightQueue>) {
    for e in events.read() {
        lights.push(e.pos);
        for d in LIGHT_NEIGHBORS {
            lights.push(e.pos + d);
        }
    }
}

fn process_light_updates(
    mut map: ResMut<ChunkMap>,
    tables: Res<BlockTables>,
    mut lights: ResMut<LightQueue>,
) {
    let lights = &mut *lights;
    for _ in 0..LIGHT_BUDGET_PER_FRAME {
        let Some(pos) = lights.queue.pop_front() else { break };
        recompute_light_cell(&mut map, &tables.0, pos, &mut lights.queue, &mut lights.touched);
    }

    if lights.touched.is_empty() {
        lights.frames_pending = 0;
        return;
    }
    lights.frames_pending += 1;
    if lights.queue.is_empty() || lights.frames_pending >= LIGHT_FLUSH_MAX_FRAMES {
        flush_touched(&mut map, &mut lights.touched);
        lights.frames_pending = 0;
    }
}

/// Queues the parts of a freshly generated (and edit-applied) chunk that its
/// generation-time sky column pass couldn't resolve on its own:
///
/// - cells the sky doesn't reach directly (caves, overhangs);
/// - light emitters;
/// - both sides of the seam with each of the four side neighbours.
///
/// Everything else - the open-air majority of a chunk - was already given
/// its exact final value by `terrain.rs`, so it's deliberately left out;
/// queueing it would multiply the per-chunk cost by an order of magnitude to
/// re-derive values that are already correct.
///
/// Both sides of each seam have to be queued because propagation only ever
/// *pulls*: this chunk's border cells pull whatever the neighbour already
/// has, but the neighbour's own border cells are what pull light *out* of
/// this chunk, and nothing else will ever ask them to.
pub fn seed_new_chunk(map: &ChunkMap, tables: &Tables, coord: IVec2, lights: &mut LightQueue) {
    use crate::config::{block_index, CS, H};

    let Some(chunk) = map.chunks.get(&coord) else { return };
    let (Some(blocks), Some(light)) = (&chunk.blocks, &chunk.light) else { return };

    let origin = IVec3::new(coord.x * CHUNK_SIZE, 0, coord.y * CHUNK_SIZE);
    for z in 0..CS {
        for x in 0..CS {
            let on_border = x == 0 || z == 0 || x == CS - 1 || z == CS - 1;
            for y in 0..H {
                let idx = block_index(x, y, z);
                let id = blocks[idx] as usize;
                let emits = tables.light[id] != [0; 3];
                let full_sky = light[idx].sky == [MAX_LIGHT; 3];
                if !emits && (tables.opaque[id] || (!on_border && full_sky)) {
                    continue;
                }
                lights.push(origin + IVec3::new(x as i32, y as i32, z as i32));
            }
        }
    }

    // The far side of each seam: one cell outside this chunk, all the way
    // around its four vertical faces - but only where this chunk actually
    // has something brighter to offer it.
    //
    // That filter is safe *here* specifically because an unloaded chunk reads
    // as fully dark (`ChunkMap::get_block_and_light`), so a chunk arriving
    // can only ever add light across its seams, never remove it. It's also
    // the difference between seeding ~4000 cells per chunk and seeding
    // almost none, which matters a lot when a dozen chunks land in one frame.
    // Note this is a *one-shot* check at seed time, not a rule the relaxation
    // runs on every visit - as a per-visit rule ("enqueue any neighbour I
    // could improve") the same idea regenerates work faster than the queue
    // drains and never terminates.
    for i in 0..CS as i32 {
        for y in 0..H as i32 {
            for (inside, outside) in [
                (IVec3::new(0, y, i), IVec3::new(-1, y, i)),
                (IVec3::new(CHUNK_SIZE - 1, y, i), IVec3::new(CHUNK_SIZE, y, i)),
                (IVec3::new(i, y, 0), IVec3::new(i, y, -1)),
                (IVec3::new(i, y, CHUNK_SIZE - 1), IVec3::new(i, y, CHUNK_SIZE)),
            ] {
                let (id, light) = map.get_block_and_light(origin + outside);
                if tables.opaque[id as usize] {
                    continue;
                }
                let offer =
                    offered(map.get_light(origin + inside), false, tables.transmission[id as usize]);
                if (0..3).any(|c| light.block[c] < offer.block[c] || light.sky[c] < offer.sky[c]) {
                    lights.push(origin + outside);
                }
            }
        }
    }
}

pub struct LightPlugin;

impl Plugin for LightPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LightQueue>().add_systems(
            Update,
            (enqueue_light_updates, process_light_updates)
                .chain()
                // Must see freshly generated chunks (which seed the queue as
                // they land) and settle before the streamer snapshots padded
                // arrays for meshing, or a chunk meshes with light that's one
                // frame stale and immediately remeshes.
                .after(ChunkPipelineSet::Collect)
                .before(ChunkPipelineSet::Stream)
                .run_if(resource_exists::<ChunkMaterials>.and(in_state(AppState::InGame))),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::{BlockId, BlockRegistry, AIR};
    use crate::config::{block_index, CS, H};
    use crate::world::Chunk;

    fn setup() -> (BlockRegistry, std::sync::Arc<Tables>, ChunkMap) {
        setup_with(|_| {})
    }

    /// `setup`, with a chance to register extra blocks first - the registry
    /// refuses new blocks once it's been compiled, so a test that needs its
    /// own light source has to get in ahead of that.
    fn setup_with(
        add: impl FnOnce(&mut BlockRegistry),
    ) -> (BlockRegistry, std::sync::Arc<Tables>, ChunkMap) {
        let mut reg = BlockRegistry::with_defaults();
        add(&mut reg);
        let atlas = crate::atlas::build_atlas(&crate::atlas::default_painters());
        let tables = reg.compile(&atlas.indices, atlas.tile_size);
        // A single chunk of solid stone, so nothing is lit until a test says
        // so - a world of air would be uniformly sky-lit and hide everything
        // these tests are actually about.
        let stone = reg.id("stone");
        let mut map = ChunkMap::default();
        map.chunks.insert(
            IVec2::ZERO,
            Chunk {
                blocks: Some(vec![stone; CS * CS * H]),
                light: Some(vec![LightCell::DARK; CS * CS * H]),
                ..Chunk::default()
            },
        );
        (reg, tables, map)
    }

    /// Hollows out a solid box so light has somewhere to travel.
    fn carve(map: &mut ChunkMap, from: IVec3, to: IVec3) {
        let blocks = map.chunks.get_mut(&IVec2::ZERO).unwrap().blocks.as_mut().unwrap();
        for z in from.z..=to.z {
            for y in from.y..=to.y {
                for x in from.x..=to.x {
                    blocks[block_index(x as usize, y as usize, z as usize)] = AIR;
                }
            }
        }
    }

    /// Runs the same relaxation `process_light_updates` does, to completion,
    /// bailing out rather than hanging if it ever stops converging.
    fn drain(map: &mut ChunkMap, tables: &Tables, seeds: &[IVec3]) {
        let mut queue: VecDeque<IVec3> = seeds.iter().copied().collect();
        let mut touched = HashSet::new();
        let mut guard = 0;
        while let Some(pos) = queue.pop_front() {
            recompute_light_cell(map, tables, pos, &mut queue, &mut touched);
            guard += 1;
            assert!(guard < 400_000, "light recompute did not converge");
        }
    }

    fn seeds_around(pos: IVec3) -> Vec<IVec3> {
        let mut v = vec![pos];
        v.extend(LIGHT_NEIGHBORS.iter().map(|&d| pos + d));
        v
    }


    /// Fills a solid column of `id` from `from` to `to` inclusive.
    fn fill(map: &mut ChunkMap, from: IVec3, to: IVec3, id: BlockId) {
        let blocks = map.chunks.get_mut(&IVec2::ZERO).unwrap().blocks.as_mut().unwrap();
        for z in from.z..=to.z {
            for y in from.y..=to.y {
                for x in from.x..=to.x {
                    blocks[block_index(x as usize, y as usize, z as usize)] = id;
                }
            }
        }
    }

    /// Opens a shaft to the sky at `(x, z)` from `y_from` upward and relaxes
    /// until convergence, returning the map ready to inspect.
    fn light_shaft(map: &mut ChunkMap, tables: &Tables, x: i32, z: i32, y_from: i32) {
        let top = IVec3::new(x, H as i32 - 1, z);
        carve(map, IVec3::new(x, y_from, z), top);
        drain(map, tables, &[top]);
    }

    #[test]
    fn a_clear_medium_is_exactly_the_same_as_air() {
        // The general transmission formula has to collapse to the old
        // behavior for the overwhelming majority of blocks, or every
        // existing lighting expectation shifts underneath us.
        for v in [0u8, 1, 40, MAX_LIGHT] {
            assert_eq!(attenuate(v, u8::MAX), v, "255 transmission must be a no-op");
        }
        assert_eq!(attenuate(MAX_LIGHT, 0), 0, "zero transmission blocks everything");
    }

    #[test]
    fn glass_dims_sky_light_only_slightly_but_leaves_dim_it_a_lot() {
        let (reg, tables, mut map) = setup();
        let (glass, leaves) = (reg.id("glass"), reg.id("leaves"));

        // Two identical shafts, one capped with glass and one with leaves,
        // with the cell of interest directly beneath each cap.
        for (x, id) in [(4, glass), (10, leaves)] {
            light_shaft(&mut map, &tables, x, 8, 30);
            fill(&mut map, IVec3::new(x, 34, 8), IVec3::new(x, 34, 8), id);
            drain(&mut map, &tables, &seeds_around(IVec3::new(x, 34, 8)));
        }

        let under_glass = map.get_light(IVec3::new(4, 33, 8)).sky;
        let under_leaves = map.get_light(IVec3::new(10, 33, 8)).sky;
        assert!(
            under_glass.iter().all(|&c| c > 200),
            "glass should pass nearly all light, got {under_glass:?}"
        );
        assert!(
            (0..3).all(|c| under_leaves[c] < under_glass[c]),
            "leaves must dim more than glass: {under_leaves:?} vs {under_glass:?}"
        );
    }

    #[test]
    fn water_tints_sky_light_toward_blue_rather_than_only_darkening_it() {
        let (reg, tables, mut map) = setup();
        let water = reg.id("water");
        light_shaft(&mut map, &tables, 8, 8, 30);
        // A three-deep pool with open sky above it.
        fill(&mut map, IVec3::new(8, 32, 8), IVec3::new(8, 34, 8), water);
        drain(&mut map, &tables, &seeds_around(IVec3::new(8, 33, 8)));

        let deep = map.get_light(IVec3::new(8, 32, 8)).sky;
        assert!(
            deep[2] > deep[1] && deep[1] > deep[0],
            "water should leave blue strongest and red weakest, got {deep:?}"
        );
        assert!(deep[2] > 0, "blue should still reach the bottom of a shallow pool");
        // The point of storing three channels: this is a *tint*, not just a
        // dimming. A single-channel sky light could only have made this
        // darker, never bluer.
        assert!(
            deep[2] as u32 * 100 / (deep[0] as u32).max(1) > 130,
            "expected a visible blue/red imbalance, got {deep:?}"
        );
    }

    #[test]
    fn stacked_media_compose_so_glass_under_water_differs_from_glass_alone() {
        // The case that motivated per-channel transmission: what reaches you
        // through glass with water on top of it must differ from what
        // reaches you through glass alone.
        let (reg, tables, mut map) = setup();
        let (glass, water) = (reg.id("glass"), reg.id("water"));

        light_shaft(&mut map, &tables, 4, 8, 30);
        fill(&mut map, IVec3::new(4, 34, 8), IVec3::new(4, 34, 8), glass);
        drain(&mut map, &tables, &seeds_around(IVec3::new(4, 34, 8)));

        light_shaft(&mut map, &tables, 10, 8, 30);
        fill(&mut map, IVec3::new(10, 34, 8), IVec3::new(10, 34, 8), glass);
        fill(&mut map, IVec3::new(10, 35, 8), IVec3::new(10, 36, 8), water);
        drain(&mut map, &tables, &seeds_around(IVec3::new(10, 35, 8)));

        let glass_only = map.get_light(IVec3::new(4, 33, 8)).sky;
        let through_both = map.get_light(IVec3::new(10, 33, 8)).sky;

        assert!(
            (0..3).all(|c| through_both[c] < glass_only[c]),
            "water above glass must cost light in every channel: \
             {through_both:?} vs {glass_only:?}"
        );
        // ...and cost it *unequally*, which is what makes the pair
        // qualitatively different rather than just dimmer.
        let glass_ratio = glass_only[0] as f32 / glass_only[2] as f32;
        let both_ratio = through_both[0] as f32 / through_both[2] as f32;
        assert!(
            both_ratio < glass_ratio * 0.9,
            "the water should skew the color balance, not just scale it: \
             {both_ratio} vs {glass_ratio}"
        );
    }

    #[test]
    fn transmission_multiplies_so_repeated_panes_keep_adding_up() {
        // The reason transmission is a multiplicative fraction on a fine
        // scale rather than a subtracted number of levels: on the old
        // 0..=16 scale a 2% absorption rounded to zero, so any number of
        // panes of glass in a row blocked exactly nothing.
        let (reg, tables, mut map) = setup();
        let glass = reg.id("glass");
        light_shaft(&mut map, &tables, 8, 8, 20);
        fill(&mut map, IVec3::new(8, 30, 8), IVec3::new(8, 37, 8), glass);
        drain(&mut map, &tables, &seeds_around(IVec3::new(8, 34, 8)));

        let above = map.get_light(IVec3::new(8, 37, 8)).sky[0];
        let below = map.get_light(IVec3::new(8, 30, 8)).sky[0];
        assert!(
            below < above,
            "eight panes of glass should cost something, got {below} under {above}"
        );
    }

    #[test]
    fn an_opaque_block_blocks_light_whatever_its_transmission_says() {
        // `transmission` is meaningless for a block light can't enter, and
        // the tables resolve that once rather than making every consumer
        // check two fields that could disagree.
        let (reg, tables, _) = setup_with(|reg| {
            reg.register(crate::blocks::BlockDef {
                id: "bogus".into(),
                transparency: crate::blocks::Transparency::No,
                transmission: crate::blocks::Transmission::Uniform(1.0),
                textures: crate::blocks::FaceTextures::all("stone"),
                ..crate::blocks::BlockDef::default()
            });
        });
        let bogus = reg.id("bogus") as usize;
        assert!(tables.opaque[bogus]);
        assert_eq!(tables.transmission[bogus], [0; 3], "opaque wins over transmission");
    }

    #[test]
    fn relax_improves_or_empties_but_never_settles_for_worse() {
        assert_eq!(relax(5, 9), 9, "a better candidate is accepted");
        assert_eq!(relax(5, 5), 5, "an equal candidate is a no-op");
        assert_eq!(relax(5, 4), 0, "a worse candidate empties instead of downgrading");
        assert_eq!(relax(0, 0), 0);
    }

    #[test]
    fn a_torch_lights_its_surroundings_and_falls_off_one_level_per_block() {
        let (reg, tables, mut map) = setup();
        let torch = reg.id("torch");
        let center = IVec3::new(8, 32, 8);
        carve(&mut map, center - IVec3::splat(6), center + IVec3::splat(6));
        map.set_block(center, torch);

        drain(&mut map, &tables, &seeds_around(center));

        let emission = tables.light[torch as usize][0];
        assert_eq!(map.get_light(center).block[0], emission, "the torch cell holds its own emission");
        // Equal falloff in all six directions - "fans out equally".
        for d in LIGHT_NEIGHBORS {
            for step in 1..=5u8 {
                let at = map.get_light(center + d * step as i32).block[0];
                assert_eq!(
                    at,
                    emission - LEVEL_STEP * step,
                    "direction {d:?}, {step} blocks out"
                );
            }
        }
    }

    #[test]
    fn light_does_not_pass_through_opaque_blocks() {
        let (reg, tables, mut map) = setup();
        let torch = reg.id("torch");
        let stone = reg.id("stone");
        // A one-cell-wide corridor bored through solid stone, so a single
        // wall block genuinely seals it. In an open room light would just
        // travel around one block - which is correct, and would make this
        // test prove nothing.
        carve(&mut map, IVec3::new(4, 32, 8), IVec3::new(12, 32, 8));
        let torch_pos = IVec3::new(5, 32, 8);
        let wall = IVec3::new(8, 32, 8);
        map.set_block(torch_pos, torch);
        map.set_block(wall, stone);

        drain(&mut map, &tables, &seeds_around(torch_pos));

        let emission = tables.light[torch as usize][0];
        assert_eq!(map.get_light(IVec3::new(6, 32, 8)).block[0], emission - LEVEL_STEP);
        assert_eq!(map.get_light(IVec3::new(7, 32, 8)).block[0], emission - 2 * LEVEL_STEP);
        assert_eq!(map.get_light(wall).block, [0; 3], "the wall itself holds no light");
        for x in 9..=12 {
            assert_eq!(
                map.get_light(IVec3::new(x, 32, 8)).block,
                [0; 3],
                "light leaked past the wall to x={x}"
            );
        }
    }

    #[test]
    fn removing_a_torch_puts_its_whole_lit_region_back_to_dark() {
        let (reg, tables, mut map) = setup();
        let torch = reg.id("torch");
        let center = IVec3::new(8, 32, 8);
        carve(&mut map, center - IVec3::splat(6), center + IVec3::splat(6));
        map.set_block(center, torch);
        drain(&mut map, &tables, &seeds_around(center));
        assert!(map.get_light(center + IVec3::X * 3).block[0] > 0);

        map.set_block(center, AIR);
        drain(&mut map, &tables, &seeds_around(center));

        for step in 0..=5 {
            assert_eq!(
                map.get_light(center + IVec3::X * step).block,
                [0; 3],
                "still lit {step} blocks out after the torch was removed"
            );
        }
    }

    #[test]
    fn a_second_torch_keeps_its_own_region_lit_when_the_first_is_removed() {
        // The exact case pull-only propagation gets wrong: cells between the
        // two torches drop to zero when the first is removed, and nothing
        // re-derives them unless the surviving torch's side pushes back.
        let (reg, tables, mut map) = setup();
        let torch = reg.id("torch");
        let a = IVec3::new(4, 32, 8);
        let b = IVec3::new(14, 32, 8);
        carve(&mut map, IVec3::new(1, 29, 5), IVec3::new(14, 35, 11));
        map.set_block(a, torch);
        map.set_block(b, torch);
        drain(&mut map, &tables, &[seeds_around(a), seeds_around(b)].concat());

        map.set_block(a, AIR);
        drain(&mut map, &tables, &seeds_around(a));

        let emission = tables.light[torch as usize][0];
        assert_eq!(map.get_light(b).block[0], emission, "the surviving torch still emits");
        // Every cell between them should now be lit purely by `b`, falling
        // off with distance from it - not stuck at the zero it hit while the
        // removal cascade swept through.
        for x in 5..14 {
            let pos = IVec3::new(x, 32, 8);
            let expected = emission - LEVEL_STEP * (b.x - x) as u8;
            assert_eq!(map.get_light(pos).block[0], expected, "at x={x}");
        }
    }

    #[test]
    fn two_lights_combine_per_channel_by_maximum_not_by_adding_up() {
        // A red lamp and a blue lamp facing each other across a corridor.
        let (reg, tables, mut map) = setup_with(|reg| {
            for (id, color) in [("red_lamp", [255, 0, 0]), ("blue_lamp", [0, 0, 255])] {
                reg.register(crate::blocks::BlockDef {
                    id: id.into(),
                    light: crate::blocks::LightSpec { level: MAX_LEVEL, color },
                    textures: crate::blocks::FaceTextures::all("stone"),
                    ..crate::blocks::BlockDef::default()
                });
            }
        });
        let (red, blue) = (reg.id("red_lamp"), reg.id("blue_lamp"));

        let a = IVec3::new(4, 32, 8);
        let b = IVec3::new(10, 32, 8);
        carve(&mut map, IVec3::new(3, 31, 7), IVec3::new(11, 33, 9));
        map.set_block(a, red);
        map.set_block(b, blue);
        drain(&mut map, &tables, &[seeds_around(a), seeds_around(b)].concat());

        let mid = map.get_light(IVec3::new(7, 32, 8)).block;
        assert_eq!(mid[0], MAX_LIGHT - 3 * LEVEL_STEP, "red arrives from 3 blocks away");
        assert_eq!(mid[2], MAX_LIGHT - 3 * LEVEL_STEP, "blue arrives from 3 blocks away");
        assert_eq!(mid[1], 0, "neither lamp emits green, so green stays dark");
        // Right next to the red lamp, red is strong and blue is weak: the
        // channels genuinely travel independently rather than being one
        // brightness with a color stapled on.
        let near_red = map.get_light(IVec3::new(5, 32, 8)).block;
        assert!(near_red[0] > near_red[2], "expected a red-dominated cell, got {near_red:?}");
    }

    #[test]
    fn sky_light_falls_straight_down_a_shaft_at_full_strength() {
        let (_, tables, mut map) = setup();
        // A shaft from the top of the world down to y=20.
        let x = 8;
        let z = 8;
        carve(&mut map, IVec3::new(x, 20, z), IVec3::new(x, H as i32 - 1, z));
        // The top cell sees open sky; everything below inherits it.
        let top = IVec3::new(x, H as i32 - 1, z);
        drain(&mut map, &tables, &[top]);

        for y in 20..H as i32 {
            assert_eq!(
                map.get_light(IVec3::new(x, y, z)).sky,
                [MAX_LIGHT; 3],
                "shaft should stay at full sky light at y={y}"
            );
        }
        assert_eq!(map.get_light(IVec3::new(x, 19, z)).sky, [0; 3], "solid floor below the shaft");
    }

    #[test]
    fn sky_light_dims_one_level_per_block_spreading_sideways_under_an_overhang() {
        let (_, tables, mut map) = setup();
        // A shaft at x=8 with a horizontal tunnel running off it at y=20.
        carve(&mut map, IVec3::new(8, 20, 8), IVec3::new(8, H as i32 - 1, 8));
        carve(&mut map, IVec3::new(8, 20, 8), IVec3::new(13, 20, 8));
        drain(&mut map, &tables, &[IVec3::new(8, H as i32 - 1, 8)]);

        for (step, x) in (9..=13).enumerate() {
            assert_eq!(
                map.get_light(IVec3::new(x, 20, 8)).sky,
                [MAX_LIGHT - LEVEL_STEP * (step as u8 + 1); 3],
                "sky light {} blocks into the tunnel",
                step + 1
            );
        }
    }

    #[test]
    fn block_light_and_sky_light_are_tracked_independently() {
        let (reg, tables, mut map) = setup();
        let torch = reg.id("torch");
        let pos = IVec3::new(8, 30, 8);
        carve(&mut map, IVec3::new(6, 28, 6), IVec3::new(10, 32, 10));
        map.set_block(pos, torch);
        drain(&mut map, &tables, &seeds_around(pos));

        let lit = map.get_light(pos + IVec3::X);
        assert!(lit.block[0] > 0, "torch light reached it");
        assert_eq!(lit.sky, [0; 3], "a sealed pocket gets no sky light, however bright the torch");
    }

    #[test]
    fn opaque_cells_hold_no_light_but_an_opaque_emitter_still_does() {
        let (reg, tables, mut map) = setup();
        let torch = reg.id("torch");
        let pos = IVec3::new(8, 30, 8);
        carve(&mut map, IVec3::new(6, 28, 6), IVec3::new(10, 32, 10));
        map.set_block(pos, torch);
        drain(&mut map, &tables, &seeds_around(pos));

        // The torch is a solid block: it emits, but the stone around it -
        // opaque, non-emitting - stores nothing at all.
        assert!(map.get_light(pos).block[0] > 0);
        assert_eq!(map.get_light(IVec3::new(6, 28, 6) - IVec3::X).block, [0; 3]);
    }
}
