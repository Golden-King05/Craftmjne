//! Biome: per-column grass tint, and large-scale biome *regions*.
//!
//! Two related but independent concepts share this module:
//!
//! - **Grass tint** (`grass_tint`, `noise_for_seed`) is a small, purely
//!   cosmetic color gradient - one 2D noise value per `(x, z)` column,
//!   mapped to a color, multiplied onto `grass_top.png`/`grass_side.png`'s
//!   grayscale mask (`blocks/grass.json`'s `tinted`/`overlay` fields,
//!   `Tables::tinted`/`has_overlay`) at *mesh* time so grass isn't flat
//!   gray. Not tied to worldgen, not read by anything but the mesher.
//! - **Biome regions** (`Biome`, `biome_at`, `region_noise_for_seed`) *are*
//!   tied to worldgen: a much larger-scale noise that partitions the map
//!   into a handful of named regions (currently `Plains`/`Snow`),
//!   consulted by `terrain.rs` (surface block, terrain height) and
//!   `world.rs` (the exposed-water-freezes rule). A [`Biome`] variant's
//!   *behavior* - what surface material it wants, whether water freezes
//!   in it - is expressed as trait-like methods on the enum
//!   (`Biome::freezes_water`, `Biome::drier`) rather than checked ad hoc
//!   wherever it matters, so a second cold biome (tundra, glacier, ...)
//!   only ever needs a `true` on the methods it shares with `Snow`, not a
//!   second copy of whatever system reads them.
//!
//! Both are deliberately computed on demand, not stored per-chunk or
//! persisted: each is a pure function of `(world_seed, x, z)` with no
//! dependency on anything that changes (not even the block grid), so -
//! same reasoning as `light.rs`'s "not persisted, on purpose" - there is
//! nothing to save. A reload recomputes the exact same answer for the
//! exact same column, and nothing needs to invalidate or re-derive either
//! one when terrain nearby changes.

use crate::noise::SimplexNoise;

/// Decorrelates this from every other noise stream seeded off the same
/// world seed (terrain height, caves, ...) - same convention as
/// `terrain.rs`'s `TerrainGenerator` (`seed ^ <distinct constant>` per
/// stream), picked to not collide with any of those.
const SEED_OFFSET: u32 = 0x7f4a_7c15;

/// How large a biome-like patch reads as, in blocks - the noise is sampled
/// at `world_coord / SCALE`. Large enough that the tint changes as gradual
/// regions rather than a blotchy per-chunk checkerboard.
const SCALE: f64 = 180.0;

/// Grass color at the dry/warm end of the gradient (noise near `-1`).
const DRY: [f32; 3] = [0.48, 0.52, 0.14];
/// Grass color at the middle of the gradient (noise near `0`) - an
/// ordinary plains green. Deliberately saturated and on the dark side (a
/// "health bar" green, not a pastel one) - the grayscale mask this
/// multiplies onto (`grass_top.png`/`grass_side.png`, brightened toward
/// white by `atlas::normalize_tint_mask_tile`) is itself quite bright, so
/// a pale tint here reads as washed-out once combined with it; a stronger,
/// darker color here is what survives that multiply and still looks like
/// grass instead of pastel mint.
const LUSH: [f32; 3] = [0.20, 0.62, 0.18];
/// Grass color at the cool/wet end of the gradient (noise near `1`).
const COOL: [f32; 3] = [0.14, 0.50, 0.32];

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

/// A fresh noise source for one world - build once (`world::
/// compile_content`, alongside `Tables`) and pass by reference into every
/// `mesher::mesh_chunk` call, the same lifecycle `Tables` itself has.
pub fn noise_for_seed(seed: u32) -> SimplexNoise {
    SimplexNoise::new(seed ^ SEED_OFFSET)
}

