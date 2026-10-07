//! Procedural terrain generator. Runs on the async compute task pool.
//! Deterministic per (seed, cx, cz) with no cross-chunk dependencies, so
//! chunks can generate in any order on any thread. Trees keep a 2-block
//! margin from chunk borders so features never spill across chunks.
//!
//! **Rivers are the one exception to "no cross-chunk dependencies."** A
//! river has to know what's uphill of it, arbitrarily far away, so its shape
//! can't be decided per-chunk in isolation - see `drainage.rs`. Chunks still
//! generate independently of each other in the sense that matters (any
//! order, any thread, no chunk waits on another chunk), but they share one
//! lazily-filled, memoized drainage network (`TerrainGenerator::drainage`).
//!
//! To customize generation, swap the generator constructed in
//! `world::compile_content` for your own.

use crate::biome::{self, Biome};
use crate::blocks::{BlockId, BlockRegistry, Transparency, AIR, AXIS_Y, FLUID_SOURCE};
use crate::config::{block_index, CHUNK_SIZE, CS, H, SEA_LEVEL, WORLD_HEIGHT};
use crate::light::{LightCell, MAX_LIGHT};
use crate::noise::{hash2, hash3, SimplexNoise};

/// Altitude at or above which any column - in any biome - gets a snow cap.
/// The `Mountain` biome's own surface (bare stone) sits *below* this on a
/// mountain's upper slopes, so a range reads as grass -> bare rock -> snow.
const SNOW_LINE: i32 = 50;

/// How far a fully `Biome::drier` column's terrain gets pushed above the
/// plain baseline, at full `biome::drier_strength` - chosen so noticeably
/// fewer columns dip below `SEA_LEVEL` there (see `generate`'s flooding
/// loop) without visibly changing the mountain/plains shape language this
/// generator already has (this only ever *adds* to whatever `detail`/
/// `mountain` noise already produced).
const DRIER_HEIGHT_BOOST: f64 = 6.0;

/// Very-low-frequency noise stream deciding where large, genuinely
/// *connected* ocean basins sit, entirely separate from `mountain`/
/// `terrain`'s local relief - see `TerrainGenerator::base_height`'s ocean
/// blend. Low enough frequency that a "this column is ocean" verdict stays
/// the same across many chunks in a row, which is what turns what used to
/// be scattered small sub-sea-level dips into real seas that actually
/// separate landmasses, instead of one giant landmass with puddles in it.
const CONTINENT_SCALE: f64 = 0.0002;
/// `continent_value` is roughly -1..=1 and centered on 0; a threshold near
/// zero is what actually produces multiple real, separated landmasses
/// (verified by `oceans_split_land_into_multiple_masses_separated_by_real_seas`).
/// Pushing it further negative (so land is the clear majority) instead
/// makes land the one *connected* mass with the ocean fragmented into many
/// small inland seas, the mirror image of the original "one giant
/// landmass" complaint this exists to fix.
const OCEAN_THRESHOLD: f32 = -0.02;
/// How much of `continent_value`'s own range a *gentle* coast's land->ocean
/// blend spans - a coastline that fades in over a few hundred blocks
/// instead of snapping at an exact contour line.
const COAST_BLEND: f32 = 0.06;
/// The land->ocean blend width a *fully steep* coast uses instead of
/// `COAST_BLEND` - narrow enough (the continent field changes by roughly
/// 0.001 per block) that the drop from the land to the sea floor happens
/// over a handful of blocks: a cliff, not a beach.
const CLIFF_BLEND: f32 = 0.0013;
/// How much higher a fully steep coast's whole landmass sits than it would
/// otherwise - what makes its cliffs *tower* over the water rather than
/// being a short step down from land barely above sea level. Applied to the
/// whole steep-coast region, not just a strip along the shore, so it reads
/// as high ground meeting the sea rather than a coastal ridge that would
/// also dam every river behind it.
const CLIFF_UPLIFT: f64 = 14.0;
/// Feature size of the coast-style noise (which stretches of coastline are
/// cliffs and which are beaches) - a bit finer than `CONTINENT_SCALE`, so a
/// single landmass can have both kinds of shore.
const COAST_STYLE_SCALE: f64 = 0.0012;
/// Target height for the deepest ocean floor - well below `SEA_LEVEL`
/// (26) so open ocean reads as a real body of water, not a shallow puddle.
const DEEP_OCEAN_FLOOR: f64 = 9.0;

/// Feature size of the mountain-*range* mask - where ranges are at all.
/// Deliberately its own low-frequency layer (`TerrainGenerator::
/// mountainness`), not baked into the plains formula: the `Mountain`
/// biome's altitude zones (`biome::zoned_biome`) and `/locate feature
/// mountain` read exactly the same mask the terrain was raised by.
const MOUNTAIN_SCALE: f64 = 0.0035;
/// Feature size of the ridged peak noise layered on top of a range.
const PEAK_SCALE: f64 = 0.012;
/// How far a range's core is lifted above the plains it rises out of.
const MOUNTAIN_LIFT: f64 = 15.0;
/// Extra height a ridge line adds on top of `MOUNTAIN_LIFT`.
const PEAK_AMPLITUDE: f64 = 16.0;

/// How strongly water is steered toward the sea, in routing-height blocks
/// per unit of `continent_value` above the coastline. Added to real terrain
/// only for *routing* (`drainage::Landscape::routing_height`), never to the
/// terrain itself: it gives every point on land a gentle overall tilt
/// toward its nearest coast, so a river crossing flat ground keeps heading
/// seaward instead of dead-ending in the first small dip, while steep
/// ground (mountains) still routes by its own real slope.
const SEAWARD_TILT: f64 = 750.0;

/// Radius (blocks) of the ring `routing_height` averages terrain over.
const ROUTING_SMOOTHING: f64 = 40.0;

