//! The player model: six boxes in the classic blocky layout, and the skin
//! image they're textured from. Pure geometry - nothing here touches Bevy's
//! ECS (`entities.rs` turns it into a mesh and an entity).
//!
//! Sizes are in "pixels" of a 16-pixel block: head 8x8x8, body 8x12x4, each
//! arm and leg 4x12x4, so the figure is 32 pixels (two blocks) tall and 16
//! (one block) across at scale 1. [`ModelData::build`] takes the height the
//! figure should be and scales to it, which is how the player's 1.8 and the
//! dummy's 2.0 can share one model.
//!
//! The figure is centred on the feet's x/z and faces **-Z** (the direction a
//! player with yaw 0 looks), so an entity's yaw rotates it the same way the
//! player's does. Its right hand is therefore on +X.
//!
//! **Skin layout** is the standard 64x64 one: each box owns a rectangle of
//! `(2d + 2w) x (d + h)` pixels at its `uv` origin, unwrapped as
//! `[right | front | left | back]` below a row holding `[top | bottom]`.
//! Anything not claimed by a box is unused (transparent in the blank skin).

pub const SKIN_SIZE: u32 = 64;
/// Height of the figure in pixels, i.e. the scale-1 height in 1/16 blocks.
pub const MODEL_PIXELS: f32 = 32.0;

#[derive(Clone, Copy, Debug)]
pub struct Part {
    pub name: &'static str,
    /// Minimum corner `[x, y, z]` in pixels, relative to the feet's centre.
    pub min: [f32; 3],
    /// Size `[w, h, d]` in pixels.
    pub size: [f32; 3],
    /// Top-left of this box's rectangle on the skin.
    pub uv: [u32; 2],
}

pub const PARTS: [Part; 6] = [
    Part { name: "head", min: [-4.0, 24.0, -4.0], size: [8.0, 8.0, 8.0], uv: [0, 0] },
    Part { name: "body", min: [-4.0, 12.0, -2.0], size: [8.0, 12.0, 4.0], uv: [16, 16] },
    Part { name: "right_arm", min: [4.0, 12.0, -2.0], size: [4.0, 12.0, 4.0], uv: [40, 16] },
    Part { name: "left_arm", min: [-8.0, 12.0, -2.0], size: [4.0, 12.0, 4.0], uv: [32, 48] },
    Part { name: "right_leg", min: [0.0, 0.0, -2.0], size: [4.0, 12.0, 4.0], uv: [0, 16] },
    Part { name: "left_leg", min: [-4.0, 0.0, -2.0], size: [4.0, 12.0, 4.0], uv: [16, 48] },
];

/// How much each face is darkened, so a single flat skin colour still reads
/// as a solid shape under this game's unlit rendering: top, bottom, then the
/// +X, -X, +Z, -Z sides.
const FACE_SHADE: [f32; 6] = [1.0, 0.55, 0.82, 0.82, 0.9, 0.7];

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// One face of a box: where on the skin it reads from, and how it sits in
/// space (`origin` plus `u`/`v` edge vectors, in pixels, relative to the box
/// minimum).
struct Face {
    normal: [f32; 3],
    shade: f32,
    rect: Rect,
    origin: [f32; 3],
    u: [f32; 3],
    v: [f32; 3],
}

