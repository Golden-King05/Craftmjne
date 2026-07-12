//! Chunk mesher — turns raw block data into render-ready vertex buffers.
//! Runs on the async compute task pool (pure data in, pure data out).
//!
//! Optimizations:
//!  - Operates on a "padded" copy of the chunk (+1 block shell from the eight
//!    neighbouring chunks) so face culling and AO never need chunk lookups.
//!  - Hidden-face culling: only faces exposed to air / transparent blocks emit.
//!  - Per-vertex ambient occlusion and directional sky shading are baked into
//!    vertex colors; the chunk shader is fully unlit.
//!  - Quads flip along the brighter AO diagonal to avoid interpolation
//!    artifacts.
//!  - Two buckets per chunk: `solid` (opaque + alpha-cutout like leaves/glass)
//!    and `water` (alpha-blended translucent pass).

use std::sync::LazyLock;

use crate::blocks::{Tables, AXIS_X, AXIS_Y, AXIS_Z, FLUID_FALLING, FLUID_SOURCE};
use crate::config::{ATLAS_TILES, CHUNK_SIZE, CS, H, TILE_SIZE, WORLD_HEIGHT};

// Padded array layout: x,z in [-1, CHUNK_SIZE], y in [-1, WORLD_HEIGHT].
pub const PAD_XZ: usize = CS + 2;
pub const PAD_Y: usize = H + 2;

const SY: i32 = 1;
const SX: i32 = PAD_Y as i32;
const SZ: i32 = (PAD_Y * PAD_XZ) as i32;
const STRIDES: [i32; 3] = [SX, SY, SZ];

#[inline]
pub fn padded_index(x: i32, y: i32, z: i32) -> usize {
    ((y + 1) * SY + (x + 1) * SX + (z + 1) * SZ) as usize
}

/// AO brightness for 0..3 occluders touching a vertex.
const AO_BRIGHT: [f32; 4] = [1.0, 0.82, 0.64, 0.46];

/// Fluid tops sit one pixel below the block top (16x16 textures -> 1/16).
const FLUID_SURFACE: f32 = 1.0 - 1.0 / TILE_SIZE as f32;

/// How falling (waterfall) segments render. `Sloped` (the default) tapers
/// each segment's exposed side walls from full height down to a one-pixel
/// sliver, so a multi-block drop reads as a cascading wedge. `Blocky` is the
/// original flat-walled look (a falling segment renders as a plain solid
/// cube) — kept as a real, working alternative rather than deleted, so a
/// future per-fluid setting or graphics option can offer either look
/// without re-deriving this. Flip this constant to switch globally for now.
#[derive(PartialEq, Eq)]
enum FallingWaterStyle {
    // Only reachable by editing `FALLING_WATER_STYLE` below - that's the
    // point (see the doc comment above), not a mistake.
    #[allow(dead_code)]
    Blocky,
    Sloped,
}

const FALLING_WATER_STYLE: FallingWaterStyle = FallingWaterStyle::Sloped;

/// Surface height for a fluid cell at `level` blocks from its source, given
/// that fluid's configured `flow_distance`. Level 0 (a permanent source) and
/// `FLUID_FALLING` (a waterfall column) both render full-height; everything
/// else steps down linearly from `FLUID_SURFACE` at level 1 to a thin film at
/// `level == flow_distance` — so a long `flow_distance` slopes gently and a
/// short one drops off steeply, with no per-fluid special-casing needed.
fn fluid_height(level: u8, flow_distance: u8) -> f32 {
    if level == FLUID_SOURCE || level == FLUID_FALLING {
        return FLUID_SURFACE;
    }
    let fd = flow_distance.max(1) as f32;
    let l = (level as f32).min(fd);
    FLUID_SURFACE * (fd - l + 1.0) / (fd + 1.0)
}