/// Upstream catchment (in `drainage::CELL`-sized cells) a point needs
/// before it counts as a river at all - below this it's hillside runoff.
/// ~48 cells is roughly a 110-block square of land draining through one
/// spot, which leaves about one cell in ten a river: a network you keep
/// running into while exploring, without a stream every few steps.
const RIVER_THRESHOLD: f32 = 48.0;
/// Catchment at which a river reaches its full width/depth. Sizes grow
/// logarithmically between `RIVER_THRESHOLD` and this, so headwaters stay
/// thin for a long way and only the trunk of a large basin gets wide.
const MAX_ACCUM_FOR_FULL_SIZE: f32 = 2000.0;
/// Half-width (blocks) of the wet channel for the smallest and largest
/// rivers - a 2-block stream up to a ~16-block-wide lowland river.
const RIVER_HALF_WIDTH: (f64, f64) = (1.0, 8.0);
/// Depth (blocks, at the centerline) of the wet channel below the water
/// surface for the smallest and largest rivers.
const RIVER_DEPTH: (f64, f64) = (1.0, 5.0);
/// The most a river's banks ever stand above its water surface - a river
/// "dug in a little, starting to become a canyon", not a canyon yet.
/// `0` is a flush river whose banks are level with the water.
const MAX_INCISION: f64 = 6.0;
/// Feature size of the river-character noise deciding how incised a river
/// is - large, so one stretch of river keeps a consistent character for a
/// long way instead of flipping between flush and dug-in every few blocks.
const RIVER_CHARACTER_SCALE: f64 = 0.0015;
/// How far (blocks) past the wet channel a river still lowers the
/// terrain toward its banks, for a flush river and a fully incised one - a
/// flush river sits in a wide, gentle valley; an incised one cuts a
/// narrower, steeper one.
const VALLEY_WIDTH: (f64, f64) = (14.0, 5.0);
/// How many drainage cells around a column's own `river_sample` checks for
/// river segments - enough to cover the widest river plus its widest
/// valley plus a node's jitter (`8 + 14 + 6` blocks < 2 cells; 3 for margin).
const SEGMENT_SEARCH_CELLS: i32 = 3;

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Where `soft_ceiling` starts bending terrain over.
const SOFT_CEILING_START: f64 = 44.0;

/// Eases heights above `SOFT_CEILING_START` asymptotically toward the
/// world's build ceiling (`WORLD_HEIGHT - 8`) instead of letting the final
/// hard clamp flatten every tall peak into the same plateau - summits stay
/// pointed, just compressed. Identity below the start, so ordinary terrain
/// is untouched.
fn soft_ceiling(h: f64) -> f64 {
    if h <= SOFT_CEILING_START {
        return h;
    }
    let room = (WORLD_HEIGHT - 8) as f64 - 0.5 - SOFT_CEILING_START;
    SOFT_CEILING_START + room * (1.0 - (-(h - SOFT_CEILING_START) / room).exp())
}

fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// What a river looks like at one world column: distance from the nearest
/// river segment's centerline, and that river's properties at the closest
/// point on it.
struct RiverSample {
    distance: f64,
    water: f64,
    half_width: f64,
    depth: f64,
    incision: f64,
    valley_width: f64,
}