fn faces(part: &Part) -> [Face; 6] {
    let [w, h, d] = part.size;
    let [u0, v0] = part.uv;
    let (wi, hi, di) = (w as u32, h as u32, d as u32);
    let rect = |x, y, rw, rh| Rect { x, y, w: rw, h: rh };
    [
        // +Y top: its bottom edge meets the front's top edge.
        Face {
            normal: [0.0, 1.0, 0.0],
            shade: FACE_SHADE[0],
            rect: rect(u0 + di, v0, wi, di),
            origin: [w, h, d],
            u: [-w, 0.0, 0.0],
            v: [0.0, 0.0, -d],
        },
        // -Y bottom.
        Face {
            normal: [0.0, -1.0, 0.0],
            shade: FACE_SHADE[1],
            rect: rect(u0 + di + wi, v0, wi, di),
            origin: [w, 0.0, 0.0],
            u: [-w, 0.0, 0.0],
            v: [0.0, 0.0, d],
        },
        // +X: the figure's right side; u runs toward the front (-Z).
        Face {
            normal: [1.0, 0.0, 0.0],
            shade: FACE_SHADE[2],
            rect: rect(u0, v0 + di, di, hi),
            origin: [w, h, d],
            u: [0.0, 0.0, -d],
            v: [0.0, -h, 0.0],
        },
        // -X: the left side; u runs away from the front.
        Face {
            normal: [-1.0, 0.0, 0.0],
            shade: FACE_SHADE[3],
            rect: rect(u0 + di + wi, v0 + di, di, hi),
            origin: [0.0, h, 0.0],
            u: [0.0, 0.0, d],
            v: [0.0, -h, 0.0],
        },
        // +Z: the back.
        Face {
            normal: [0.0, 0.0, 1.0],
            shade: FACE_SHADE[4],
            rect: rect(u0 + 2 * di + wi, v0 + di, wi, hi),
            origin: [0.0, h, d],
            u: [w, 0.0, 0.0],
            v: [0.0, -h, 0.0],
        },
        // -Z: the front. Seen from in front, +X is on the viewer's left.
        Face {
            normal: [0.0, 0.0, -1.0],
            shade: FACE_SHADE[5],
            rect: rect(u0 + di, v0 + di, wi, hi),
            origin: [w, h, 0.0],
            u: [-w, 0.0, 0.0],
            v: [0.0, -h, 0.0],
        },
    ]
}

/// The skin rectangles one box claims, for tests and the skin painter.
pub fn part_rects(part: &Part) -> Vec<Rect> {
    faces(part).iter().map(|f| f.rect).collect()
}

/// Flat vertex data for the whole figure, ready for a mesh.
#[derive(Clone, Debug, Default)]
pub struct ModelData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Per-vertex RGBA, grey: the face shading, multiplied into the skin.
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

impl ModelData {
    /// The figure scaled so it stands `height` blocks tall, feet at y = 0.
    pub fn build(height: f32) -> Self {
        let scale = height / MODEL_PIXELS;
        let mut data = ModelData::default();
        for part in &PARTS {
            for face in faces(part) {
                let base = data.positions.len() as u32;
                let corners = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
                for (a, b) in corners {
                    let p = [
                        part.min[0] + face.origin[0] + a * face.u[0] + b * face.v[0],
                        part.min[1] + face.origin[1] + a * face.u[1] + b * face.v[1],
                        part.min[2] + face.origin[2] + a * face.u[2] + b * face.v[2],
                    ];
                    data.positions.push([p[0] * scale, p[1] * scale, p[2] * scale]);
                    data.normals.push(face.normal);
                    let uv_x = face.rect.x as f32 + a * face.rect.w as f32;
                    let uv_y = face.rect.y as f32 + b * face.rect.h as f32;
                    data.uvs.push([uv_x / SKIN_SIZE as f32, uv_y / SKIN_SIZE as f32]);
                    data.colors.push([face.shade, face.shade, face.shade, 1.0]);
                }
                // Wind counter-clockwise as seen from outside.
                let cross = cross3(face.u, face.v);
                let outward = dot3(cross, face.normal) > 0.0;
                let quad: [u32; 6] = if outward { [0, 1, 2, 0, 2, 3] } else { [0, 2, 1, 0, 3, 2] };
                data.indices.extend(quad.iter().map(|i| base + i));
            }
        }
        data
    }

