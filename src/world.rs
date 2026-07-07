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
use crate::terrain::TerrainGenerator;

const MAX_GEN_TASKS: usize = 12;
const MAX_MESH_TASKS: usize = 8;

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

/// Startup: build the atlas, compile the registry, construct the generator.
/// Runs before any render/UI setup that needs atlas indices.
pub fn compile_content(
    mut commands: Commands,
    mut registry: ResMut<BlockRegistry>,
    painters: Res<Painters>,
    settings: Res<WorldSettings>,
) {
    let atlas = build_atlas(&painters);
    let tables = registry.compile(&atlas.indices);
    let generator = TerrainGenerator::new(settings.seed, &registry);
    commands.insert_resource(Atlas(atlas));
    commands.insert_resource(BlockTables(tables));
    commands.insert_resource(WorldGen(Arc::new(generator)));
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
    mut tasks: Query<(Entity, &mut GenTask)>,
) {
    for (entity, mut gen_task) in &mut tasks {
        let Some(blocks) = block_on(future::poll_once(&mut gen_task.task)) else {
            continue;
        };
        commands.entity(entity).despawn();
        map.gen_in_flight -= 1;
        map.needs_scan = true;
        if let Some(chunk) = map.chunks.get_mut(&gen_task.coord) {
            chunk.blocks = Some(blocks);
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
        app.insert_resource(BlockRegistry::with_defaults())
            .insert_resource(default_painters())
            .insert_resource(ChunkMap { needs_scan: true, ..ChunkMap::default() })
            .add_event::<BlockSetEvent>()
            .add_event::<ChunkMeshedEvent>()
            .add_systems(Startup, compile_content)
            .add_systems(
                Update,
                (collect_gen_tasks, collect_mesh_tasks, stream_chunks)
                    .chain()
                    .run_if(resource_exists::<ChunkMaterials>),
            );
    }
}
