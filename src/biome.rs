//! Per-column biome grass tint.
//!
//! A minimal, real biome concept: one 2D noise value per `(x, z)` column,
//! mapped through a small hand-picked color gradient. Not a temperature/
//! humidity system, not tied to worldgen, not read by anything but the
//! mesher - just enough to make `grass_top.png`/`grass_side.png`'s
//! grayscale mask (`blocks/grass.json`'s `tinted` field, `Tables::tinted`)
//! actually vary across the world instead of rendering as flat gray.
//!
//! Deliberately computed at *mesh* time, not stored per-chunk or persisted:
//! it's a pure function of `(world_seed, x, z)` with no dependency on
//! anything that changes (not even the block grid), so - same reasoning as
//! `light.rs`'s "not persisted, on purpose" - there is nothing to save. A
//! reload recomputes the exact same color for the exact same column, and
//! nothing needs to invalidate or re-derive it when terrain nearby changes.

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
const DRY: [f32; 3] = [0.65, 0.68, 0.32];
/// Grass color at the middle of the gradient (noise near `0`) - an
/// ordinary plains green.
const LUSH: [f32; 3] = [0.42, 0.70, 0.32];
/// Grass color at the cool/wet end of the gradient (noise near `1`).
const COOL: [f32; 3] = [0.32, 0.60, 0.46];

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
        // exactly LUSH there by construction (lerp3(DRY, LUSH, 1.0) and
        // lerp3(LUSH, COOL, 0.0)), so this pins that down as a real
        // invariant rather than something that just happens to look right.
        assert_eq!(lerp3(DRY, LUSH, 1.0), LUSH);
        assert_eq!(lerp3(LUSH, COOL, 0.0), LUSH);
    }
}