    /// Smallest and largest corner of the whole figure.
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for p in &self.positions {
            for i in 0..3 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
        (lo, hi)
    }
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The blank skin: every face a plain light grey, the rest of the image
/// transparent. RGBA8, `SKIN_SIZE` x `SKIN_SIZE`.
pub fn blank_skin() -> Vec<u8> {
    const GREY: [u8; 4] = [206, 206, 212, 255];
    let size = SKIN_SIZE as usize;
    let mut pixels = vec![0u8; size * size * 4];
    for part in &PARTS {
        for rect in part_rects(part) {
            for y in rect.y..rect.y + rect.h {
                for x in rect.x..rect.x + rect.w {
                    let i = (y as usize * size + x as usize) * 4;
                    pixels[i..i + 4].copy_from_slice(&GREY);
                }
            }
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_figure_is_two_blocks_tall_and_one_wide_at_scale_one() {
        let (lo, hi) = ModelData::build(2.0).bounds();
        assert!((lo[1]).abs() < 1e-6, "feet at y=0");
        assert!((hi[1] - 2.0).abs() < 1e-6, "{hi:?}");
        assert!((hi[0] - lo[0] - 1.0).abs() < 1e-6, "arm to arm is a block");
        assert!((hi[0] + lo[0]).abs() < 1e-6, "centred on x");
        assert!((hi[2] + lo[2]).abs() < 1e-6, "centred on z");
    }

    #[test]
    fn it_scales_to_the_height_asked_for() {
        let (_, hi) = ModelData::build(1.8).bounds();
        assert!((hi[1] - 1.8).abs() < 1e-6);
    }

    #[test]
    fn no_two_boxes_claim_the_same_skin_pixel_and_all_fit_the_image() {
        let mut claimed = vec![false; (SKIN_SIZE * SKIN_SIZE) as usize];
        for part in &PARTS {
            for r in part_rects(part) {
                assert!(r.x + r.w <= SKIN_SIZE && r.y + r.h <= SKIN_SIZE, "{} leaves the skin", part.name);
                for y in r.y..r.y + r.h {
                    for x in r.x..r.x + r.w {
                        let i = (y * SKIN_SIZE + x) as usize;
                        assert!(!claimed[i], "{}: pixel ({x},{y}) is claimed twice", part.name);
                        claimed[i] = true;
                    }
                }
            }
        }
    }

    #[test]
    fn every_face_is_the_right_size_on_the_skin() {
        for part in &PARTS {
            let [w, h, d] = part.size;
            let rects = part_rects(part);
            let dims: Vec<(u32, u32)> = rects.iter().map(|r| (r.w, r.h)).collect();
            let want = [(w, d), (w, d), (d, h), (d, h), (w, h), (w, h)].map(|(a, b)| (a as u32, b as u32));
            assert_eq!(dims, want, "{}", part.name);
        }
    }

    #[test]
    fn every_face_winds_outward_and_uvs_stay_on_the_skin() {
        let data = ModelData::build(2.0);
        assert_eq!(data.positions.len(), 6 * 6 * 4);
        assert_eq!(data.indices.len(), 6 * 6 * 6);
        for tri in data.indices.chunks(3) {
            let p: Vec<[f32; 3]> = tri.iter().map(|&i| data.positions[i as usize]).collect();
            let e1 = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
            let e2 = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
            let n = data.normals[tri[0] as usize];
            assert!(dot3(cross3(e1, e2), n) > 0.0, "triangle {tri:?} winds against its normal");
        }
        for uv in &data.uvs {
            assert!((0.0..=1.0).contains(&uv[0]) && (0.0..=1.0).contains(&uv[1]), "{uv:?}");
        }
    }

    #[test]
    fn the_front_faces_negative_z_and_the_right_arm_is_on_positive_x() {
        let data = ModelData::build(2.0);
        // The head's front face is the one reading from (8, 8) on the skin.
        let front: Vec<_> = data
            .uvs
            .iter()
            .enumerate()
            .filter(|(i, uv)| {
                (uv[0] - 8.0 / 64.0).abs() < 1e-6 && (uv[1] - 8.0 / 64.0).abs() < 1e-6 && data.normals[*i] == [0.0, 0.0, -1.0]
            })
            .collect();
        assert_eq!(front.len(), 1, "one corner sits at the front face's top-left");
        // Seen from the front, the face's top-left is the figure's +X side.
        assert!(data.positions[front[0].0][0] > 0.0);
        let right_arm = &PARTS[2];
        assert_eq!(right_arm.name, "right_arm");
        assert!(right_arm.min[0] > 0.0);
    }

    #[test]
    fn the_blank_skin_paints_exactly_the_claimed_pixels() {
        let skin = blank_skin();
        let opaque = skin.chunks(4).filter(|p| p[3] == 255).count();
        let claimed: u32 = PARTS.iter().flat_map(part_rects).map(|r| r.w * r.h).sum();
        assert_eq!(opaque as u32, claimed);
        assert_eq!(skin.len(), (SKIN_SIZE * SKIN_SIZE * 4) as usize);
    }
}
