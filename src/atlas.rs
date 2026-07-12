//! Texture atlas: packs 16x16 tiles into one RGBA image, so every chunk
//! renders with a single material. Every tile is procedurally painted by
//! default (no image assets required to run the project) - but any tile
//! can be overridden by dropping a real `textures/blocks/<name>.png` next
//! to the executable (or the repo root, for `cargo run`/tests): exactly
//! `TILE_SIZE`x`TILE_SIZE`, checked before the procedural painter for that
//! name ever runs. See `textures/blocks/README.md` for the full list of
//! names the built-in blocks look for.
//!
//! Extension point: push your own painter into the [`Painters`] resource from
//! your plugin's `build()`:
//! ```ignore
//! painters.register("ruby", |p, rng| { /* p.px(x, y, [r, g, b, a]) */ });
//! ```

use bevy::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::{ATLAS_TILES, TILE_SIZE};
use crate::noise::{hash_str, mulberry32};

pub const ATLAS_PX: usize = ATLAS_TILES * TILE_SIZE;
const S: i32 = TILE_SIZE as i32;

/// Draws one 16x16 tile. Coordinates are tile-local; out-of-range is clipped.
pub struct TilePainter<'a> {
    buf: &'a mut [u8],
    x0: usize,
    y0: usize,
}

impl TilePainter<'_> {
    pub fn px(&mut self, x: i32, y: i32, c: [f32; 3]) {
        self.pxa(x, y, [c[0], c[1], c[2], 255.0]);
    }

    pub fn pxa(&mut self, x: i32, y: i32, c: [f32; 4]) {
        if !(0..S).contains(&x) || !(0..S).contains(&y) {
            return;
        }
        let i = ((self.y0 + y as usize) * ATLAS_PX + self.x0 + x as usize) * 4;
        for ch in 0..4 {
            self.buf[i + ch] = c[ch].clamp(0.0, 255.0) as u8;
        }
    }

    /// Fills the tile with a base color, jittering each pixel's brightness.
    pub fn noisy_fill(&mut self, rng: &mut dyn FnMut() -> f32, base: [f32; 3], jitter: f32) {
        for y in 0..S {
            for x in 0..S {
                let j = (rng() - 0.5) * jitter;
                self.px(x, y, [base[0] + j, base[1] + j, base[2] + j]);
            }
        }
    }
}

pub type PaintFn = Box<dyn Fn(&mut TilePainter, &mut dyn FnMut() -> f32) + Send + Sync>;

/// Ordered painter list; atlas tile index = registration order.
#[derive(Resource)]
pub struct Painters(pub Vec<(String, PaintFn)>);

impl Painters {
    pub fn register(
        &mut self,
        name: &str,
        f: impl Fn(&mut TilePainter, &mut dyn FnMut() -> f32) + Send + Sync + 'static,
    ) {
        assert!(
            self.0.iter().all(|(n, _)| n != name),
            "painter {name:?} already registered"
        );
        self.0.push((name.into(), Box::new(f)));
    }
}

pub struct AtlasData {
    /// RGBA8, ATLAS_PX x ATLAS_PX.
    pub pixels: Vec<u8>,
    pub indices: HashMap<String, u16>,
}

/// Where to look for `textures/blocks/`: next to the running executable
/// first (how an installed/shipped build finds files shipped next to it),
/// falling back to the current working directory (how `cargo run`/tests
/// find the one at the repo root - Cargo runs both with the package root
/// as cwd). Mirrors `blocks::find_blocks_dir` exactly. Not finding this
/// directory at all is fine - it just means every tile falls back to its
/// procedural painter, same as before this existed.
fn find_textures_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("textures").join("blocks");
            if candidate.is_dir() {
                return candidate;
            }
        }
    }
    PathBuf::from("textures").join("blocks")
}

