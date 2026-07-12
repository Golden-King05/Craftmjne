//! World: owns chunk data, streams chunks around the player, and coordinates
//! generation/meshing jobs on the async compute task pool.
//!
//! Chunk lifecycle:
//!   requested -> generating (task) -> ready -> meshing (task) -> rendered
//!
//! Block data for visited chunks stays in memory when meshes are unloaded, so
//! player edits persist while roaming. Meshing requires the eight neighbouring
//! chunks to exist (the mesher gets a 1-block padded shell for seamless
//! culling and ambient occlusion across chunk borders).

use bevy::prelude::*;
use bevy::tasks::{block_on, futures_lite::future, AsyncComputeTaskPool, Task};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use crate::atlas::{build_atlas, default_painters, AtlasData, Painters};
use crate::blocks::{
    BlockId, BlockRegistry, BlockTables, Tables, AIR, AXIS_Y, FLUID_FALLING, FLUID_SOURCE,
};
use crate::config::{block_index, WorldSettings, CHUNK_SIZE, H, WORLD_HEIGHT};
use crate::icons::{build_icon_atlas, IconAtlasData};
use crate::mesher::{mesh_chunk, padded_index, ChunkMeshData, PAD_XZ, PAD_Y};
use crate::player::Player;
use crate::render::ChunkMaterials;
use crate::save::{BlockEdit, GameMode, PlayerSave, SaveStore, WorldData};
use crate::state::{ActiveWorld, AppState};
use crate::terrain::{GeneratedChunk, TerrainGenerator};

const MAX_GEN_TASKS: usize = 12;
const MAX_MESH_TASKS: usize = 8;
const AUTOSAVE_INTERVAL: f32 = 30.0;

#[derive(Resource, Clone)]
pub struct WorldGen(pub Arc<TerrainGenerator>);

/// Non-render atlas data (pixel buffer + name->tile map), built at startup.
#[derive(Resource)]
pub struct Atlas(pub AtlasData);

/// Non-render baked isometric icon data (see `icons.rs`), built at startup
/// right after `Atlas` since it's derived from the same tile pixels.
#[derive(Resource)]
pub struct IconAtlas(pub IconAtlasData);

#[derive(Event)]
pub struct BlockSetEvent {
    pub pos: IVec3,
    pub id: BlockId,
    pub prev: BlockId,
    /// The placed orientation (`blocks::AXIS_X/Y/Z`) - `AXIS_Y` (the
    /// default) for anything that doesn't rotate. Carried on the event so
    /// `record_edits`/`write_save` can persist it alongside the block id.
    pub axis: u8,
}

#[derive(Event)]
pub struct ChunkMeshedEvent(pub IVec2);

#[derive(Default)]
pub struct Chunk {
    pub blocks: Option<Vec<BlockId>>,
    /// Parallel to `blocks`; meaningful only where the corresponding block id
    /// is a fluid (see `blocks::FLUID_SOURCE`/`FLUID_FALLING`). Always
    /// `Some` whenever `blocks` is.
    pub fluid_level: Option<Vec<u8>>,
    /// Parallel to `blocks`; meaningful only where the corresponding block id
    /// rotates (`blocks::Tables::rotates`). Always `Some` whenever `blocks`
    /// is - same lifecycle as `fluid_level`. Unlike `fluid_level` (simulated,
    /// re-derivable, never saved), this *is* player-chosen state and round-
    /// trips through `BlockSetEvent`/`EditLog`/`save::BlockEdit`.
    pub axis: Option<Vec<u8>>,
    pub version: u32,
    pub dirty: bool,
    pub meshing: bool,
    pub meshed: bool,
    pub solid_entity: Option<Entity>,
    pub water_entity: Option<Entity>,
}

#[derive(Resource, Default)]
pub struct ChunkMap {
    pub chunks: HashMap<IVec2, Chunk>,
    pub gen_in_flight: usize,
    pub mesh_in_flight: usize,
    pub needs_scan: bool,
    last_player_chunk: Option<IVec2>,
}

impl ChunkMap {
    #[inline]
    fn chunk_coord(wx: i32, wz: i32) -> IVec2 {
        IVec2::new(wx.div_euclid(CHUNK_SIZE), wz.div_euclid(CHUNK_SIZE))
    }

    pub fn get_block(&self, pos: IVec3) -> BlockId {
        if pos.y < 0 || pos.y >= WORLD_HEIGHT {
            return AIR;
        }
        let Some(chunk) = self.chunks.get(&Self::chunk_coord(pos.x, pos.z)) else {
            return AIR;
        };
        let Some(blocks) = &chunk.blocks else { return AIR };
        blocks[block_index(
            pos.x.rem_euclid(CHUNK_SIZE) as usize,
            pos.y as usize,
            pos.z.rem_euclid(CHUNK_SIZE) as usize,
        )]
    }

    /// Physics-safe solidity: unloaded terrain counts as solid.
    pub fn is_solid(&self, tables: &Tables, pos: IVec3) -> bool {
        if pos.y < 0 {
            return true;
        }
        if pos.y >= WORLD_HEIGHT {
            return false;
        }
        let Some(chunk) = self.chunks.get(&Self::chunk_coord(pos.x, pos.z)) else {
            return true;
        };
        let Some(blocks) = &chunk.blocks else { return true };
        tables.solid[blocks[block_index(
            pos.x.rem_euclid(CHUNK_SIZE) as usize,
            pos.y as usize,
            pos.z.rem_euclid(CHUNK_SIZE) as usize,
        )] as usize]
    }

