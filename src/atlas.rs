//! Texture atlas: packs tiles into one RGBA image, so every chunk renders
//! with a single material. Every tile is procedurally painted at
//! `BASE_TILE_SIZE` (16x16) by default (no image assets required to run
//! the project) - but any tile can be overridden by dropping a real
//! `textures/blocks/<name>.png` next to the executable (or the repo root,
//! for `cargo run`/tests): any size in [`ALLOWED_TILE_SIZES`], checked
//! before the procedural painter for that name ever runs. See
//! `textures/blocks/README.md` for the full list of names the built-in
//! blocks look for.
//!
//! **The whole atlas runs at one resolution, chosen automatically.** It's
//! the largest size found among whatever custom textures are supplied
//! (`BASE_TILE_SIZE` if none are) - like a Minecraft resource pack, not a
//! per-tile choice. Procedural tiles and any custom tile smaller than that
//! get nearest-neighbor upscaled to match, so pixel art stays crisp instead
//! of blurring, and everything - the mesher's UVs, baked inventory icons,
//! the GPU texture itself - follows the same chosen resolution
//! (`Tables::tile_size`, threaded from `BlockRegistry::compile`).
//!
//! Extension point: push your own painter into the [`Painters`] resource from
//! your plugin's `build()`:
//! ```ignore
//! painters.register("ruby", |p, rng| { /* p.px(x, y, [r, g, b, a]) */ });
//! ```

use bevy::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::ATLAS_TILES;
use crate::noise::{hash_str, mulberry32};

/// The resolution every procedural painter draws at natively, and the
/// minimum/default atlas resolution when no custom textures are supplied.
pub const BASE_TILE_SIZE: usize = 16;
/// Every size a `textures/blocks/*.png` file is allowed to be - a plain
/// multiple of `BASE_TILE_SIZE`, so nearest-neighbor upscaling from the
/// base procedural resolution (or from a smaller custom tile) always lines
/// up on exact pixel boundaries with no fractional blending.
pub const ALLOWED_TILE_SIZES: [usize; 3] = [16, 32, 64];

const S: i32 = BASE_TILE_SIZE as i32;

/// Draws one `BASE_TILE_SIZE`x`BASE_TILE_SIZE` tile into a buffer whose row
/// stride is `stride` pixels - decoupled from the buffer's own width so
/// the same type paints both a small native-resolution scratch tile
/// (`stride == BASE_TILE_SIZE`, later upscaled if the atlas resolution
/// ended up larger) and directly into the shared atlas at its final
/// position when the atlas is running at the base resolution (`stride ==
/// atlas row width`). Every painter is written assuming a 16x16 canvas -
/// this never varies, regardless of the atlas's eventual resolution;
/// upscaling happens as a separate blit step in `build_atlas`, not by
/// asking painters to draw bigger. Coordinates are tile-local;
/// out-of-range is clipped.
pub struct TilePainter<'a> {
    buf: &'a mut [u8],
    x0: usize,
    y0: usize,
    stride: usize,
}

impl TilePainter<'_> {
    pub fn px(&mut self, x: i32, y: i32, c: [f32; 3]) {
        self.pxa(x, y, [c[0], c[1], c[2], 255.0]);
    }

    pub fn pxa(&mut self, x: i32, y: i32, c: [f32; 4]) {
        if !(0..S).contains(&x) || !(0..S).contains(&y) {
            return;
        }
        let i = ((self.y0 + y as usize) * self.stride + self.x0 + x as usize) * 4;
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

/// Whether a texture ended up as real art or the auto-generated "missing
/// texture" placeholder - shared between `atlas.rs` (block/UI tiles) and
/// `sky.rs` (sun/moon), aggregated by `texture_report::TextureReport` for
/// `/texture-report` (`commands.rs`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextureStatus {
    /// Real art (procedural, or a valid custom override) - working as
    /// intended.
    Working,
    /// Fell back to the checkerboard placeholder - a custom file existed
    /// but failed to load, or nothing was ever registered for this name.
    /// The game keeps running; this is a visible, non-fatal problem.
    Placeholder,
}

/// Ordered painter list; atlas tile index = registration order.
#[derive(Resource, Default)]
pub struct Painters {
    entries: Vec<(String, PaintFn)>,
    /// Names that got the automatic placeholder via `ensure_registered`
    /// rather than a real hand-written painter - tracked so
    /// `build_atlas_from_dir` can tell "no custom art, using the block's own
    /// intended procedural painter" (working as intended) apart from
    /// "nobody ever wrote a painter for this name" (broken-but-functioning),
    /// which otherwise look identical by the time `build_atlas` runs
    /// `paint` for either case.
    placeholders: std::collections::HashSet<String>,
}

