//! Procedural terrain generator. Runs on the async compute task pool.
//! Deterministic per (seed, cx, cz) with no cross-chunk dependencies, so
//! chunks can generate in any order on any thread. Trees keep a 2-block
//! margin from chunk borders so features never spill across chunks.
//!
//! **Rivers are the one exception to "no cross-chunk dependencies."** A
//! river has to flow downhill across many chunks in a row, so its shape
//! can't be decided per-chunk in isolation - see `RegionHydrology`. Chunks
//! still generate independently of each other in the sense that matters
//! (any order, any thread, no chunk waits on another chunk), but many
//! chunks now share one read-only, lazily-built-and-cached region of
//! precomputed flow data behind a `Mutex` (`TerrainGenerator::hydrology`).
//!
//! To customize generation, swap the generator constructed in
//! `world::compile_content` for your own.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::biome::{self, Biome};
use crate::blocks::{BlockId, BlockRegistry, Transparency, AIR, AXIS_Y, FLUID_SOURCE};
use crate::config::{block_index, CHUNK_SIZE, CS, H, SEA_LEVEL, WORLD_HEIGHT};
use crate::light::{LightCell, MAX_LIGHT};
use crate::noise::{hash2, hash3, SimplexNoise};

const SNOW_LINE: i32 = 45;

/// How far a fully `Biome::drier` column's terrain gets pushed above the
/// plain baseline, at full `biome::drier_strength` - chosen so noticeably
/// fewer columns dip below `SEA_LEVEL` there (see `generate`'s flooding
/// loop) without visibly changing the mountain/plains shape language this
/// generator already has (this only ever *adds* to whatever `detail`/
/// `mountain` noise already produced).
const DRIER_HEIGHT_BOOST: f64 = 6.0;

/// Very-low-frequency noise stream deciding where large, genuinely
/// *connected* ocean basins sit, entirely separate from `mountain`/
/// `terrain`'s local relief - see `TerrainGenerator::surface_height`'s
/// ocean blend. Low enough frequency that a "this column is ocean" verdict
/// stays the same across many chunks in a row, which is what turns what
/// used to be scattered small sub-sea-level dips into real seas that
/// actually separate landmasses, instead of one giant landmass with
/// puddles in it.
const CONTINENT_SCALE: f64 = 0.0006;
/// `continent_value` is roughly -1..=1 and centered on 0; a threshold near
/// zero is what actually produces multiple real, separated landmasses
/// (verified by `oceans_split_land_into_multiple_masses_separated_by_real_seas`)
/// - pushing it further negative (so land is the clear majority) instead
/// makes land the one *connected* mass with the ocean fragmented into many
/// small inland seas, the mirror image of the original "one giant
/// landmass" complaint this exists to fix.
const OCEAN_THRESHOLD: f32 = -0.02;
/// How much of `continent_value`'s own range the land->ocean blend spans,
/// centered on `OCEAN_THRESHOLD` - a coastline that fades in over a few
/// hundred blocks instead of snapping at an exact contour line.
const COAST_BLEND: f32 = 0.18;
/// Target height for the deepest ocean floor - well below `SEA_LEVEL`
/// (26) so open ocean reads as a real body of water, not a shallow puddle.
const DEEP_OCEAN_FLOOR: f64 = 9.0;

/// Size of one river hydrology region, in blocks - see `RegionHydrology`.
const REGION_BLOCKS: i32 = 512;
/// Sample spacing within a region's flow grid, in blocks. Rivers are a
/// landscape-scale feature; this is plenty of resolution to route a
/// believable path and accumulate realistic flow without paying per-block
/// cost.
const FLOW_CELL: i32 = 8;
const FLOW_GRID: usize = (REGION_BLOCKS / FLOW_CELL) as usize;

/// Minimum accumulated upstream area (in flow-grid cells) before a cell
/// counts as a river at all - below this it's just ordinary hillside
/// runoff with nothing carved.
const RIVER_THRESHOLD: f32 = 8.0;
/// Accumulated area at which a river reaches its full carved width/depth -
/// tuned (see this module's tests) so only a real, well-fed drainage
/// channel gets there within one bounded region, not every minor stream.
const MAX_ACCUM_FOR_FULL_CARVE: f32 = 55.0;
/// How many blocks a fully-grown river cuts below its natural (pre-river)
/// height.
const MAX_RIVER_CARVE: f64 = 20.0;