/// Loads a hand-supplied tile for `name` from `<dir>/<name>.png`, decoded
/// to raw `TILE_SIZE`x`TILE_SIZE` RGBA8 - `None` (not an error) if the file
/// just isn't there, since falling back to the procedural painter for
/// anything not yet supplied is the whole point. Panics on a *malformed*
/// file (wrong dimensions, unreadable/corrupt PNG) instead of silently
/// falling back - a broken file sitting in `textures/blocks/` is far more
/// likely a mistake worth surfacing loudly than something to paper over
/// with a procedural placeholder nobody asked for, matching how
/// `blocks::load_from_dir` treats a malformed block file.
fn load_custom_tile(dir: &Path, name: &str) -> Option<Vec<u8>> {
    let path = dir.join(format!("{name}.png"));
    if !path.is_file() {
        return None;
    }
    let img = image::open(&path)
        .unwrap_or_else(|err| panic!("failed to read texture {path:?}: {err}"))
        .into_rgba8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    if w != TILE_SIZE || h != TILE_SIZE {
        panic!(
            "texture {path:?} is {w}x{h}, but every block texture must be exactly \
             {TILE_SIZE}x{TILE_SIZE}"
        );
    }
    Some(img.into_raw())
}

pub fn build_atlas(painters: &Painters) -> AtlasData {
    assert!(
        painters.0.len() <= ATLAS_TILES * ATLAS_TILES,
        "too many textures for a {ATLAS_TILES}x{ATLAS_TILES} atlas"
    );
    let mut pixels = vec![0u8; ATLAS_PX * ATLAS_PX * 4];
    let mut indices = HashMap::new();
    let textures_dir = find_textures_dir();
    for (i, (name, paint)) in painters.0.iter().enumerate() {
        let x0 = (i % ATLAS_TILES) * TILE_SIZE;
        let y0 = (i / ATLAS_TILES) * TILE_SIZE;
        if let Some(custom) = load_custom_tile(&textures_dir, name) {
            for y in 0..TILE_SIZE {
                let src = y * TILE_SIZE * 4;
                let dst = ((y0 + y) * ATLAS_PX + x0) * 4;
                pixels[dst..dst + TILE_SIZE * 4].copy_from_slice(&custom[src..src + TILE_SIZE * 4]);
            }
        } else {
            let mut tile = TilePainter { buf: &mut pixels, x0, y0 };
            let mut rng = mulberry32(hash_str(name));
            paint(&mut tile, &mut rng);
        }
        indices.insert(name.clone(), i as u16);
    }
    AtlasData { pixels, indices }
}

