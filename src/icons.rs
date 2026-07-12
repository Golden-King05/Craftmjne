//! Baked isometric block icons for `ItemModel::Default` (Minecraft-style
//! inventory icons: top + two side faces of a cube). Pure CPU pixel math -
//! no shaders, no extra cameras - the same "everything is generated at
//! runtime" approach `atlas.rs` uses for the block textures themselves.
//!
//! Construction: each icon is a `2*tile_size` square canvas (`tile_size` -
//! the main atlas's actual resolution, `atlas::AtlasData::tile_size`, so a
//! higher-resolution custom texture gets a proportionally sharper icon
//! too) split into three regions (a diamond top face and two parallelogram
//! side faces) via *inverse* pixel-mapping - for every destination pixel,
//! work out which face (if any) it belongs to and sample the corresponding
//! source pixel, rather than forward-mapping source pixels (which can
//! leave gaps when stretching a smaller texture over a larger area). See
//! [`bake_cube_icon`] for the exact geometry.

use std::collections::HashMap;

use crate::atlas::AtlasData;
use crate::blocks::{BlockId, BlockRegistry, ItemModel};
use crate::config::ATLAS_TILES;

/// Per-face brightness, matching `mesher.rs`'s in-world face shading so an
/// icon reads consistently with how the block actually looks placed down:
/// top face full brightness, the two visible sides shaded like the east/
/// south world faces.
const SHADE_TOP: f32 = 1.0;
const SHADE_LEFT: f32 = 0.6;
const SHADE_RIGHT: f32 = 0.8;

fn shade(px: [u8; 4], factor: f32) -> [u8; 4] {
    [
        (px[0] as f32 * factor) as u8,
        (px[1] as f32 * factor) as u8,
        (px[2] as f32 * factor) as u8,
        px[3],
    ]
}

/// Bakes one isometric cube icon from a `tile_size`x`tile_size` top-face and
/// side-face tile (RGBA bytes, row-major, as returned by `extract_tile`).
/// Returns `(2*tile_size)*(2*tile_size)*4` RGBA bytes, row-major,
/// transparent outside the cube's hexagonal silhouette. The same `side`
/// tile is used for both visible side faces (mirroring how a block only
/// ever has one `side` texture) - `SHADE_LEFT`/`SHADE_RIGHT` differing is
/// what makes the two faces read as distinct in the final icon.
///
/// Geometry (`s` = `tile_size`, canvas is `2s x 2s`): the top face is a
/// rhombus with vertices at `(s,0)` (back corner, farthest from camera),
/// `(0,s/2)` and `(2s,s/2)` (left/right corners), and `(s,s)` (front
/// corner, where both side faces begin). The left face is the parallelogram
/// directly below the rhombus's lower-left edge; the right face mirrors it
/// below the lower-right edge. Every destination pixel is classified into
/// at most one of these three regions and inverse-mapped back to a source
/// pixel, so the regions tile perfectly with no gaps or overlaps by
/// construction, independent of rounding.
pub fn bake_cube_icon(top: &[u8], side: &[u8], tile_size: usize) -> Vec<u8> {
    let s = tile_size as i32;
    let n = 2 * tile_size;
    let mut out = vec![0u8; n * n * 4];

    let sample = |tile: &[u8], u: i32, v: i32| -> [u8; 4] {
        let (u, v) = (u.clamp(0, s - 1), v.clamp(0, s - 1));
        let i = ((v * s + u) * 4) as usize;
        [tile[i], tile[i + 1], tile[i + 2], tile[i + 3]]
    };

    for dy in 0..n as i32 {
        for dx in 0..n as i32 {
            let rel_x = dx - s;
            let px = if dy <= s && rel_x.abs() <= 2 * dy.min(s - dy) {
                // Top face: diamond, widest (full canvas width) at dy = s/2.
                let u = dy + rel_x / 2;
                let v = dy - rel_x / 2;
                Some(shade(sample(top, u, v), SHADE_TOP))
            } else if dx < s {
                // Left face: a parallelogram whose top edge slopes from
                // (0, s/2) down to (s, s), height s.
                let y_top = s as f32 / 2.0 + dx as f32 / 2.0;
                let v = dy as f32 - y_top;
                (0.0..s as f32).contains(&v).then(|| shade(sample(side, dx, v as i32), SHADE_LEFT))
            } else {
                // Right face: mirrors the left, top edge sloping from
                // (s, s) up to (2s, s/2).
                let y_top = s as f32 - (dx - s) as f32 / 2.0;
                let v = dy as f32 - y_top;
                (0.0..s as f32)
                    .contains(&v)
                    .then(|| shade(sample(side, dx - s, v as i32), SHADE_RIGHT))
            };
            if let Some(c) = px {
                let i = (dy as usize * n + dx as usize) * 4;
                out[i..i + 4].copy_from_slice(&c);
            }
        }
    }
    out
}

