//! Little pictures of controller buttons, so the Keybinds screen can show the button
//! itself (a green A, a red B, a bumper, a D-pad with the right arm lit) beside its name.
//!
//! Like the GUI skin they are painted procedurally at startup, so nothing needs art
//! files, and a texture pack or mod can replace any of them with
//! `textures/gui/glyphs/<button id>.png` (a whole multiple of [`SIZE`] square, any
//! scale, e.g. a 48x48 `south.png`). A file that is the wrong shape or won't decode is
//! ignored and the built-in picture stays. Face buttons use Xbox colours and letters;
//! the PlayStation shapes are in the button's label.
//!
//! Spawn a node with [`PadGlyph`]; [`apply_glyphs`] gives it the image.

use std::collections::HashMap;
use std::path::PathBuf;

use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::gamepad::PadButton;

/// Glyphs are painted on a 12x12 grid of "pixels" and drawn at twice that.
pub const SIZE: u32 = 12;
const SCALE: u32 = 2;

type Rgba = [u8; 4];

struct Canvas {
    px: Vec<Rgba>,
}

impl Canvas {
    fn new() -> Self {
        Self { px: vec![[0, 0, 0, 0]; (SIZE * SIZE) as usize] }
    }
    fn set(&mut self, x: i32, y: i32, c: Rgba) {
        if (0..SIZE as i32).contains(&x) && (0..SIZE as i32).contains(&y) {
            self.px[(y as u32 * SIZE + x as u32) as usize] = c;
        }
    }
    fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgba) {
        for y in y0..y1 {
            for x in x0..x1 {
                self.set(x, y, c);
            }
        }
    }
    fn disc(&mut self, r: f32, fill: Rgba, edge: Rgba) {
        let c = (SIZE as f32 - 1.0) / 2.0;
        for y in 0..SIZE as i32 {
            for x in 0..SIZE as i32 {
                let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
                if d <= r - 0.9 {
                    self.set(x, y, fill);
                } else if d <= r {
                    self.set(x, y, edge);
                }
            }
        }
    }
    /// A 3x5 letter, top-left at (x, y).
    fn letter(&mut self, ch: char, x: i32, y: i32, c: Rgba) {
        for (row, bits) in glyph_rows(ch).iter().enumerate() {
            for col in 0..3 {
                if bits & (0b100 >> col) != 0 {
                    self.set(x + col, y + row as i32, c);
                }
            }
        }
    }
    fn outline(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgba) {
        for x in x0..x1 {
            self.set(x, y0, c);
            self.set(x, y1 - 1, c);
        }
        for y in y0..y1 {
            self.set(x0, y, c);
            self.set(x1 - 1, y, c);
        }
    }
    fn scaled(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity((SIZE * SIZE * SCALE * SCALE * 4) as usize);
        for y in 0..SIZE * SCALE {
            for x in 0..SIZE * SCALE {
                out.extend_from_slice(&self.px[((y / SCALE) * SIZE + x / SCALE) as usize]);
            }
        }
        out
    }
}

fn glyph_rows(ch: char) -> [u8; 5] {
    match ch {
        'A' => [0b010, 0b101, 0b111, 0b101, 0b101],
        'B' => [0b110, 0b101, 0b110, 0b101, 0b110],
        'X' => [0b101, 0b101, 0b010, 0b101, 0b101],
        'Y' => [0b101, 0b101, 0b010, 0b010, 0b010],
        'L' => [0b100, 0b100, 0b100, 0b100, 0b111],
        'R' => [0b110, 0b101, 0b110, 0b101, 0b101],
        'T' => [0b111, 0b010, 0b010, 0b010, 0b010],
        _ => [0; 5],
    }
}

const WHITE: Rgba = [245, 245, 245, 255];
const DARK: Rgba = [28, 28, 34, 255];
const GREY: Rgba = [118, 122, 136, 255];
const LIGHT: Rgba = [210, 214, 226, 255];