/// A bounded (`REGION_BLOCKS` square) patch of precomputed river data,
/// built once per region the first time any chunk inside it is generated
/// and cached for the lifetime of the owning `TerrainGenerator` (see
/// `TerrainGenerator::river_carve`). This is the "real flow simulation"
/// this generator's rivers use, chosen over a cheaper noise-band
/// approximation: a genuinely *global* flow simulation isn't possible for
/// a chunk generator with no fixed world size (there's no bound on how far
/// upstream a river's catchment could extend), so a large-but-bounded
/// region is the deliberate middle ground - each river's drainage basin is
/// confined to a single region and simply fades out (see `sample_carve`)
/// before reaching that region's edge, rather than the generator ever
/// trying to reconcile flow across an unbounded number of neighbours.
///
/// Deliberately *not* a `bevy::prelude::Resource` cached in `world.rs` -
/// it's private, derived-and-disposable data belonging entirely to terrain
/// generation, with no reason for anything outside this module to see a
/// region's raw flow grid.
struct RegionHydrology {
    /// River carve depth in blocks, on one padded `(FLOW_GRID + 2)` square
    /// grid per region (a 1-cell halo on every side, forced to zero) so
    /// `sample_carve`'s bilinear lookup never needs a neighbouring
    /// region's data, and a river always tapers to nothing before the
    /// region's own edge instead of cutting off abruptly.
    carve: Vec<f32>,
}

/// The actual flow-accumulation algorithm, pulled out as a pure function of
/// a padded `(grid + 2)` square height field so it can be driven by a
/// synthetic, hand-picked height field in tests - the real entry point
/// (`RegionHydrology::build`) still resolves its own input by sampling the
/// generator's noise, mirroring `atlas::build_atlas`/`build_atlas_from_dir`'s
/// split for exactly the same reason.
///
/// Steepest-descent flow accumulation: process cells from highest to
/// lowest so every upstream contributor has already deposited its flow
/// into its one downhill neighbour by the time a cell is visited - the
/// standard O(n log n) priority-order accumulation, no iterative
/// relaxation needed since water only ever flows one way, downhill.
/// Returns a padded `(grid + 2)` square carve grid with the halo ring left
/// at zero.
fn carve_from_heights(height: &[f32], grid: usize) -> Vec<f32> {
    let p = grid + 2;
    debug_assert_eq!(height.len(), p * p);

    let mut order: Vec<(usize, usize)> =
        (1..p - 1).flat_map(|gz| (1..p - 1).map(move |gx| (gx, gz))).collect();
    order.sort_by(|a, b| height[b.0 + p * b.1].partial_cmp(&height[a.0 + p * a.1]).unwrap());

    let mut accum = vec![1f32; grid * grid];
    for &(gx, gz) in &order {
        let h = height[gx + p * gz];
        let mut best: Option<(usize, usize, f32)> = None;
        for dz in -1i32..=1 {
            for dx in -1i32..=1 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                let nx = (gx as i32 + dx) as usize;
                let nz = (gz as i32 + dz) as usize;
                let drop = h - height[nx + p * nz];
                if drop > best.map_or(0.0, |(_, _, bd)| bd) {
                    best = Some((nx, nz, drop));
                }
            }
        }
        let idx = (gx - 1) + grid * (gz - 1);
        if let Some((nx, nz, _)) = best {
            if (1..=grid).contains(&nx) && (1..=grid).contains(&nz) {
                accum[(nx - 1) + grid * (nz - 1)] += accum[idx];
            }
            // Flow whose steepest descent exits into the halo leaves the
            // region and is simply dropped - see `RegionHydrology`'s own
            // doc comment.
        }
    }

    let mut carve = vec![0f32; p * p];
    for gz in 1..p - 1 {
        for gx in 1..p - 1 {
            let a = accum[(gx - 1) + grid * (gz - 1)];
            if a > RIVER_THRESHOLD {
                let t = ((a - RIVER_THRESHOLD) / (MAX_ACCUM_FOR_FULL_CARVE - RIVER_THRESHOLD))
                    .clamp(0.0, 1.0);
                // sqrt: a real channel's depth grows quickly with a little
                // accumulated flow and levels off after, rather than
                // growing linearly all the way to the cap.
                carve[gx + p * gz] = t.sqrt() * MAX_RIVER_CARVE as f32;
            }
        }
    }
    carve
}

impl RegionHydrology {
    fn build(gen: &TerrainGenerator, region: (i32, i32)) -> Self {
        let p = FLOW_GRID + 2;
        let origin_x = region.0 * REGION_BLOCKS;
        let origin_z = region.1 * REGION_BLOCKS;

        // Sample the generator's own pre-river height formula on the
        // padded grid (including the 1-cell halo) so every interior cell's
        // steepest-descent direction can be found without needing a
        // neighbouring region's data at all.
        let mut height = vec![0f32; p * p];
        for gz in 0..p {
            for gx in 0..p {
                let wx = origin_x + (gx as i32 - 1) * FLOW_CELL;
                let wz = origin_z + (gz as i32 - 1) * FLOW_CELL;
                height[gx + p * gz] = gen.surface_height(wx, wz) as f32;
            }
        }

        Self { carve: carve_from_heights(&height, FLOW_GRID) }
    }