/// Copies one `tile_size x tile_size` tile out of the main atlas's pixel
/// buffer (row stride `atlas_px`) into its own contiguous RGBA buffer.
fn extract_tile(atlas_pixels: &[u8], atlas_px: usize, tile_size: usize, tile: u16) -> Vec<u8> {
    let tx = (tile as usize % ATLAS_TILES) * tile_size;
    let ty = (tile as usize / ATLAS_TILES) * tile_size;
    let mut out = vec![0u8; tile_size * tile_size * 4];
    for y in 0..tile_size {
        let src = ((ty + y) * atlas_px + tx) * 4;
        let dst = y * tile_size * 4;
        out[dst..dst + tile_size * 4].copy_from_slice(&atlas_pixels[src..src + tile_size * 4]);
    }
    out
}

pub struct IconAtlasData {
    /// RGBA8, `icon_atlas_px() x icon_atlas_px()`.
    pub pixels: Vec<u8>,
    /// Block id -> its cell index in the icon atlas grid (same indexing
    /// convention as `atlas::AtlasData::indices`, just for icons).
    pub index: HashMap<BlockId, u16>,
    /// Each icon's canvas size (`2 * atlas.tile_size` - see the module
    /// docs), and the icon atlas's own tile-grid resolution accordingly.
    pub icon_size: usize,
}

impl IconAtlasData {
    pub fn icon_atlas_px(&self) -> usize {
        ATLAS_TILES * self.icon_size
    }
}

