//! Biomes, and the climate maps they come from.
//!
//! Every world has two large-scale **climate maps**, pure functions of
//! `(world_seed, x, z)` like every other noise layer:
//!
//! - **temperature** (`ClimateMaps::temperature`), `-1.0` (frozen) to `1.0`
//!   (hot), varying over thousands of blocks, so cold country clumps
//!   together with cold and warm with warm;
//! - **humidity** (`ClimateMaps::humidity`), `-1.0` (arid) to `1.0` (wet),
//!   on a somewhat smaller scale.
//!
//! A column's biome is read off those maps through declarative tables
//! (`LAND_BIOMES`, `SEA_BIOMES`) - ranges of temperature and humidity, the
//! way a Whittaker diagram lays out real biomes - rather than thresholded
//! ad hoc wherever a biome matters. Today only temperature separates
//! anything (Snow vs Plains; cold, temperate and warm seas), but a desert
//! (hot and dry) or a jungle (hot and wet) is one more row in a table, and
//! it lands next to the climates that suit it automatically.
//!
//! Land and sea are decided by the terrain, not here: `terrain::
//! TerrainGenerator::biome_at` asks `region_biome_at` for land and
//! `sea_biome_at` for open ocean, then layers altitude zones (mountains)
//! and overlays (icebergs) on top. So a cold region's coast meets a cold
//! sea, not a sheet of ice stretching out over the water.
//!
//! **Grass tint** (`grass_tint`) is read from the same maps - drier grass
//! is yellower, colder grass bluer - plus a fine detail layer so it still
//! varies across a field. It's applied at *mesh* time onto `grass_top.png`/
//! `grass_side.png`'s grayscale mask (`Tables::tinted`/`has_overlay`).
//!
//! Nothing here is stored or saved: a reload recomputes the same answer
//! for the same column, and nothing needs invalidating when blocks change
//! (the same reasoning as `light.rs`'s "not persisted, on purpose").

use crate::noise::SimplexNoise;

/// Decorrelates each climate stream from every other noise stream seeded
/// off the same world seed - same `seed ^ <distinct constant>` convention
/// as `terrain.rs`.
const TINT_SEED_OFFSET: u32 = 0x7f4a_7c15;
const TEMPERATURE_SEED_OFFSET: u32 = 0xa511_e9c3;
const HUMIDITY_SEED_OFFSET: u32 = 0x3c6e_f372;

/// How large a climate zone reads as, in blocks: temperature changes over
/// the scale of whole regions you travel through, humidity a bit faster.
const TEMPERATURE_SCALE: f64 = 1400.0;
const HUMIDITY_SCALE: f64 = 800.0;
/// The scale of grass tint's fine variation within a climate.
const TINT_DETAIL_SCALE: f64 = 180.0;

/// Below this temperature, land is `Biome::Snow`. Tuned (see `snow_covers_
/// a_real_but_minority_share_of_land`) so snow is a real region you find,
/// not the default.
pub const COLD: f32 = -0.3;
/// Seas below this temperature are cold, above `WARM_SEA` warm. A little
/// warmer than `COLD`, so a cold sea reaches a bit past the end of the snow
/// on shore, as sea ice and cold currents do.
pub const COLD_SEA: f32 = -0.2;
pub const WARM_SEA: f32 = 0.3;

/// Grass color where it's dry (humidity near `-1`).
const DRY: [f32; 3] = [0.48, 0.52, 0.14];
/// An ordinary plains green. Deliberately saturated and on the dark side (a
/// "health bar" green, not a pastel one) - the grayscale mask this
/// multiplies onto (brightened toward white by `atlas::
/// normalize_tint_mask_tile`) is itself quite bright, so a pale tint here
/// reads as washed-out once combined with it.
const LUSH: [f32; 3] = [0.20, 0.62, 0.18];
/// Grass color where it's cold.
const COOL: [f32; 3] = [0.14, 0.50, 0.32];

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

/// The climate at one column.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Climate {
    pub temperature: f32,
    pub humidity: f32,
}

/// One world's climate maps - build once (`world::enter_world`, and one per
/// `TerrainGenerator`) and share by reference.
pub struct ClimateMaps {
    temperature: SimplexNoise,
    humidity: SimplexNoise,
    tint_detail: SimplexNoise,
}

impl ClimateMaps {
    pub fn for_seed(seed: u32) -> Self {
        Self {
            temperature: SimplexNoise::new(seed ^ TEMPERATURE_SEED_OFFSET),
            humidity: SimplexNoise::new(seed ^ HUMIDITY_SEED_OFFSET),
            tint_detail: SimplexNoise::new(seed ^ TINT_SEED_OFFSET),
        }
    }