impl Painters {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        name: &str,
        f: impl Fn(&mut TilePainter, &mut dyn FnMut() -> f32) + Send + Sync + 'static,
    ) {
        assert!(
            self.entries.iter().all(|(n, _)| n != name),
            "painter {name:?} already registered"
        );
        self.entries.push((name.into(), Box::new(f)));
    }

    fn contains(&self, name: &str) -> bool {
        self.entries.iter().any(|(n, _)| n == name)
    }

    /// Registers the "missing texture" placeholder painter under `name` if
    /// nothing's registered there yet - a no-op otherwise. Lets
    /// `texture_scheme`-derived names (see `blocks::TextureScheme`) resolve
    /// to *something* renderable without every block author having to
    /// hand-write a procedural painter (or supply real art) for every
    /// derived name up front; drop a matching `textures/blocks/<name>.png`
    /// in later and it overrides this exactly like any other tile.
    pub fn ensure_registered(&mut self, name: &str) {
        if !self.contains(name) {
            self.register(name, missing_texture_painter);
            self.placeholders.insert(name.to_string());
        }
    }
}

/// A checkerboard magenta/black "missing texture" placeholder - the classic
/// game-dev signal that a tile has no real art yet, instead of either
/// crashing or silently reusing some other block's texture. `rng` is
/// per-name (seeded from the tile's own name, see `build_atlas_from_dir`),
/// so different placeholder tiles still look distinguishable from each
/// other rather than being bit-for-bit identical.
fn missing_texture_painter(t: &mut TilePainter, rng: &mut dyn FnMut() -> f32) {
    for y in 0..S {
        for x in 0..S {
            let j = (rng() - 0.5) * 10.0;
            let c = if (x / 4 + y / 4) % 2 == 0 { [230.0, 30.0, 220.0] } else { [20.0, 20.0, 20.0] };
            t.px(x, y, [c[0] + j, c[1] + j, c[2] + j]);
        }
    }
}

pub struct AtlasData {
    /// RGBA8, `atlas_px() x atlas_px()` (`atlas_px() == ATLAS_TILES *
    /// tile_size`).
    pub pixels: Vec<u8>,
    pub indices: HashMap<String, u16>,
    /// The resolution this atlas actually got built at - see the module
    /// docs. One of [`ALLOWED_TILE_SIZES`].
    pub tile_size: usize,
    /// Per-name load outcome, same order as `painters` was registered in -
    /// what `world::compile_content` feeds into `texture_report::TextureReport`.
    pub texture_status: Vec<(String, TextureStatus)>,
}