/// Paints one button at [`SIZE`]x[`SIZE`].
pub fn paint(b: PadButton) -> Vec<Rgba> {
    let mut c = Canvas::new();
    match b {
        PadButton::South | PadButton::East | PadButton::West | PadButton::North => {
            let (fill, ch, ink) = match b {
                PadButton::South => ([62, 168, 84, 255], 'A', WHITE),
                PadButton::East => ([205, 66, 66, 255], 'B', WHITE),
                PadButton::West => ([66, 118, 214, 255], 'X', WHITE),
                _ => ([228, 184, 52, 255], 'Y', DARK),
            };
            c.disc(6.0, fill, [fill[0] / 2, fill[1] / 2, fill[2] / 2, 255]);
            c.letter(ch, 4, 3, ink);
        }
        PadButton::LeftBumper | PadButton::RightBumper | PadButton::LeftTrigger | PadButton::RightTrigger => {
            let trigger = matches!(b, PadButton::LeftTrigger | PadButton::RightTrigger);
            let left = matches!(b, PadButton::LeftBumper | PadButton::LeftTrigger);
            let fill = if trigger { [92, 96, 112, 255] } else { GREY };
            c.rect(0, 2, 12, 10, DARK);
            c.rect(1, 3, 11, 9, fill);
            if trigger {
                // Rounded top corners: a trigger is curved.
                for (x, y) in [(0, 2), (11, 2), (1, 3), (10, 3)] {
                    c.set(x, y, [0, 0, 0, 0]);
                }
            }
            c.letter(if left { 'L' } else { 'R' }, 2, 4, WHITE);
            c.letter(if trigger { 'T' } else { 'B' }, 6, 4, WHITE);
        }
        PadButton::LeftStick | PadButton::RightStick => {
            c.disc(6.0, [58, 60, 72, 255], LIGHT);
            c.letter(if b == PadButton::LeftStick { 'L' } else { 'R' }, 4, 3, WHITE);
        }
        PadButton::Start | PadButton::Select => {
            c.rect(0, 3, 12, 9, DARK);
            c.rect(1, 4, 11, 8, [70, 74, 90, 255]);
            if b == PadButton::Start {
                for y in 5..7 {
                    for x in 3..9 {
                        c.set(x, y, WHITE);
                    }
                }
                c.set(3, 4, WHITE);
                c.set(3, 7, WHITE);
            } else {
                c.outline(3, 4, 7, 8, LIGHT);
                c.outline(5, 5, 9, 7, WHITE);
            }
        }
        PadButton::DPadUp | PadButton::DPadDown | PadButton::DPadLeft | PadButton::DPadRight => {
            c.rect(0, 4, 12, 8, GREY);
            c.rect(4, 0, 8, 12, GREY);
            c.rect(4, 4, 8, 8, [96, 100, 114, 255]);
            let (x0, y0, x1, y1) = match b {
                PadButton::DPadUp => (4, 0, 8, 4),
                PadButton::DPadDown => (4, 8, 8, 12),
                PadButton::DPadLeft => (0, 4, 4, 8),
                _ => (8, 4, 12, 8),
            };
            c.rect(x0, y0, x1, y1, WHITE);
        }
    }
    c.px
}

/// A glyph's pixels (any scale) ready to upload.
#[derive(Clone, Debug)]
pub struct GlyphImage {
    pub size: u32,
    pub rgba: Vec<u8>,
    pub custom: bool,
}

/// `<dir>/<id>.png` if it is a usable replacement: decodes, square, a whole multiple of [`SIZE`].
pub fn load_override(dir: &std::path::Path, b: PadButton) -> Option<GlyphImage> {
    let path = dir.join(format!("{}.png", b.id()));
    let img = image::open(&path).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    if w != h || w == 0 || w % SIZE != 0 {
        warn!("Glyph {} is {w}x{h}; it must be square and a whole multiple of {SIZE}. Using the built-in one.", path.display());
        return None;
    }
    Some(GlyphImage { size: w, rgba: img.into_raw(), custom: true })
}