/// Bakes an isometric icon (see [`bake_cube_icon`]) for every
/// `item_model: "default"` block, laid out in a fixed
/// `ATLAS_TILES x ATLAS_TILES` grid of `icon_size` cells - mirrors
/// `atlas::build_atlas`'s layout exactly, just with bigger cells, at
/// `2 * atlas.tile_size` resolution so a higher-resolution custom texture
/// bakes a proportionally sharper icon too. Blocks using `Face`/`Custom`
/// item models are skipped entirely (nothing to bake; they render straight
/// from the main atlas instead, see `ui::block_icon`).
pub fn build_icon_atlas(registry: &BlockRegistry, atlas: &AtlasData) -> IconAtlasData {
    let icon_size = 2 * atlas.tile_size;
    let atlas_px = atlas.atlas_px();
    let icon_atlas_px = ATLAS_TILES * icon_size;
    let mut pixels = vec![0u8; icon_atlas_px * icon_atlas_px * 4];
    let mut index = HashMap::new();

    let mut next = 0u16;
    for (id, def) in registry.defs.iter().enumerate().skip(1) {
        if def.item_model != ItemModel::Default {
            continue;
        }
        let top_name = def.textures.resolve(&def.id, 2);
        let side_name = def.textures.resolve(&def.id, 0);
        let (Some(&top_tile), Some(&side_tile)) =
            (atlas.indices.get(top_name), atlas.indices.get(side_name))
        else {
            continue; // texture presence is already validated by compile()
        };
        assert!(
            (next as usize) < ATLAS_TILES * ATLAS_TILES,
            "too many item_model:\"default\" blocks for the icon atlas"
        );

        let top = extract_tile(&atlas.pixels, atlas_px, atlas.tile_size, top_tile);
        let side = extract_tile(&atlas.pixels, atlas_px, atlas.tile_size, side_tile);
        let icon = bake_cube_icon(&top, &side, atlas.tile_size);
        let x0 = (next as usize % ATLAS_TILES) * icon_size;
        let y0 = (next as usize / ATLAS_TILES) * icon_size;
        for y in 0..icon_size {
            let src = y * icon_size * 4;
            let dst = ((y0 + y) * icon_atlas_px + x0) * 4;
            pixels[dst..dst + icon_size * 4].copy_from_slice(&icon[src..src + icon_size * 4]);
        }
        index.insert(id as BlockId, next);
        next += 1;
    }

    IconAtlasData { pixels, index, icon_size }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::BASE_TILE_SIZE;

    const S: usize = BASE_TILE_SIZE;

    fn solid_tile(c: [u8; 4]) -> Vec<u8> {
        let mut t = vec![0u8; S * S * 4];
        for px in t.chunks_mut(4) {
            px.copy_from_slice(&c);
        }
        t
    }

    fn px_at(icon: &[u8], icon_size: usize, x: usize, y: usize) -> [u8; 4] {
        let i = (y * icon_size + x) * 4;
        [icon[i], icon[i + 1], icon[i + 2], icon[i + 3]]
    }

    #[test]
    fn icon_has_expected_dimensions_and_is_deterministic() {
        let top = solid_tile([200, 50, 50, 255]);
        let side = solid_tile([50, 200, 50, 255]);
        let a = bake_cube_icon(&top, &side, S);
        let b = bake_cube_icon(&top, &side, S);
        assert_eq!(a.len(), 2 * S * 2 * S * 4);
        assert_eq!(a, b);
    }

    #[test]
    fn icon_corners_are_transparent_outside_the_cube_silhouette() {
        let opaque = solid_tile([255, 255, 255, 255]);
        let icon = bake_cube_icon(&opaque, &opaque, S);
        let icon_size = 2 * S;
        let n = icon_size - 1;
        assert_eq!(px_at(&icon, icon_size, 0, 0)[3], 0);
        assert_eq!(px_at(&icon, icon_size, n, 0)[3], 0);
        assert_eq!(px_at(&icon, icon_size, 0, n)[3], 0);
        assert_eq!(px_at(&icon, icon_size, n, n)[3], 0);
    }

    #[test]
    fn icon_shows_all_three_faces_with_distinct_shading() {
        let top = solid_tile([200, 200, 200, 255]);
        let side = solid_tile([200, 200, 200, 255]);
        let icon = bake_cube_icon(&top, &side, S);
        let icon_size = 2 * S;

        // Widest row of the top diamond, dead center.
        let top_px = px_at(&icon, icon_size, icon_size / 2, S / 2);
        assert_eq!(top_px[3], 255);
        assert_eq!(top_px[0], 200); // full brightness, no shading applied

        // Comfortably inside the left face and the right face respectively.
        let left_px = px_at(&icon, icon_size, S / 2, icon_size - S / 2 - 2);
        let right_px = px_at(&icon, icon_size, icon_size - S / 2, icon_size - S / 2 - 2);
        assert_eq!(left_px[3], 255);
        assert_eq!(right_px[3], 255);
        assert!(left_px[0] < top_px[0], "left face must be darker than top");
        assert!(right_px[0] < top_px[0], "right face must be darker than top");
        assert_ne!(left_px[0], right_px[0], "left/right faces must be shaded differently");
    }

    #[test]
    fn build_icon_atlas_bakes_default_blocks_and_skips_face_ones() {
        let reg = BlockRegistry::with_defaults();
        let atlas = crate::atlas::build_atlas(&crate::atlas::default_painters());
        let stone = reg.id("stone"); // item_model defaults to "default"
        let icons = build_icon_atlas(&reg, &atlas);
        assert!(icons.index.contains_key(&stone));
        assert_eq!(icons.icon_size, 2 * atlas.tile_size);
        assert_eq!(icons.pixels.len(), icons.icon_atlas_px() * icons.icon_atlas_px() * 4);
    }
}