/// What a single world column generates as: its solid surface height, and
/// how high water stands over it, if at all.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColumnProfile {
    /// The topmost solid block's `y`.
    pub height: i32,
    /// The topmost water block's `y`, or `None` for a dry column. Either a
    /// river's surface (which can sit well above `SEA_LEVEL` - rivers run
    /// downhill from wherever they start) or `SEA_LEVEL` itself for any
    /// column whose ground is below it.
    pub water_top: Option<i32>,
    /// Inside a river's wet channel (as opposed to sea/lake water).
    pub in_river: bool,
    /// A dry column right at a river's edge, low enough to be its bank -
    /// generated as sand rather than grass.
    pub river_bank: bool,
    /// Within reach of a river's channel or valley at all - caves are kept
    /// out of these, so a river never drains into one.
    pub near_river: bool,
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
    /// The mountain-*range* mask - see `MOUNTAIN_SCALE` and `mountainness`.
    mountain: SimplexNoise,
    /// Ridged peak noise layered onto a range - see `PEAK_SCALE`.
    peaks: SimplexNoise,
    /// Continent/ocean shaping noise - see `CONTINENT_SCALE`'s doc comment.
    continent: SimplexNoise,
    /// Which stretches of coast are cliffs - see `coast_steepness`.
    coast: SimplexNoise,
    /// How incised each river is - see `river_incision`.
    river_character: SimplexNoise,
    cave_a: SimplexNoise,
    cave_b: SimplexNoise,
    /// Which *region* biome a column belongs to before altitude zones - see
    /// `biome.rs`'s module docs and `biome_at`.
    biome: SimplexNoise,
    /// The world-wide drainage network rivers are drawn from - see
    /// `drainage.rs`. Lazily filled and memoized as chunks ask; the one
    /// piece of terrain generation that isn't purely a function of its own
    /// chunk coordinate - see this module's own doc comment.
    drainage: crate::drainage::Network,
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
            peaks: SimplexNoise::new(seed ^ 0x6c8e_9cf5),
            continent: SimplexNoise::new(seed ^ 0x27d4_eb2f),
            coast: SimplexNoise::new(seed ^ 0x1656_67b1),
            river_character: SimplexNoise::new(seed ^ 0xd3a2_646c),
            cave_a: SimplexNoise::new(seed ^ 0x85ebca6b),
            cave_b: SimplexNoise::new(seed ^ 0xc2b2ae35),
            biome: biome::region_noise_for_seed(seed),
            drainage: crate::drainage::Network::new(seed),
        }
    }

    fn continent_value(&self, wx: i32, wz: i32) -> f32 {
        (self.continent.fbm2(wx as f64 * CONTINENT_SCALE, wz as f64 * CONTINENT_SCALE, 3) as f32)
            .clamp(-1.0, 1.0)
    }

    /// How strongly world column `(x, z)` sits inside a mountain range,
    /// `0.0` (plains) to `1.0` (a range's core). The one definition of
    /// "where mountains are": `land_relief` raises the terrain by it,
    /// `biome::zoned_biome` only applies altitude zones (and so the
    /// `Mountain` biome) where it's strong, and `/locate feature mountain`
    /// searches for it - so the landform and the biome on top of it can
    /// never disagree about where a range is.
    pub fn mountainness(&self, wx: i32, wz: i32) -> f64 {
        let m = self.mountain.fbm2(wx as f64 * MOUNTAIN_SCALE, wz as f64 * MOUNTAIN_SCALE, 3) * 0.5 + 0.5;
        smoothstep((m - 0.55) / 0.2)
    }

    /// How much of a cliff the coast near `(x, z)` is, `0.0` (a gentle,
    /// beach-like shore - exactly the coast this generator had before
    /// cliffs existed) to `1.0` (land held high right up to a sheer drop
    /// into deep water).
    fn coast_steepness(&self, wx: i32, wz: i32) -> f64 {
        let n = self.coast.fbm2(wx as f64 * COAST_STYLE_SCALE, wz as f64 * COAST_STYLE_SCALE, 2);
        // A narrow window, so a stretch of coast is decisively one or the
        // other and only briefly in between - a half-steep coast is just a
        // slightly short beach, not a recognisable cliff.
        smoothstep((n - 0.2) / 0.12)
    }

    /// How far a river at `(x, z)` has cut below its banks, `0.0` (flush -
    /// banks level with the water) to `MAX_INCISION`. Sampled per river
    /// node of the drainage network, then interpolated along the
    /// river, so a river's character drifts gradually along its length.
    fn river_incision(&self, x: f64, z: f64) -> f64 {
        let n = self.river_character.fbm2(x * RIVER_CHARACTER_SCALE, z * RIVER_CHARACTER_SCALE, 2);
        MAX_INCISION * smoothstep((n + 0.1) / 0.5)
    }

    /// Plains relief plus a mountain range wherever `mountainness` says
    /// there is one - continent-blind on purpose, see `base_height` for how
    /// it's combined with the ocean.
    fn land_relief(&self, wx: i32, wz: i32) -> f64 {
        let detail = self.terrain.fbm2(wx as f64 * 0.011, wz as f64 * 0.011, 4);
        let plains = 27.0 + detail * 5.0;
        let range = self.mountainness(wx, wz);
        if range <= 0.0 {
            return plains;
        }
        // Ridged noise: `1 - |n|` peaks exactly where the noise crosses
        // zero, which runs in long connected lines - ridges, rather than
        // the round hills plain fbm makes. Squared to sharpen the crest.
        let ridge = 1.0 - self.peaks.fbm2(wx as f64 * PEAK_SCALE, wz as f64 * PEAK_SCALE, 4).abs();
        let mountain = MOUNTAIN_LIFT + PEAK_AMPLITUDE * ridge * ridge + detail * 6.0;
        plains + range * mountain
    }

    /// Land relief meeting the ocean - the terrain before any biome boost
    /// or river, unclamped.
    fn base_height(&self, wx: i32, wz: i32) -> f64 {
        let steep = self.coast_steepness(wx, wz);
        // The soft ceiling goes on exactly once, over everything that can
        // raise land - applying it inside `land_relief` too compressed every
        // peak twice and quietly kept ranges below the snow line.
        let land = soft_ceiling(self.land_relief(wx, wz) + CLIFF_UPLIFT * steep);
        // Interpolated geometrically, not linearly: the blend width spans
        // ~45x between a beach and a cliff, and a linear mix would leave
        // even a mostly-steep coast hundreds of blocks wide.
        let blend = (COAST_BLEND as f64).powf(1.0 - steep) * (CLIFF_BLEND as f64).powf(steep);
        let ocean_t = ((OCEAN_THRESHOLD - self.continent_value(wx, wz)) as f64 / blend).clamp(0.0, 1.0);
        if ocean_t <= 0.0 {
            return land;
        }
        // A bit of the existing detail noise, scaled down, keeps the
        // seafloor from reading as a perfectly flat plate.
        let seafloor_detail = self.terrain.fbm2(wx as f64 * 0.02, wz as f64 * 0.02, 3) * 3.0;
        lerp(land, DEEP_OCEAN_FLOOR + seafloor_detail, ocean_t)
    }

    /// The continent/coast/mountain terrain alone, with no biome boost and
    /// no rivers - see `effective_height` for what actually generates.
    pub fn surface_height(&self, wx: i32, wz: i32) -> i32 {
        (self.base_height(wx, wz).floor() as i32).clamp(2, WORLD_HEIGHT - 8)
    }

    /// The terrain rivers flow over: `base_height` plus the `Biome::drier`
    /// boost, before any river has cut into it. What `drainage.rs`
    /// routes water across, and what a river's banks are measured against.
    fn natural_height(&self, wx: i32, wz: i32) -> f64 {
        let boost = biome::drier_strength(&self.biome, wx, wz) as f64 * DRIER_HEIGHT_BOOST;
        (self.base_height(wx, wz) + boost).clamp(2.0, (WORLD_HEIGHT - 8) as f64)
    }

    /// The nearest river to `(x, z)`, if one is within reach: the closest
    /// segment between a river node and the node it drains into, with the
    /// river's properties interpolated along it.
    fn river_sample(&self, wx: i32, wz: i32) -> Option<RiverSample> {
        use crate::drainage::CELL;
        let net = &self.drainage;
        let home = (wx.div_euclid(CELL), wz.div_euclid(CELL));
        let (px, pz) = (wx as f64 + 0.5, wz as f64 + 0.5);

        // One end of a segment: (position, water, half-width, depth, incision).
        let point = |cell: crate::drainage::Cell| {
            let (x, z) = net.node_pos(cell);
            let accumulation = net.accumulation(self, cell);
            let size = ((accumulation / RIVER_THRESHOLD).ln() / (MAX_ACCUM_FOR_FULL_SIZE / RIVER_THRESHOLD).ln())
                .clamp(0.0, 1.0) as f64;
            (
                (x, z),
                net.water_level(self, cell, RIVER_THRESHOLD) as f64,
                lerp(RIVER_HALF_WIDTH.0, RIVER_HALF_WIDTH.1, size),
                lerp(RIVER_DEPTH.0, RIVER_DEPTH.1, size),
                self.river_incision(x, z),
            )
        };

        let mut best: Option<(f64, RiverSample)> = None;
        for dz in -SEGMENT_SEARCH_CELLS..=SEGMENT_SEARCH_CELLS {
            for dx in -SEGMENT_SEARCH_CELLS..=SEGMENT_SEARCH_CELLS {
                let cell = (home.0 + dx, home.1 + dz);
                if net.accumulation(self, cell) <= RIVER_THRESHOLD {
                    continue;
                }
                let a = point(cell);
                // Downstream of a river cell is always a river cell too (it
                // drains at least as much); a river ending at the sea or a
                // pit is a segment of zero length - a round end.
                let b = net.downstream(self, cell).map_or(a, point);
                let ((ax, az), (bx, bz)) = (a.0, b.0);
                let (sx, sz) = (bx - ax, bz - az);
                let len2 = sx * sx + sz * sz;
                let t = if len2 > 0.0 { (((px - ax) * sx + (pz - az) * sz) / len2).clamp(0.0, 1.0) } else { 0.0 };
                let d = ((px - ax - sx * t).powi(2) + (pz - az - sz * t).powi(2)).sqrt();
                if best.as_ref().is_some_and(|(bd, _)| *bd <= d) {
                    continue;
                }
                let incision = lerp(a.4, b.4, t);
                best = Some((
                    d,
                    RiverSample {
                        distance: d,
                        water: lerp(a.1, b.1, t),
                        half_width: lerp(a.2, b.2, t),
                        depth: lerp(a.3, b.3, t),
                        incision,
                        valley_width: lerp(VALLEY_WIDTH.0, VALLEY_WIDTH.1, incision / MAX_INCISION),
                    },
                ));
            }
        }
        best.map(|(_, sample)| sample)
    }

    /// One column's profile *before* the levee rule in `column_profile` -
    /// what the river and sea alone make of it.
    fn raw_column(&self, wx: i32, wz: i32) -> ColumnProfile {
        let natural = self.natural_height(wx, wz);
        let mut h = natural;
        let mut river_water = None;
        let (mut river_bank, mut near_river) = (false, false);

        if let Some(r) = self.river_sample(wx, wz) {
            let water = r.water.floor();
            let half_width = r.half_width;
            if r.distance <= half_width + r.valley_width + 1.0 {
                near_river = true;
            }
            if half_width >= 0.5 && r.distance <= half_width {
                // The wet channel: a rounded bed, deepest at the centerline,
                // always at least one block below the water.
                let k = 1.0 - (r.distance / half_width).powi(2);
                h = natural.min(water - 1.0 - (r.depth - 1.0) * k);
                river_water = Some(water as i32);
            } else {
                // The valley: terrain within reach is pulled down toward the
                // banks (`water + incision`), right at the water's edge and
                // easing back to untouched terrain over `valley_width`. A
                // flush river (incision 0) has banks level with its water;
                // an incised one has a wall `incision` blocks tall there.
                let banks = water + r.incision;
                if natural > banks {
                    let ease = smoothstep((r.distance - half_width) / r.valley_width);
                    h = lerp(banks, natural, ease);
                }
                river_bank = half_width >= 0.5 && r.distance <= half_width + 2.0 && h <= water + 1.0;
            }
        }

        let height = (h.floor() as i32).clamp(2, WORLD_HEIGHT - 8);
        let sea = (height < SEA_LEVEL).then_some(SEA_LEVEL);
        ColumnProfile {
            height,
            water_top: river_water.max(sea),
            in_river: river_water.is_some(),
            river_bank,
            near_river,
        }
    }

    /// What world column `(x, z)` actually generates as - terrain, rivers,
    /// sea - see `ColumnProfile`.
    ///
    /// River water can sit above sea level, so unlike the sea it isn't
    /// automatically held in by the terrain around it. The **levee rule**
    /// guarantees it is: a dry column next to a river is raised to at least
    /// every adjacent column's water surface, so water never stands beside
    /// open air on dry land. The only place two water surfaces meet at
    /// different heights is *within* a river, where it steps down -
    /// rapids, intentionally.
    pub fn column_profile(&self, wx: i32, wz: i32) -> ColumnProfile {
        let mut col = self.raw_column(wx, wz);
        if col.water_top.is_none() && col.near_river {
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                if let Some(top) = self.raw_column(wx + dx, wz + dz).water_top {
                    col.height = col.height.max(top);
                }
            }
        }
        col
    }

    /// The solid surface height world column `(x, z)` really generates at -
    /// `column_profile(..).height`. Anything that needs the *real* height a
    /// column generated at - including this generator's own tests - has to
    /// go through this, not `surface_height`.
    pub fn effective_height(&self, wx: i32, wz: i32) -> i32 {
        self.column_profile(wx, wz).height
    }

    /// The biome world column `(x, z)` really is: its region biome
    /// (`biome::region_biome_at`), replaced by an altitude zone's biome on
    /// a mountain range's upper slopes (`biome::zoned_biome`). The one
    /// place a column's full biome is decided - worldgen and `world.rs`'s
    /// runtime freezing rule both ask this, never the region noise alone.
    pub fn biome_at(&self, wx: i32, wz: i32) -> Biome {
        self.biome_with_height(wx, wz, self.effective_height(wx, wz))
    }

    /// `biome_at`, for a caller that already knows the column's height.
    fn biome_with_height(&self, wx: i32, wz: i32, height: i32) -> Biome {
        biome::zoned_biome(biome::region_biome_at(&self.biome, wx, wz), self.mountainness(wx, wz), height)
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
                let col = self.column_profile(wx, wz);
                let h = col.height;
                heights[x + CS * z] = h;
                let biome = self.biome_with_height(wx, wz, h);
                let underwater = col.water_top.is_some();

                let beach = h <= SEA_LEVEL + 1 || col.river_bank;
                let snowy = h >= SNOW_LINE;
                // Precedence, top to bottom: anything under water is a sand
                // bed; the Mountain biome is bare rock up to the snow line;
                // the Snow biome is snow (over beaches too); then beaches
                // and river banks, the altitude snow cap, and grass.
                let top_id = if underwater {
                    ids.sand
                } else if biome == Biome::Mountain {
                    if snowy { ids.snow } else { ids.stone }
                } else if biome == Biome::Snow {
                    ids.snow
                } else if beach {
                    ids.sand
                } else if snowy {
                    ids.snow
                } else {
                    ids.grass
                };
                let fill_id = if underwater || beach {
                    ids.sand
                } else if biome == Biome::Mountain {
                    ids.stone
                } else {
                    ids.dirt
                };
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
                // Flood up to this column's water surface - the sea, or a
                // river standing above it. In a biome where water freezes
                // (`Biome::freezes_water`), the exposed top layer (the only
                // one with air directly above it) generates as ice instead,
                // matching what `world.rs`'s `freeze_exposed_water` would
                // convert it to anyway if a later change exposed a
                // still-liquid top layer to air.
                if let Some(top) = col.water_top {
                    for y in (h + 1)..=top {
                        blocks[base + y as usize] =
                            if y == top && biome.freezes_water() { ids.ice } else { ids.water };
                    }
                    // Gravel patches on sea and river beds.
                    if hash2(wx, wz, seed ^ 0x1234) < 0.3 {
                        blocks[base + h as usize] = ids.gravel;
                    }
                } else if biome == Biome::Mountain && hash2(wx, wz, seed ^ 0x4d7e) < 0.15 {
                    // Scree on bare mountain rock.
                    if top_id == ids.stone {
                        blocks[base + h as usize] = ids.gravel;
                    }
                }

                // Carve "spaghetti" caves on dry land columns - kept away
                // from water, and from river valleys, so a cave never opens
                // under a sea floor or beside a river and drains it.
                if !underwater && !col.near_river && h > SEA_LEVEL + 1 {
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

impl crate::drainage::Landscape for TerrainGenerator {
    fn routing_height(&self, x: f64, z: f64) -> f64 {
        let (wx, wz) = (x.floor() as i32, z.floor() as i32);
        // Signed, not clamped at the coast: the tilt keeps sloping on past
        // the shoreline, so flat coastal ground just inside the ocean zone
        // (where the sea hasn't actually started yet) still drains to it
        // instead of forming a dead-flat trap one step from the water.
        let inland = (self.continent_value(wx, wz) - OCEAN_THRESHOLD) as f64;
        // The terrain as water "sees" it at the scale of a whole valley:
        // averaged over a ring `ROUTING_SMOOTHING` blocks out, so a dip a
        // few blocks across can't trap a river - only a real basin can.
        let r = ROUTING_SMOOTHING;
        let mut sum = self.natural_height(wx, wz);
        for k in 0..8 {
            let a = k as f64 * std::f64::consts::FRAC_PI_4;
            sum += self.natural_height((x + r * a.cos()).floor() as i32, (z + r * a.sin()).floor() as i32);
        }
        sum / 9.0 + SEAWARD_TILT * inland
    }

    fn ground_height(&self, x: f64, z: f64) -> f64 {
        self.natural_height(x.floor() as i32, z.floor() as i32)
    }

    /// The open ocean only - a continent's sea, not every low dip in the
    /// plains that happens to flood. Rivers flow *through* inland lakes
    /// (their surface simply meets the lake's at sea level) and keep going;
    /// they only end where they actually reach the sea.
    fn is_sea(&self, x: f64, z: f64) -> bool {
        let (wx, wz) = (x.floor() as i32, z.floor() as i32);
        self.continent_value(wx, wz) < OCEAN_THRESHOLD && self.natural_height(wx, wz) < SEA_LEVEL as f64
    }

    fn incision(&self, x: f64, z: f64) -> f64 {
        self.river_incision(x, z)
    }

    fn base_level(&self) -> f64 {
        SEA_LEVEL as f64
    }
}

/// A landscape feature `/locate feature <name>` can search for - see
/// `Feature::matches_column` for what each one actually means on this
/// generator's terrain, and `TerrainGenerator::locate_feature` for the
/// search itself. Mirrors `Biome`'s own `ALL`/`name`/`parse` shape
/// (`biome.rs`) so both qualifiers plug into `/locate`'s argument
/// autocomplete and parsing the exact same way.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Feature {
    River,
    Ocean,
    Mountain,
}

impl Feature {
    pub const ALL: [Feature; 3] = [Feature::River, Feature::Ocean, Feature::Mountain];

    /// The lowercase name a player types to refer to this feature in chat
    /// commands - the inverse of `Feature::parse`.
    pub fn name(self) -> &'static str {
        match self {
            Feature::River => "river",
            Feature::Ocean => "ocean",
            Feature::Mountain => "mountain",
        }
    }

    /// Parses a player-typed feature name (case-insensitive), the inverse
    /// of `Feature::name`.
    pub fn parse(s: &str) -> Option<Feature> {
        Feature::ALL.into_iter().find(|f| f.name().eq_ignore_ascii_case(s))
    }

    /// Whether world column `(wx, wz)` counts as this feature, on `gen`'s
    /// own terrain - `terrain.rs` is the one source of truth for what a
    /// river/ocean/mountain actually is, so `/locate` never maintains a
    /// second, potentially-disagreeing definition.
    fn matches_column(self, gen: &TerrainGenerator, wx: i32, wz: i32) -> bool {
        match self {
            // Standing in a river's wet channel - the same channel
            // `generate` floods, not an approximation of it.
            Feature::River => gen.column_profile(wx, wz).in_river,
            Feature::Ocean => gen.surface_height(wx, wz) < SEA_LEVEL,
            // The landform, not the biome on top of it: anywhere inside a
            // real range (`mountainness`), whatever altitude zone this
            // particular column falls in. `/locate biome mountain` is the
            // one that finds the desolate peaks.
            Feature::Mountain => gen.mountainness(wx, wz) >= biome::MOUNTAIN_RANGE_THRESHOLD,
        }
    }
}