/// The default 16x16 pixel-art set for the built-in blocks.
pub fn default_painters() -> Painters {
    let mut p = Painters(Vec::new());

    p.register("stone", |t, rng| t.noisy_fill(rng, [127.0, 127.0, 127.0], 26.0));

    p.register("dirt", |t, rng| {
        t.noisy_fill(rng, [134.0, 96.0, 67.0], 30.0);
        for _ in 0..10 {
            let (x, y) = ((rng() * 16.0) as i32, (rng() * 16.0) as i32);
            t.px(x, y, [100.0, 70.0, 48.0]);
        }
    });

    p.register("grass_top", |t, rng| {
        t.noisy_fill(rng, [104.0, 168.0, 62.0], 30.0);
        for _ in 0..14 {
            let (x, y) = ((rng() * 16.0) as i32, (rng() * 16.0) as i32);
            t.px(x, y, [88.0, 148.0, 52.0]);
        }
    });

    p.register("grass_side", |t, rng| {
        t.noisy_fill(rng, [134.0, 96.0, 67.0], 30.0);
        for x in 0..16 {
            let depth = 2 + (rng() * 2.4) as i32; // ragged grass fringe
            for y in 0..depth {
                let j = (rng() - 0.5) * 26.0;
                t.px(x, y, [104.0 + j, 168.0 + j, 62.0 + j]);
            }
        }
    });

    p.register("sand", |t, rng| t.noisy_fill(rng, [219.0, 207.0, 160.0], 18.0));

    p.register("gravel", |t, rng| {
        t.noisy_fill(rng, [130.0, 124.0, 120.0], 20.0);
        for _ in 0..18 {
            let (x, y) = ((rng() * 15.0) as i32, (rng() * 15.0) as i32);
            let c = 90.0 + rng() * 80.0;
            t.px(x, y, [c, c * 0.96, c * 0.9]);
            t.px(x + 1, y, [c * 0.8, c * 0.78, c * 0.75]);
        }
    });

    p.register("water", |t, rng| {
        t.noisy_fill(rng, [50.0, 108.0, 190.0], 14.0);
        // A few soft, low-contrast, narrow highlights - not the bold, wide
        // streaks this used to have. Every water block samples this exact
        // same baked tile, so anything bold/distinctive here repeats
        // identically at every block boundary across a big lake, which the
        // eye reads as an obvious tiled stamp (a "grid") rather than one
        // continuous surface. Subtle and close to the base color instead.
        for _ in 0..3 {
            let y = (rng() * 16.0) as i32;
            let x = (rng() * 14.0) as i32;
            for dx in 0..2 {
                t.px(x + dx, y, [68.0, 124.0, 200.0]);
            }
        }
    });

    p.register("log_side", |t, rng| {
        for x in 0..16 {
            let stripe = x % 4 < 2;
            for y in 0..16 {
                let j = (rng() - 0.5) * 18.0;
                let c: [f32; 3] = if stripe { [109.0, 85.0, 50.0] } else { [88.0, 66.0, 38.0] };
                t.px(x, y, [c[0] + j, c[1] + j, c[2] + j]);
            }
        }
    });

    p.register("log_top", |t, rng| {
        t.noisy_fill(rng, [109.0, 85.0, 50.0], 14.0);
        for y in 0..16 {
            for x in 0..16 {
                let d = (x as f32 - 7.5).abs().max((y as f32 - 7.5).abs());
                if (d as i32) % 2 == 0 && d < 7.0 {
                    let j = (rng() - 0.5) * 12.0;
                    t.px(x, y, [168.0 + j, 138.0 + j, 92.0 + j]);
                }
            }
        }
    });

    p.register("leaves", |t, rng| {
        for y in 0..16 {
            for x in 0..16 {
                if rng() < 0.14 {
                    t.pxa(x, y, [0.0, 0.0, 0.0, 0.0]); // see-through holes
                } else {
                    let j = (rng() - 0.5) * 44.0;
                    t.px(x, y, [58.0 + j * 0.4, 128.0 + j, 44.0 + j * 0.4]);
                }
            }
        }
    });

    p.register("planks", |t, rng| {
        t.noisy_fill(rng, [176.0, 142.0, 88.0], 16.0);
        let seam = [122.0, 96.0, 56.0];
        for y in [3, 7, 11, 15] {
            for x in 0..16 {
                t.px(x, y, seam);
            }
        }
        for (x, y) in [(4, 1), (12, 5), (2, 9), (10, 13)] {
            t.px(x, y, seam);
        }
    });

    p.register("cobblestone", |t, rng| {
        t.noisy_fill(rng, [110.0, 110.0, 110.0], 18.0);
        for _ in 0..7 {
            let cx = 1 + (rng() * 11.0) as i32;
            let cy = 1 + (rng() * 11.0) as i32;
            let w = 3 + (rng() * 3.0) as i32;
            let h = 3 + (rng() * 3.0) as i32;
            let c = 118.0 + rng() * 34.0;
            for y in 0..h {
                for x in 0..w {
                    let edge = x == 0 || y == 0 || x == w - 1 || y == h - 1;
                    let v = if edge { 74.0 } else { c + (rng() - 0.5) * 14.0 };
                    t.px((cx + x).min(15), (cy + y).min(15), [v, v, v]);
                }
            }
        }
    });

    p.register("glass", |t, rng| {
        let frame = [208.0, 232.0, 238.0];
        for y in 0..16 {
            for x in 0..16 {
                if x == 0 || y == 0 || x == 15 || y == 15 {
                    t.px(x, y, frame);
                } else {
                    t.pxa(x, y, [0.0, 0.0, 0.0, 0.0]);
                }
            }
        }
        let _ = rng();
        for i in 2..7 {
            t.px(i, 8 - i, [226.0, 244.0, 248.0]); // streak
        }
        for i in 4..12 {
            t.px(i, 16 - i, [226.0, 244.0, 248.0]);
        }
    });

    p.register("bedrock", |t, rng| t.noisy_fill(rng, [70.0, 70.0, 70.0], 60.0));

    p.register("snow", |t, rng| t.noisy_fill(rng, [241.0, 246.0, 250.0], 10.0));

    p.register("bricks", |t, rng| {
        for y in 0..16i32 {
            let row = y / 4;
            for x in 0..16i32 {
                let mortar_y = y % 4 == 3;
                let mortar_x = (x + if row % 2 == 1 { 4 } else { 0 }) % 8 == 7;
                if mortar_y || mortar_x {
                    t.px(x, y, [178.0, 170.0, 160.0]);
                } else {
                    let j = (rng() - 0.5) * 20.0;
                    t.px(x, y, [150.0 + j, 72.0 + j * 0.5, 62.0 + j * 0.5]);
                }
            }
        }
    });

    fn ore(spec: [f32; 3]) -> impl Fn(&mut TilePainter, &mut dyn FnMut() -> f32) {
        move |t, rng| {
            t.noisy_fill(rng, [127.0, 127.0, 127.0], 26.0);
            for _ in 0..5 {
                let cx = 1 + (rng() * 12.0) as i32;
                let cy = 1 + (rng() * 12.0) as i32;
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    if rng() < 0.85 {
                        let j = (rng() - 0.5) * 20.0;
                        t.px(cx + dx, cy + dy, [spec[0] + j, spec[1] + j, spec[2] + j]);
                    }
                }
            }
        }
    }
    p.register("coal_ore", ore([38.0, 38.0, 40.0]));
    p.register("iron_ore", ore([216.0, 175.0, 147.0]));

    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_builds_deterministically() {
        let a = build_atlas(&default_painters());
        let b = build_atlas(&default_painters());
        assert_eq!(a.pixels, b.pixels);
        assert_eq!(a.indices.len(), 18);
        // stone tile is fully opaque, leaves tile has holes
        let stone = *a.indices.get("stone").unwrap() as usize;
        let leaves = *a.indices.get("leaves").unwrap() as usize;
        let tile_alpha = |idx: usize| -> Vec<u8> {
            let (tx, ty) = (idx % ATLAS_TILES * TILE_SIZE, idx / ATLAS_TILES * TILE_SIZE);
            (0..TILE_SIZE * TILE_SIZE)
                .map(|i| a.pixels[((ty + i / TILE_SIZE) * ATLAS_PX + tx + i % TILE_SIZE) * 4 + 3])
                .collect()
        };
        assert!(tile_alpha(stone).iter().all(|&a| a == 255));
        assert!(tile_alpha(leaves).iter().any(|&a| a == 0));
    }

    /// A throwaway scratch directory, removed when the guard drops.
    struct TempDir(PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn temp_dir() -> TempDir {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("craftmjne-atlas-test-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn write_png(path: &Path, w: u32, h: u32, pixel: [u8; 4]) {
        let img = image::RgbaImage::from_fn(w, h, |_, _| image::Rgba(pixel));
        img.save(path).unwrap();
    }

    #[test]
    fn load_custom_tile_returns_none_when_the_file_is_missing() {
        let dir = temp_dir();
        assert!(load_custom_tile(&dir.0, "nonexistent").is_none());
    }

    #[test]
    fn load_custom_tile_reads_a_correctly_sized_png() {
        let dir = temp_dir();
        let pixel = [10, 20, 30, 255];
        write_png(&dir.0.join("ruby.png"), TILE_SIZE as u32, TILE_SIZE as u32, pixel);
        let pixels = load_custom_tile(&dir.0, "ruby").expect("file exists and is the right size");
        assert_eq!(pixels.len(), TILE_SIZE * TILE_SIZE * 4);
        assert_eq!(&pixels[0..4], &pixel);
    }

    #[test]
    #[should_panic(expected = "must be exactly")]
    fn load_custom_tile_rejects_the_wrong_dimensions() {
        let dir = temp_dir();
        write_png(&dir.0.join("ruby.png"), 32, 32, [10, 20, 30, 255]);
        load_custom_tile(&dir.0, "ruby");
    }
}