    /// `-1.0` (frozen) to `1.0` (hot) at column `(x, z)`.
    pub fn temperature(&self, x: i32, z: i32) -> f32 {
        let s = TEMPERATURE_SCALE;
        // Two octaves, not three: a third adds enough local variation that
        // temperature 100 blocks away is barely more alike than 10,000 away.
        (self.temperature.fbm2(x as f64 / s, z as f64 / s, 2) as f32).clamp(-1.0, 1.0)
    }

    /// `-1.0` (arid) to `1.0` (wet) at column `(x, z)`.
    pub fn humidity(&self, x: i32, z: i32) -> f32 {
        let s = HUMIDITY_SCALE;
        (self.humidity.fbm2(x as f64 / s, z as f64 / s, 3) as f32).clamp(-1.0, 1.0)
    }

    pub fn at(&self, x: i32, z: i32) -> Climate {
        Climate { temperature: self.temperature(x, z), humidity: self.humidity(x, z) }
    }
}

/// The tint color for grass at world column `(x, z)`, each channel
/// `0.0..=1.0` - what gets multiplied onto a tinted face's atlas sample.
/// `[1.0, 1.0, 1.0]`, the untinted no-op, is baked directly by the mesher
/// for every other face; this is the only place a real color is produced.
///
/// Humidity (nudged by a fine detail layer, so a field isn't one flat
/// color) runs it from dry yellow-green to lush green; cold pulls it toward
/// a cool blue-green. Every step is a lerp between in-range colors with a
/// clamped `0..=1` weight, so the result needs no clamping of its own.
pub fn grass_tint(climate: &ClimateMaps, x: i32, z: i32) -> [f32; 3] {
    let detail = climate.tint_detail.fbm2(x as f64 / TINT_DETAIL_SCALE, z as f64 / TINT_DETAIL_SCALE, 2) as f32;
    let wet = ((climate.humidity(x, z) + 0.35 * detail + 0.6) / 1.2).clamp(0.0, 1.0);
    let cold = ((COLD + 0.5 - climate.temperature(x, z)) / 0.5).clamp(0.0, 1.0);
    lerp3(lerp3(DRY, LUSH, wet), COOL, cold)
}

/// Which biome region a world column belongs to - see the module docs for
/// how land/sea, altitude zones and overlays combine, and why each
/// variant's behavior lives as a method here instead of an ad hoc check
/// wherever it matters.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Biome {
    Plains,
    Snow,
    /// The desolate upper slopes of a mountain range - bare rock and scree,
    /// snow-capped above `terrain::SNOW_LINE`. Never a *region* biome: it
    /// only exists as an altitude zone (`ALTITUDE_ZONES`) on top of whatever
    /// region a range rises out of, so climbing a mountain changes biome as
    /// you go up.
    Mountain,
    WarmSea,
    TemperateSea,
    /// Open water too cold for most things - but not frozen over: only the
    /// shallows along its coasts ice up (`terrain::SHORE_ICE_DEPTH`).
    ColdSea,
    /// An overlay on a cold sea: icebergs adrift, thickest near cold coasts
    /// where they calve, thinning out across the open water.
    Icebergs,
}

impl Biome {
    /// Whether an exposed water source (a source block with air directly
    /// above it) freezes into ice in this biome - both at worldgen time
    /// (`terrain.rs`'s lake flooding) and afterward, whenever a later
    /// change exposes a new one to air (`world.rs`'s `freeze_exposed_
    /// water`). A trait of the biome, not a `Biome::Snow` check baked into
    /// either system directly, so a future cold biome (tundra, glacier,
    /// ...) gets the exact same behavior just by returning `true` here too.
    /// Seas don't: the open ocean doesn't freeze over (a cold sea's
    /// shore ice is generation's job, not this rule's).
    pub fn freezes_water(self) -> bool {
        matches!(self, Biome::Snow | Biome::Mountain)
    }

    /// Whether this biome's terrain generates a bit higher than the plain
    /// baseline (see `terrain.rs`'s `DRIER_HEIGHT_BOOST`), so fewer of its
    /// columns dip below sea level and flood into a lake.
    pub fn drier(self) -> bool {
        matches!(self, Biome::Snow)
    }

    /// Open-ocean biomes (as opposed to land, and the lakes and ponds on it).
    pub fn is_sea(self) -> bool {
        matches!(self, Biome::WarmSea | Biome::TemperateSea | Biome::ColdSea | Biome::Icebergs)
    }