/// The tint color for grass at world column `(x, z)`, each channel
/// `0.0..=1.0` - what gets multiplied onto a tinted face's atlas sample
/// (`Tables::tinted`). `[1.0, 1.0, 1.0]`, the untinted no-op, is baked
/// directly by the mesher for every other face; this function is the only
/// place a real color is ever produced.
///
/// `t` (`n + 1.0` or `n`) is guaranteed `0.0..=1.0` by the clamp above, and
/// `lerp3` between two already-valid-range colors with a `0..=1` `t` can
/// never leave that range either - so the result needs no further clamping,
/// rather than trusting `fbm2`'s own "roughly in [-1, 1]" bound to hold
/// exactly.
pub fn grass_tint(noise: &SimplexNoise, x: i32, z: i32) -> [f32; 3] {
    let n = (noise.fbm2(x as f64 / SCALE, z as f64 / SCALE, 2) as f32).clamp(-1.0, 1.0);
    if n < 0.0 {
        lerp3(DRY, LUSH, n + 1.0)
    } else {
        lerp3(LUSH, COOL, n)
    }
}

/// Decorrelates the biome-*region* noise from every other stream,
/// including `grass_tint`'s own `SEED_OFFSET` - regions and grass tint are
/// independent concepts (a snow-biome column has no grass to tint at all)
/// and must not accidentally share a pattern.
const REGION_SEED_OFFSET: u32 = 0xa511_e9c3;

/// How large one biome region reads as, in blocks - deliberately much
/// bigger than grass tint's own `SCALE`, so "snow biome" reads as a real,
/// sprawling region you travel through rather than a patch the size of a
/// grass color variation.
const REGION_SCALE: f64 = 640.0;

/// Above this raw region-noise value, a column is `Biome::Snow`; at or
/// below it, `Biome::Plains`. Tuned (see `region_noise_area_fractions_
/// land_in_a_reasonable_range`) so snow biome covers a real but minority
/// share of the map - noticeable when you find it, not the default.
const SNOW_THRESHOLD: f32 = 0.45;

/// Which large-scale biome region a world column belongs to - see the
/// module docs for how this differs from grass tint, and why each
/// variant's behavior lives as a method here instead of an ad hoc check
/// wherever it matters.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Biome {
    Plains,
    Snow,
}

impl Biome {
    /// Whether an exposed water source (a source block with air directly
    /// above it) freezes into ice in this biome - both at worldgen time
    /// (`terrain.rs`'s lake flooding) and afterward, whenever a later
    /// change exposes a new one to air (`world.rs`'s `freeze_exposed_
    /// water`). A trait of the biome, not a `Biome::Snow` check baked into
    /// either system directly, so a future cold biome (tundra, glacier,
    /// ...) gets the exact same behavior just by returning `true` here too
    /// - zero changes needed to either the generator or the runtime rule.
    pub fn freezes_water(self) -> bool {
        matches!(self, Biome::Snow)
    }

    /// Whether this biome's terrain generates a bit higher than the plain
    /// baseline (see `terrain.rs`'s `TerrainGenerator::height_boost`), so
    /// fewer of its columns naturally dip below sea level and flood into a
    /// lake. Same trait shape as `freezes_water`, for the same reason - a
    /// biome's *desire* for fewer lakes is a property of the biome, not
    /// something the height formula should special-case by name.
    pub fn drier(self) -> bool {
        matches!(self, Biome::Snow)
    }
}

/// A fresh biome-*region* noise source for one world - same lifecycle as
/// `noise_for_seed`'s grass-tint noise (build once, reuse for every
/// lookup), but a wholly separate stream and consulted by wholly separate
/// code (`terrain.rs` at generation time, `world.rs` at runtime) - see the
/// module docs.
pub fn region_noise_for_seed(seed: u32) -> SimplexNoise {
    SimplexNoise::new(seed ^ REGION_SEED_OFFSET)
}

/// The raw region-noise value at world column `(x, z)`, clamped to
/// `-1.0..=1.0`. Exposed separately from `biome_at` (rather than folding
/// the threshold check in) so a caller that wants a *smooth* biome-strength
/// signal - `terrain.rs`'s height boost, so the terrain doesn't step
/// abruptly at the exact `Biome::Snow` edge - can use the same underlying
/// noise `biome_at`'s hard classification is built from, instead of a
/// second, potentially-disagreeing noise stream.
pub fn region_noise_value(noise: &SimplexNoise, x: i32, z: i32) -> f32 {
    (noise.fbm2(x as f64 / REGION_SCALE, z as f64 / REGION_SCALE, 2) as f32).clamp(-1.0, 1.0)
}

