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
//! - **Sky light** — one channel, `0..=`[`MAX_LIGHT`], "how much of the open
//!   sky reaches this cell". Deliberately *not* stored pre-multiplied by the
//!   time of day: it's a property of the world's shape, so the day/night
//!   cycle can scale (and tint - see below) it every frame with no
//!   relighting or remeshing at all.
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

/// The brightest a light channel can be. A cell at `MAX_LIGHT` renders at
/// full texture brightness; each block travelled costs one level, so a
/// `MAX_LIGHT` emitter is still faintly visible `MAX_LIGHT - 1` blocks away.
pub const MAX_LIGHT: u8 = 16;

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

/// One cell's stored light. 4 bytes, held in a `Vec` parallel to
/// `Chunk::blocks` exactly like `fluid_level`/`axis` (see CLAUDE.md's note on
/// per-cell dynamic state).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LightCell {
    /// Per-channel colored block light, `0..=MAX_LIGHT`.
    pub block: [u8; 3],
    /// How much open sky reaches here, `0..=MAX_LIGHT`. Uncolored - the tint
    /// is applied at render time from the current sky color, so one stored
    /// channel serves both a blue noon and a red-moon midnight.
    pub sky: u8,
}

impl LightCell {
    pub const DARK: LightCell = LightCell { block: [0; 3], sky: 0 };
    /// What sits above the top of the world: nothing blocking the sky.
    pub const OPEN_SKY: LightCell = LightCell { block: [0; 3], sky: MAX_LIGHT };
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

/// What one cell would offer a neighbour in direction `toward_down`
/// (`true` only for the cell directly below it): every channel loses one
/// level, except sky light at full strength, which falls straight down
/// undiminished so a deep shaft stays lit all the way to the bottom.
fn offered(from: LightCell, toward_down: bool) -> LightCell {
    LightCell {
        block: [
            from.block[0].saturating_sub(1),
            from.block[1].saturating_sub(1),
            from.block[2].saturating_sub(1),
        ],
        sky: if toward_down && from.sky == MAX_LIGHT {
            MAX_LIGHT
        } else {
            from.sky.saturating_sub(1)
        },
    }
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
    let mut candidate = LightCell { block: tables.light[id as usize], sky: 0 };
    if !tables.opaque[id as usize] {
        for (i, d) in LIGHT_NEIGHBORS.iter().enumerate() {
            let offer = offered(map.get_light(pos + *d), i == NEIGHBOR_UP);
            for c in 0..3 {
                candidate.block[c] = candidate.block[c].max(offer.block[c]);
            }
            candidate.sky = candidate.sky.max(offer.sky);
        }
    }

    let next = LightCell {
        block: [
            relax(current.block[0], candidate.block[0]),
            relax(current.block[1], candidate.block[1]),
            relax(current.block[2], candidate.block[2]),
        ],
        sky: relax(current.sky, candidate.sky),
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
    if (0..3).any(|c| next.block[c] < current.block[c]) || next.sky < current.sky {
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
                if !emits && (tables.opaque[id] || (!on_border && light[idx].sky == MAX_LIGHT)) {
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
                let offer = offered(map.get_light(origin + inside), false);
                if (0..3).any(|c| light.block[c] < offer.block[c]) || light.sky < offer.sky {
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
    use crate::blocks::{BlockRegistry, AIR};
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
                assert_eq!(at, emission - step, "direction {d:?}, {step} blocks out");
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
        assert_eq!(map.get_light(IVec3::new(6, 32, 8)).block[0], emission - 1);
        assert_eq!(map.get_light(IVec3::new(7, 32, 8)).block[0], emission - 2);
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
            let expected = emission - (b.x - x) as u8;
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
                    light: crate::blocks::LightSpec { level: MAX_LIGHT as u32, color },
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
        assert_eq!(mid[0], MAX_LIGHT - 3, "red arrives from 3 blocks away");
        assert_eq!(mid[2], MAX_LIGHT - 3, "blue arrives from 3 blocks away");
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
                MAX_LIGHT,
                "shaft should stay at full sky light at y={y}"
            );
        }
        assert_eq!(map.get_light(IVec3::new(x, 19, z)).sky, 0, "solid floor below the shaft");
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
                MAX_LIGHT - (step as u8 + 1),
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
        assert_eq!(lit.sky, 0, "a sealed pocket gets no sky light, however bright the torch");
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
