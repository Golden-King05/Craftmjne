//! Chunk mesher — turns raw block data into render-ready vertex buffers.
//! Runs on the async compute task pool (pure data in, pure data out).
//!
//! Optimizations:
//!  - Operates on a "padded" copy of the chunk (+1 block shell from the eight
//!    neighbouring chunks) so face culling and AO never need chunk lookups.
//!  - Hidden-face culling: only faces exposed to air / transparent blocks emit.
//!  - Per-vertex ambient occlusion, directional face shading and the propagated
//!    voxel light (`light.rs`) are all baked into vertex colors; the chunk
//!    shader is fully unlit.
//!
//!  - Quads flip along the brighter AO diagonal to avoid interpolation
//!    artifacts.
//!  - Two buckets per chunk: `solid` (opaque + alpha-cutout like leaves/glass)
//!    and `water` (alpha-blended translucent pass).
//!
//! ## What the vertex color channels mean
//!
//! `RGB` is the colored **block** light for that vertex (torches and the
//! like), already multiplied by face shading and ambient occlusion, and
//! already floored at [`AMBIENT_LIGHT`].
//!
//! **Sky** light for the same vertex needs three channels of its own, since
//! media tint what passes through them (`light.rs`), and it is spread across
//! two attributes: red in the vertex color's `A` - which is *not* a
//! transparency, the fragment's alpha comes from the texture and the
//! material's `base_alpha`, so that channel was sitting unused - and green
//! and blue in `Mesh::ATTRIBUTE_UV_1` (see [`MeshBucket::sky_gb`]).
//!
//! Keeping sky light separate from block light is the whole point:
//! `chunk.wgsl` combines them as `max(block_rgb, sky_rgb * sky_light)` every
//! frame, which is what lets the day/night cycle brighten, dim and tint the
//! world's sky lighting without touching a single mesh.

use std::sync::LazyLock;

use crate::atlas::BASE_TILE_SIZE;
use crate::blocks::{BlockId, Tables, AIR, AXIS_X, AXIS_Y, AXIS_Z, FLUID_FALLING, FLUID_SOURCE};
use crate::config::{ATLAS_TILES, CHUNK_SIZE, CS, H, WORLD_HEIGHT};
use crate::light::{LightCell, AMBIENT_LIGHT, MAX_LIGHT};

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

/// How far to nudge a face inward, along its own outward normal, when its
/// neighbour is a *different*, non-opaque block (glass next to water, say).
/// Neither block's face gets culled there - unlike an opaque neighbour, or
/// the same fluid id, there's a real reason to see both (glass's actual
/// cutout holes, the water surface beyond them) - so both faces render at
/// what would otherwise be the exact same plane, and alpha-blended/cutout
/// geometry sitting exactly coincident like that z-fights (flickers,
/// visibly tears, as the depth test can't consistently pick a winner).
/// Pulling each side in by a hair fixes it with no visible seam.
const COINCIDENT_FACE_BIAS: f32 = 1.0 / 512.0;

/// Fluid tops sit one sixteenth of a block below the true top - a gameplay-
/// geometry constant tied to the base 16x16 grid, deliberately independent
/// of the atlas's actual resolution (`Tables::tile_size`): a hand-supplied
/// 64x64 water texture doesn't need a proportionally smaller dip, it just
/// needs the same "one sixteenth" visual notch a 16x16 one gets.
const FLUID_SURFACE: f32 = 1.0 - 1.0 / BASE_TILE_SIZE as f32;

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
    /// Outward unit normal - used to nudge a face's plane slightly inward
    /// when it would otherwise sit exactly coincident with a different
    /// non-opaque neighbour's own face (see the z-fighting note where this
    /// is used, in the main mesh loop).
    dir: [f32; 3],
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
        Face { neighbor_ofs, dir: [dir[0] as f32, dir[1] as f32, dir[2] as f32], shade, corners }
    })
});

const UV_TILE: f32 = 1.0 / ATLAS_TILES as f32;
// The half-texel inset that prevents atlas bleeding at tile borders (and
// the resulting sampled span) depends on the atlas's actual per-tile pixel
// resolution, which isn't known until runtime (`Tables::uv_pad`/`uv_span`,
// baked in by `BlockRegistry::compile` - see `atlas.rs`'s module docs).