    /// Bilinearly samples this region's carve grid at a world position
    /// known to fall inside it (`local_x`/`local_z`, relative to the
    /// region's own origin). Smooth interpolation between the sparse
    /// `FLOW_CELL`-spaced samples is what gives a river its width and a
    /// natural taper for free, with no separate "how wide is this river"
    /// logic anywhere.
    fn sample_carve(&self, local_x: i32, local_z: i32) -> f64 {
        let p = FLOW_GRID + 2;
        let fx = local_x as f64 / FLOW_CELL as f64 + 1.0;
        let fz = local_z as f64 / FLOW_CELL as f64 + 1.0;
        let x0 = (fx.floor() as i64).clamp(0, p as i64 - 1) as usize;
        let z0 = (fz.floor() as i64).clamp(0, p as i64 - 1) as usize;
        let x1 = (x0 + 1).min(p - 1);
        let z1 = (z0 + 1).min(p - 1);
        let tx = (fx - x0 as f64).clamp(0.0, 1.0);
        let tz = (fz - z0 as f64).clamp(0.0, 1.0);
        let c00 = self.carve[x0 + p * z0] as f64;
        let c10 = self.carve[x1 + p * z0] as f64;
        let c01 = self.carve[x0 + p * z1] as f64;
        let c11 = self.carve[x1 + p * z1] as f64;
        let a = c00 + (c10 - c00) * tx;
        let b = c01 + (c11 - c01) * tx;
        a + (b - a) * tz
    }
}

/// A freshly generated chunk's block ids plus its fluid levels. Every
/// generated fluid cell (currently just sea-level flooding) starts as a
/// permanent source (`FLUID_SOURCE`) — the ocean is static, not simulated;
/// only player-placed/spread water flows (see `world.rs`'s `FluidQueue`).
pub struct GeneratedChunk {
    pub blocks: Vec<BlockId>,
    pub fluid: Vec<u8>,
    /// Parallel to `blocks`, like `fluid`. Terrain never generates a
    /// non-default orientation today (tree trunks are always upright), so
    /// this is always filled with `AXIS_Y` - only player placement
    /// (`interact.rs`) ever writes a different value.
    pub axis: Vec<u8>,
    /// Parallel to `blocks`. Only the straight-down sky column is filled in
    /// here (see `fill_sky_columns`); everything else - block light, and sky
    /// light that has to turn a corner into a cave or under an overhang - is
    /// left to `light.rs`'s propagation once the chunk is in the world and
    /// its neighbours are known.
    pub light: Vec<LightCell>,
}

struct TerrainIds {
    stone: BlockId,
    dirt: BlockId,
    grass: BlockId,
    sand: BlockId,
    gravel: BlockId,
    water: BlockId,
    ice: BlockId,
    log: BlockId,
    leaves: BlockId,
    bedrock: BlockId,
    snow: BlockId,
    coal: BlockId,
    iron: BlockId,
}

pub struct TerrainGenerator {
    seed: u32,
    ids: TerrainIds,
    /// Per-block-id opacity, so `fill_sky_columns` can tell what stops
    /// sunlight without needing the compiled `Tables` (which don't exist yet
    /// when a generator is constructed, and which generation otherwise has
    /// no reason to depend on).
    opaque: Vec<bool>,
    terrain: SimplexNoise,
    mountain: SimplexNoise,
    /// Continent/ocean shaping noise - see `CONTINENT_SCALE`'s doc comment.
    continent: SimplexNoise,
    cave_a: SimplexNoise,
    cave_b: SimplexNoise,
    /// Which biome region a column belongs to - see `biome.rs`'s module
    /// docs. A separate stream from `terrain`/`mountain` (built via
    /// `biome::region_noise_for_seed`, the same function `world.rs`'s
    /// runtime `BiomeMap` resource uses) so both agree on the exact same
    /// classification for the same seed and column without needing to
    /// literally share one noise object.
    biome: SimplexNoise,
    /// Cache of per-region river data, built lazily the first time any
    /// chunk in that region is generated - see `RegionHydrology` and
    /// `river_carve`. A `Mutex` because chunk generation runs on the async
    /// compute task pool across many threads at once
    /// (`world.rs`'s `stream_chunks`); this is the one piece of terrain
    /// generation state that isn't purely a function of its own chunk
    /// coordinate any more - see this module's own doc comment at the top
    /// of the file.
    hydrology: Mutex<HashMap<(i32, i32), Arc<RegionHydrology>>>,
}