    /// Border edits affect neighbouring chunks' culling/AO shells too, so a
    /// write on a chunk-edge cell also bumps up to 3 neighbouring chunks.
    fn touch_borders(&mut self, coord: IVec2, lx: i32, lz: i32) {
        let dxs: &[i32] = if lx == 0 { &[-1, 0] } else if lx == CHUNK_SIZE - 1 { &[0, 1] } else { &[0] };
        let dzs: &[i32] = if lz == 0 { &[-1, 0] } else if lz == CHUNK_SIZE - 1 { &[0, 1] } else { &[0] };
        for &dx in dxs {
            for &dz in dzs {
                if dx == 0 && dz == 0 {
                    continue;
                }
                if let Some(n) = self.chunks.get_mut(&(coord + IVec2::new(dx, dz))) {
                    n.version += 1;
                    n.dirty = true;
                }
            }
        }
    }

    /// Writes a block, marks the chunk (and border neighbours) for remeshing.
    /// Returns the previous block id, or None if the chunk isn't loaded.
    pub fn set_block(&mut self, pos: IVec3, id: BlockId) -> Option<BlockId> {
        if pos.y < 0 || pos.y >= WORLD_HEIGHT {
            return None;
        }
        let coord = Self::chunk_coord(pos.x, pos.z);
        let lx = pos.x.rem_euclid(CHUNK_SIZE);
        let lz = pos.z.rem_euclid(CHUNK_SIZE);

        let chunk = self.chunks.get_mut(&coord)?;
        let blocks = chunk.blocks.as_mut()?;
        let idx = block_index(lx as usize, pos.y as usize, lz as usize);
        let prev = blocks[idx];
        if prev == id {
            return None;
        }
        blocks[idx] = id;
        chunk.version += 1;
        chunk.dirty = true;

        self.touch_borders(coord, lx, lz);
        self.needs_scan = true;
        Some(prev)
    }

    /// Directly overwrites a cell's stored fluid level without touching its
    /// block id (used right after placing a fluid block, so it starts as a
    /// permanent source rather than whatever level that cell last held).
    /// No-op if the chunk isn't loaded.
    pub fn set_fluid_level_raw(&mut self, pos: IVec3, level: u8) {
        if pos.y < 0 || pos.y >= WORLD_HEIGHT {
            return;
        }
        let coord = Self::chunk_coord(pos.x, pos.z);
        let idx = block_index(
            pos.x.rem_euclid(CHUNK_SIZE) as usize,
            pos.y as usize,
            pos.z.rem_euclid(CHUNK_SIZE) as usize,
        );
        if let Some(levels) = self.chunks.get_mut(&coord).and_then(|c| c.fluid_level.as_mut()) {
            levels[idx] = level;
        }
    }

    /// Directly overwrites a cell's stored rotation axis without touching
    /// its block id - mirrors `set_fluid_level_raw` exactly. No-op if the
    /// chunk isn't loaded.
    pub fn set_axis_raw(&mut self, pos: IVec3, axis: u8) {
        if pos.y < 0 || pos.y >= WORLD_HEIGHT {
            return;
        }
        let coord = Self::chunk_coord(pos.x, pos.z);
        let idx = block_index(
            pos.x.rem_euclid(CHUNK_SIZE) as usize,
            pos.y as usize,
            pos.z.rem_euclid(CHUNK_SIZE) as usize,
        );
        if let Some(axes) = self.chunks.get_mut(&coord).and_then(|c| c.axis.as_mut()) {
            axes[idx] = axis;
        }
    }

    /// Reads a cell's stored fluid level. Only meaningful when `get_block`
    /// for the same position is a fluid id; returns `FLUID_SOURCE` for
    /// unloaded chunks (harmless, since callers gate on the block id first).
    fn get_fluid_level(&self, pos: IVec3) -> u8 {
        if pos.y < 0 || pos.y >= WORLD_HEIGHT {
            return FLUID_SOURCE;
        }
        let Some(chunk) = self.chunks.get(&Self::chunk_coord(pos.x, pos.z)) else {
            return FLUID_SOURCE;
        };
        let Some(levels) = &chunk.fluid_level else { return FLUID_SOURCE };
        levels[block_index(
            pos.x.rem_euclid(CHUNK_SIZE) as usize,
            pos.y as usize,
            pos.z.rem_euclid(CHUNK_SIZE) as usize,
        )]
    }

    /// Sets both a cell's block id and fluid level in one write, used by the
    /// spread/dry-up simulation (`recompute_cell`). Unlike `set_block`, this
    /// does *not* fire a `BlockSetEvent` — simulated flow isn't a player
    /// edit and shouldn't bloat the save file. Returns false if the chunk
    /// isn't loaded.
    fn set_fluid_cell(&mut self, pos: IVec3, id: BlockId, level: u8) -> bool {
        if pos.y < 0 || pos.y >= WORLD_HEIGHT {
            return false;
        }
        let coord = Self::chunk_coord(pos.x, pos.z);
        let lx = pos.x.rem_euclid(CHUNK_SIZE);
        let lz = pos.z.rem_euclid(CHUNK_SIZE);
        let Some(chunk) = self.chunks.get_mut(&coord) else { return false };
        let (Some(blocks), Some(levels)) = (chunk.blocks.as_mut(), chunk.fluid_level.as_mut())
        else {
            return false;
        };
        let idx = block_index(lx as usize, pos.y as usize, lz as usize);
        blocks[idx] = id;
        levels[idx] = level;
        chunk.version += 1;
        chunk.dirty = true;

        self.touch_borders(coord, lx, lz);
        self.needs_scan = true;
        true
    }