/// Which biome region world column `(x, z)` belongs to.
pub fn biome_at(noise: &SimplexNoise, x: i32, z: i32) -> Biome {
    if region_noise_value(noise, x, z) > SNOW_THRESHOLD {
        Biome::Snow
    } else {
        Biome::Plains
    }
}

/// How wide (in raw region-noise units) `drier_strength` ramps over,
/// centered on `SNOW_THRESHOLD` - continuous rather than a step, so a
/// column's terrain is already rising as it approaches a drier biome
/// instead of stepping abruptly at the exact spot the surface block also
/// changes. A real elevation cliff would be far more visually jarring than
/// the hard edge in which *texture* a column's surface uses, which this
/// generator already accepts happens at `terrain::SNOW_LINE`.
const DRIER_BLEND: f32 = 0.2;

/// How strongly `Biome::drier` behavior applies at world column `(x, z)`,
/// `0.0..=1.0` - not the same hard cutoff `biome_at`'s classification
/// uses, so a continuous effect (`terrain.rs`'s height boost) can ramp in
/// smoothly around that edge instead of stepping abruptly right at it.
/// Tied to the one drier biome that exists today (`Biome::Snow`, via
/// `SNOW_THRESHOLD`) rather than taking a `Biome` parameter - a second
/// drier biome with its own separate threshold would need this
/// generalized, but nothing here needs that yet.
pub fn drier_strength(noise: &SimplexNoise, x: i32, z: i32) -> f32 {
    let n = region_noise_value(noise, x, z);
    ((n - (SNOW_THRESHOLD - DRIER_BLEND)) / (2.0 * DRIER_BLEND)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_and_column_always_gives_the_same_tint() {
        let noise = noise_for_seed(42);
        assert_eq!(grass_tint(&noise, 100, -50), grass_tint(&noise, 100, -50));
    }

    #[test]
    fn different_seeds_can_disagree_at_the_same_column() {
        // Not guaranteed at any *one* column (two seeds could coincide
        // there), but across many columns at least one should differ, or
        // the seed isn't actually influencing anything.
        let a = noise_for_seed(1);
        let b = noise_for_seed(2);
        assert!(
            (0..50).any(|i| grass_tint(&a, i * 37, i * 19) != grass_tint(&b, i * 37, i * 19)),
            "two different seeds produced identical tint at every sampled column"
        );
    }

    #[test]
    fn nearby_columns_are_smooth_not_a_coin_flip() {
        // Adjacent blocks should differ gently, not jump between the
        // gradient's extremes - that's what tells this apart from a
        // per-block hash, and it's the whole point of using coherent noise
        // (SimplexNoise) instead of one.
        let noise = noise_for_seed(7);
        let a = grass_tint(&noise, 500, 500);
        let b = grass_tint(&noise, 501, 500);
        for c in 0..3 {
            assert!((a[c] - b[c]).abs() < 0.05, "channel {c} jumped too much: {a:?} -> {b:?}");
        }
    }

    #[test]
    fn every_channel_stays_in_the_valid_color_range() {
        let noise = noise_for_seed(99);
        for i in -5..5 {
            for j in -5..5 {
                let tint = grass_tint(&noise, i * 401, j * 601);
                for c in tint {
                    assert!((0.0..=1.0).contains(&c), "{tint:?} has an out-of-range channel");
                }
            }
        }
    }

    #[test]
    fn the_gradient_is_continuous_at_its_midpoint() {
        // grass_tint branches on n < 0.0 vs n >= 0.0 - if the two branches'
        // endpoints didn't actually agree, grass would visibly snap to a
        // different color right at the boundary. Both branches evaluate to
        // (within float rounding) exactly LUSH there by construction
        // (lerp3(DRY, LUSH, 1.0) and lerp3(LUSH, COOL, 0.0)), so this pins
        // that down as a real invariant rather than something that just
        // happens to look right. An epsilon, not `assert_eq!`, because
        // `a + (b - a) * 1.0` isn't guaranteed bit-identical to `b` in f32 -
        // some constant pairs round exactly, some don't, and the *color
        // choice* shouldn't be constrained by which ones happen to.
        let close = |a: [f32; 3], b: [f32; 3]| (0..3).all(|c| (a[c] - b[c]).abs() < 1e-6);
        assert!(close(lerp3(DRY, LUSH, 1.0), LUSH));
        assert!(close(lerp3(LUSH, COOL, 0.0), LUSH));
    }

    #[test]
    fn biome_at_is_deterministic_for_the_same_seed_and_column() {
        let noise = region_noise_for_seed(42);
        assert_eq!(biome_at(&noise, 1000, -2000), biome_at(&noise, 1000, -2000));
    }

    #[test]
    fn nearby_region_columns_are_smooth_not_a_coin_flip() {
        let noise = region_noise_for_seed(7);
        let a = region_noise_value(&noise, 500, 500);
        let b = region_noise_value(&noise, 501, 500);
        assert!((a - b).abs() < 0.05, "region noise jumped too much: {a} -> {b}");
    }

    #[test]
    fn region_noise_area_fractions_land_in_a_reasonable_range() {
        // Measured, not assumed: SNOW_THRESHOLD was picked by actually
        // sampling a wide grid across several seeds and checking the
        // resulting share of Snow columns, rather than guessing a value
        // against fbm2's own "roughly -1..=1" bound and hoping it lands
        // somewhere sensible.
        for seed in [1u32, 2, 3, 4, 5] {
            let noise = region_noise_for_seed(seed);
            let mut snow = 0;
            let mut total = 0;
            for i in -40..40 {
                for j in -40..40 {
                    if biome_at(&noise, i * 97, j * 131) == Biome::Snow {
                        snow += 1;
                    }
                    total += 1;
                }
            }
            let fraction = snow as f32 / total as f32;
            assert!(
                (0.05..=0.35).contains(&fraction),
                "seed {seed}: snow biome covered {:.1}% of sampled columns, expected a real \
                 but minority share",
                fraction * 100.0
            );
        }
    }

    #[test]
    fn snow_biome_freezes_water_and_generates_drier_but_plains_does_not() {
        assert!(Biome::Snow.freezes_water());
        assert!(Biome::Snow.drier());
        assert!(!Biome::Plains.freezes_water());
        assert!(!Biome::Plains.drier());
    }

    #[test]
    fn drier_strength_is_zero_well_below_the_threshold_and_one_well_above_it() {
        assert_eq!(drier_strength_from_raw(SNOW_THRESHOLD - DRIER_BLEND - 0.1), 0.0);
        assert_eq!(drier_strength_from_raw(SNOW_THRESHOLD + DRIER_BLEND + 0.1), 1.0);
    }

    #[test]
    fn drier_strength_is_over_half_exactly_where_biome_at_switches_to_snow() {
        // The blend is centered on SNOW_THRESHOLD, so strength crosses 0.5
        // right where the hard classification also flips - the two are
        // built from the same noise value on purpose (see `region_noise_
        // value`'s doc comment) and must agree on where "drier" starts.
        assert_eq!(drier_strength_from_raw(SNOW_THRESHOLD), 0.5);
    }

    /// `drier_strength` itself always re-samples the noise at a real
    /// column; these two tests care only about the shape of the ramp
    /// against the raw value, so this reimplements just that formula
    /// directly rather than hunting for real `(x, z)` coordinates that
    /// happen to produce a specific noise value.
    fn drier_strength_from_raw(n: f32) -> f32 {
        ((n - (SNOW_THRESHOLD - DRIER_BLEND)) / (2.0 * DRIER_BLEND)).clamp(0.0, 1.0)
    }
}