impl TerrainGenerator {
    pub fn new(seed: u32, reg: &BlockRegistry) -> Self {
        Self {
            seed,
            opaque: reg
                .defs
                .iter()
                .map(|def| def.transparency == Transparency::No)
                .collect(),
            ids: TerrainIds {
                stone: reg.id("stone"),
                dirt: reg.id("dirt"),
                grass: reg.id("grass"),
                sand: reg.id("sand"),
                gravel: reg.id("gravel"),
                water: reg.id("water"),
                ice: reg.id("ice"),
                log: reg.id("log"),
                leaves: reg.id("leaves"),
                bedrock: reg.id("bedrock"),
                snow: reg.id("snow"),
                coal: reg.id("coal_ore"),
                iron: reg.id("iron_ore"),
            },
            terrain: SimplexNoise::new(seed),
            mountain: SimplexNoise::new(seed ^ 0x9e3779b9),
            continent: SimplexNoise::new(seed ^ 0x27d4_eb2f),
            cave_a: SimplexNoise::new(seed ^ 0x85ebca6b),
            cave_b: SimplexNoise::new(seed ^ 0xc2b2ae35),
            biome: biome::region_noise_for_seed(seed),
            hydrology: Mutex::new(HashMap::new()),
        }
    }

    fn continent_value(&self, wx: i32, wz: i32) -> f32 {
        (self.continent.fbm2(wx as f64 * CONTINENT_SCALE, wz as f64 * CONTINENT_SCALE, 3) as f32)
            .clamp(-1.0, 1.0)
    }

    /// The mountain/plains local relief formula, unchanged from before
    /// oceans existed - still continent-blind on purpose, see
    /// `surface_height` for how it's combined with `continent_value`.
    fn land_relief(&self, wx: i32, wz: i32) -> f64 {
        // Low-frequency mask blends flat plains into mountains.
        let m = self.mountain.fbm2(wx as f64 * 0.0035, wz as f64 * 0.0035, 3) * 0.5 + 0.5;
        let mountain = m * m;
        let detail = self.terrain.fbm2(wx as f64 * 0.011, wz as f64 * 0.011, 4);
        27.0 + detail * (5.0 + mountain * 24.0) + mountain * 10.0
    }

    pub fn surface_height(&self, wx: i32, wz: i32) -> i32 {
        let land = self.land_relief(wx, wz);
        let ocean_t = (((OCEAN_THRESHOLD - self.continent_value(wx, wz)) / COAST_BLEND) as f64)
            .clamp(0.0, 1.0);
        let h = if ocean_t <= 0.0 {
            land
        } else {
            // A bit of the existing detail noise, scaled down, keeps the
            // seafloor from reading as a perfectly flat plate.
            let seafloor_detail = self.terrain.fbm2(wx as f64 * 0.02, wz as f64 * 0.02, 3) * 3.0;
            let ocean = DEEP_OCEAN_FLOOR + seafloor_detail;
            land + (ocean - land) * ocean_t
        };
        (h.floor() as i32).clamp(2, WORLD_HEIGHT - 8)
    }

    /// How many blocks to cut this column's `surface_height` down by, from
    /// `RegionHydrology`'s cached per-region flow accumulation - `0.0` for
    /// the overwhelming majority of columns (anything not on or very near
    /// a river's path). Builds and caches that region's hydrology on first
    /// use.
    fn river_carve(&self, wx: i32, wz: i32) -> f64 {
        let region = (wx.div_euclid(REGION_BLOCKS), wz.div_euclid(REGION_BLOCKS));
        let hydrology = {
            let mut cache = self.hydrology.lock().unwrap();
            cache.entry(region).or_insert_with(|| Arc::new(RegionHydrology::build(self, region))).clone()
        };
        let local_x = wx - region.0 * REGION_BLOCKS;
        let local_z = wz - region.1 * REGION_BLOCKS;
        hydrology.sample_carve(local_x, local_z)
    }

    /// `surface_height`, plus the `Biome::drier` height boost and minus
    /// any `river_carve` a river cuts through it - what a column's surface
    /// height (and therefore whether it floods into water) really ends up
    /// being. `surface_height` itself stays biome- and river-blind on
    /// purpose (its own continent/mountain/plains shape has nothing to do
    /// with either), but anything that needs to know the *real* height a
    /// specific column generated at - including this generator's own
    /// tests - has to go through this, not `surface_height` alone.
    fn effective_height(&self, wx: i32, wz: i32) -> i32 {
        let boost = biome::drier_strength(&self.biome, wx, wz) as f64 * DRIER_HEIGHT_BOOST;
        let carve = self.river_carve(wx, wz);
        ((self.surface_height(wx, wz) as f64 + boost - carve).round() as i32)
            .clamp(2, WORLD_HEIGHT - 8)
    }