    /// Topmost solid block in a column, or None if the chunk isn't generated.
    pub fn surface_y(&self, tables: &Tables, wx: i32, wz: i32) -> Option<i32> {
        let chunk = self.chunks.get(&Self::chunk_coord(wx, wz))?;
        let blocks = chunk.blocks.as_ref()?;
        let base = block_index(
            wx.rem_euclid(CHUNK_SIZE) as usize,
            0,
            wz.rem_euclid(CHUNK_SIZE) as usize,
        );
        (0..H).rev().find(|&y| tables.solid[blocks[base + y] as usize] as bool).map(|y| y as i32)
    }

    fn neighbors_ready(&self, coord: IVec2) -> bool {
        for dz in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                match self.chunks.get(&(coord + IVec2::new(dx, dz))) {
                    Some(c) if c.blocks.is_some() => {}
                    _ => return false,
                }
            }
        }
        true
    }

    /// Copies chunk blocks (+fluid levels +rotation axes) plus a 1-block
    /// shell from the 8 neighbours into padded arrays. Y-major layout keeps
    /// this a series of column copies.
    fn build_padded(&self, coord: IVec2) -> (Vec<BlockId>, Vec<u8>, Vec<u8>) {
        let mut padded = vec![AIR; PAD_XZ * PAD_XZ * PAD_Y];
        let mut padded_fluid = vec![FLUID_SOURCE; PAD_XZ * PAD_XZ * PAD_Y];
        let mut padded_axis = vec![AXIS_Y; PAD_XZ * PAD_XZ * PAD_Y];
        for pz in -1..=CHUNK_SIZE {
            let ncz = coord.y + if pz < 0 { -1 } else if pz >= CHUNK_SIZE { 1 } else { 0 };
            let lz = pz.rem_euclid(CHUNK_SIZE) as usize;
            for px in -1..=CHUNK_SIZE {
                let ncx = coord.x + if px < 0 { -1 } else if px >= CHUNK_SIZE { 1 } else { 0 };
                let lx = px.rem_euclid(CHUNK_SIZE) as usize;
                let chunk = &self.chunks[&IVec2::new(ncx, ncz)];
                let src = chunk.blocks.as_ref().unwrap();
                let src_fluid = chunk.fluid_level.as_ref().unwrap();
                let src_axis = chunk.axis.as_ref().unwrap();
                let src_base = block_index(lx, 0, lz);
                let dst_base = padded_index(px, 0, pz);
                padded[dst_base..dst_base + H].copy_from_slice(&src[src_base..src_base + H]);
                padded_fluid[dst_base..dst_base + H]
                    .copy_from_slice(&src_fluid[src_base..src_base + H]);
                padded_axis[dst_base..dst_base + H]
                    .copy_from_slice(&src_axis[src_base..src_base + H]);
                padded[dst_base - 1] = 1; // below the world: solid, culls bottom faces
                // above the world stays 0 (air)
            }
        }
        (padded, padded_fluid, padded_axis)
    }

    pub fn stats(&self) -> (usize, usize) {
        let generated = self.chunks.values().filter(|c| c.blocks.is_some()).count();
        let meshed = self.chunks.values().filter(|c| c.meshed).count();
        (generated, meshed)
    }
}

#[derive(Component)]
struct GenTask {
    coord: IVec2,
    task: Task<GeneratedChunk>,
}

#[derive(Component)]
struct MeshTask {
    coord: IVec2,
    version: u32,
    task: Task<ChunkMeshData>,
}

/// Edits made this session, keyed by world position (last write wins). Reset
/// fresh on every `enter_world`; flushed to disk by `exit_world` and the
/// periodic autosave. Stores `(id, axis)` rather than just `id` so a
/// rotated block's orientation survives a save/reload, not just its type.
#[derive(Resource, Default)]
pub struct EditLog(pub HashMap<IVec3, (BlockId, u8)>);

/// Edits loaded from a save, grouped by chunk so `collect_gen_tasks` can
/// apply them in O(1) right after a chunk finishes procedurally generating.
#[derive(Resource, Default)]
struct PendingEdits(HashMap<IVec2, Vec<(IVec3, BlockId, u8)>>);

#[derive(Resource, Default)]
struct AutosaveTimer(f32);

/// Positions needing a fluid spread/dry-up recompute, fed by `BlockSetEvent`
/// and by cells whose neighbours just changed. See `recompute_cell` for the
/// actual relaxation rule; this is deliberately id-agnostic (keyed only on
/// `Tables::fluid`/`flow_distance`) so a future second fluid (lava, say)
/// needs zero changes here.
#[derive(Resource, Default)]
struct FluidQueue(VecDeque<IVec3>);

/// A source/falling cell's neighbours to touch when it changes.
const FLUID_NEIGHBORS: [IVec3; 6] =
    [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z, IVec3::Y, IVec3::NEG_Y];