/// Every button's picture; later directories override earlier ones.
pub fn build_glyphs(dirs: &[PathBuf]) -> HashMap<PadButton, GlyphImage> {
    PadButton::ALL
        .into_iter()
        .map(|b| {
            let mut c = Canvas::new();
            c.px = paint(b);
            let mut img = GlyphImage { size: SIZE * SCALE, rgba: c.scaled(), custom: false };
            for d in dirs {
                if let Some(found) = load_override(d, b) {
                    img = found;
                }
            }
            (b, img)
        })
        .collect()
}

#[derive(Resource, Default)]
pub struct PadGlyphs {
    handles: HashMap<PadButton, Handle<Image>>,
}

impl PadGlyphs {
    pub fn handle(&self, b: PadButton) -> Option<Handle<Image>> {
        self.handles.get(&b).cloned()
    }
}

pub fn upload_glyphs(images: HashMap<PadButton, GlyphImage>, assets: &mut Assets<Image>) -> PadGlyphs {
    let mut out = PadGlyphs::default();
    for (b, img) in images {
        let mut image = Image::new(
            Extent3d { width: img.size, height: img.size, depth_or_array_layers: 1 },
            TextureDimension::D2,
            img.rgba,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
        );
        image.sampler = ImageSampler::nearest();
        out.handles.insert(b, assets.add(image));
    }
    out
}

fn build(
    mut commands: Commands,
    assets: Option<ResMut<Assets<Image>>>,
    installed: Option<Res<crate::mods::InstalledMods>>,
    selection: Option<Res<crate::packs::PackSelection>>,
    installed_packs: Option<Res<crate::packs::InstalledPacks>>,
) {
    let Some(mut assets) = assets else { return };
    let mods_dir = installed.map(|i| i.0.clone()).unwrap_or_else(crate::mods::installed_mods_dir);
    let packs = crate::packs::active_pack_paths(selection.as_deref(), installed_packs.as_deref());
    let dirs: Vec<PathBuf> = crate::gui_skin::gui_texture_dirs(&mods_dir, &packs).into_iter().map(|d| d.join("glyphs")).collect();
    commands.insert_resource(upload_glyphs(build_glyphs(&dirs), &mut assets));
}

/// Shows a controller button's picture on a node.
#[derive(Component, Clone, Copy, Debug)]
pub struct PadGlyph(pub PadButton);

pub fn apply_glyphs(glyphs: Option<Res<PadGlyphs>>, mut nodes: Query<(Entity, &PadGlyph, Option<&mut ImageNode>)>, mut commands: Commands) {
    let Some(glyphs) = glyphs else { return };
    for (e, g, existing) in &mut nodes {
        let Some(image) = glyphs.handle(g.0) else { continue };
        match existing {
            Some(mut node) => {
                if node.image != image {
                    node.image = image; // the packs changed: show the new picture
                }
            }
            None => {
                commands.entity(e).try_insert(ImageNode::new(image));
            }
        }
    }
}

/// Rebuilds the glyphs when the packs in force change.
fn rebuild(
    mut commands: Commands,
    assets: Option<ResMut<Assets<Image>>>,
    installed: Option<Res<crate::mods::InstalledMods>>,
    effective: Option<Res<crate::packs::EffectivePacks>>,
    mut last: Local<Option<Vec<PathBuf>>>,
) {
    let (Some(mut assets), Some(effective)) = (assets, effective) else { return };
    if last.as_ref() == Some(&effective.0) {
        return;
    }
    let first = last.is_none();
    *last = Some(effective.0.clone());
    if first {
        return;
    }
    let mods_dir = installed.map(|i| i.0.clone()).unwrap_or_else(crate::mods::installed_mods_dir);
    let dirs: Vec<PathBuf> = crate::gui_skin::gui_texture_dirs(&mods_dir, &effective.0).into_iter().map(|d| d.join("glyphs")).collect();
    commands.insert_resource(upload_glyphs(build_glyphs(&dirs), &mut assets));
}