    pub fn generate(&self, cx: i32, cz: i32) -> GeneratedChunk {
        let ids = &self.ids;
        let seed = self.seed;
        let mut blocks = vec![AIR; CS * CS * H];
        let mut heights = [0i32; CS * CS];
        let mut surface = [AIR; CS * CS];

        for z in 0..CS {
            for x in 0..CS {
                let wx = cx * CHUNK_SIZE + x as i32;
                let wz = cz * CHUNK_SIZE + z as i32;
                let biome = biome::biome_at(&self.biome, wx, wz);
                // A `Biome::drier` column's terrain sits a bit higher than
                // the plain baseline (continuously, via `drier_strength` -
                // no elevation cliff at the biome edge), so fewer of its
                // columns dip below SEA_LEVEL and flood into a lake below.
                let h = self.effective_height(wx, wz);
                heights[x + CS * z] = h;

                let beach = h <= SEA_LEVEL + 1;
                let snowy = h >= SNOW_LINE;
                // Snow biome always shows snow at the surface - it takes
                // priority over both the sandy-beach and mountain-altitude
                // cases, which stay exactly as they were for every other
                // biome.
                let top_id = if biome == Biome::Snow {
                    ids.snow
                } else if beach {
                    ids.sand
                } else if snowy {
                    ids.snow
                } else {
                    ids.grass
                };
                let fill_id = if beach { ids.sand } else { ids.dirt };
                surface[x + CS * z] = top_id;

                let base = block_index(x, 0, z);
                blocks[base] = ids.bedrock;
                for y in 1..=h {
                    blocks[base + y as usize] = if y == h {
                        top_id
                    } else if y >= h - 3 {
                        fill_id
                    } else {
                        ids.stone
                    };
                }
                // Flood water up to sea level - in a biome where water
                // freezes (`Biome::freezes_water`), the exposed top layer
                // (the only one with air directly above it) generates as
                // ice instead, matching what `world.rs`'s `freeze_exposed_
                // water` would convert it to anyway if a later change
                // exposed a still-liquid top layer to air.
                for y in (h + 1)..=SEA_LEVEL {
                    let exposed = y == SEA_LEVEL;
                    blocks[base + y as usize] =
                        if exposed && biome.freezes_water() { ids.ice } else { ids.water };
                }
                // Gravel patches on the sea floor.
                if h < SEA_LEVEL && hash2(wx, wz, seed ^ 0x1234) < 0.3 {
                    blocks[base + h as usize] = ids.gravel;
                }

                // Carve "spaghetti" caves on land columns (kept away from
                // water so we don't punch holes into the sea floor).
                if h > SEA_LEVEL + 1 {
                    for y in 4..(h - 2) {
                        let a = self
                            .cave_a
                            .noise3(wx as f64 * 0.045, y as f64 * 0.075, wz as f64 * 0.045);
                        if a.abs() > 0.09 {
                            continue;
                        }
                        let b = self
                            .cave_b
                            .noise3(wx as f64 * 0.045, y as f64 * 0.075, wz as f64 * 0.045);
                        if b.abs() < 0.09 {
                            blocks[base + y as usize] = AIR;
                        }
                    }
                }

                // Ore veins.
                for y in 2..(h - 3).min(40) {
                    if blocks[base + y as usize] != ids.stone {
                        continue;
                    }
                    let r = hash3(wx, y, wz, seed ^ 0xabcd);
                    if r < 0.006 && y < 28 {
                        blocks[base + y as usize] = ids.iron;
                    } else if r < 0.018 {
                        blocks[base + y as usize] = ids.coal;
                    }
                }
            }
        }

        // Trees (second pass; margin keeps canopies inside this chunk).
        for z in 2..CS - 2 {
            for x in 2..CS - 2 {
                if surface[x + CS * z] != ids.grass {
                    continue;
                }
                let wx = cx * CHUNK_SIZE + x as i32;
                let wz = cz * CHUNK_SIZE + z as i32;
                let r = hash2(wx, wz, seed ^ 0x51f3);
                if r >= 0.012 {
                    continue;
                }

                let h = heights[x + CS * z];
                let trunk_h = 4 + ((r * 1000.0) as i32) % 3;
                if h + trunk_h + 2 >= WORLD_HEIGHT {
                    continue;
                }
                let base = block_index(x, 0, z);
                if blocks[base + h as usize] != ids.grass {
                    continue; // surface was carved away
                }

                blocks[base + h as usize] = ids.dirt;
                for t in 1..=trunk_h {
                    blocks[base + (h + t) as usize] = ids.log;
                }

                // Canopy: two wide layers, a narrow layer, and a cap.
                for ly in (trunk_h - 2)..=(trunk_h + 1) {
                    let radius: i32 = if ly <= trunk_h - 1 { 2 } else { 1 };
                    for dz in -radius..=radius {
                        for dx in -radius..=radius {
                            if dx.abs() == radius
                                && dz.abs() == radius
                                && (radius == 2 || ly == trunk_h + 1)
                            {
                                continue; // clip corners
                            }
                            let idx = block_index(
                                (x as i32 + dx) as usize,
                                (h + ly) as usize,
                                (z as i32 + dz) as usize,
                            );
                            if blocks[idx] == AIR {
                                blocks[idx] = ids.leaves;
                            }
                        }
                    }
                }
                blocks[base + (h + trunk_h + 1) as usize] = ids.leaves;
            }
        }

        GeneratedChunk {
            fluid: vec![FLUID_SOURCE; blocks.len()],
            axis: vec![AXIS_Y; blocks.len()],
            light: self.fill_sky_columns(&blocks),
            blocks,
        }
    }