/// The 4 side neighbours a fluid spreads through / checks for the
/// infinite-source rule (unlike `FLUID_NEIGHBORS`, no up/down).
const FLUID_SIDES: [IVec3; 4] = [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z];

/// How "far from a source" a level counts as for comparison purposes —
/// source and falling cells both rank `0` (best), a plain flowing level
/// ranks as itself. Used both to pick the best neighbour to spread from and
/// to decide whether a candidate is actually an improvement (see
/// `recompute_cell`'s never-downgrade rule).
fn fluid_rank(level: u8) -> u8 {
    if level == FLUID_SOURCE || level == FLUID_FALLING { 0 } else { level }
}

/// Queues the position of every `BlockSetEvent` plus its 6 neighbours for a
/// fluid recompute — covers both "a fluid was placed/removed here" and "a
/// solid obstruction just appeared/disappeared next to a fluid".
fn enqueue_fluid_updates(mut events: EventReader<BlockSetEvent>, mut queue: ResMut<FluidQueue>) {
    for e in events.read() {
        queue.0.push_back(e.pos);
        for d in FLUID_NEIGHBORS {
            queue.0.push_back(e.pos + d);
        }
    }
}

const FLUID_TICK: f32 = 1.0 / 12.0;
const FLUID_TICKS_PER_FRAME: u32 = 4;
const FLUID_BUDGET_PER_TICK: usize = 512;

/// Budgeted, ticked relaxation so a big spread is visibly gradual (like
/// Minecraft's own fluid ticks) instead of resolving in a single frame.
fn process_fluid_updates(
    mut map: ResMut<ChunkMap>,
    tables: Res<BlockTables>,
    mut queue: ResMut<FluidQueue>,
    time: Res<Time>,
    mut acc: Local<f32>,
) {
    if queue.0.is_empty() {
        return;
    }
    *acc += time.delta_secs();
    let mut ticks = 0;
    while *acc >= FLUID_TICK && ticks < FLUID_TICKS_PER_FRAME {
        *acc -= FLUID_TICK;
        ticks += 1;
        for _ in 0..FLUID_BUDGET_PER_TICK {
            let Some(pos) = queue.0.pop_front() else { break };
            recompute_cell(&mut map, &tables.0, pos, &mut queue.0);
        }
    }
}

/// The core "write once, works for any fluid" spread/dry-up rule for one
/// cell:
///  - A permanent source (level `FLUID_SOURCE`) never changes.
///  - If the cell directly above is any fluid, this cell becomes a
///    full-height `FLUID_FALLING` column of that same fluid (a waterfall).
///  - Otherwise, look at the 4 lateral neighbours: among whichever fluid
///    reaches this cell at the lowest effective level (sources/falling count
///    as 0), spread one level further out, capped by that fluid's
///    `flow_distance`. A falling neighbour only counts as a lateral supply
///    once it's landed (blocked below) — otherwise every height along an
///    open shaft would bleed sideways and flood far more than
///    `flow_distance` would suggest (a waterfall should pool at the
///    bottom, not leak out its whole length).
///  - "Infinite water" source-conversion: if at least 2 of the 4 lateral
///    neighbours are permanent sources of the fluid this cell would host,
///    it becomes a source itself instead of a flowing/falling cell (the
///    classic "two sources either side of a gap" trick).
///  - If neither applies and this cell currently holds a non-source fluid,
///    it dries up (back to air).
///  - An already-fluid cell only ever *improves* (lower level) or dries —
///    never adopts a worse level than it already has. Without this a
///    removed source's former network can thrash forever, each cell
///    re-deriving a slightly-worse level from a neighbour doing the same.
/// A fluid can only occupy air, a `replaceable` block, or itself — anything
/// else blocks it outright, matching the existing placement rule in
/// `interact.rs`.
fn recompute_cell(map: &mut ChunkMap, tables: &Tables, pos: IVec3, queue: &mut VecDeque<IVec3>) {
    if pos.y < 0 || pos.y >= WORLD_HEIGHT {
        return;
    }
    let id = map.get_block(pos);
    let is_fluid_here = tables.fluid[id as usize];
    if is_fluid_here && map.get_fluid_level(pos) == FLUID_SOURCE {
        return; // permanent source
    }

    let above_id = map.get_block(pos + IVec3::Y);
    let mut candidate: Option<(BlockId, u8)> =
        tables.fluid[above_id as usize].then_some((above_id, FLUID_FALLING));

    if candidate.is_none() {
        for d in FLUID_SIDES {
            let npos = pos + d;
            let nid = map.get_block(npos);
            if !tables.fluid[nid as usize] {
                continue;
            }
            let nlevel = map.get_fluid_level(npos);
            if nlevel == FLUID_FALLING {
                // A waterfall segment only spreads sideways once it lands
                // (the cell below it is blocked). While it still has open
                // space to keep falling into, it isn't a lateral supply —
                // otherwise every height along an open shaft would bleed
                // sideways and flood volumes far larger than flow_distance.
                let below = npos.y - 1;
                let below_open = below >= 0 && {
                    let below_id = map.get_block(IVec3::new(npos.x, below, npos.z));
                    below_id == AIR || tables.replaceable[below_id as usize]
                };
                if below_open {
                    continue;
                }
            }
            let eff = fluid_rank(nlevel);
            let fd = tables.flow_distance[nid as usize].max(1);
            if eff >= fd {
                continue; // already at max range, can't spread further
            }
            let level = eff + 1;
            if candidate.as_ref().is_none_or(|&(_, best)| level < best) {
                candidate = Some((nid, level));
            }
        }
    }

    // "Infinite water": 2+ side neighbours that are already permanent
    // sources of the same fluid upgrade this cell straight to a source too,
    // regardless of whether it got here via falling or lateral spread.
    if let Some((cid, clevel)) = candidate {
        if clevel != FLUID_SOURCE {
            let source_sides = FLUID_SIDES
                .iter()
                .filter(|&&d| {
                    let npos = pos + d;
                    map.get_block(npos) == cid && map.get_fluid_level(npos) == FLUID_SOURCE
                })
                .count();
            if source_sides >= 2 {
                candidate = Some((cid, FLUID_SOURCE));
            }
        }
    }

    match candidate {
        Some((cid, clevel)) => {
            if id == cid {
                let cur_rank = fluid_rank(map.get_fluid_level(pos));
                let cand_rank = fluid_rank(clevel);
                if cand_rank == cur_rank {
                    return; // already correct
                }
                if cand_rank > cur_rank {
                    // The best supply currently visible is *worse* than what
                    // this cell already has — its real supply chain was cut.
                    // Dry instead of drifting to a worse-but-still-wet
                    // level: accepting a worse candidate here can thrash
                    // indefinitely as a removed source's former network
                    // keeps "downgrading" through itself (cells feeding
                    // each other slightly-worse levels forever). Drying is
                    // monotonic — a cell only ever dries once — and any
                    // neighbour with a genuinely still-valid path simply
                    // re-floods it on a later recompute.
                    if map.set_fluid_cell(pos, AIR, FLUID_SOURCE) {
                        queue.extend(FLUID_NEIGHBORS.iter().map(|&d| pos + d));
                    }
                    return;
                }
            }
            let can_host = id == AIR || tables.replaceable[id as usize] || id == cid;
            if !can_host {
                return; // obstructed
            }
            if map.set_fluid_cell(pos, cid, clevel) {
                queue.extend(FLUID_NEIGHBORS.iter().map(|&d| pos + d));
            }
        }
        None => {
            if is_fluid_here && map.set_fluid_cell(pos, AIR, FLUID_SOURCE) {
                queue.extend(FLUID_NEIGHBORS.iter().map(|&d| pos + d));
            }
        }
    }
}