    /// Cold enough at sea for shore ice - a cold sea, and the icebergs on it.
    pub fn is_cold_sea(self) -> bool {
        matches!(self, Biome::ColdSea | Biome::Icebergs)
    }

    /// Every biome that exists, for anything that needs to enumerate them
    /// (`/locate biome`'s argument autocomplete and parsing) instead of
    /// re-deriving the list by hand.
    pub const ALL: [Biome; 7] = [
        Biome::Plains,
        Biome::Snow,
        Biome::Mountain,
        Biome::WarmSea,
        Biome::TemperateSea,
        Biome::ColdSea,
        Biome::Icebergs,
    ];

    /// The lowercase name a player types to refer to this biome in chat
    /// commands - the inverse of `Biome::parse`.
    pub fn name(self) -> &'static str {
        match self {
            Biome::Plains => "plains",
            Biome::Snow => "snow",
            Biome::Mountain => "mountain",
            Biome::WarmSea => "warm_sea",
            Biome::TemperateSea => "temperate_sea",
            Biome::ColdSea => "cold_sea",
            Biome::Icebergs => "icebergs",
        }
    }

    /// Parses a player-typed biome name (case-insensitive), the inverse of
    /// `Biome::name`.
    pub fn parse(s: &str) -> Option<Biome> {
        Biome::ALL.into_iter().find(|b| b.name().eq_ignore_ascii_case(s))
    }
}

/// One row of a climate table: the biome for temperatures and humidities
/// in these ranges (lower bound inclusive, upper exclusive).
pub struct ClimateBand {
    pub temperature: (f32, f32),
    pub humidity: (f32, f32),
    pub biome: Biome,
}

const ANY: (f32, f32) = (-2.0, 2.0);

/// Land biomes by climate, first match wins. A new land biome is a new
/// row: a desert would be `temperature: (0.4, 2.0), humidity: (-2.0,
/// -0.3)`, placed before `Plains`.
pub const LAND_BIOMES: &[ClimateBand] = &[
    ClimateBand { temperature: (-2.0, COLD), humidity: ANY, biome: Biome::Snow },
    ClimateBand { temperature: ANY, humidity: ANY, biome: Biome::Plains },
];

/// Open-sea biomes by climate, first match wins.
pub const SEA_BIOMES: &[ClimateBand] = &[
    ClimateBand { temperature: (-2.0, COLD_SEA), humidity: ANY, biome: Biome::ColdSea },
    ClimateBand { temperature: (WARM_SEA, 2.0), humidity: ANY, biome: Biome::WarmSea },
    ClimateBand { temperature: ANY, humidity: ANY, biome: Biome::TemperateSea },
];

/// The first row of `table` whose ranges hold `climate` (the last row,
/// if none does - every table ends in a catch-all).
pub fn classify(table: &[ClimateBand], climate: Climate) -> Biome {
    let holds = |(lo, hi): (f32, f32), v: f32| v >= lo && v < hi;
    table
        .iter()
        .find(|b| holds(b.temperature, climate.temperature) && holds(b.humidity, climate.humidity))
        .unwrap_or(&table[table.len() - 1])
        .biome
}

/// Which land biome region world column `(x, z)` belongs to - the biome
/// before altitude zones, and before deciding whether the column is land
/// at all. Not the final answer for a column: `terrain::TerrainGenerator::
/// biome_at` is, and it's what worldgen and the runtime freezing rule use.
/// Named `region_` so this can't be mistaken for it.
pub fn region_biome_at(climate: &ClimateMaps, x: i32, z: i32) -> Biome {
    classify(LAND_BIOMES, climate.at(x, z))
}

/// Which sea biome world column `(x, z)` would be if it's open ocean -
/// before overlays (icebergs).
pub fn sea_biome_at(climate: &ClimateMaps, x: i32, z: i32) -> Biome {
    classify(SEA_BIOMES, climate.at(x, z))
}

/// How strongly a column must sit inside a mountain range
/// (`terrain::TerrainGenerator::mountainness`, `0.0..=1.0`) before
/// `ALTITUDE_ZONES` apply to it at all - outside a range, a high column
/// (a steep coast's raised headland, say) keeps its region biome.
pub const MOUNTAIN_RANGE_THRESHOLD: f64 = 0.5;

/// One band of a mountain's vertical biome layering: at or above
/// `min_height`, a column on a range becomes `biome`.
pub struct AltitudeZone {
    pub min_height: i32,
    pub biome: Biome,
}