    /// Sunlight straight down: every column starts at full strength from the
    /// top of the world and keeps it until the first opaque block, below
    /// which it's dark until `light.rs` propagates something in sideways.
    ///
    /// Doing this here rather than leaving it entirely to the propagation
    /// queue is what keeps chunk streaming cheap: it needs no neighbour
    /// information at all (a column is self-contained), and it settles the
    /// overwhelming majority of a chunk's cells - everything in open air -
    /// to their exact final value, so a chunk's very first mesh is already
    /// correctly lit outdoors and the queue only has the genuinely
    /// cross-chunk and enclosed cases left to work out.
    fn fill_sky_columns(&self, blocks: &[BlockId]) -> Vec<LightCell> {
        let mut light = vec![LightCell::DARK; blocks.len()];
        for z in 0..CS {
            for x in 0..CS {
                let base = block_index(x, 0, z);
                for y in (0..H).rev() {
                    if self.opaque[blocks[base + y] as usize] {
                        break;
                    }
                    light[base + y].sky = [MAX_LIGHT; 3];
                }
            }
        }
        light
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::BlockRegistry;

    #[test]
    fn generation_is_deterministic_and_sane() {
        let reg = BlockRegistry::with_defaults();
        let gen = TerrainGenerator::new(7, &reg);
        let a = gen.generate(3, -2).blocks;
        let b = gen.generate(3, -2).blocks;
        assert_eq!(a, b);
        assert_eq!(a.len(), CS * CS * H);

        let bedrock = reg.id("bedrock");
        let stone = reg.id("stone");
        for z in 0..CS {
            for x in 0..CS {
                assert_eq!(a[block_index(x, 0, z)], bedrock);
                // top of the world is air
                assert_eq!(a[block_index(x, H - 1, z)], AIR);
            }
        }
        assert!(a.iter().any(|&b| b == stone));
    }

    #[test]
    fn different_seeds_differ() {
        let reg = BlockRegistry::with_defaults();
        let a = TerrainGenerator::new(1, &reg).generate(0, 0).blocks;
        let b = TerrainGenerator::new(2, &reg).generate(0, 0).blocks;
        assert_ne!(a, b);
    }

    /// Flood-fills a boolean grid (4-connectivity) and returns each
    /// connected component's cell count, largest first.
    fn connected_component_sizes(grid: &[bool], n: usize) -> Vec<usize> {
        let mut visited = vec![false; grid.len()];
        let mut sizes = Vec::new();
        for start in 0..grid.len() {
            if visited[start] || !grid[start] {
                continue;
            }
            let mut stack = vec![start];
            visited[start] = true;
            let mut size = 0;
            while let Some(i) = stack.pop() {
                size += 1;
                let (x, z) = (i % n, i / n);
                for (dx, dz) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                    let (nx, nz) = (x as i32 + dx, z as i32 + dz);
                    if nx < 0 || nz < 0 || nx >= n as i32 || nz >= n as i32 {
                        continue;
                    }
                    let ni = nx as usize + n * nz as usize;
                    if !visited[ni] && grid[ni] {
                        visited[ni] = true;
                        stack.push(ni);
                    }
                }
            }
            sizes.push(size);
        }
        sizes.sort_unstable_by(|a, b| b.cmp(a));
        sizes
    }