/// Startup: build the atlas, bake isometric icons for it (`icons.rs`), and
/// compile the registry's flat lookup tables. Runs before any render/UI
/// setup that needs atlas indices. Does not touch `WorldGen` — that's
/// constructed per-world by `enter_world`, since each world has its own seed.
pub fn compile_content(
    mut commands: Commands,
    mut registry: ResMut<BlockRegistry>,
    painters: Res<Painters>,
) {
    let atlas = build_atlas(&painters);
    let icon_atlas = build_icon_atlas(&registry, &atlas);
    let tables = registry.compile(&atlas.indices);
    commands.insert_resource(Atlas(atlas));
    commands.insert_resource(IconAtlas(icon_atlas));
    commands.insert_resource(BlockTables(tables));
}

/// `OnEnter(AppState::InGame)`: builds the terrain generator for the active
/// world's seed, resets chunk/task state left over from any previous world,
/// loads that world's saved edits (grouped by chunk for cheap application),
/// restores the saved player position if this isn't a brand new world, and
/// makes the world's game mode available as a resource (read by
/// `player::player_update` to gate flying).
fn enter_world(
    mut commands: Commands,
    active: Res<ActiveWorld>,
    registry: Res<BlockRegistry>,
    store: Res<SaveStore>,
    mut map: ResMut<ChunkMap>,
    tasks: Query<Entity, Or<(With<GenTask>, With<MeshTask>)>>,
    mut players: Query<&mut Player>,
) {
    let generator = TerrainGenerator::new(active.meta.seed, &registry);
    commands.insert_resource(WorldGen(Arc::new(generator)));
    commands.insert_resource(active.meta.mode);

    for e in &tasks {
        commands.entity(e).despawn();
    }
    for chunk in map.chunks.values_mut() {
        for e in [chunk.solid_entity.take(), chunk.water_entity.take()].into_iter().flatten() {
            commands.entity(e).despawn();
        }
    }
    *map = ChunkMap { needs_scan: true, ..ChunkMap::default() };

    let data = store.load_data(&active.slug);
    let mut grouped: HashMap<IVec2, Vec<(IVec3, BlockId, u8)>> = HashMap::new();
    for edit in data.edits {
        // Unknown block names (e.g. from a mod no longer installed) are
        // skipped rather than failing the whole load.
        let Ok(id) = registry.by_name(&edit.block) else { continue };
        let pos = IVec3::new(edit.x, edit.y, edit.z);
        let coord = IVec2::new(edit.x.div_euclid(CHUNK_SIZE), edit.z.div_euclid(CHUNK_SIZE));
        grouped.entry(coord).or_default().push((pos, id, edit.axis));
    }
    commands.insert_resource(PendingEdits(grouped));
    commands.insert_resource(EditLog::default());
    commands.insert_resource(AutosaveTimer::default());
    commands.insert_resource(FluidQueue::default());

    if let Ok(mut player) = players.single_mut() {
        *player = Player::default();
        if let Some(saved) = data.player {
            player.pos = Vec3::new(saved.x, saved.y, saved.z);
            player.yaw = saved.yaw;
            player.pitch = saved.pitch;
            player.fly = saved.fly && active.meta.mode == GameMode::Creative;
            player.spawned = true;
        }
    }
}