/// Which atlas tile a rotated block's face `f` (0:+x,1:-x,2:+y,3:-y,4:+z,
/// 5:-z, matching `Tables::tiles`) should show, given its stored orientation
/// `axis` (`blocks::AXIS_X/Y/Z`). The two faces whose normal lies along
/// `axis` show the block's "cap" texture (the `top`/`bottom` tile slots,
/// preserving which end is which); every other face shows its `side`
/// texture. For `axis == AXIS_Y` this reduces to exactly the plain `tiles
/// [id*6+f]` lookup, so it's cheap and correct to call unconditionally once
/// `Tables::rotates` says a block id cares about rotation at all - no
/// separate "unrotated" code path needed.
fn rotated_tile(tables: &Tables, id: u16, axis: u8, f: usize) -> u16 {
    let face_axis = match f {
        0 | 1 => AXIS_X,
        2 | 3 => AXIS_Y,
        _ => AXIS_Z,
    };
    let base = id as usize * 6;
    if face_axis == axis {
        tables.tiles[base + if f % 2 == 0 { 2 } else { 3 }]
    } else {
        tables.tiles[base] // any side slot - all four are identical by construction
    }
}

struct FaceCorner {
    pos: [f32; 3],
    uv: [f32; 2],
    /// Padded-index offsets of the (side1, side2, corner) AO neighbours.
    ao: [i32; 3],
}

struct Face {
    neighbor_ofs: i32,
    shade: f32,
    corners: [FaceCorner; 4],
}

/// Face order: 0:+x 1:-x 2:+y 3:-y 4:+z 5:-z (matches `Tables::tiles`).
/// Corner positions / uvs use a well-tested layout; triangles are
/// (0,1,2)(2,1,3), or (0,1,3)(0,3,2) when AO-flipped.
static FACES: LazyLock<[Face; 6]> = LazyLock::new(|| {
    const DEFS: [([i32; 3], f32, [([i32; 3], [f32; 2]); 4]); 6] = [
        ([1, 0, 0], 0.6, [
            ([1, 1, 1], [0.0, 1.0]), ([1, 0, 1], [0.0, 0.0]),
            ([1, 1, 0], [1.0, 1.0]), ([1, 0, 0], [1.0, 0.0]),
        ]),
        ([-1, 0, 0], 0.6, [
            ([0, 1, 0], [0.0, 1.0]), ([0, 0, 0], [0.0, 0.0]),
            ([0, 1, 1], [1.0, 1.0]), ([0, 0, 1], [1.0, 0.0]),
        ]),
        ([0, 1, 0], 1.0, [
            ([0, 1, 1], [1.0, 1.0]), ([1, 1, 1], [0.0, 1.0]),
            ([0, 1, 0], [1.0, 0.0]), ([1, 1, 0], [0.0, 0.0]),
        ]),
        ([0, -1, 0], 0.5, [
            ([1, 0, 1], [1.0, 0.0]), ([0, 0, 1], [0.0, 0.0]),
            ([1, 0, 0], [1.0, 1.0]), ([0, 0, 0], [0.0, 1.0]),
        ]),
        ([0, 0, 1], 0.8, [
            ([0, 0, 1], [0.0, 0.0]), ([1, 0, 1], [1.0, 0.0]),
            ([0, 1, 1], [0.0, 1.0]), ([1, 1, 1], [1.0, 1.0]),
        ]),
        ([0, 0, -1], 0.8, [
            ([1, 0, 0], [0.0, 0.0]), ([0, 0, 0], [1.0, 0.0]),
            ([1, 1, 0], [0.0, 1.0]), ([0, 1, 0], [1.0, 1.0]),
        ]),
    ];

    DEFS.map(|(dir, shade, corners)| {
        let normal_axis = if dir[0] != 0 { 0 } else if dir[1] != 0 { 1 } else { 2 };
        let neighbor_ofs = dir[0] * SX + dir[1] * SY + dir[2] * SZ;
        let tangents: Vec<usize> = (0..3).filter(|&a| a != normal_axis).collect();
        let (u, v) = (tangents[0], tangents[1]);
        let corners = corners.map(|(pos, uv)| {
            let du = if pos[u] == 1 { 1 } else { -1 };
            let dv = if pos[v] == 1 { 1 } else { -1 };
            let side1 = neighbor_ofs + du * STRIDES[u];
            let side2 = neighbor_ofs + dv * STRIDES[v];
            FaceCorner {
                pos: pos.map(|c| c as f32),
                uv,
                ao: [side1, side2, side1 + dv * STRIDES[v]],
            }
        });
        Face { neighbor_ofs, shade, corners }
    })
});