/// How far out a `/locate` search goes before giving up, in blocks.
/// Generous relative to every noise scale a search predicate could be
/// built on here (`biome::REGION_SCALE` 640; land rarely sits more than
/// ~2000 blocks from the sea at `CONTINENT_SCALE`), so
/// a real search essentially always finds its target well inside this
/// bound - it exists only to guarantee `/locate` terminates instead of
/// promising to find something that could structurally not exist nearby.
const LOCATE_MAX_RADIUS: i32 = 6000;
/// Spacing between sampled columns while searching, in blocks. A `/locate`
/// result is meant to get the player close, not land on the single exact
/// nearest block, so sampling coarser than every block is both cheap and
/// plenty precise for that purpose.
const LOCATE_STEP: i32 = 16;

/// Searches outward from `(origin_x, origin_z)` in expanding square rings
/// (each `LOCATE_STEP` further out than the last) for the nearest sampled
/// column `is_match` accepts, returning its world `(x, z)` - or `None` if
/// nothing matched within `LOCATE_MAX_RADIUS`. Shared by every `/locate`
/// qualifier (biome, feature) so each just supplies its own predicate
/// instead of reimplementing the search.
///
/// Stops at the first ring that has any match at all, picking that ring's
/// own closest-by-real-distance hit - not a perfect global nearest-neighbor
/// search (a slightly closer match can in principle sit just inside the
/// *next* ring's near edge), but exact enough to actually get a player
/// to a real nearby example, which is all a locate command promises.
fn locate_nearest(origin_x: i32, origin_z: i32, is_match: impl Fn(i32, i32) -> bool) -> Option<(i32, i32)> {
    let ox = origin_x.div_euclid(LOCATE_STEP) * LOCATE_STEP;
    let oz = origin_z.div_euclid(LOCATE_STEP) * LOCATE_STEP;
    if is_match(ox, oz) {
        return Some((ox, oz));
    }

    let mut r = LOCATE_STEP;
    while r <= LOCATE_MAX_RADIUS {
        let mut best: Option<(i32, i32, i64)> = None;
        for dz in (-r..=r).step_by(LOCATE_STEP as usize) {
            for dx in (-r..=r).step_by(LOCATE_STEP as usize) {
                if dx.abs() != r && dz.abs() != r {
                    continue; // interior of the square - already checked on a smaller ring
                }
                let (x, z) = (ox + dx, oz + dz);
                if !is_match(x, z) {
                    continue;
                }
                let d2 = i64::from(dx) * i64::from(dx) + i64::from(dz) * i64::from(dz);
                if best.is_none_or(|(_, _, best_d2)| d2 < best_d2) {
                    best = Some((x, z, d2));
                }
            }
        }
        if let Some((x, z, _)) = best {
            return Some((x, z));
        }
        r += LOCATE_STEP;
    }
    None
}