/// A mountain's biomes from its base upward, lowest band first - below the
/// first band a column keeps its region biome (plains at the foot of a
/// range). Adding a middle band later (spruce forest on the lower slopes)
/// is one more entry here; nothing that reads it changes.
pub const ALTITUDE_ZONES: &[AltitudeZone] = &[AltitudeZone { min_height: 78, biome: Biome::Mountain }];

/// A column's full biome from its region biome, how strongly it sits in a
/// mountain range, and its height - pure, so the zoning is testable
/// without generating terrain. `terrain::TerrainGenerator::biome_at` is
/// the real caller.
pub fn zoned_biome(region: Biome, mountainness: f64, height: i32) -> Biome {
    if mountainness < MOUNTAIN_RANGE_THRESHOLD {
        return region;
    }
    ALTITUDE_ZONES.iter().rev().find(|z| height >= z.min_height).map_or(region, |z| z.biome)
}

/// How wide (in temperature units) `drier_strength` ramps over, centered
/// on `COLD` - continuous rather than a step, so a column's terrain is
/// already rising as it approaches a drier biome instead of stepping
/// abruptly where the surface block also changes.
const DRIER_BLEND: f32 = 0.15;

/// How strongly `Biome::drier` behavior applies at world column `(x, z)`,
/// `0.0..=1.0`, from the same temperature map the hard Snow/Plains
/// classification uses, so the two agree on where "drier" starts.
pub fn drier_strength(climate: &ClimateMaps, x: i32, z: i32) -> f32 {
    drier_strength_at(climate.temperature(x, z))
}

