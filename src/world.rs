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
use std::collections::HashMap;
use std::sync::Arc;

use crate::atlas::{build_atlas, default_painters, AtlasData, Painters};
use crate::blocks::{BlockId, BlockRegistry, BlockTables, Tables, AIR};
use crate::config::{block_index, WorldSettings, CHUNK_SIZE, H, WORLD_HEIGHT};
use crate::mesher::{mesh_chunk, padded_index, ChunkMeshData, PAD_XZ, PAD_Y};
use crate::player::Player;
use crate::render::ChunkMaterials;
use crate::save::{BlockEdit, PlayerSave, SaveStore, WorldData};
use crate::state::{ActiveWorld, AppState};
use crate::terrain::TerrainGenerator;

const MAX_GEN_TASKS: usize = 12;
const MAX_MESH_TASKS: usize = 8;
const AUTOSAVE_INTERVAL: f32 = 30.0;

#[derive(Resource, Clone)]
pub struct WorldGen(pub Arc<TerrainGenerator>);

/// Non-render atlas data (pixel buffer + name->tile map), built at startup.
#[derive(Resource)]
pub struct Atlas(pub AtlasData);

#[derive(Event)]
pub struct BlockSetEvent {
    pub pos: IVec3,
    pub id: BlockId,
    pub prev: BlockId,
}

#[derive(Event)]
pub struct ChunkMeshedEvent(pub IVec2);

#[derive(Default)]
pub struct Chunk {
    pub blocks: Option<Vec<BlockId>>,
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

        // Border edits affect neighbouring chunks' culling/AO shells too.
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

        self.needs_scan = true;
        Some(prev)
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

    /// Copies chunk blocks plus a 1-block shell from the 8 neighbours into a
    /// padded array. Y-major layout keeps this a series of column copies.
    fn build_padded(&self, coord: IVec2) -> Vec<BlockId> {
        let mut padded = vec![AIR; PAD_XZ * PAD_XZ * PAD_Y];
        for pz in -1..=CHUNK_SIZE {
            let ncz = coord.y + if pz < 0 { -1 } else if pz >= CHUNK_SIZE { 1 } else { 0 };
            let lz = pz.rem_euclid(CHUNK_SIZE) as usize;
            for px in -1..=CHUNK_SIZE {
                let ncx = coord.x + if px < 0 { -1 } else if px >= CHUNK_SIZE { 1 } else { 0 };
                let lx = px.rem_euclid(CHUNK_SIZE) as usize;
                let src = self.chunks[&IVec2::new(ncx, ncz)].blocks.as_ref().unwrap();
                let src_base = block_index(lx, 0, lz);
                let dst_base = padded_index(px, 0, pz);
                padded[dst_base..dst_base + H].copy_from_slice(&src[src_base..src_base + H]);
                padded[dst_base - 1] = 1; // below the world: solid, culls bottom faces
                // above the world stays 0 (air)
            }
        }
        padded
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
    task: Task<Vec<BlockId>>,
}

#[derive(Component)]
struct MeshTask {
    coord: IVec2,
    version: u32,
    task: Task<ChunkMeshData>,
}

/// Edits made this session, keyed by world position (last write wins). Reset
/// fresh on every `enter_world`; flushed to disk by `exit_world` and the
/// periodic autosave.
#[derive(Resource, Default)]
pub struct EditLog(pub HashMap<IVec3, BlockId>);

/// Edits loaded from a save, grouped by chunk so `collect_gen_tasks` can
/// apply them in O(1) right after a chunk finishes procedurally generating.
#[derive(Resource, Default)]
struct PendingEdits(HashMap<IVec2, Vec<(IVec3, BlockId)>>);

#[derive(Resource, Default)]
struct AutosaveTimer(f32);

/// Startup: build the atlas and compile the registry's flat lookup tables.
/// Runs before any render/UI setup that needs atlas indices. Does not touch
/// `WorldGen` — that's constructed per-world by `enter_world`, since each
/// world has its own seed.
pub fn compile_content(
    mut commands: Commands,
    mut registry: ResMut<BlockRegistry>,
    painters: Res<Painters>,
) {
    let atlas = build_atlas(&painters);
    let tables = registry.compile(&atlas.indices);
    commands.insert_resource(Atlas(atlas));
    commands.insert_resource(BlockTables(tables));
}

/// `OnEnter(AppState::InGame)`: builds the terrain generator for the active
/// world's seed, resets chunk/task state left over from any previous world,
/// loads that world's saved edits (grouped by chunk for cheap application),
/// and restores the saved player position if this isn't a brand new world.
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
    let mut grouped: HashMap<IVec2, Vec<(IVec3, BlockId)>> = HashMap::new();
    for edit in data.edits {
        // Unknown block names (e.g. from a mod no longer installed) are
        // skipped rather than failing the whole load.
        let Ok(id) = registry.by_name(&edit.block) else { continue };
        let pos = IVec3::new(edit.x, edit.y, edit.z);
        let coord = IVec2::new(edit.x.div_euclid(CHUNK_SIZE), edit.z.div_euclid(CHUNK_SIZE));
        grouped.entry(coord).or_default().push((pos, id));
    }
    commands.insert_resource(PendingEdits(grouped));
    commands.insert_resource(EditLog::default());
    commands.insert_resource(AutosaveTimer::default());

    if let Ok(mut player) = players.single_mut() {
        *player = Player::default();
        if let Some(saved) = data.player {
            player.pos = Vec3::new(saved.x, saved.y, saved.z);
            player.yaw = saved.yaw;
            player.pitch = saved.pitch;
            player.fly = saved.fly;
            player.spawned = true;
        }
    }
}

fn record_edits(mut events: EventReader<BlockSetEvent>, mut log: ResMut<EditLog>) {
    for e in events.read() {
        log.0.insert(e.pos, e.id);
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
        .map(|(pos, &id)| BlockEdit { x: pos.x, y: pos.y, z: pos.z, block: registry.def(id).name.clone() })
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

/// `OnExit(AppState::InGame)`: persist this session's edits and player pose.
fn exit_world(
    store: Res<SaveStore>,
    active: Res<ActiveWorld>,
    log: Res<EditLog>,
    registry: Res<BlockRegistry>,
    players: Query<&Player>,
) {
    write_save(&store, &active, &log, &registry, players.single().ok());
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
        let padded = map.build_padded(coord);
        let chunk = map.chunks.get_mut(&coord).unwrap();
        chunk.meshing = true;
        chunk.dirty = false;
        let version = chunk.version;
        map.mesh_in_flight += 1;
        let tables = tables.0.clone();
        let task = pool.spawn(async move { mesh_chunk(&padded, &tables) });
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
        let Some(blocks) = block_on(future::poll_once(&mut gen_task.task)) else {
            continue;
        };
        commands.entity(entity).despawn();
        map.gen_in_flight -= 1;
        map.needs_scan = true;
        if map.chunks.get_mut(&gen_task.coord).is_none() {
            continue; // world was exited/switched while this chunk was generating
        }
        map.chunks.get_mut(&gen_task.coord).unwrap().blocks = Some(blocks);
        // Re-apply any saved edits for this chunk on top of the fresh terrain.
        if let Some(edits) = pending.0.remove(&gen_task.coord) {
            for (pos, id) in edits {
                map.set_block(pos, id);
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
        app.insert_resource(BlockRegistry::with_defaults())
            .insert_resource(default_painters())
            .insert_resource(ChunkMap { needs_scan: true, ..ChunkMap::default() })
            .init_resource::<EditLog>()
            .init_resource::<PendingEdits>()
            .init_resource::<AutosaveTimer>()
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
            );
    }
}