#[derive(Default)]
pub struct MeshBucket {
    pub positions: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// `rgb` = block light, `a` = the *red* channel of sky light.
    pub colors: Vec<[f32; 4]>,
    /// Sky light's green and blue channels, uploaded as `Mesh::ATTRIBUTE_UV_1`.
    ///
    /// Sky light needs three interpolated floats per vertex now that media
    /// tint it, and the vertex color had exactly one slot free. Rather than
    /// grow the vertex layout with a custom attribute - which would mean
    /// replacing Bevy's standard mesh vertex shader, and this renderer has no
    /// other reason to own one - the remaining two channels go in the second
    /// UV set, which is unused here (chunks sample one atlas) and which Bevy
    /// already plumbs through untouched: adding `ATTRIBUTE_UV_1` to a mesh
    /// makes its pipeline define `VERTEX_UVS_B` and pass `uv_b` to the
    /// fragment stage, interpolated like any other varying.
    ///
    /// The packing is deliberately *not* three values bit-packed into the one
    /// spare float: vertex attributes are interpolated across each triangle,
    /// and interpolating packed integers produces garbage between vertices.
    /// Each channel needs its own interpolated float.
    pub sky_gb: Vec<[f32; 2]>,
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

/// One chunk plus the 1-block shell from its eight neighbours, in the padded
/// layout `padded_index` addresses. Four arrays that must always be indexed
/// identically and stay the same length, so they travel as one value rather
/// than four positional parameters nothing would stop a caller reordering.
/// Built by `world::ChunkMap::build_padded`.
pub struct PaddedChunk {
    pub blocks: Vec<BlockId>,
    pub fluid: Vec<u8>,
    pub axis: Vec<u8>,
    pub light: Vec<LightCell>,
}

impl PaddedChunk {
    /// An all-air, unlit padded chunk - the starting point `build_padded`
    /// copies into, and what tests build their scenarios on top of.
    pub fn empty() -> Self {
        let n = PAD_XZ * PAD_XZ * PAD_Y;
        Self {
            blocks: vec![AIR; n],
            fluid: vec![FLUID_SOURCE; n],
            axis: vec![AXIS_Y; n],
            light: vec![LightCell::DARK; n],
        }
    }
}

/// Smoothly lit vertex value: the average light over the (up to) four cells
/// that touch this vertex on the *outside* of the face - the direct
/// neighbour plus the two side cells and the diagonal corner, which is
/// exactly the set ambient occlusion already samples. Opaque cells are
/// skipped rather than averaged in as darkness; including them would draw a
/// dark seam along every wall/floor join, since those cells are solid rock
/// that light was never in a position to occupy.
fn vertex_light(padded: &PaddedChunk, tables: &Tables, cells: [usize; 4]) -> ([f32; 3], [f32; 3]) {
    let mut block_sum = [0u32; 3];
    let mut sky_sum = [0u32; 3];
    let mut count = 0u32;
    for cell in cells {
        if tables.opaque[padded.blocks[cell] as usize] {
            continue;
        }
        let l = padded.light[cell];
        for c in 0..3 {
            block_sum[c] += l.block[c] as u32;
            sky_sum[c] += l.sky[c] as u32;
        }
        count += 1;
    }
    if count == 0 {
        return ([0.0; 3], [0.0; 3]);
    }
    let inv = 1.0 / (count as f32 * MAX_LIGHT as f32);
    (block_sum.map(|v| v as f32 * inv), sky_sum.map(|v| v as f32 * inv))
}

pub fn mesh_chunk(padded: &PaddedChunk, tables: &Tables) -> ChunkMeshData {
    debug_assert_eq!(padded.blocks.len(), PAD_XZ * PAD_XZ * PAD_Y);
    debug_assert_eq!(padded.fluid.len(), padded.blocks.len());
    debug_assert_eq!(padded.axis.len(), padded.blocks.len());
    debug_assert_eq!(padded.light.len(), padded.blocks.len());
    let mut solid = MeshBucket::default();
    let mut water = MeshBucket::default();

    for z in 0..CHUNK_SIZE {
        for x in 0..CHUNK_SIZE {
            let mut idx = padded_index(x, 0, z);
            for y in 0..WORLD_HEIGHT {
                let id = padded.blocks[idx];
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
                let axis = if tables.rotates[id as usize] { padded.axis[cell] } else { AXIS_Y };
                let level = padded.fluid[cell];
                let flow_dist = tables.flow_distance[id as usize];
                // Covered by more of the same fluid above -> render full
                // height (it's not this stack's exposed surface).
                let cap = if is_fluid {
                    if padded.blocks[cell + SY as usize] == id {
                        1.0
                    } else {
                        fluid_height(level, flow_dist)
                    }
                } else {
                    1.0
                };

                for (f, face) in FACES.iter().enumerate() {
                    let n_cell = (cell as i32 + face.neighbor_ofs) as usize;
                    let nid = padded.blocks[n_cell];
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
                                let n_level = padded.fluid[n_cell];
                                let n_cap = if padded.blocks[n_cell + SY as usize] == id {
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
                    // (different) partial bottom, or under `Blocky` style -
                    // and, critically, only applies to the *last* segment of
                    // a fall (nothing but the same fluid continuing below).
                    // Every segment used to taper unconditionally, which
                    // left a real gap: segment N's wall stopped a sixteenth
                    // short of its own floor while segment N-1 below it (if
                    // also "covered above" and hence full-height on top)
                    // started exactly at its own ceiling - a periodic
                    // sixteenth-of-a-block notch at every internal block
                    // boundary down a multi-block waterfall, not the single
                    // continuous slope the cascade was supposed to read as.
                    if is_fluid
                        && is_side
                        && level == FLUID_FALLING
                        && !stepped
                        && FALLING_WATER_STYLE == FallingWaterStyle::Sloped
                        && padded.blocks[cell - SY as usize] != id
                    {
                        bottom = 1.0 / BASE_TILE_SIZE as f32;
                    }

                    // Reaching here with a real (non-air) neighbour means
                    // that neighbour is non-opaque (an opaque one already
                    // hit `continue` above) and, if it's the same fluid id,
                    // already went through the step-wall path (a real
                    // height difference, not a coincident plane). What's
                    // left - a different, non-opaque block, e.g. glass next
                    // to water - has both sides' faces rendering at what
                    // would otherwise be the exact same plane, so nudge
                    // this one inward to avoid z-fighting (see
                    // `COINCIDENT_FACE_BIAS`).
                    let bias = if nid != 0 && nid != id { COINCIDENT_FACE_BIAS } else { 0.0 };

                    let tile = rotated_tile(tables, id, axis, f) as usize;
                    let tu = (tile % ATLAS_TILES) as f32 * UV_TILE;
                    let tv = (tile / ATLAS_TILES) as f32 * UV_TILE;
                    let vi = bucket.positions.len() as u32;
                    let mut ao = [1.0f32; 4];

                    for (ci, c) in face.corners.iter().enumerate() {
                        let mut bright = 1.0;
                        if !is_translucent {
                            let occ = |o: i32| {
                                tables.opaque[padded.blocks[(cell as i32 + o) as usize] as usize]
                                    as u32
                            };
                            let (s1, s2) = (occ(c.ao[0]), occ(c.ao[1]));
                            let level = if s1 == 1 && s2 == 1 { 3 } else { s1 + s2 + occ(c.ao[2]) };
                            bright = AO_BRIGHT[level as usize];
                        }
                        ao[ci] = bright;

                        bucket.positions.push([
                            x as f32 + c.pos[0] - bias * face.dir[0],
                            y as f32 + if c.pos[1] == 1.0 { cap } else { bottom } - bias * face.dir[1],
                            z as f32 + c.pos[2] - bias * face.dir[2],
                        ]);
                        bucket.uvs.push([
                            tu + tables.uv_pad + c.uv[0] * tables.uv_span,
                            tv + tables.uv_pad + (1.0 - c.uv[1]) * tables.uv_span,
                        ]);

                        // The four cells touching this vertex outside the
                        // face - the same neighbourhood the AO above just
                        // sampled, reused for smooth lighting.
                        let at = |o: i32| (cell as i32 + o) as usize;
                        let (block_rgb, sky_rgb) = vertex_light(
                            padded,
                            tables,
                            [n_cell, at(c.ao[0]), at(c.ao[1]), at(c.ao[2])],
                        );
                        let shade = face.shade * bright;
                        bucket.colors.push([
                            block_rgb[0].max(AMBIENT_LIGHT) * shade,
                            block_rgb[1].max(AMBIENT_LIGHT) * shade,
                            block_rgb[2].max(AMBIENT_LIGHT) * shade,
                            sky_rgb[0] * shade,
                        ]);
                        // Sky green/blue ride in UV_1 - see `MeshBucket::
                        // sky_gb`. Red stays in the color alpha it already
                        // occupied, so this adds one attribute rather than
                        // moving what was already working.
                        bucket.sky_gb.push([sky_rgb[1] * shade, sky_rgb[2] * shade]);
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
        let tables = reg.compile(&atlas.indices, atlas.tile_size);
        (reg, tables)
    }

    /// Uniform full sky light everywhere, so tests that aren't about
    /// lighting see the same brightness the pre-lighting mesher produced.
    fn lit_padded() -> PaddedChunk {
        let mut padded = PaddedChunk::empty();
        padded.light.fill(LightCell::OPEN_SKY);
        padded
    }

    #[test]
    fn lone_block_emits_six_faces() {
        let (reg, tables) = tables();
        let mut padded = lit_padded();
        padded.blocks[padded_index(8, 30, 8)] = reg.id("stone");
        let mesh = mesh_chunk(&padded, &tables);
        assert_eq!(mesh.solid.positions.len(), 6 * 4);
        assert_eq!(mesh.solid.indices.len(), 6 * 6);
        assert!(mesh.water.is_empty());
    }

    #[test]
    fn buried_block_emits_nothing() {
        let (reg, tables) = tables();
        let stone = reg.id("stone");
        let mut padded = lit_padded();
        padded.blocks.fill(stone);
        // one exposed face at the top only for the interior column we check:
        // actually fully solid volume -> zero faces inside; boundary faces
        // depend on the shell, which is also stone here.
        let mesh = mesh_chunk(&padded, &tables);
        assert!(mesh.solid.is_empty());
        // poke a hole: the neighbouring block gains exactly one face
        padded.blocks[padded_index(8, 30, 8)] = 0;
        let mesh = mesh_chunk(&padded, &tables);
        assert_eq!(mesh.solid.positions.len(), 6 * 4); // 6 cavity walls
    }

    #[test]
    fn water_goes_to_translucent_bucket_with_lowered_top() {
        let (reg, tables) = tables();
        let mut padded = lit_padded();
        padded.blocks[padded_index(4, 10, 4)] = reg.id("water");
        let mesh = mesh_chunk(&padded, &tables);
        assert!(mesh.solid.is_empty());
        assert_eq!(mesh.water.positions.len(), 6 * 4);
        let max_y = mesh.water.positions.iter().map(|p| p[1]).fold(0.0, f32::max);
        assert_eq!(max_y, 10.0 + FLUID_SURFACE);
    }

    #[test]
    fn flowing_water_is_shallower_than_a_source() {
        let (reg, tables) = tables();
        let water = reg.id("water");
        let mut padded = lit_padded();
        padded.blocks[padded_index(4, 10, 4)] = water;
        padded.fluid[padded_index(4, 10, 4)] = 3; // 3 blocks from a source
        let mesh = mesh_chunk(&padded, &tables);
        let max_y = mesh.water.positions.iter().map(|p| p[1]).fold(0.0, f32::max);
        assert!(max_y < 10.0 + FLUID_SURFACE);
        assert!(max_y > 10.0);
    }

    #[test]
    fn adjacent_water_at_different_levels_gets_a_step_wall() {
        let (reg, tables) = tables();
        let water = reg.id("water");
        let mut padded = lit_padded();
        padded.blocks[padded_index(4, 10, 4)] = water;
        padded.blocks[padded_index(5, 10, 4)] = water;
        padded.fluid[padded_index(4, 10, 4)] = 1;
        padded.fluid[padded_index(5, 10, 4)] = 4; // shallower neighbour
        let mesh = mesh_chunk(&padded, &tables);
        // the boundary between them must render a wall face instead of being
        // fully culled (same-id neighbours at equal height cull completely).
        assert!(!mesh.water.is_empty());

        // sanity: identical levels on both sides fully cull that face.
        padded.fluid[padded_index(5, 10, 4)] = 1;
        let level_mesh = mesh_chunk(&padded, &tables);
        assert!(level_mesh.water.positions.len() < mesh.water.positions.len());
    }

    #[test]
    fn glass_next_to_water_does_not_z_fight() {
        let (reg, tables) = tables();
        let water = reg.id("water");
        let glass = reg.id("glass");
        let mut padded = lit_padded();
        padded.blocks[padded_index(4, 10, 4)] = water;
        padded.blocks[padded_index(5, 10, 4)] = glass;
        let mesh = mesh_chunk(&padded, &tables);

        // Neither block culls the other's face at their shared boundary
        // (glass has real cutout holes you're meant to see the water
        // through), so both still render - but at the *exact* same plane
        // that's a textbook z-fight. Water's +x face (touching glass) must
        // land pulled back from x=5, and glass's -x face (touching water)
        // pulled back the other way - not filtering "any vertex near x=5"
        // (the water block's top/bottom/z faces also have corners at x=5
        // as part of their own footprint, correctly unbiased, since they
        // aren't the coincident face at all), but checking for the exact
        // biased coordinate each one's touching face should land on.
        let water_biased_x = 5.0 - COINCIDENT_FACE_BIAS;
        assert!(
            mesh.water.positions.iter().any(|p| (p[0] - water_biased_x).abs() < 1e-6),
            "expected water's face touching glass pulled back to x={water_biased_x}, got {:?}",
            mesh.water.positions.iter().map(|p| p[0]).collect::<Vec<_>>()
        );
        let glass_biased_x = 5.0 + COINCIDENT_FACE_BIAS;
        assert!(
            mesh.solid.positions.iter().any(|p| (p[0] - glass_biased_x).abs() < 1e-6),
            "expected glass's face touching water pulled back to x={glass_biased_x}, got {:?}",
            mesh.solid.positions.iter().map(|p| p[0]).collect::<Vec<_>>()
        );
    }

    #[test]
    fn falling_water_tapers_its_side_walls_to_a_sliver() {
        let (reg, tables) = tables();
        let water = reg.id("water");
        let mut padded = lit_padded();
        // A falling segment fed from a source directly above it, open air on
        // every side and below - the classic mid-air waterfall shaft.
        padded.blocks[padded_index(4, 11, 4)] = water;
        padded.fluid[padded_index(4, 11, 4)] = FLUID_SOURCE;
        padded.blocks[padded_index(4, 10, 4)] = water;
        padded.fluid[padded_index(4, 10, 4)] = FLUID_FALLING;
        let mesh = mesh_chunk(&padded, &tables);

        let ys: Vec<f32> = mesh.water.positions.iter().map(|p| p[1]).collect();
        let sliver = 10.0 + 1.0 / BASE_TILE_SIZE as f32;
        // The side walls' tapered bottom edge (the one-pixel sliver)...
        assert!(ys.iter().any(|&y| (y - sliver).abs() < 1e-4), "no tapered sliver in {ys:?}");
        // ...the side walls' top edge, still touching the source above...
        assert!(ys.iter().any(|&y| (y - 11.0).abs() < 1e-4), "no full-height top in {ys:?}");
        // ...and the true floor (the bottom face, unaffected by the taper).
        assert!(ys.iter().any(|&y| (y - 10.0).abs() < 1e-4), "no untouched floor in {ys:?}");
    }

    #[test]
    fn a_multi_block_fall_only_tapers_its_last_segment() {
        let (reg, tables) = tables();
        let water = reg.id("water");
        let mut padded = lit_padded();
        // Source -> two falling segments -> open air: the classic multi-
        // block waterfall shaft. Only the last segment (y=10, nothing but
        // air below it) should taper; the internal boundary between the two
        // falling segments (y=11/y=10) must connect with no gap at all.
        padded.blocks[padded_index(4, 12, 4)] = water;
        padded.fluid[padded_index(4, 12, 4)] = FLUID_SOURCE;
        padded.blocks[padded_index(4, 11, 4)] = water;
        padded.fluid[padded_index(4, 11, 4)] = FLUID_FALLING;
        padded.blocks[padded_index(4, 10, 4)] = water;
        padded.fluid[padded_index(4, 10, 4)] = FLUID_FALLING;
        let mesh = mesh_chunk(&padded, &tables);

        let ys: Vec<f32> = mesh.water.positions.iter().map(|p| p[1]).collect();
        let sliver = 10.0 + 1.0 / BASE_TILE_SIZE as f32;
        // The internal segment (y=11) must NOT show a tapered edge partway
        // up its own height - it should run the full block, connecting flush
        // to y=10's ceiling at exactly y=11.0.
        assert!(!ys.iter().any(|&y| (y - (11.0 + 1.0 / BASE_TILE_SIZE as f32)).abs() < 1e-4),
            "internal segment shows a tapered edge (the gap bug) in {ys:?}");
        assert!(ys.iter().any(|&y| (y - 11.0).abs() < 1e-4),
            "no seam at the internal boundary (y=11.0) in {ys:?}");
        // Only the true last segment (y=10) tapers to the sliver.
        assert!(ys.iter().any(|&y| (y - sliver).abs() < 1e-4),
            "no tapered sliver on the last segment in {ys:?}");
    }

    #[test]
    fn ao_darkens_corner_vertices() {
        let (reg, tables) = tables();
        let stone = reg.id("stone");
        let mut padded = lit_padded();
        padded.blocks[padded_index(8, 30, 8)] = stone;
        padded.blocks[padded_index(9, 31, 8)] = stone; // occluder above the +x neighbour
        let mesh = mesh_chunk(&padded, &tables);
        // Some top-face vertex of the base block must now be darker than a
        // fully unoccluded one. AO rides on the sky channel here (the scene
        // has full sky light and no block light), same as it rides on every
        // other channel.
        let top_lights: Vec<f32> = mesh
            .solid
            .positions
            .iter()
            .zip(&mesh.solid.colors)
            .filter(|(p, _)| p[1] == 31.0)
            .map(|(_, c)| c[3])
            .collect();
        assert!(!top_lights.is_empty());
        assert!(top_lights.iter().any(|&l| l < 1.0));
        assert!(top_lights.iter().any(|&l| l == 1.0));
    }

    #[test]
    fn sky_light_lands_in_the_vertex_alpha_channel_and_block_light_in_rgb() {
        let (reg, tables) = tables();
        let stone = reg.id("stone");

        // Fully sky-lit, no block light: alpha carries it, rgb sits at the
        // ambient floor.
        let mut padded = lit_padded();
        padded.blocks[padded_index(8, 30, 8)] = stone;
        let mesh = mesh_chunk(&padded, &tables);
        let top = horizontal_face_color(&mesh, 31.0);
        assert_eq!(top[3], 1.0, "full sky light should reach the alpha channel");
        assert!(
            (top[0] - AMBIENT_LIGHT).abs() < 1e-6,
            "no block light means rgb should be exactly the ambient floor, got {top:?}"
        );

        // The mirror image: full block light, no sky at all.
        let mut padded = PaddedChunk::empty();
        padded.light.fill(LightCell { block: [MAX_LIGHT; 3], sky: [0; 3] });
        padded.blocks[padded_index(8, 30, 8)] = stone;
        let mesh = mesh_chunk(&padded, &tables);
        let top = horizontal_face_color(&mesh, 31.0);
        assert_eq!(top[3], 0.0, "no sky light");
        assert_eq!(top[0], 1.0, "full block light");
    }

    #[test]
    fn colored_block_light_is_baked_per_channel() {
        let (reg, tables) = tables();
        let stone = reg.id("stone");
        let mut padded = PaddedChunk::empty();
        // A deep-red light: full red, half green, no blue.
        padded.light.fill(LightCell { block: [MAX_LIGHT, MAX_LIGHT / 2, 0], sky: [0; 3] });
        padded.blocks[padded_index(8, 30, 8)] = stone;
        let mesh = mesh_chunk(&padded, &tables);

        let top = horizontal_face_color(&mesh, 31.0);
        assert_eq!(top[0], 1.0);
        // Not exactly 0.5: `MAX_LIGHT / 2` is integer division on an odd
        // maximum, so the expected value comes from the same arithmetic
        // rather than from a rounded-off constant.
        let half = (MAX_LIGHT / 2) as f32 / MAX_LIGHT as f32;
        assert!((top[1] - half).abs() < 1e-6, "half-strength green, got {top:?}");
        assert!(
            (top[2] - AMBIENT_LIGHT).abs() < 1e-6,
            "an unlit channel falls back to ambient, not to zero, got {top:?}"
        );
    }

    #[test]
    fn a_dark_cell_still_gets_the_ambient_floor_rather_than_pure_black() {
        let (reg, tables) = tables();
        let stone = reg.id("stone");
        let mut padded = PaddedChunk::empty(); // no light at all anywhere
        padded.blocks[padded_index(8, 30, 8)] = stone;
        let mesh = mesh_chunk(&padded, &tables);

        for c in &mesh.solid.colors {
            assert!(c[0] > 0.0, "nothing should bake to pure black: {c:?}");
            // Still shaded per face, so the floor isn't a flat wash.
            assert!(c[0] <= AMBIENT_LIGHT + 1e-6);
            assert_eq!(c[3], 0.0);
        }
        let top = horizontal_face_color(&mesh, 31.0);
        let bottom = horizontal_face_color(&mesh, 30.0);
        assert!(top[0] > bottom[0], "face shading must survive the ambient floor");
    }

    /// The color of the first vertex of the horizontal quad sitting at height
    /// `y`. Matching whole quads rather than individual vertices matters: a
    /// side face has two of its four corners at the block's top height too,
    #[test]
    fn every_per_vertex_buffer_stays_the_same_length() {
        // Bevy panics at mesh-upload time if a mesh's vertex attributes
        // disagree on how many vertices there are, and this project can't
        // catch that in a test any other way - the failure happens on the
        // render thread with a real GPU, which no test here has. Sky light
        // now spans two attributes filled at two separate `push` sites in
        // the same loop, so a future edit adding a vertex to one and not the
        // other is a live hazard rather than a hypothetical one.
        let (reg, tables) = tables();
        let mut padded = PaddedChunk::empty();
        padded.light.fill(LightCell::OPEN_SKY);
        // A scene with both buckets populated: stone for `solid`, water for
        // the translucent pass.
        padded.blocks[padded_index(4, 30, 4)] = reg.id("stone");
        padded.blocks[padded_index(8, 30, 8)] = reg.id("water");
        let mesh = mesh_chunk(&padded, &tables);

        for (name, bucket) in [("solid", &mesh.solid), ("water", &mesh.water)] {
            let n = bucket.positions.len();
            assert!(n > 0, "{name} bucket should have geometry to check");
            assert_eq!(bucket.uvs.len(), n, "{name}: uvs");
            assert_eq!(bucket.colors.len(), n, "{name}: colors");
            assert_eq!(bucket.sky_gb.len(), n, "{name}: sky_gb");
        }
    }

    /// so a per-vertex search finds a *side* face (shade 0.6) when it meant
    /// to find the top one (shade 1.0).
    fn horizontal_face_color(mesh: &ChunkMeshData, y: f32) -> [f32; 4] {
        mesh.solid
            .positions
            .chunks_exact(4)
            .zip(mesh.solid.colors.chunks_exact(4))
            .find(|(quad, _)| quad.iter().all(|p| p[1] == y))
            .map(|(_, colors)| colors[0])
            .expect("no horizontal face at that height")
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
        let mut padded = PaddedChunk::empty();
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
                            let dst = padded_index(px, y as i32, pz);
                            let src = block_index(lx, y, lz);
                            padded.blocks[dst] = chunk.blocks[src];
                            padded.fluid[dst] = chunk.fluid[src];
                            padded.light[dst] = chunk.light[src];
                        }
                    }
                }
            }
        }
        let mesh = mesh_chunk(&padded, &tables);
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