impl TerrainGenerator {
    /// `/locate feature <name>`'s search: the nearest column (to
    /// `(origin_x, origin_z)`) that qualifies as `feature` on this
    /// generator's own terrain, or `None` if nothing did within
    /// `LOCATE_MAX_RADIUS`.
    pub fn locate_feature(&self, feature: Feature, origin_x: i32, origin_z: i32) -> Option<(i32, i32)> {
        locate_nearest(origin_x, origin_z, |x, z| feature.matches_column(self, x, z))
    }

    /// `/locate biome <name>`'s search: the nearest column whose full
    /// biome (`biome_at` - region *and* altitude zone) is `biome`, or
    /// `None` if nothing was within `LOCATE_MAX_RADIUS`. Checks the cheap
    /// region noise first and only computes a column's real height when
    /// altitude could actually change the answer (inside a mountain range).
    pub fn locate_biome(&self, target: Biome, origin_x: i32, origin_z: i32) -> Option<(i32, i32)> {
        locate_nearest(origin_x, origin_z, |x, z| {
            let region = biome::region_biome_at(&self.biome, x, z);
            let range = self.mountainness(x, z);
            if range < biome::MOUNTAIN_RANGE_THRESHOLD {
                return region == target;
            }
            biome::zoned_biome(region, range, self.effective_height(x, z)) == target
        })
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

    /// Every column's profile over a `size`-block square starting at
    /// `(x0, z0)`, row-major - shared by the river tests, which need to see
    /// whole stretches of river and their surroundings at once.
    fn profiles(gen: &TerrainGenerator, x0: i32, z0: i32, size: i32) -> Vec<ColumnProfile> {
        (0..size * size).map(|i| gen.column_profile(x0 + i % size, z0 + i / size)).collect()
    }

    const WINDOW: i32 = 160;

    /// Top-left corners of `WINDOW`-sized squares centered on real river
    /// nodes (seed 7) - found through the drainage network rather than
    /// hardcoded, so the tests keep looking at rivers whatever the
    /// continents happen to do. Half are on upland rivers (surface well
    /// above the sea), half anywhere.
    fn river_windows(gen: &TerrainGenerator) -> Vec<(i32, i32)> {
        let net = &gen.drainage;
        let (mut upland, mut any) = (vec![], vec![]);
        for i in -60..60 {
            for j in -60..60 {
                let cell = (i * 4, j * 4);
                if net.accumulation(gen, cell) <= RIVER_THRESHOLD {
                    continue;
                }
                let (x, z) = net.node_pos(cell);
                let corner = (x as i32 - WINDOW / 2, z as i32 - WINDOW / 2);
                if net.water_level(gen, cell, RIVER_THRESHOLD) >= SEA_LEVEL as f32 + 6.0 && upland.len() < 3 {
                    upland.push(corner);
                } else if any.len() < 3 {
                    any.push(corner);
                }
            }
        }
        assert!(upland.len() == 3 && any.len() == 3, "found only {upland:?} upland / {any:?} other rivers");
        upland.into_iter().chain(any).collect()
    }

    /// The invariant the levee rule (`column_profile`) exists for: river
    /// water can stand above sea level, so nothing but the generator's own
    /// care holds it in - every wet river column's sideways neighbours
    /// must be either water themselves (the river continuing, possibly a
    /// step lower) or solid ground at least as high as its surface. A
    /// violation is water hanging beside open air on dry land.
    #[test]
    fn river_water_never_stands_beside_open_air_on_dry_land() {
        let reg = BlockRegistry::with_defaults();
        let gen = TerrainGenerator::new(7, &reg);
        let mut checked = 0;
        for (x0, z0) in river_windows(&gen) {
            let p = profiles(&gen, x0, z0, WINDOW);
            for z in 1..WINDOW - 1 {
                for x in 1..WINDOW - 1 {
                    let col = p[(x + WINDOW * z) as usize];
                    let (true, Some(top)) = (col.in_river, col.water_top) else { continue };
                    checked += 1;
                    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                        let n = p[(x + dx + WINDOW * (z + dz)) as usize];
                        assert!(
                            n.water_top.is_some() || n.height >= top,
                            "river water at ({}, {}) top {top} leaks beside dry ground at height {}",
                            x0 + x,
                            z0 + z,
                            n.height
                        );
                    }
                }
            }
        }
        assert!(checked > 100, "only {checked} river columns sampled - the windows missed the rivers");
    }