fn drier_strength_at(temperature: f32) -> f32 {
    ((COLD + DRIER_BLEND - temperature) / (2.0 * DRIER_BLEND)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_and_column_always_gives_the_same_tint_and_climate() {
        let c = ClimateMaps::for_seed(42);
        assert_eq!(grass_tint(&c, 100, -50), grass_tint(&c, 100, -50));
        assert_eq!(c.at(1000, -2000), c.at(1000, -2000));
    }

    #[test]
    fn different_seeds_can_disagree_at_the_same_column() {
        let (a, b) = (ClimateMaps::for_seed(1), ClimateMaps::for_seed(2));
        assert!((0..50).any(|i| grass_tint(&a, i * 37, i * 19) != grass_tint(&b, i * 37, i * 19)));
        assert!((0..50).any(|i| a.at(i * 997, i * 619) != b.at(i * 997, i * 619)));
    }

    #[test]
    fn nearby_columns_are_smooth_not_a_coin_flip() {
        let c = ClimateMaps::for_seed(7);
        let (a, b) = (grass_tint(&c, 500, 500), grass_tint(&c, 501, 500));
        for i in 0..3 {
            assert!((a[i] - b[i]).abs() < 0.05, "channel {i} jumped too much: {a:?} -> {b:?}");
        }
        let (a, b) = (c.at(500, 500), c.at(501, 500));
        assert!((a.temperature - b.temperature).abs() < 0.02 && (a.humidity - b.humidity).abs() < 0.02);
    }

    #[test]
    fn every_tint_channel_stays_in_the_valid_color_range() {
        let c = ClimateMaps::for_seed(99);
        for i in -20..20 {
            for j in -20..20 {
                let tint = grass_tint(&c, i * 401, j * 601);
                assert!(tint.iter().all(|v| (0.0..=1.0).contains(v)), "{tint:?}");
            }
        }
    }

    /// The whole point of a climate map: neighbours share a climate. A
    /// column's temperature says a lot about a column 100 blocks away, and
    /// very little about one 10,000 blocks away.
    #[test]
    fn climates_clump_together_over_hundreds_of_blocks() {
        let c = ClimateMaps::for_seed(3);
        let (mut near, mut far, mut n) = (0.0, 0.0, 0);
        for i in -30..30 {
            for j in -30..30 {
                let (x, z) = (i * 211, j * 307);
                let t = c.temperature(x, z);
                near += (t - c.temperature(x + 100, z)).abs();
                far += (t - c.temperature(x + 10_000, z + 7_000)).abs();
                n += 1;
            }
        }
        let (near, far) = (near / n as f32, far / n as f32);
        // Two unrelated columns differ by ~0.35 on average; a neighbour 100
        // blocks over should be a small fraction of that.
        assert!(near < 0.15 && near * 2.5 < far, "temperature 100 blocks away differs by {near:.3}, 10k away by {far:.3}");
    }

    #[test]
    fn snow_covers_a_real_but_minority_share_of_land() {
        for seed in [1u32, 2, 3, 4, 5] {
            let c = ClimateMaps::for_seed(seed);
            let (mut snow, mut total) = (0, 0);
            for i in -40..40 {
                for j in -40..40 {
                    snow += (region_biome_at(&c, i * 197, j * 231) == Biome::Snow) as i32;
                    total += 1;
                }
            }
            let fraction = snow as f32 / total as f32;
            assert!((0.05..=0.35).contains(&fraction), "seed {seed}: snow covered {:.1}%", fraction * 100.0);
        }
    }

    /// Seas split three ways by temperature, and the cold sea sits where
    /// the land beside it would be snow - cold country meets cold water.
    #[test]
    fn seas_split_by_temperature_and_cold_seas_lie_off_cold_land() {
        let c = ClimateMaps::for_seed(4);
        let mut seen = std::collections::HashSet::new();
        for i in -60..60 {
            for j in -60..60 {
                let (x, z) = (i * 173, j * 239);
                let sea = sea_biome_at(&c, x, z);
                seen.insert(sea);
                if region_biome_at(&c, x, z) == Biome::Snow {
                    assert_eq!(sea, Biome::ColdSea, "snow at ({x}, {z}) beside a {sea:?}");
                }
            }
        }
        for want in [Biome::ColdSea, Biome::TemperateSea, Biome::WarmSea] {
            assert!(seen.contains(&want), "no {want:?} anywhere");
        }
    }

    #[test]
    fn region_and_sea_tables_only_produce_their_own_kind() {
        let c = ClimateMaps::for_seed(7);
        for i in -100..100 {
            for j in -100..100 {
                let (x, z) = (i * 137, j * 151);
                let land = region_biome_at(&c, x, z);
                assert!(!land.is_sea() && land != Biome::Mountain, "{land:?}");
                let sea = sea_biome_at(&c, x, z);
                assert!(sea.is_sea() && sea != Biome::Icebergs, "{sea:?}");
            }
        }
    }

    #[test]
    fn classify_takes_the_first_matching_row() {
        let at = |t, h| Climate { temperature: t, humidity: h };
        assert_eq!(classify(LAND_BIOMES, at(COLD - 0.01, 0.0)), Biome::Snow);
        assert_eq!(classify(LAND_BIOMES, at(COLD, 0.0)), Biome::Plains);
        assert_eq!(classify(SEA_BIOMES, at(WARM_SEA, 0.9)), Biome::WarmSea);
        assert_eq!(classify(SEA_BIOMES, at(0.0, -0.9)), Biome::TemperateSea);
    }

    #[test]
    fn only_cold_land_freezes_water_or_generates_drier() {
        assert!(Biome::Snow.freezes_water() && Biome::Snow.drier());
        assert!(!Biome::Plains.freezes_water() && !Biome::Plains.drier());
        assert!(Biome::Mountain.freezes_water() && !Biome::Mountain.drier());
        // The open sea doesn't freeze over, however cold.
        for sea in [Biome::ColdSea, Biome::Icebergs, Biome::TemperateSea, Biome::WarmSea] {
            assert!(!sea.freezes_water());
        }
    }

    #[test]
    fn drier_strength_ramps_across_the_snow_line() {
        assert_eq!(drier_strength_at(COLD + DRIER_BLEND + 0.1), 0.0);
        assert_eq!(drier_strength_at(COLD - DRIER_BLEND - 0.1), 1.0);
        assert_eq!(drier_strength_at(COLD), 0.5);
    }

    #[test]
    fn altitude_zones_only_apply_on_a_mountain_range() {
        let top = ALTITUDE_ZONES[0].min_height;
        assert_eq!(zoned_biome(Biome::Plains, 1.0, top - 1), Biome::Plains);
        assert_eq!(zoned_biome(Biome::Plains, 1.0, top), Biome::Mountain);
        assert_eq!(zoned_biome(Biome::Snow, MOUNTAIN_RANGE_THRESHOLD, top + 5), Biome::Mountain);
        assert_eq!(zoned_biome(Biome::Plains, MOUNTAIN_RANGE_THRESHOLD - 0.01, top + 10), Biome::Plains);
    }

    #[test]
    fn biome_names_round_trip_through_parse() {
        for b in Biome::ALL {
            assert_eq!(Biome::parse(b.name()), Some(b));
            assert_eq!(Biome::parse(&b.name().to_uppercase()), Some(b));
        }
        assert_eq!(Biome::parse("desert"), None);
    }
}