    /// The direct test for the user-facing complaint oceans were built to
    /// fix: land must genuinely split into more than one landmass (not one
    /// giant landmass with a few puddles in it), and those landmasses must
    /// be separated by a real, *connected* sea rather than scattered
    /// isolated lakes.
    #[test]
    fn oceans_split_land_into_multiple_masses_separated_by_real_seas() {
        let reg = BlockRegistry::with_defaults();
        // Wide enough (relative to CONTINENT_SCALE's own wavelength) to
        // realistically span more than one land/ocean cycle - too small a
        // sample window would just show one landmass and one sea even
        // when the underlying noise genuinely produces several of each.
        let n = 300;
        let step = 40;
        let total = (n * n) as f64;
        for seed in [1u32, 7, 999] {
            let gen = TerrainGenerator::new(seed, &reg);
            let mut ocean = vec![false; n * n];
            for gz in 0..n {
                for gx in 0..n {
                    let wx = (gx as i32 - n as i32 / 2) * step;
                    let wz = (gz as i32 - n as i32 / 2) * step;
                    ocean[gx + n * gz] = gen.surface_height(wx, wz) < SEA_LEVEL;
                }
            }
            let land: Vec<bool> = ocean.iter().map(|&o| !o).collect();

            // A "real" landmass/sea is a connected component covering a
            // meaningful slice of the sampled area - a handful of noise-
            // driven single-cell islands/ponds (the long tail every run
            // has) don't count as a second landmass or a real sea.
            let land_components = connected_component_sizes(&land, n);
            let real_landmasses =
                land_components.iter().filter(|&&c| c as f64 >= total * 0.01).count();
            assert!(
                real_landmasses >= 2,
                "seed {seed}: expected multiple real separated landmasses, found {real_landmasses} (sizes {:?})",
                land_components
            );

            let ocean_components = connected_component_sizes(&ocean, n);
            let largest_ocean = ocean_components[0];
            assert!(
                largest_ocean as f64 >= total * 0.05,
                "seed {seed}: no ocean basin is large enough to read as a real sea (largest is {largest_ocean})"
            );
        }
    }

    /// A real river, generated through the whole pipeline (not the
    /// synthetic grid `hydrology_measure` exercises), should actually cut
    /// a meaningfully lower, water-filled channel through otherwise dry
    /// land - the concrete, user-facing behavior all of `RegionHydrology`
    /// exists to produce. Searches across several `REGION_BLOCKS`-sized
    /// regions (not just one) since how strongly any single region's
    /// terrain channels flow varies with its own local shape.
    #[test]
    fn a_real_river_cuts_a_water_filled_channel_through_dry_land() {
        let reg = BlockRegistry::with_defaults();
        let water = reg.id("water");
        let ice = reg.id("ice");
        let seed = 7;
        let gen = TerrainGenerator::new(seed, &reg);

        let mut found = false;
        let mut best_carve = 0;
        'search: for cz in -80..80 {
            for cx in -80..80 {
                let wx = cx * CHUNK_SIZE + 8;
                let wz = cz * CHUNK_SIZE + 8;
                let natural = gen.surface_height(wx, wz);
                if natural <= SEA_LEVEL + 4 {
                    continue; // only interested in a river cutting through real dry land
                }
                let carved = gen.effective_height(wx, wz);
                let carve = natural - carved;
                if carve > best_carve {
                    best_carve = carve;
                }
                if carve < 6 {
                    continue; // not a substantially-carved river cell
                }
                let chunk = gen.generate(cx, cz);
                let (_, top) = column_top(&chunk, 8, 8);
                assert!(
                    top == water || top == ice,
                    "a deeply-carved column at ({wx},{wz}) should have generated as water/ice"
                );
                found = true;
                break 'search;
            }
        }
        assert!(found, "no substantially-carved river column found (deepest seen: {best_carve})");
    }

    #[test]
    fn snow_biome_columns_generate_snow_at_low_altitude_not_grass() {
        let reg = BlockRegistry::with_defaults();
        let snow = reg.id("snow");
        let seed = 7;
        let gen = TerrainGenerator::new(seed, &reg);
        let noise = crate::biome::region_noise_for_seed(seed);

        // A real Snow-biome column *below* the pre-existing mountain-
        // altitude SNOW_LINE, so this can only pass because of the biome
        // itself, not the altitude cap that already put snow on
        // mountaintops before this feature existed.
        let mut checked = false;
        'search: for cx in -20..20 {
            for cz in -20..20 {
                let wx = cx * CHUNK_SIZE + 8;
                let wz = cz * CHUNK_SIZE + 8;
                if crate::biome::biome_at(&noise, wx, wz) != crate::biome::Biome::Snow {
                    continue;
                }
                if gen.effective_height(wx, wz) >= SNOW_LINE {
                    continue; // would trivially be snow via altitude anyway
                }
                let chunk = gen.generate(cx, cz);
                let (_, top) = column_top(&chunk, 8, 8);
                assert_eq!(top, snow, "snow-biome column at ({wx},{wz}) did not generate snow");
                checked = true;
                break 'search;
            }
        }
        assert!(checked, "no low-altitude Snow-biome column found in the sampled area");
    }

    #[test]
    fn snow_biome_lake_surfaces_generate_as_ice_not_water() {
        let reg = BlockRegistry::with_defaults();
        let ice = reg.id("ice");
        let seed = 7;
        let gen = TerrainGenerator::new(seed, &reg);
        let noise = crate::biome::region_noise_for_seed(seed);

        let mut found = false;
        'search: for cx in -15..15 {
            for cz in -15..15 {
                let chunk = gen.generate(cx, cz);
                for z in 0..CS {
                    for x in 0..CS {
                        let wx = cx * CHUNK_SIZE + x as i32;
                        let wz = cz * CHUNK_SIZE + z as i32;
                        if crate::biome::biome_at(&noise, wx, wz) != crate::biome::Biome::Snow {
                            continue;
                        }
                        if chunk.blocks[block_index(x, SEA_LEVEL as usize, z)] == ice {
                            found = true;
                            break 'search;
                        }
                    }
                }
            }
        }
        assert!(found, "no snow-biome lake surface generated as ice in the sampled area");
    }

    /// The topmost non-air block in column `(x, z)`, as `(y, id)` - for
    /// tests that care about what a real generated column's surface
    /// actually ended up as, not what `surface_height` alone would predict
    /// (which doesn't include `generate`'s biome height boost).
    fn column_top(chunk: &GeneratedChunk, x: usize, z: usize) -> (i32, BlockId) {
        for y in (0..H).rev() {
            let id = chunk.blocks[block_index(x, y, z)];
            if id != AIR {
                return (y as i32, id);
            }
        }
        panic!("column ({x}, {z}) is entirely air");
    }
}