    /// "Rivers run from high ground down to sea level" means river water
    /// that actually stands above the sea, not dry trenches in the uplands
    /// that only fill once they've been cut below `SEA_LEVEL`. And the
    /// generated chunk has to agree with the profile: water (or ice) right
    /// up to that surface.
    #[test]
    fn rivers_hold_water_above_sea_level_in_the_uplands() {
        let reg = BlockRegistry::with_defaults();
        let (water, ice) = (reg.id("water"), reg.id("ice"));
        let gen = TerrainGenerator::new(7, &reg);
        let mut highest = None;
        for (x0, z0) in river_windows(&gen) {
            let p = profiles(&gen, x0, z0, WINDOW);
            for (i, col) in p.iter().enumerate() {
                if let (true, Some(top)) = (col.in_river, col.water_top) {
                    if highest.is_none_or(|(_, _, t)| top > t) {
                        highest = Some((x0 + i as i32 % WINDOW, z0 + i as i32 / WINDOW, top));
                    }
                }
            }
        }
        let (x, z, top) = highest.expect("no river columns in the sampled windows");
        assert!(top >= SEA_LEVEL + 4, "highest river surface found was only {top}");

        let chunk = gen.generate(x.div_euclid(CHUNK_SIZE), z.div_euclid(CHUNK_SIZE));
        let (y, id) = column_top(&chunk, x.rem_euclid(CHUNK_SIZE) as usize, z.rem_euclid(CHUNK_SIZE) as usize);
        assert_eq!(y, top, "generated water surface disagrees with the profile at ({x}, {z})");
        assert!(id == water || id == ice, "top of an upland river at ({x}, {z}) isn't water");
    }