fn record_edits(mut events: EventReader<BlockSetEvent>, mut log: ResMut<EditLog>) {
    for e in events.read() {
        log.0.insert(e.pos, (e.id, e.axis));
    }
}

/// Serializes the current `EditLog` + player pose and writes it to disk.
/// Shared by `exit_world` and the periodic autosave.
fn write_save(
    store: &SaveStore,
    active: &ActiveWorld,
    log: &EditLog,
    registry: &BlockRegistry,
    player: Option<&Player>,
) {
    let edits = log
        .0
        .iter()
        .map(|(pos, &(id, axis))| BlockEdit {
            x: pos.x,
            y: pos.y,
            z: pos.z,
            block: registry.def(id).id.clone(),
            axis,
        })
        .collect();
    let player = player.map(|p| PlayerSave {
        x: p.pos.x,
        y: p.pos.y,
        z: p.pos.z,
        yaw: p.yaw,
        pitch: p.pitch,
        fly: p.fly,
    });
    let _ = store.save_data(&active.slug, &WorldData { player, edits });
}

fn autosave(
    time: Res<Time>,
    mut timer: ResMut<AutosaveTimer>,
    store: Res<SaveStore>,
    active: Res<ActiveWorld>,
    log: Res<EditLog>,
    registry: Res<BlockRegistry>,
    players: Query<&Player>,
) {
    timer.0 += time.delta_secs();
    if timer.0 < AUTOSAVE_INTERVAL {
        return;
    }
    timer.0 = 0.0;
    write_save(&store, &active, &log, &registry, players.single().ok());
}

/// `OnExit(AppState::InGame)`: persist this session's edits and player pose,
/// then despawn the chunk world's render entities. Without this, whatever
/// shows after leaving (the main menu, most commonly, since its own UI is
/// intentionally semi-transparent/not full-bleed) kept rendering a frozen
/// snapshot of the world just left instead of the plain sky-colour backdrop
/// menus are supposed to have — `enter_world` only ever cleaned this up on
/// the *next* world load, leaving a gap for however long the player sat at
/// the menu in between.
fn exit_world(
    mut commands: Commands,
    store: Res<SaveStore>,
    active: Res<ActiveWorld>,
    log: Res<EditLog>,
    registry: Res<BlockRegistry>,
    players: Query<&Player>,
    mut map: ResMut<ChunkMap>,
    tasks: Query<Entity, Or<(With<GenTask>, With<MeshTask>)>>,
) {
    write_save(&store, &active, &log, &registry, players.single().ok());

    for e in &tasks {
        commands.entity(e).despawn();
    }
    for chunk in map.chunks.values_mut() {
        for e in [chunk.solid_entity.take(), chunk.water_entity.take()].into_iter().flatten() {
            commands.entity(e).despawn();
        }
    }
    *map = ChunkMap::default();
}

/// Figures out what to generate/mesh/unload. Cheap (a few hundred map hits)
/// and only runs when something changed.
#[allow(clippy::too_many_arguments)]
fn stream_chunks(
    mut commands: Commands,
    mut map: ResMut<ChunkMap>,
    settings: Res<WorldSettings>,
    tables: Res<BlockTables>,
    gen: Res<WorldGen>,
    players: Query<&Player>,
) {
    let Ok(player) = players.single() else { return };
    let pc = ChunkMap::chunk_coord(player.pos.x.floor() as i32, player.pos.z.floor() as i32);
    if map.last_player_chunk != Some(pc) {
        map.last_player_chunk = Some(pc);
        map.needs_scan = true;
    }
    if !map.needs_scan {
        return;
    }
    map.needs_scan = false;

    let r = settings.render_distance;
    let rd = r + 1; // data radius: one ring beyond meshes for padded shells
    let pool = AsyncComputeTaskPool::get();

    let mut gen_candidates: Vec<(i32, IVec2)> = Vec::new();
    let mut mesh_candidates: Vec<(i32, IVec2)> = Vec::new();

    for dz in -rd..=rd {
        for dx in -rd..=rd {
            let d2 = dx * dx + dz * dz;
            if d2 > rd * rd + 1 {
                continue;
            }
            let coord = pc + IVec2::new(dx, dz);
            match map.chunks.get(&coord) {
                None => gen_candidates.push((d2, coord)),
                Some(c) if c.blocks.is_some() && !c.meshing => {
                    let want = (!c.meshed && d2 <= r * r + 1) || (c.meshed && c.dirty);
                    if want && map.neighbors_ready(coord) {
                        mesh_candidates.push((d2, coord));
                    }
                }
                _ => {}
            }
        }
    }

    gen_candidates.sort_by_key(|(d2, _)| *d2);
    for (_, coord) in gen_candidates {
        if map.gen_in_flight >= MAX_GEN_TASKS {
            break;
        }
        map.chunks.insert(coord, Chunk::default());
        map.gen_in_flight += 1;
        let gen = gen.0.clone();
        let task = pool.spawn(async move { gen.generate(coord.x, coord.y) });
        commands.spawn(GenTask { coord, task });
    }

    mesh_candidates.sort_by_key(|(d2, _)| *d2);
    for (_, coord) in mesh_candidates {
        if map.mesh_in_flight >= MAX_MESH_TASKS {
            break;
        }
        let (padded, padded_fluid, padded_axis) = map.build_padded(coord);
        let chunk = map.chunks.get_mut(&coord).unwrap();
        chunk.meshing = true;
        chunk.dirty = false;
        let version = chunk.version;
        map.mesh_in_flight += 1;
        let tables = tables.0.clone();
        let task =
            pool.spawn(async move { mesh_chunk(&padded, &padded_fluid, &padded_axis, &tables) });
        commands.spawn(MeshTask { coord, version, task });
    }

    // Drop far meshes (block data is kept, so edits survive roaming).
    let unload_r2 = (r + 2) * (r + 2);
    let mut to_despawn: Vec<Entity> = Vec::new();
    for (coord, chunk) in map.chunks.iter_mut() {
        if !chunk.meshed {
            continue;
        }
        let d = *coord - pc;
        if d.x * d.x + d.y * d.y > unload_r2 {
            to_despawn.extend(chunk.solid_entity.take());
            to_despawn.extend(chunk.water_entity.take());
            chunk.meshed = false;
        }
    }
    for e in to_despawn {
        commands.entity(e).despawn();
    }
}