#[cfg(test)]
mod hydrology_measure {
    use super::*;

    fn valley_height(grid: usize) -> Vec<f32> {
        let p = grid + 2;
        let cx = (grid as f32 + 1.0) / 2.0; // centerline between the two middle columns
        let mut height = vec![0f32; p * p];
        for gz in 0..p {
            for gx in 0..p {
                let side = 5.0 * (gx as f32 - cx).abs();
                let along = gz as f32;
                let tiebreak = 0.001 * gx as f32;
                height[gx + p * gz] = 100.0 + side + along + tiebreak;
            }
        }
        height
    }

    /// A synthetic V-shaped valley (steep side slope funnels every column
    /// toward one center channel, shallow along-valley slope drains that
    /// channel toward one edge) exercises the real algorithm end to end
    /// without depending on procedural noise - the same "give tests a way
    /// to inject the controlled input a real entry point resolves
    /// automatically" split as `atlas::build_atlas_from_dir`.
    #[test]
    fn carve_from_heights_channels_every_column_into_one_widening_stream() {
        let grid = 16;
        let p = grid + 2;
        let height = valley_height(grid);
        let carve = carve_from_heights(&height, grid);
        let center_gx = 8; // where valley_height's side slope bottoms out

        // Nothing at the very top row can have received any inflow yet -
        // every interior cell there is its own source, below the river
        // threshold.
        for gx in 1..p - 1 {
            assert_eq!(carve[gx + p * (p - 2)], 0.0, "top row should carve nothing at gx={gx}");
        }

        // Off-channel columns never accumulate enough to carve at all.
        for gz in 1..p - 1 {
            for gx in 1..p - 1 {
                if gx == center_gx {
                    continue;
                }
                assert_eq!(carve[gx + p * gz], 0.0, "gx={gx} gz={gz} should be off-channel");
            }
        }

        // The channel itself widens (carves deeper) every step closer to
        // its outlet, as more upstream columns have had a chance to merge
        // into it - a real accumulating stream, not a fixed-width ditch.
        let mut prev = 0.0f32;
        let mut saw_carving = false;
        for gz in (1..p - 1).rev() {
            let c = carve[center_gx + p * gz];
            if c > 0.0 || prev > 0.0 {
                assert!(c >= prev, "carve should not shrink moving downstream (gz={gz})");
                saw_carving = true;
            }
            prev = c;
        }
        assert!(saw_carving, "expected the center channel to carve somewhere");
        // And it should actually reach its full, non-trivial depth by the
        // outlet, not just barely poke above zero.
        assert!(carve[center_gx + p * 1] > 5.0);
    }

    /// A perfectly flat height field has no downhill direction anywhere,
    /// so nothing should ever accumulate enough flow to carve - water has
    /// nowhere to flow, there is no river.
    #[test]
    fn carve_from_heights_carves_nothing_on_flat_ground() {
        let grid = 10;
        let p = grid + 2;
        let height = vec![50.0f32; p * p];
        let carve = carve_from_heights(&height, grid);
        assert!(carve.iter().all(|&c| c == 0.0));
    }
}