    /// The point of `river_incision`: some rivers sit flush with the ground
    /// beside them (bank level with the water) and others have cut down
    /// into it, a few blocks below their banks - both kinds have to
    /// actually occur, not just be expressible.
    #[test]
    fn rivers_come_both_flush_and_dug_in() {
        let reg = BlockRegistry::with_defaults();
        let gen = TerrainGenerator::new(7, &reg);
        let (mut flush, mut dug_in) = (0, 0);
        for (x0, z0) in river_windows(&gen) {
            let p = profiles(&gen, x0, z0, WINDOW);
            for z in 1..WINDOW - 1 {
                for x in 1..WINDOW - 1 {
                    let col = p[(x + WINDOW * z) as usize];
                    let (true, Some(top)) = (col.in_river, col.water_top) else { continue };
                    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                        let n = p[(x + dx + WINDOW * (z + dz)) as usize];
                        if n.water_top.is_some() {
                            continue;
                        }
                        match n.height - top {
                            0 => flush += 1,
                            3.. => dug_in += 1,
                            _ => {}
                        }
                    }
                }
            }
        }
        assert!(flush > 20 && dug_in > 20, "flush banks: {flush}, dug-in banks: {dug_in}");
    }

    /// Rivers aren't confined to any box: somewhere in the sampled world a
    /// single river runs a kilometre from where it first counts as a river
    /// to its end - about twice the side of the 512-block regions rivers
    /// used to be trapped inside. (Measured over three seeds: median ~300
    /// blocks, 90th percentile ~700, longest ~1000-1100 inside a 3200-block
    /// window, which itself cuts the longest ones short.)
    #[test]
    fn some_rivers_run_for_over_a_kilometre() {
        use crate::drainage::CELL;
        let reg = BlockRegistry::with_defaults();
        let gen = TerrainGenerator::new(7, &reg);
        let net = &gen.drainage;
        let mut longest = 0;
        // Every cell, not a sparse sample: a river is only counted from its
        // source, and a sparse grid mostly misses sources.
        for i in -100..100 {
            for j in -100..100 {
                let cell = (i, j);
                // A river source: a river cell no river flows into.
                if net.accumulation(&gen, cell) <= RIVER_THRESHOLD
                    || net.donors(&gen, cell).any(|d| net.accumulation(&gen, d) > RIVER_THRESHOLD)
                {
                    continue;
                }
                let (mut c, mut steps) = (cell, 0);
                while let Some(d) = net.downstream(&gen, c) {
                    c = d;
                    steps += 1;
                }
                longest = longest.max(steps * CELL);
            }
        }
        assert!(longest >= 1000, "longest river found runs only {longest} blocks");
    }

    /// "Wide and thin rivers": both a 2-3 block stream and a broad river
    /// (10+ blocks across) have to actually occur.
    #[test]
    fn rivers_come_both_thin_and_wide() {
        let reg = BlockRegistry::with_defaults();
        let gen = TerrainGenerator::new(7, &reg);
        let net = &gen.drainage;
        let (mut thin, mut wide) = (0, 0);
        for i in -60..60 {
            for j in -60..60 {
                let cell = (i * 4, j * 4);
                let a = net.accumulation(&gen, cell);
                if a <= RIVER_THRESHOLD {
                    continue;
                }
                let size = ((a / RIVER_THRESHOLD).ln() / (MAX_ACCUM_FOR_FULL_SIZE / RIVER_THRESHOLD).ln()).clamp(0.0, 1.0);
                let width = 2.0 * lerp(RIVER_HALF_WIDTH.0, RIVER_HALF_WIDTH.1, size as f64);
                if width <= 3.0 {
                    thin += 1;
                } else if width >= 10.0 {
                    wide += 1;
                }
            }
        }
        assert!(thin > 20 && wide > 3, "thin: {thin}, wide: {wide}");
    }

    /// Steep coasts (`coast_steepness`) have to actually produce cliffs
    /// standing well above the water, and gentle coasts have to stay
    /// beaches - judged on columns sitting right at a continent's shoreline,
    /// one sample per stretch of coast (counting sea-level *crossings*
    /// instead would let every lake and wiggle on a low beach coast swamp
    /// the cliffs, which cross only once). Mountain ranges are left out: a
    /// range running into the sea is a tall shore too, and would let this
    /// pass even with `coast_steepness` switched off entirely.
    #[test]
    fn coasts_come_both_as_cliffs_and_as_beaches() {
        let reg = BlockRegistry::with_defaults();
        for seed in [1u32, 7, 42] {
            let gen = TerrainGenerator::new(seed, &reg);
            let (mut cliffs, mut beaches, mut shore) = (0, 0, 0);
            // Wide (8000 blocks a side): continents are large enough that a
            // smaller window can contain a single stretch of coast, which
            // may simply happen to be all beach or all cliff.
            for i in 0..250 {
                for j in 0..250 {
                    let (x, z) = (i * 32 - 4000, j * 32 - 4000);
                    let c = gen.continent_value(x, z);
                    if !(OCEAN_THRESHOLD..OCEAN_THRESHOLD + 0.01).contains(&c) || gen.mountainness(x, z) > 0.0 {
                        continue;
                    }
                    shore += 1;
                    let h = gen.surface_height(x, z);
                    if h >= SEA_LEVEL + 8 {
                        cliffs += 1;
                    } else if h <= SEA_LEVEL + 2 {
                        beaches += 1;
                    }
                }
            }
            assert!(cliffs * 10 >= shore, "seed {seed}: only {cliffs}/{shore} shoreline columns are cliffs");
            assert!(beaches * 10 >= shore, "seed {seed}: only {beaches}/{shore} shoreline columns are beaches");
        }
    }

    /// Mountains are their own layer now, not a bump in the plains: inside
    /// a range, terrain stands far above the land outside one, and a good
    /// share of it reaches the snow line.
    #[test]
    fn mountain_ranges_rise_well_above_the_plains() {
        let reg = BlockRegistry::with_defaults();
        let gen = TerrainGenerator::new(7, &reg);
        let (mut range, mut range_sum, mut snowy, mut plain, mut plain_sum) = (0, 0i64, 0, 0, 0i64);
        for i in 0..200 {
            for j in 0..200 {
                let (x, z) = (i * 10 - 1000, j * 10 - 1000);
                let h = gen.surface_height(x, z);
                if h < SEA_LEVEL {
                    continue;
                }
                if gen.mountainness(x, z) >= crate::biome::MOUNTAIN_RANGE_THRESHOLD {
                    range += 1;
                    range_sum += h as i64;
                    snowy += (h >= SNOW_LINE) as i32;
                } else if gen.mountainness(x, z) == 0.0 {
                    plain += 1;
                    plain_sum += h as i64;
                }
            }
        }
        let (range_mean, plain_mean) = (range_sum as f64 / range as f64, plain_sum as f64 / plain as f64);
        assert!(range_mean > plain_mean + 12.0, "range mean {range_mean:.1} vs plains {plain_mean:.1}");
        assert!(snowy * 10 >= range, "only {snowy}/{range} range columns reach the snow line");
    }

    /// The vertical layering: on a range, the `Mountain` biome only ever
    /// appears up high, the same range's lower slopes keep their region
    /// biome, and nowhere off a range is ever `Mountain` however high it is.
    #[test]
    fn the_mountain_biome_sits_only_on_the_upper_slopes_of_real_ranges() {
        let reg = BlockRegistry::with_defaults();
        let gen = TerrainGenerator::new(7, &reg);
        let zone = crate::biome::ALTITUDE_ZONES[0].min_height;
        let (mut peaks, mut lower_slopes) = (0, 0);
        for i in 0..150 {
            for j in 0..150 {
                let (x, z) = (i * 12 - 900, j * 12 - 900);
                let h = gen.effective_height(x, z);
                let in_range = gen.mountainness(x, z) >= crate::biome::MOUNTAIN_RANGE_THRESHOLD;
                if gen.biome_at(x, z) == Biome::Mountain {
                    assert!(in_range && h >= zone, "Mountain biome at ({x}, {z}), height {h}, off a range's peaks");
                    peaks += 1;
                } else if in_range && h < zone && h >= SEA_LEVEL {
                    lower_slopes += 1;
                }
            }
        }
        assert!(peaks > 50 && lower_slopes > 50, "peaks: {peaks}, lower slopes: {lower_slopes}");
    }

    /// "Desolate": a Mountain-biome column's ground is bare rock, scree or a
    /// snow cap - never grass, so no tree can root there. (A tree just below
    /// the treeline can still lean its canopy over the line - that's
    /// foliage, not ground, so `ground_top` looks past it.)
    #[test]
    fn mountain_peaks_generate_as_bare_rock_or_snow() {
        let reg = BlockRegistry::with_defaults();
        let allowed = [reg.id("stone"), reg.id("gravel"), reg.id("snow")];
        let gen = TerrainGenerator::new(7, &reg);
        let mut checked = 0;
        'search: for cz in -40..40 {
            for cx in -40..40 {
                let (wx, wz) = (cx * CHUNK_SIZE + 8, cz * CHUNK_SIZE + 8);
                if gen.biome_at(wx, wz) != Biome::Mountain {
                    continue;
                }
                let chunk = gen.generate(cx, cz);
                for z in 0..CS {
                    for x in 0..CS {
                        let (wx, wz) = (cx * CHUNK_SIZE + x as i32, cz * CHUNK_SIZE + z as i32);
                        if gen.biome_at(wx, wz) != Biome::Mountain || gen.column_profile(wx, wz).water_top.is_some() {
                            continue;
                        }
                        let top = ground_top(&chunk, x, z, &[reg.id("leaves"), reg.id("log")]);
                        assert!(allowed.contains(&top), "Mountain column ({wx}, {wz}) has a {top} surface");
                        checked += 1;
                    }
                }
                if checked > 200 {
                    break 'search;
                }
            }
        }
        assert!(checked > 50, "only {checked} Mountain-biome columns found");
    }

    #[test]
    fn snow_biome_columns_generate_snow_at_low_altitude_not_grass() {
        let reg = BlockRegistry::with_defaults();
        let snow = reg.id("snow");
        let gen = TerrainGenerator::new(7, &reg);

        // A real, dry Snow-biome column *below* the SNOW_LINE altitude cap,
        // so this can only pass because of the biome itself.
        let mut checked = false;
        'search: for cx in -20..20 {
            for cz in -20..20 {
                let wx = cx * CHUNK_SIZE + 8;
                let wz = cz * CHUNK_SIZE + 8;
                let col = gen.column_profile(wx, wz);
                if gen.biome_at(wx, wz) != Biome::Snow || col.water_top.is_some() || col.height >= SNOW_LINE {
                    continue;
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
    fn freezing_biome_water_surfaces_generate_as_ice_not_water() {
        let reg = BlockRegistry::with_defaults();
        let ice = reg.id("ice");
        let gen = TerrainGenerator::new(7, &reg);

        let mut found = false;
        'search: for cx in -15..15 {
            for cz in -15..15 {
                let chunk = gen.generate(cx, cz);
                for z in 0..CS {
                    for x in 0..CS {
                        let wx = cx * CHUNK_SIZE + x as i32;
                        let wz = cz * CHUNK_SIZE + z as i32;
                        let Some(top) = gen.column_profile(wx, wz).water_top else { continue };
                        if !gen.biome_at(wx, wz).freezes_water() {
                            continue;
                        }
                        assert_eq!(chunk.blocks[block_index(x, top as usize, z)], ice, "unfrozen surface at ({wx},{wz})");
                        found = true;
                        break 'search;
                    }
                }
            }
        }
        assert!(found, "no water surface in a freezing biome in the sampled area");
    }

    /// The topmost block in column `(x, z)` that isn't air or one of
    /// `skip` (tree parts) - the ground itself.
    fn ground_top(chunk: &GeneratedChunk, x: usize, z: usize, skip: &[BlockId]) -> BlockId {
        (0..H)
            .rev()
            .map(|y| chunk.blocks[block_index(x, y, z)])
            .find(|id| *id != AIR && !skip.contains(id))
            .expect("column is entirely air")
    }

    /// The topmost non-air block in column `(x, z)`, as `(y, id)` - for
    /// tests that care about what a real generated column's surface
    /// actually ended up as.
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