fn collect_gen_tasks(
    mut commands: Commands,
    mut map: ResMut<ChunkMap>,
    mut pending: ResMut<PendingEdits>,
    mut tasks: Query<(Entity, &mut GenTask)>,
) {
    for (entity, mut gen_task) in &mut tasks {
        let Some(generated) = block_on(future::poll_once(&mut gen_task.task)) else {
            continue;
        };
        commands.entity(entity).despawn();
        map.gen_in_flight -= 1;
        map.needs_scan = true;
        if map.chunks.get_mut(&gen_task.coord).is_none() {
            continue; // world was exited/switched while this chunk was generating
        }
        let chunk = map.chunks.get_mut(&gen_task.coord).unwrap();
        chunk.blocks = Some(generated.blocks);
        chunk.fluid_level = Some(generated.fluid);
        chunk.axis = Some(generated.axis);
        // Re-apply any saved edits for this chunk on top of the fresh terrain.
        if let Some(edits) = pending.0.remove(&gen_task.coord) {
            for (pos, id, axis) in edits {
                map.set_block(pos, id);
                map.set_axis_raw(pos, axis);
            }
        }
    }
}

fn collect_mesh_tasks(
    mut commands: Commands,
    mut map: ResMut<ChunkMap>,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<ChunkMaterials>,
    mut tasks: Query<(Entity, &mut MeshTask)>,
    mut meshed_events: EventWriter<ChunkMeshedEvent>,
) {
    for (entity, mut mesh_task) in &mut tasks {
        let Some(data) = block_on(future::poll_once(&mut mesh_task.task)) else {
            continue;
        };
        commands.entity(entity).despawn();
        map.mesh_in_flight -= 1;
        map.needs_scan = true;
        let coord = mesh_task.coord;
        let Some(chunk) = map.chunks.get_mut(&coord) else { continue };
        chunk.meshing = false;
        chunk.meshed = true;
        if chunk.version != mesh_task.version {
            chunk.dirty = true; // edited while meshing; apply now, remesh after
        }

        // Replace existing chunk entities with the fresh geometry.
        for old in [chunk.solid_entity.take(), chunk.water_entity.take()].into_iter().flatten() {
            commands.entity(old).despawn();
        }
        let origin = Transform::from_xyz(
            (coord.x * CHUNK_SIZE) as f32,
            0.0,
            (coord.y * CHUNK_SIZE) as f32,
        );
        for (bucket, material, slot) in [
            (data.solid, &materials.solid, &mut chunk.solid_entity),
            (data.water, &materials.water, &mut chunk.water_entity),
        ] {
            if bucket.is_empty() {
                continue;
            }
            let mesh = crate::render::bucket_to_mesh(bucket);
            *slot = Some(
                commands
                    .spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(material.clone()), origin))
                    .id(),
            );
        }
        meshed_events.write(ChunkMeshedEvent(coord));
    }
}

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<WorldSettings>() {
            app.insert_resource(WorldSettings::default());
        }
        if !app.world().contains_resource::<SaveStore>() {
            app.insert_resource(SaveStore::default());
        }
        if !app.world().contains_resource::<GameMode>() {
            app.insert_resource(GameMode::default());
        }
        app.insert_resource(BlockRegistry::with_defaults())
            .insert_resource(default_painters())
            .insert_resource(ChunkMap { needs_scan: true, ..ChunkMap::default() })
            .init_resource::<EditLog>()
            .init_resource::<PendingEdits>()
            .init_resource::<AutosaveTimer>()
            .init_resource::<FluidQueue>()
            .add_event::<BlockSetEvent>()
            .add_event::<ChunkMeshedEvent>()
            .add_systems(Startup, compile_content)
            .add_systems(OnEnter(AppState::InGame), enter_world)
            .add_systems(OnExit(AppState::InGame), exit_world)
            .add_systems(
                Update,
                (collect_gen_tasks, collect_mesh_tasks)
                    .chain()
                    .run_if(resource_exists::<ChunkMaterials>),
            )
            .add_systems(
                Update,
                stream_chunks
                    .after(collect_mesh_tasks)
                    .run_if(resource_exists::<ChunkMaterials>.and(in_state(AppState::InGame))),
            )
            .add_systems(
                Update,
                (record_edits, autosave)
                    .run_if(in_state(AppState::InGame)),
            )
            .add_systems(
                Update,
                (enqueue_fluid_updates, process_fluid_updates)
                    .chain()
                    .after(collect_mesh_tasks)
                    .run_if(resource_exists::<ChunkMaterials>.and(in_state(AppState::InGame))),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::BlockRegistry;
    use crate::config::CS;

    fn empty_chunk() -> Chunk {
        Chunk {
            blocks: Some(vec![AIR; CS * CS * H]),
            fluid_level: Some(vec![FLUID_SOURCE; CS * CS * H]),
            ..Chunk::default()
        }
    }

    fn setup() -> (BlockId, BlockId, Arc<Tables>, ChunkMap) {
        let mut reg = BlockRegistry::with_defaults();
        let atlas = crate::atlas::build_atlas(&crate::atlas::default_painters());
        let water = reg.id("water");
        let stone = reg.id("stone");
        let tables = reg.compile(&atlas.indices);
        let map = ChunkMap { chunks: HashMap::from([(IVec2::ZERO, empty_chunk())]), ..ChunkMap::default() };
        (water, stone, tables, map)
    }

    /// Fills an entire horizontal layer solid, so lateral-spread tests
    /// aren't accidentally also exercising the (separately tested) falling
    /// behaviour just because the synthetic chunk has no ground.
    fn fill_floor(map: &mut ChunkMap, y: i32, id: BlockId) {
        let blocks = map.chunks.get_mut(&IVec2::ZERO).unwrap().blocks.as_mut().unwrap();
        for z in 0..CS {
            for x in 0..CS {
                blocks[block_index(x, y as usize, z)] = id;
            }
        }
    }

    /// Drives the same relaxation `process_fluid_updates` runs, until the
    /// queue empties, bailing out instead of hanging if something regresses
    /// into an infinite oscillation.
    fn drain(map: &mut ChunkMap, tables: &Tables, seed: IVec3) {
        let mut queue = VecDeque::from([seed]);
        let mut guard = 0;
        while let Some(pos) = queue.pop_front() {
            recompute_cell(map, tables, pos, &mut queue);
            guard += 1;
            assert!(guard < 50_000, "fluid recompute did not converge");
        }
    }

    #[test]
    fn two_sources_either_side_of_a_gap_makes_the_gap_a_source() {
        let (water, stone, tables, mut map) = setup();
        fill_floor(&mut map, 9, stone);
        let a = IVec3::new(4, 10, 4);
        let gap = IVec3::new(5, 10, 4);
        let b = IVec3::new(6, 10, 4);
        map.set_fluid_cell(a, water, FLUID_SOURCE);
        map.set_fluid_cell(b, water, FLUID_SOURCE);

        drain(&mut map, &tables, gap);

        assert_eq!(map.get_block(gap), water);
        assert_eq!(map.get_fluid_level(gap), FLUID_SOURCE);
    }

    #[test]
    fn a_single_source_neighbour_stays_flowing_not_a_source() {
        let (water, stone, tables, mut map) = setup();
        fill_floor(&mut map, 9, stone);
        let a = IVec3::new(4, 10, 4);
        let next = IVec3::new(5, 10, 4);
        map.set_fluid_cell(a, water, FLUID_SOURCE);

        drain(&mut map, &tables, next);

        assert_eq!(map.get_block(next), water);
        assert_eq!(map.get_fluid_level(next), 1);
    }

    #[test]
    fn waterfall_over_open_space_lands_before_spreading_sideways() {
        let (water, _stone, tables, mut map) = setup();
        let source = IVec3::new(5, 10, 4);
        map.set_fluid_cell(source, water, FLUID_SOURCE);

        drain(&mut map, &tables, source + IVec3::NEG_Y);

        // Straight down to the floor: fully fluid.
        for y in 0..10 {
            assert_eq!(map.get_block(IVec3::new(5, y, 4)), water, "column not fluid at y={y}");
        }
        // Mid-fall, one block sideways: must stay dry — a falling segment
        // over open space is not a lateral supply until it lands, or this
        // would flood a whole sheet instead of a narrow waterfall.
        for y in 1..9 {
            assert_eq!(map.get_block(IVec3::new(6, y, 4)), AIR, "unexpected sideways leak at y={y}");
        }
        // Only at the floor does it spread outward.
        assert_eq!(map.get_block(IVec3::new(6, 0, 4)), water);
    }

    #[test]
    fn flowing_water_dries_up_once_its_source_is_removed() {
        let (water, stone, tables, mut map) = setup();
        fill_floor(&mut map, 9, stone);
        let source = IVec3::new(4, 10, 4);
        let next = IVec3::new(5, 10, 4);
        map.set_fluid_cell(source, water, FLUID_SOURCE);
        drain(&mut map, &tables, next);
        assert_eq!(map.get_block(next), water);

        // Remove the source directly (bypassing set_block/BlockSetEvent, the
        // same way the simulation itself writes) and re-run the relaxation
        // the way `enqueue_fluid_updates` would after a real break event.
        map.set_fluid_cell(source, AIR, FLUID_SOURCE);
        drain(&mut map, &tables, next);

        assert_eq!(map.get_block(next), AIR);
    }
}