const UV_TILE: f32 = 1.0 / ATLAS_TILES as f32;
/// Half-texel inset prevents atlas bleeding at tile borders.
const UV_PAD: f32 = 0.5 / (ATLAS_TILES * TILE_SIZE) as f32;
const UV_SPAN: f32 = UV_TILE - 2.0 * UV_PAD;

#[derive(Default)]
pub struct MeshBucket {
    pub positions: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

impl MeshBucket {
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }
}

pub struct ChunkMeshData {
    pub solid: MeshBucket,
    pub water: MeshBucket,
}

pub fn mesh_chunk(
    padded: &[u16],
    padded_fluid: &[u8],
    padded_axis: &[u8],
    tables: &Tables,
) -> ChunkMeshData {
    debug_assert_eq!(padded.len(), PAD_XZ * PAD_XZ * PAD_Y);
    debug_assert_eq!(padded_fluid.len(), padded.len());
    debug_assert_eq!(padded_axis.len(), padded.len());
    let mut solid = MeshBucket::default();
    let mut water = MeshBucket::default();

    for z in 0..CHUNK_SIZE {
        for x in 0..CHUNK_SIZE {
            let mut idx = padded_index(x, 0, z);
            for y in 0..WORLD_HEIGHT {
                let id = padded[idx];
                idx += 1; // SY == 1: next Y
                if id == 0 {
                    continue;
                }
                let cell = idx - 1;

                // Bucket routing follows the rendering mode (`transparency:
                // full` -> alpha-blended); the lowered top surface follows
                // `fluid` instead, so a non-fluid `full`-transparency block
                // (fancy translucent glass, say) doesn't get a fluid top,
                // and a future non-`full` fluid still would.
                let is_translucent = tables.translucent[id as usize];
                let bucket = if is_translucent { &mut water } else { &mut solid };
                let is_fluid = tables.fluid[id as usize];
                let axis = if tables.rotates[id as usize] { padded_axis[cell] } else { AXIS_Y };
                let level = padded_fluid[cell];
                let flow_dist = tables.flow_distance[id as usize];
                // Covered by more of the same fluid above -> render full
                // height (it's not this stack's exposed surface).
                let cap = if is_fluid {
                    if padded[cell + SY as usize] == id { 1.0 } else { fluid_height(level, flow_dist) }
                } else {
                    1.0
                };

                for (f, face) in FACES.iter().enumerate() {
                    let n_cell = (cell as i32 + face.neighbor_ofs) as usize;
                    let nid = padded[n_cell];
                    let is_side = matches!(f, 0 | 1 | 4 | 5);
                    let mut bottom = 0.0f32;
                    let mut stepped = false;
                    if nid != 0 {
                        if tables.opaque[nid as usize] {
                            continue;
                        }
                        if nid == id {
                            if is_fluid && is_side {
                                // Same fluid next door at a different level:
                                // draw a partial "step" wall from its surface
                                // up to ours instead of culling the face
                                // outright (a flat cull would leave a visible
                                // gap between two different-height cells).
                                let n_level = padded_fluid[n_cell];
                                let n_cap = if padded[n_cell + SY as usize] == id {
                                    1.0
                                } else {
                                    fluid_height(n_level, flow_dist)
                                };
                                if n_cap + 1e-4 >= cap {
                                    continue;
                                }
                                bottom = n_cap;
                                stepped = true;
                            } else {
                                continue;
                            }
                        }
                    }
                    // A falling (waterfall) segment's exposed walls taper
                    // from full height at the top - touching whatever feeds
                    // it from directly above - down to a one-pixel sliver at
                    // the bottom, instead of a flat rectangular wall. Chained
                    // down a multi-block drop this reads as one continuous
                    // cascade rather than a stack of solid cubes. Doesn't
                    // apply where the step-wall case above already set a
                    // (different) partial bottom, or under `Blocky` style.
                    if is_fluid
                        && is_side
                        && level == FLUID_FALLING
                        && !stepped
                        && FALLING_WATER_STYLE == FallingWaterStyle::Sloped
                    {
                        bottom = 1.0 / TILE_SIZE as f32;
                    }

                    let tile = rotated_tile(tables, id, axis, f) as usize;
                    let tu = (tile % ATLAS_TILES) as f32 * UV_TILE;
                    let tv = (tile / ATLAS_TILES) as f32 * UV_TILE;
                    let vi = bucket.positions.len() as u32;
                    let mut ao = [1.0f32; 4];

                    for (ci, c) in face.corners.iter().enumerate() {
                        let mut bright = 1.0;
                        if !is_translucent {
                            let occ = |o: i32| {
                                tables.opaque[padded[(cell as i32 + o) as usize] as usize] as u32
                            };
                            let (s1, s2) = (occ(c.ao[0]), occ(c.ao[1]));
                            let level = if s1 == 1 && s2 == 1 { 3 } else { s1 + s2 + occ(c.ao[2]) };
                            bright = AO_BRIGHT[level as usize];
                        }
                        ao[ci] = bright;

                        bucket.positions.push([
                            x as f32 + c.pos[0],
                            y as f32 + if c.pos[1] == 1.0 { cap } else { bottom },
                            z as f32 + c.pos[2],
                        ]);
                        bucket.uvs.push([
                            tu + UV_PAD + c.uv[0] * UV_SPAN,
                            tv + UV_PAD + (1.0 - c.uv[1]) * UV_SPAN,
                        ]);
                        let light = face.shade * bright;
                        bucket.colors.push([light, light, light, 1.0]);
                    }

                    if ao[0] + ao[3] > ao[1] + ao[2] {
                        bucket.indices.extend([vi, vi + 1, vi + 3, vi, vi + 3, vi + 2]);
                    } else {
                        bucket.indices.extend([vi, vi + 1, vi + 2, vi + 2, vi + 1, vi + 3]);
                    }
                }
            }
        }
    }

    ChunkMeshData { solid, water }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::BlockRegistry;
    use crate::config::block_index;

    fn tables() -> (BlockRegistry, std::sync::Arc<Tables>) {
        let mut reg = BlockRegistry::with_defaults();
        let atlas = crate::atlas::build_atlas(&crate::atlas::default_painters());
        let tables = reg.compile(&atlas.indices);
        (reg, tables)
    }

    fn empty_padded() -> Vec<u16> {
        vec![0u16; PAD_XZ * PAD_XZ * PAD_Y]
    }

    fn empty_fluid() -> Vec<u8> {
        vec![0u8; PAD_XZ * PAD_XZ * PAD_Y]
    }

    fn empty_axis() -> Vec<u8> {
        vec![AXIS_Y; PAD_XZ * PAD_XZ * PAD_Y]
    }

    #[test]
    fn lone_block_emits_six_faces() {
        let (reg, tables) = tables();
        let mut padded = empty_padded();
        padded[padded_index(8, 30, 8)] = reg.id("stone");
        let mesh = mesh_chunk(&padded, &empty_fluid(), &empty_axis(), &tables);
        assert_eq!(mesh.solid.positions.len(), 6 * 4);
        assert_eq!(mesh.solid.indices.len(), 6 * 6);
        assert!(mesh.water.is_empty());
    }

    #[test]
    fn buried_block_emits_nothing() {
        let (reg, tables) = tables();
        let stone = reg.id("stone");
        let mut padded = vec![stone; PAD_XZ * PAD_XZ * PAD_Y];
        // one exposed face at the top only for the interior column we check:
        // actually fully solid volume -> zero faces inside; boundary faces
        // depend on the shell, which is also stone here.
        let mesh = mesh_chunk(&padded, &empty_fluid(), &empty_axis(), &tables);
        assert!(mesh.solid.is_empty());
        // poke a hole: the neighbouring block gains exactly one face
        padded[padded_index(8, 30, 8)] = 0;
        let mesh = mesh_chunk(&padded, &empty_fluid(), &empty_axis(), &tables);
        assert_eq!(mesh.solid.positions.len(), 6 * 4); // 6 cavity walls
    }

    #[test]
    fn water_goes_to_translucent_bucket_with_lowered_top() {
        let (reg, tables) = tables();
        let mut padded = empty_padded();
        padded[padded_index(4, 10, 4)] = reg.id("water");
        let mesh = mesh_chunk(&padded, &empty_fluid(), &empty_axis(), &tables);
        assert!(mesh.solid.is_empty());
        assert_eq!(mesh.water.positions.len(), 6 * 4);
        let max_y = mesh.water.positions.iter().map(|p| p[1]).fold(0.0, f32::max);
        assert_eq!(max_y, 10.0 + FLUID_SURFACE);
    }

    #[test]
    fn flowing_water_is_shallower_than_a_source() {
        let (reg, tables) = tables();
        let water = reg.id("water");
        let mut padded = empty_padded();
        let mut fluid = empty_fluid();
        padded[padded_index(4, 10, 4)] = water;
        fluid[padded_index(4, 10, 4)] = 3; // 3 blocks from a source
        let mesh = mesh_chunk(&padded, &fluid, &empty_axis(), &tables);
        let max_y = mesh.water.positions.iter().map(|p| p[1]).fold(0.0, f32::max);
        assert!(max_y < 10.0 + FLUID_SURFACE);
        assert!(max_y > 10.0);
    }

    #[test]
    fn adjacent_water_at_different_levels_gets_a_step_wall() {
        let (reg, tables) = tables();
        let water = reg.id("water");
        let mut padded = empty_padded();
        let mut fluid = empty_fluid();
        padded[padded_index(4, 10, 4)] = water;
        padded[padded_index(5, 10, 4)] = water;
        fluid[padded_index(4, 10, 4)] = 1;
        fluid[padded_index(5, 10, 4)] = 4; // shallower neighbour
        let mesh = mesh_chunk(&padded, &fluid, &empty_axis(), &tables);
        // the boundary between them must render a wall face instead of being
        // fully culled (same-id neighbours at equal height cull completely).
        assert!(!mesh.water.is_empty());

        // sanity: identical levels on both sides fully cull that face.
        fluid[padded_index(5, 10, 4)] = 1;
        let level_mesh = mesh_chunk(&padded, &fluid, &empty_axis(), &tables);
        assert!(level_mesh.water.positions.len() < mesh.water.positions.len());
    }

    #[test]
    fn falling_water_tapers_its_side_walls_to_a_sliver() {
        let (reg, tables) = tables();
        let water = reg.id("water");
        let mut padded = empty_padded();
        let mut fluid = empty_fluid();
        // A falling segment fed from a source directly above it, open air on
        // every side and below - the classic mid-air waterfall shaft.
        padded[padded_index(4, 11, 4)] = water;
        fluid[padded_index(4, 11, 4)] = FLUID_SOURCE;
        padded[padded_index(4, 10, 4)] = water;
        fluid[padded_index(4, 10, 4)] = FLUID_FALLING;
        let mesh = mesh_chunk(&padded, &fluid, &empty_axis(), &tables);

        let ys: Vec<f32> = mesh.water.positions.iter().map(|p| p[1]).collect();
        let sliver = 10.0 + 1.0 / TILE_SIZE as f32;
        // The side walls' tapered bottom edge (the one-pixel sliver)...
        assert!(ys.iter().any(|&y| (y - sliver).abs() < 1e-4), "no tapered sliver in {ys:?}");
        // ...the side walls' top edge, still touching the source above...
        assert!(ys.iter().any(|&y| (y - 11.0).abs() < 1e-4), "no full-height top in {ys:?}");
        // ...and the true floor (the bottom face, unaffected by the taper).
        assert!(ys.iter().any(|&y| (y - 10.0).abs() < 1e-4), "no untouched floor in {ys:?}");
    }

    #[test]
    fn ao_darkens_corner_vertices() {
        let (reg, tables) = tables();
        let stone = reg.id("stone");
        let mut padded = empty_padded();
        padded[padded_index(8, 30, 8)] = stone;
        padded[padded_index(9, 31, 8)] = stone; // occluder above the +x neighbour
        let mesh = mesh_chunk(&padded, &empty_fluid(), &empty_axis(), &tables);
        // some top-face vertex of the base block must now be darker than shade 1.0
        let top_lights: Vec<f32> = mesh
            .solid
            .positions
            .iter()
            .zip(&mesh.solid.colors)
            .filter(|(p, _)| p[1] == 31.0)
            .map(|(_, c)| c[0])
            .collect();
        assert!(!top_lights.is_empty());
        assert!(top_lights.iter().any(|&l| l < 1.0));
        assert!(top_lights.iter().any(|&l| l == 1.0));
    }

    #[test]
    fn rotated_tile_moves_the_cap_texture_to_the_axis_faces() {
        let (reg, tables) = tables();
        let log = reg.id("log");
        let base = log as usize * 6;
        let (top, bottom, side) = (tables.tiles[base + 2], tables.tiles[base + 3], tables.tiles[base]);

        // Unrotated (axis Y, the default): identical to the plain lookup -
        // top/bottom faces show the cap, the four sides show bark.
        for f in 0..6 {
            let expected = if f == 2 { top } else if f == 3 { bottom } else { side };
            assert_eq!(rotated_tile(&tables, log, AXIS_Y, f), expected, "face {f}");
        }

        // Axis X (placed against a side face, lying east-west): the cap
        // moves to the +x/-x faces, and the *original* top/bottom faces
        // (now the long sides of the log) show bark instead.
        assert_eq!(rotated_tile(&tables, log, AXIS_X, 0), top);
        assert_eq!(rotated_tile(&tables, log, AXIS_X, 1), bottom);
        assert_eq!(rotated_tile(&tables, log, AXIS_X, 2), side);
        assert_eq!(rotated_tile(&tables, log, AXIS_X, 3), side);
        assert_eq!(rotated_tile(&tables, log, AXIS_X, 4), side);
        assert_eq!(rotated_tile(&tables, log, AXIS_X, 5), side);

        // Axis Z: same idea, cap on the +z/-z faces instead.
        assert_eq!(rotated_tile(&tables, log, AXIS_Z, 4), top);
        assert_eq!(rotated_tile(&tables, log, AXIS_Z, 5), bottom);
        assert_eq!(rotated_tile(&tables, log, AXIS_Z, 0), side);
    }

    #[test]
    fn meshes_a_real_terrain_chunk() {
        let (reg, tables) = tables();
        let gen = crate::terrain::TerrainGenerator::new(1337, &reg);
        // build padded from 3x3 generated chunks
        let mut padded = empty_padded();
        let mut fluid = empty_fluid();
        for ncz in -1..=1i32 {
            for ncx in -1..=1i32 {
                let chunk = gen.generate(ncx, ncz);
                for lz in 0..CS {
                    for lx in 0..CS {
                        let px = ncx * CHUNK_SIZE + lx as i32;
                        let pz = ncz * CHUNK_SIZE + lz as i32;
                        if !(-1..=CHUNK_SIZE).contains(&px) || !(-1..=CHUNK_SIZE).contains(&pz) {
                            continue;
                        }
                        for y in 0..H {
                            padded[padded_index(px, y as i32, pz)] =
                                chunk.blocks[block_index(lx, y, lz)];
                            fluid[padded_index(px, y as i32, pz)] =
                                chunk.fluid[block_index(lx, y, lz)];
                        }
                    }
                }
            }
        }
        let mesh = mesh_chunk(&padded, &fluid, &empty_axis(), &tables);
        assert!(mesh.solid.positions.len() > 1000);
        assert_eq!(mesh.solid.positions.len() % 4, 0);
        assert_eq!(mesh.solid.indices.len() / 6, mesh.solid.positions.len() / 4);
        assert_eq!(mesh.solid.uvs.len(), mesh.solid.positions.len());
        assert_eq!(mesh.solid.colors.len(), mesh.solid.positions.len());
        // all indices in range
        let n = mesh.solid.positions.len() as u32;
        assert!(mesh.solid.indices.iter().all(|&i| i < n));
    }
}