impl AtlasData {
    pub fn atlas_px(&self) -> usize {
        ATLAS_TILES * self.tile_size
    }
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

/// The outcome of looking for a hand-supplied `<dir>/<name>.png`.
enum CustomTile {
    /// No file at this path - fall back to the procedural painter, since
    /// that's the whole point of the override mechanism existing at all.
    Absent,
    /// A file exists but failed to decode, or wasn't square/one of
    /// [`ALLOWED_TILE_SIZES`] - falls back to the "missing texture"
    /// placeholder (not the block's real procedural painter, and not a
    /// crash) so a broken custom file is visibly, distinctly wrong instead
    /// of either taking down the game or silently rendering normal-looking
    /// art that would mask the mistake.
    Malformed,
    Loaded(Vec<u8>, usize),
}

/// Loads a hand-supplied tile for `name` from `<dir>/<name>.png`, decoded to
/// raw RGBA8 plus its native size. See [`CustomTile`] for what each outcome
/// means - nothing here panics; a malformed file degrades to the
/// placeholder via `build_atlas_from_dir` instead of crashing the game.
fn load_custom_tile(dir: &Path, name: &str) -> CustomTile {
    let path = dir.join(format!("{name}.png"));
    if !path.is_file() {
        return CustomTile::Absent;
    }
    let Ok(img) = image::open(&path) else {
        return CustomTile::Malformed;
    };
    let img = img.into_rgba8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    if w != h || !ALLOWED_TILE_SIZES.contains(&w) {
        return CustomTile::Malformed;
    }
    CustomTile::Loaded(img.into_raw(), w)
}

/// Nearest-neighbor upscales a square `src_size`x`src_size` RGBA8 buffer to
/// `dst_size`x`dst_size` (a whole-number multiple of `src_size` - callers
/// only ever scale between sizes drawn from [`ALLOWED_TILE_SIZES`], so this
/// is always exact, no fractional sampling). Nearest-neighbor rather than
/// any blurring filter deliberately - it replicates pixels instead of
/// blending them, so pixel art stays crisp instead of turning to mush.
fn upscale_nearest(src: &[u8], src_size: usize, dst_size: usize) -> Vec<u8> {
    let factor = dst_size / src_size;
    let mut dst = vec![0u8; dst_size * dst_size * 4];
    for y in 0..dst_size {
        let sy = y / factor;
        for x in 0..dst_size {
            let sx = x / factor;
            let si = (sy * src_size + sx) * 4;
            let di = (y * dst_size + x) * 4;
            dst[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    dst
}

pub fn build_atlas(painters: &Painters) -> AtlasData {
    build_atlas_from_dir(painters, &find_textures_dir())
}

/// The real logic behind `build_atlas`, taking an explicit textures
/// directory rather than resolving one internally - split out purely so
/// tests can point it at a controlled scratch directory instead of the
/// real `textures/blocks/` (see `atlas_uses_a_larger_custom_texture_as_
/// the_whole_atlas_resolution` below). `build_atlas` itself, used
/// everywhere else, is unaffected - it always resolves the real directory.
fn build_atlas_from_dir(painters: &Painters, textures_dir: &Path) -> AtlasData {
    assert!(
        painters.entries.len() <= ATLAS_TILES * ATLAS_TILES,
        "too many textures for a {ATLAS_TILES}x{ATLAS_TILES} atlas"
    );

    // Every custom tile gets decoded once up front, both to pick the
    // atlas's resolution (the largest one found, `BASE_TILE_SIZE` if none)
    // and so the second pass below never re-reads a file from disk.
    let custom: Vec<CustomTile> =
        painters.entries.iter().map(|(name, _)| load_custom_tile(textures_dir, name)).collect();
    let tile_size = custom
        .iter()
        .filter_map(|c| match c {
            CustomTile::Loaded(_, size) => Some(*size),
            _ => None,
        })
        .max()
        .unwrap_or(BASE_TILE_SIZE);

    let atlas_px = ATLAS_TILES * tile_size;
    let mut pixels = vec![0u8; atlas_px * atlas_px * 4];
    let mut indices = HashMap::new();
    let mut texture_status = Vec::with_capacity(painters.entries.len());
    for (i, (name, paint)) in painters.entries.iter().enumerate() {
        let x0 = (i % ATLAS_TILES) * tile_size;
        let y0 = (i / ATLAS_TILES) * tile_size;

        let (tile_pixels, status): (Vec<u8>, TextureStatus) = match &custom[i] {
            CustomTile::Loaded(pixels, size) if *size == tile_size => (pixels.clone(), TextureStatus::Working),
            CustomTile::Loaded(pixels, size) => {
                (upscale_nearest(pixels, *size, tile_size), TextureStatus::Working)
            }
            // A broken custom file: paint the placeholder itself (not this
            // name's real painter) so the problem stays visible instead of
            // silently rendering normal-looking procedural art.
            CustomTile::Malformed => {
                (painted_tile(&missing_texture_painter, name, tile_size), TextureStatus::Placeholder)
            }
            CustomTile::Absent => {
                let status = if painters.placeholders.contains(name) {
                    TextureStatus::Placeholder
                } else {
                    TextureStatus::Working
                };
                (painted_tile(&**paint, name, tile_size), status)
            }
        };
        for y in 0..tile_size {
            let src = y * tile_size * 4;
            let dst = ((y0 + y) * atlas_px + x0) * 4;
            pixels[dst..dst + tile_size * 4].copy_from_slice(&tile_pixels[src..src + tile_size * 4]);
        }
        indices.insert(name.clone(), i as u16);
        texture_status.push((name.clone(), status));
    }
    AtlasData { pixels, indices, tile_size, texture_status }
}

/// Runs `paint` into a fresh `BASE_TILE_SIZE` scratch tile (seeded from
/// `name`, so the same name always paints the same way) and upscales to
/// `tile_size` if the atlas resolution ended up larger - the one place
/// this happens, shared by both a name's own painter (the common case) and
/// the placeholder painter standing in for a malformed custom file.
fn painted_tile(paint: &(dyn Fn(&mut TilePainter, &mut dyn FnMut() -> f32) + Send + Sync), name: &str, tile_size: usize) -> Vec<u8> {
    let mut scratch = vec![0u8; BASE_TILE_SIZE * BASE_TILE_SIZE * 4];
    let mut tile = TilePainter { buf: &mut scratch, x0: 0, y0: 0, stride: BASE_TILE_SIZE };
    let mut rng = mulberry32(hash_str(name));
    paint(&mut tile, &mut rng);
    if tile_size == BASE_TILE_SIZE {
        scratch
    } else {
        upscale_nearest(&scratch, BASE_TILE_SIZE, tile_size)
    }
}

/// The default 16x16 pixel-art set for the built-in blocks.
pub fn default_painters() -> Painters {
    let mut p = Painters::new();

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

    // A wall-torch look on every face: a dark backing, a wooden stick up the
    // middle, and a bright flame at the top. The block is a plain solid cube
    // for now (see `blocks/torch.json`) - it's the lighting that makes it a
    // torch, so the texture is what has to carry the shape.
    p.register("torch", |t, rng| {
        t.noisy_fill(rng, [38.0, 32.0, 28.0], 12.0);
        for y in 0..10 {
            for x in 7..9 {
                let j = (rng() - 0.5) * 16.0;
                t.px(x, y, [126.0 + j, 92.0 + j, 54.0 + j]);
            }
        }
        // Flame: a hot near-white core fading out to orange at the edges.
        for (y, half) in [(10, 2), (11, 2), (12, 2), (13, 1), (14, 1)] {
            for x in (8 - half)..(8 + half) {
                let core = x == 7 || x == 8;
                let j = (rng() - 0.5) * 22.0;
                let c: [f32; 3] =
                    if core && y < 13 { [255.0, 244.0, 190.0] } else { [252.0, 168.0, 54.0] };
                t.px(x, y, [c[0] + j, c[1] + j, c[2] + j]);
            }
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
        assert_eq!(a.indices.len(), 19);
        // No custom textures are supplied in this test run, so the atlas
        // stays at the base procedural resolution.
        assert_eq!(a.tile_size, BASE_TILE_SIZE);
        // stone tile is fully opaque, leaves tile has holes
        let stone = *a.indices.get("stone").unwrap() as usize;
        let leaves = *a.indices.get("leaves").unwrap() as usize;
        let tile_alpha = |idx: usize| -> Vec<u8> {
            let (tx, ty) = (idx % ATLAS_TILES * a.tile_size, idx / ATLAS_TILES * a.tile_size);
            (0..a.tile_size * a.tile_size)
                .map(|i| a.pixels[((ty + i / a.tile_size) * a.atlas_px() + tx + i % a.tile_size) * 4 + 3])
                .collect()
        };
        assert!(tile_alpha(stone).iter().all(|&a| a == 255));
        assert!(tile_alpha(leaves).iter().any(|&a| a == 0));
    }

    #[test]
    fn ensure_registered_only_adds_a_painter_when_the_name_is_missing() {
        let mut painters = Painters::new();
        painters.register("stone", |t, rng| t.noisy_fill(rng, [1.0, 1.0, 1.0], 0.0));
        assert_eq!(painters.entries.len(), 1);

        painters.ensure_registered("stone"); // already registered - no-op
        assert_eq!(painters.entries.len(), 1);
        assert!(!painters.placeholders.contains("stone"));

        painters.ensure_registered("ruby"); // missing - gets the placeholder
        assert_eq!(painters.entries.len(), 2);
        assert!(painters.contains("ruby"));
        assert!(painters.placeholders.contains("ruby"));
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
    fn load_custom_tile_returns_absent_when_the_file_is_missing() {
        let dir = temp_dir();
        assert!(matches!(load_custom_tile(&dir.0, "nonexistent"), CustomTile::Absent));
    }

    #[test]
    fn load_custom_tile_reads_a_correctly_sized_png() {
        let dir = temp_dir();
        let pixel = [10, 20, 30, 255];
        write_png(&dir.0.join("ruby.png"), BASE_TILE_SIZE as u32, BASE_TILE_SIZE as u32, pixel);
        let CustomTile::Loaded(pixels, size) = load_custom_tile(&dir.0, "ruby") else {
            panic!("expected a loaded tile");
        };
        assert_eq!(size, BASE_TILE_SIZE);
        assert_eq!(pixels.len(), BASE_TILE_SIZE * BASE_TILE_SIZE * 4);
        assert_eq!(&pixels[0..4], &pixel);
    }

    #[test]
    fn load_custom_tile_accepts_every_allowed_larger_size() {
        let dir = temp_dir();
        for &size in &ALLOWED_TILE_SIZES {
            write_png(&dir.0.join("ruby.png"), size as u32, size as u32, [1, 2, 3, 255]);
            let CustomTile::Loaded(_, detected) = load_custom_tile(&dir.0, "ruby") else {
                panic!("expected a loaded tile at size {size}");
            };
            assert_eq!(detected, size);
        }
    }

    #[test]
    fn load_custom_tile_reports_malformed_for_a_disallowed_size() {
        let dir = temp_dir();
        write_png(&dir.0.join("ruby.png"), 20, 20, [10, 20, 30, 255]);
        assert!(matches!(load_custom_tile(&dir.0, "ruby"), CustomTile::Malformed));
    }

    #[test]
    fn load_custom_tile_reports_malformed_for_an_unreadable_file() {
        let dir = temp_dir();
        std::fs::write(dir.0.join("ruby.png"), b"not actually a png").unwrap();
        assert!(matches!(load_custom_tile(&dir.0, "ruby"), CustomTile::Malformed));
    }

    #[test]
    fn a_malformed_custom_texture_falls_back_to_the_placeholder_instead_of_panicking() {
        let dir = temp_dir();
        write_png(&dir.0.join("stone.png"), 20, 20, [10, 20, 30, 255]); // disallowed size
        let atlas = build_atlas_from_dir(&default_painters(), &dir.0);

        let status: std::collections::HashMap<_, _> = atlas.texture_status.iter().cloned().collect();
        assert_eq!(status["stone"], TextureStatus::Placeholder);
        // Everything else, untouched by the malformed file, is still working.
        assert_eq!(status["dirt"], TextureStatus::Working);
    }

    #[test]
    fn upscale_nearest_replicates_each_source_pixel_into_a_block() {
        // 2x2 source -> 4x4 dest (factor 2): each source pixel becomes a
        // 2x2 block of identical pixels, not a blend of its neighbours.
        let src = [
            [255u8, 0, 0, 255], [0, 255, 0, 255],
            [0, 0, 255, 255], [255, 255, 0, 255],
        ];
        let mut buf = vec![0u8; 2 * 2 * 4];
        for (i, px) in src.iter().enumerate() {
            buf[i * 4..i * 4 + 4].copy_from_slice(px);
        }
        let up = upscale_nearest(&buf, 2, 4);
        assert_eq!(up.len(), 4 * 4 * 4);
        // top-left 2x2 block all matches the original top-left pixel
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let i = (y * 4 + x) * 4;
            assert_eq!(&up[i..i + 4], &src[0]);
        }
        // bottom-right 2x2 block all matches the original bottom-right pixel
        for (x, y) in [(2, 2), (3, 2), (2, 3), (3, 3)] {
            let i = (y * 4 + x) * 4;
            assert_eq!(&up[i..i + 4], &src[3]);
        }
    }

    #[test]
    fn atlas_uses_a_larger_custom_texture_as_the_whole_atlas_resolution() {
        let dir = temp_dir();
        // Only "stone" gets a custom (larger) texture - everything else,
        // including every other custom-less procedural tile, must still
        // upscale to match, since the whole atlas runs at one resolution.
        write_png(&dir.0.join("stone.png"), 32, 32, [200, 10, 10, 255]);
        let atlas = build_atlas_from_dir(&default_painters(), &dir.0);

        assert_eq!(atlas.tile_size, 32, "one 32x32 custom tile should raise the whole atlas to 32");
        assert_eq!(atlas.pixels.len(), atlas.atlas_px() * atlas.atlas_px() * 4);

        // The custom stone tile's corner pixel is exactly what was supplied.
        let stone = *atlas.indices.get("stone").unwrap() as usize;
        let (tx, ty) = (stone % ATLAS_TILES * 32, stone / ATLAS_TILES * 32);
        let i = (ty * atlas.atlas_px() + tx) * 4;
        assert_eq!(&atlas.pixels[i..i + 4], &[200, 10, 10, 255]);

        // A procedural (non-custom) tile, e.g. "dirt", got upscaled 2x2 per
        // source pixel rather than staying a native-resolution 16x16 block
        // sitting in the corner of a 32x32 slot - spot-check that its
        // (0,0) and (1,1) pixels (which a 2x nearest-neighbor upscale maps
        // to the exact same source pixel) match exactly.
        let dirt = *atlas.indices.get("dirt").unwrap() as usize;
        let (dx, dy) = (dirt % ATLAS_TILES * 32, dirt / ATLAS_TILES * 32);
        let px = |x: usize, y: usize| {
            let i = ((dy + y) * atlas.atlas_px() + dx + x) * 4;
            &atlas.pixels[i..i + 4]
        };
        assert_eq!(px(0, 0), px(1, 1), "a 2x nearest-neighbor upscale must replicate, not blend");
    }
}