pub struct PadGlyphPlugin;

impl Plugin for PadGlyphPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, build).add_systems(Update, (rebuild, apply_glyphs).chain());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let d = std::env::temp_dir().join(format!("craftmjne-glyph-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn every_button_has_a_visible_glyph_and_no_two_look_alike() {
        let mut seen: Vec<(PadButton, Vec<Rgba>)> = Vec::new();
        for b in PadButton::ALL {
            let px = paint(b);
            assert_eq!(px.len(), (SIZE * SIZE) as usize);
            let visible = px.iter().filter(|p| p[3] > 0).count();
            assert!(visible >= 20, "{b:?} is basically blank ({visible} pixels)");
            for (other, o) in &seen {
                assert_ne!(&px, o, "{b:?} and {other:?} are the same picture");
            }
            seen.push((b, px));
        }
    }

    #[test]
    fn the_dpad_lights_the_arm_that_was_chosen() {
        let lit = |b: PadButton, x: usize, y: usize| paint(b)[y * SIZE as usize + x] == WHITE;
        assert!(lit(PadButton::DPadUp, 5, 1) && !lit(PadButton::DPadDown, 5, 1));
        assert!(lit(PadButton::DPadDown, 5, 10) && !lit(PadButton::DPadUp, 5, 10));
        assert!(lit(PadButton::DPadLeft, 1, 5) && lit(PadButton::DPadRight, 10, 5));
    }

    #[test]
    fn face_buttons_wear_the_xbox_colours() {
        let centre = |b: PadButton| paint(b)[2 * SIZE as usize + 5];
        assert!(centre(PadButton::South)[1] > centre(PadButton::South)[0], "A is green");
        assert!(centre(PadButton::East)[0] > centre(PadButton::East)[1], "B is red");
        assert!(centre(PadButton::West)[2] > centre(PadButton::West)[0], "X is blue");
        let y = centre(PadButton::North);
        assert!(y[0] > 150 && y[1] > 150 && y[2] < 100, "Y is yellow");
    }

    fn write_png(path: &std::path::Path, size: u32) {
        image::RgbaImage::from_pixel(size, size, image::Rgba([1, 2, 3, 255])).save(path).unwrap();
    }

    #[test]
    fn a_texture_pack_can_replace_a_glyph_at_any_scale_and_bad_files_are_ignored() {
        let dir = tmp("pack");
        write_png(&dir.join("south.png"), 48); // 4x
        write_png(&dir.join("east.png"), 13); // not a multiple
        std::fs::write(dir.join("west.png"), b"nope").unwrap();
        let glyphs = build_glyphs(&[dir.clone()]);
        assert!(glyphs[&PadButton::South].custom && glyphs[&PadButton::South].size == 48);
        assert!(!glyphs[&PadButton::East].custom && !glyphs[&PadButton::West].custom);
        assert_eq!(glyphs[&PadButton::North].size, SIZE * SCALE);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_glyph_node_gets_its_picture() {
        let mut app = App::new();
        app.init_resource::<Assets<Image>>();
        let glyphs = {
            let mut assets = app.world_mut().resource_mut::<Assets<Image>>();
            upload_glyphs(build_glyphs(&[]), &mut assets)
        };
        let want = glyphs.handle(PadButton::North).unwrap();
        app.insert_resource(glyphs).add_systems(Update, apply_glyphs);
        let e = app.world_mut().spawn((Node::default(), PadGlyph(PadButton::North))).id();
        app.update();
        assert_eq!(app.world().get::<ImageNode>(e).unwrap().image, want);
    }

    #[test]
    fn the_readme_documents_the_glyph_folder() {
        let readme = std::fs::read_to_string("textures/gui/README.md").unwrap();
        assert!(readme.contains("glyphs"), "textures/gui/README.md must explain textures/gui/glyphs/");
        for b in PadButton::ALL {
            assert!(readme.contains(b.id()), "{} missing from the glyph list", b.id());
        }
    }
}
