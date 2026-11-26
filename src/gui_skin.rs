//! The textures every menu is drawn with: rustic wood, stone and leather, as real
//! images instead of flat coloured boxes.
//!
//! **Elements.** Each [`Element`] is one named piece of the interface (a panel, a
//! button in each state, a text field, an inventory slot, the tiled background...).
//! It has a *logical* size in "skin pixels" and a slice border (the part of the image
//! that must not stretch). The game paints every element procedurally at startup, so
//! the menus look right with no art files at all.
//!
//! **Retexturing.** Put `textures/gui/<name>.png` next to the game (or inside any
//! installed mod) and it replaces that element. The PNG may be any whole multiple of
//! the logical size - a 3x-sized `button.png` is 3x the logical width and height - and
//! the slice border scales with it, so a texture pack can be as detailed as it likes
//! without declaring anything. Later directories win (the game, then mods in name
//! order). A file that is the wrong shape or won't decode is skipped and the
//! procedural one stays, so a bad pack can never leave a button invisible.
//! `docs/` and the README list every element name.
//!
//! **Use.** Spawn a node with [`Skinned`] and the [`apply_skin`] system gives it the
//! image (a hover variant switches by itself). Buttons go through `ui_theme`, which
//! picks the element from a button's style and state. Because the look is looked up
//! by name at draw time, replacing the [`GuiSkin`] resource restyles everything.

use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::ui::widget::NodeImageMode;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Procedural elements are painted at logical size times this, so one skin pixel is a
/// chunky block on screen (a slice border is measured in image pixels and drawn 1:1).
pub const PAINT_SCALE: u32 = 3;
/// The tiled background is bigger blocks still.
const BACKGROUND_SCALE: u32 = 4;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Element {
    /// The big framed board a screen's controls sit on (dark oak).
    Panel,
    /// A leather patch: list entries, settings body.
    Card,
    CardAlt,
    /// A stone slab button, in each state.
    Button,
    ButtonHover,
    ButtonPressed,
    ButtonSelected,
    ButtonPrimary,
    ButtonPrimaryHover,
    ButtonPrimaryPressed,
    ButtonDanger,
    ButtonDangerHover,
    ButtonDangerPressed,
    /// A recessed text box.
    Field,
    FieldFocus,
    /// An inventory slot.
    Slot,
    SlotHover,
    /// A short plank: the line under a heading.
    Plank,
    /// The tile that fills the screen behind a menu (dirt).
    Background,
}

impl Element {
    pub const ALL: [Element; 19] = [
        Element::Panel,
        Element::Card,
        Element::CardAlt,
        Element::Button,
        Element::ButtonHover,
        Element::ButtonPressed,
        Element::ButtonSelected,
        Element::ButtonPrimary,
        Element::ButtonPrimaryHover,
        Element::ButtonPrimaryPressed,
        Element::ButtonDanger,
        Element::ButtonDangerHover,
        Element::ButtonDangerPressed,
        Element::Field,
        Element::FieldFocus,
        Element::Slot,
        Element::SlotHover,
        Element::Plank,
        Element::Background,
    ];

    /// The file name (without `.png`) that overrides this element.
    pub fn name(self) -> &'static str {
        match self {
            Element::Panel => "panel",
            Element::Card => "card",
            Element::CardAlt => "card_alt",
            Element::Button => "button",
            Element::ButtonHover => "button_hover",
            Element::ButtonPressed => "button_pressed",
            Element::ButtonSelected => "button_selected",
            Element::ButtonPrimary => "button_primary",
            Element::ButtonPrimaryHover => "button_primary_hover",
            Element::ButtonPrimaryPressed => "button_primary_pressed",
            Element::ButtonDanger => "button_danger",
            Element::ButtonDangerHover => "button_danger_hover",
            Element::ButtonDangerPressed => "button_danger_pressed",
            Element::Field => "field",
            Element::FieldFocus => "field_focus",
            Element::Slot => "slot",
            Element::SlotHover => "slot_hover",
            Element::Plank => "plank",
            Element::Background => "background",
        }
    }

    /// Size in skin pixels.
    pub fn logical_size(self) -> (u32, u32) {
        match self {
            Element::Panel => (24, 24),
            Element::Card | Element::CardAlt => (16, 16),
            Element::Field | Element::FieldFocus => (16, 12),
            Element::Slot | Element::SlotHover => (14, 14),
            Element::Plank => (16, 4),
            Element::Background => (16, 16),
            _ => (16, 12),
        }
    }

    /// Skin pixels at each edge that keep their size when the node is stretched.
    pub fn border(self) -> u32 {
        match self {
            Element::Panel => 6,
            Element::Card | Element::CardAlt => 4,
            Element::Slot | Element::SlotHover => 3,
            Element::Plank | Element::Background => 0,
            _ => 4,
        }
    }

    fn paint_scale(self) -> u32 {
        if self == Element::Background { BACKGROUND_SCALE } else { PAINT_SCALE }
    }
}

// ---------------------------------------------------------------------------
// Pixels
// ---------------------------------------------------------------------------

type Rgb = [u8; 3];

/// A small RGBA canvas in skin pixels.
pub struct Canvas {
    pub w: u32,
    pub h: u32,
    pub px: Vec<[u8; 4]>,
}

impl Canvas {
    fn new(w: u32, h: u32) -> Self {
        Self { w, h, px: vec![[0, 0, 0, 255]; (w * h) as usize] }
    }
    fn set(&mut self, x: u32, y: u32, c: Rgb) {
        if x < self.w && y < self.h {
            self.px[(y * self.w + x) as usize] = [c[0], c[1], c[2], 255];
        }
    }
    fn get(&self, x: u32, y: u32) -> Rgb {
        let p = self.px[(y * self.w + x) as usize];
        [p[0], p[1], p[2]]
    }
    fn fill(&mut self, f: impl Fn(u32, u32) -> Rgb) {
        for y in 0..self.h {
            for x in 0..self.w {
                let c = f(x, y);
                self.set(x, y, c);
            }
        }
    }
    /// Rescales every pixel inside the box `x0..x1, y0..y1`.
    fn shade_rect(&mut self, x0: u32, y0: u32, x1: u32, y1: u32, k: f32) {
        for y in y0..y1.min(self.h) {
            for x in x0..x1.min(self.w) {
                let c = self.get(x, y);
                self.set(x, y, shade(c, k));
            }
        }
    }
    /// A one-pixel bevel round the edge `depth` pixels thick: lit top-left, dark
    /// bottom-right (or the reverse when `raised` is false).
    fn bevel(&mut self, depth: u32, raised: bool, light: f32, dark: f32) {
        for d in 0..depth {
            for i in d..(self.w - d) {
                let (top, bot) = if raised { (light, dark) } else { (dark, light) };
                let t = self.get(i, d);
                self.set(i, d, shade(t, top));
                let b = self.get(i, self.h - 1 - d);
                self.set(i, self.h - 1 - d, shade(b, bot));
            }
            for j in (d + 1)..(self.h - d - 1) {
                let (left, right) = if raised { (light, dark) } else { (dark, light) };
                let l = self.get(d, j);
                self.set(d, j, shade(l, left));
                let r = self.get(self.w - 1 - d, j);
                self.set(self.w - 1 - d, j, shade(r, right));
            }
        }
    }
    /// A one-pixel outline in one colour, `inset` pixels in from the edge.
    fn outline(&mut self, inset: u32, c: Rgb) {
        if self.w <= inset * 2 || self.h <= inset * 2 {
            return;
        }
        for i in inset..(self.w - inset) {
            self.set(i, inset, c);
            self.set(i, self.h - 1 - inset, c);
        }
        for j in inset..(self.h - inset) {
            self.set(inset, j, c);
            self.set(self.w - 1 - inset, j, c);
        }
    }
    /// Upscales by a whole factor, nearest neighbour.
    fn scaled(&self, k: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity((self.w * self.h * k * k * 4) as usize);
        for y in 0..self.h * k {
            for x in 0..self.w * k {
                out.extend_from_slice(&self.px[((y / k) * self.w + x / k) as usize]);
            }
        }
        out
    }
}

fn shade(c: Rgb, k: f32) -> Rgb {
    [(c[0] as f32 * k).clamp(0.0, 255.0) as u8, (c[1] as f32 * k).clamp(0.0, 255.0) as u8, (c[2] as f32 * k).clamp(0.0, 255.0) as u8]
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t) as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t) as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t) as u8,
    ]
}

/// A repeatable pseudo-random number in `0..1` from a position and a seed.
fn hash(x: u32, y: u32, seed: u32) -> f32 {
    let mut h = x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA77) ^ seed.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h & 0xFFFF) as f32 / 65535.0
}

// ---- materials -------------------------------------------------------------

/// Horizontal wood planks of `plank_h` pixels, with grain and seams. Wraps in x so a
/// tiled centre has no vertical seam.
fn wood(w: u32, h: u32, base: Rgb, plank_h: u32, seed: u32) -> Canvas {
    let mut c = Canvas::new(w, h);
    c.fill(|x, y| {
        let plank = y / plank_h;
        let tone = 0.86 + 0.28 * hash(plank, 0, seed);
        // Grain: long streaks that shift per row.
        let streak = hash(x / 5, y, seed ^ 0x51) * 0.14 - 0.07;
        let speck = if hash(x, y, seed ^ 0x77) > 0.93 { 0.88 } else { 1.0 };
        let mut col = shade(base, (tone + streak) * speck);
        // Seam at the top of each plank.
        if y % plank_h == 0 {
            col = shade(col, 0.62);
        } else if y % plank_h == 1 {
            col = shade(col, 1.08);
        }
        col
    });
    c
}

/// Cobblestone-ish speckled stone.
fn stone(w: u32, h: u32, base: Rgb, seed: u32) -> Canvas {
    let mut c = Canvas::new(w, h);
    c.fill(|x, y| {
        let blotch = hash(x / 3, y / 3, seed) * 0.22 - 0.11;
        let fine = hash(x, y, seed ^ 0x3F) * 0.12 - 0.06;
        let crack = if hash(x, y, seed ^ 0x99) > 0.965 { 0.80 } else { 1.0 };
        shade(base, (1.0 + blotch + fine) * crack)
    });
    c
}

/// Leather with a stitched seam `inset` pixels in from the edge.
fn leather(w: u32, h: u32, base: Rgb, stitch: Rgb, seed: u32) -> Canvas {
    let mut c = Canvas::new(w, h);
    c.fill(|x, y| {
        let pore = hash(x, y, seed) * 0.16 - 0.08;
        let wrinkle = hash(x / 4, y / 2, seed ^ 0x21) * 0.12 - 0.06;
        shade(base, 1.0 + pore + wrinkle)
    });
    // Dashed stitching one pixel inside the border.
    let (iw, ih) = (w, h);
    for i in 1..iw - 1 {
        if i % 3 != 0 {
            c.set(i, 1, stitch);
            c.set(i, ih - 2, stitch);
        }
    }
    for j in 1..ih - 1 {
        if j % 3 != 0 {
            c.set(1, j, stitch);
            c.set(iw - 2, j, stitch);
        }
    }
    c
}

// ---- elements --------------------------------------------------------------

const GOLD: Rgb = [232, 190, 84];
const OAK: Rgb = [92, 62, 36];
const DARK_OAK: Rgb = [58, 38, 22];
const LEATHER: Rgb = [112, 70, 40];
const STITCH: Rgb = [196, 150, 90];
const STONE: Rgb = [118, 118, 116];
const MOSS: Rgb = [74, 112, 62];
const BRICK: Rgb = [122, 58, 50];

/// Paints an element at its logical size (before upscaling).
pub fn paint(e: Element) -> Canvas {
    let (w, h) = e.logical_size();
    let seed = e as u32 * 7919 + 13;
    match e {
        Element::Panel => {
            let mut c = wood(w, h, DARK_OAK, 4, seed);
            // Dark outer frame, lit inner lip.
            c.outline(0, [24, 15, 9]);
            c.outline(1, shade(OAK, 1.1));
            c.outline(2, shade(OAK, 0.8));
            c.outline(3, shade(DARK_OAK, 1.35));
            c.outline(4, shade(DARK_OAK, 0.6));
            c.outline(5, [30, 20, 12]);
            // Corner nails.
            for (x, y) in [(2, 2), (w - 3, 2), (2, h - 3), (w - 3, h - 3)] {
                c.set(x, y, [170, 170, 160]);
            }
            c
        }
        Element::Card | Element::CardAlt => {
            let base = if e == Element::Card { LEATHER } else { shade(LEATHER, 0.84) };
            let mut c = leather(w, h, base, STITCH, seed);
            c.outline(0, shade(base, 0.45));
            c.bevel(1, true, 1.0, 1.0);
            c
        }
        Element::Button | Element::ButtonHover | Element::ButtonPressed | Element::ButtonSelected => {
            let (base, edge) = match e {
                Element::ButtonHover => (shade(STONE, 1.16), GOLD),
                Element::ButtonPressed => (shade(STONE, 0.72), [48, 48, 46]),
                Element::ButtonSelected => (mix(STONE, MOSS, 0.55), GOLD),
                _ => (STONE, [40, 40, 38]),
            };
            slab(w, h, base, edge, e != Element::ButtonPressed, seed)
        }
        Element::ButtonPrimary | Element::ButtonPrimaryHover | Element::ButtonPrimaryPressed => {
            let (base, edge) = match e {
                Element::ButtonPrimaryHover => (shade(MOSS, 1.3), GOLD),
                Element::ButtonPrimaryPressed => (shade(MOSS, 0.7), [26, 44, 22]),
                _ => (MOSS, [28, 48, 24]),
            };
            slab(w, h, base, edge, e != Element::ButtonPrimaryPressed, seed)
        }
        Element::ButtonDanger | Element::ButtonDangerHover | Element::ButtonDangerPressed => {
            let (base, edge) = match e {
                Element::ButtonDangerHover => (shade(BRICK, 1.3), [255, 120, 100]),
                Element::ButtonDangerPressed => (shade(BRICK, 0.7), [50, 22, 20]),
                _ => (BRICK, [52, 24, 22]),
            };
            slab(w, h, base, edge, e != Element::ButtonDangerPressed, seed)
        }
        Element::Field | Element::FieldFocus => {
            let mut c = wood(w, h, [40, 28, 18], 4, seed);
            c.shade_rect(0, 0, w, h, 0.7);
            c.outline(0, if e == Element::FieldFocus { GOLD } else { [18, 12, 8] });
            c.bevel(1, false, 1.5, 0.6);
            if e == Element::FieldFocus {
                c.outline(1, shade(GOLD, 0.6));
            }
            c
        }
        Element::Slot | Element::SlotHover => {
            let base = if e == Element::SlotHover { [92, 92, 90] } else { [62, 62, 60] };
            let mut c = stone(w, h, base, seed);
            c.outline(0, if e == Element::SlotHover { GOLD } else { [26, 26, 25] });
            c.bevel(1, false, 1.0, 0.55);
            c.bevel(2, false, 1.25, 0.7);
            c
        }
        Element::Plank => {
            let mut c = wood(w, h, OAK, 4, seed);
            c.outline(0, [30, 20, 12]);
            c
        }
        Element::Background => {
            let mut c = Canvas::new(w, h);
            c.fill(|x, y| {
                let n = hash(x, y, seed);
                let base = [86, 60, 40];
                let mut col = shade(base, 0.8 + 0.4 * n);
                if hash(x / 2, y / 2, seed ^ 0xAB) > 0.9 {
                    col = shade(STONE, 0.55 + 0.2 * n); // an embedded pebble
                }
                col
            });
            c
        }
    }
}

/// A stone-slab button face: speckled stone, a dark outline, and a bevel.
fn slab(w: u32, h: u32, base: Rgb, edge: Rgb, raised: bool, seed: u32) -> Canvas {
    let mut c = stone(w, h, base, seed);
    c.outline(0, edge);
    c.bevel(1, raised, 1.28, 0.68);
    if raised {
        c.bevel(2, true, 1.1, 0.88);
    }
    c
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// One element's pixels, ready to upload, and how many pixels per skin pixel it has.
#[derive(Clone, Debug)]
pub struct SkinImage {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
    pub scale: u32,
    /// True when a texture file supplied it.
    pub custom: bool,
}

impl SkinImage {
    /// Slice border in image pixels.
    pub fn border_px(&self, e: Element) -> f32 {
        (e.border() * self.scale) as f32
    }
}

/// Reads `<dir>/<name>.png` for `e` if it is usable: decodes, and is a whole multiple of
/// the element's logical size. `None` for absent *or* bad (the caller keeps what it had).
pub fn load_override(dir: &Path, e: Element) -> Option<SkinImage> {
    let path = dir.join(format!("{}.png", e.name()));
    let img = image::open(&path).ok()?.to_rgba8();
    let (lw, lh) = e.logical_size();
    let (w, h) = img.dimensions();
    if w == 0 || w % lw != 0 || h % lh != 0 || w / lw != h / lh {
        warn!("GUI texture {} is {w}x{h}; it must be a whole multiple of {lw}x{lh}. Using the built-in one.", path.display());
        return None;
    }
    Some(SkinImage { w, h, rgba: img.into_raw(), scale: w / lw, custom: true })
}

/// Every element, procedural unless a later directory overrides it.
pub fn build_skin_images(dirs: &[PathBuf]) -> HashMap<Element, SkinImage> {
    let mut out = HashMap::new();
    for e in Element::ALL {
        let canvas = paint(e);
        let k = e.paint_scale();
        let mut img = SkinImage { w: canvas.w * k, h: canvas.h * k, rgba: canvas.scaled(k), scale: k, custom: false };
        for dir in dirs {
            if let Some(found) = load_override(dir, e) {
                img = found;
            }
        }
        out.insert(e, img);
    }
    out
}

/// The directories that can hold GUI textures, lowest priority first: the game's own, then every
/// installed mod's resource pack (in folder-name order), then the player's enabled resource packs
/// (`user_packs`, lowest first). Later wins.
pub fn gui_texture_dirs(installed_mods: &Path, user_packs: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs = vec![crate::packs::core_root().join("textures").join("gui")];
    if let Ok(rd) = std::fs::read_dir(installed_mods) {
        let mut mods: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        mods.sort();
        dirs.extend(mods.into_iter().map(|m| m.join(crate::packs::MOD_PACK_DIR).join("textures").join("gui")));
    }
    dirs.extend(user_packs.iter().map(|p| p.join("textures").join("gui")));
    dirs
}

// ---------------------------------------------------------------------------
// Bevy side
// ---------------------------------------------------------------------------

/// The uploaded textures.
#[derive(Resource, Default)]
pub struct GuiSkin {
    handles: HashMap<Element, (Handle<Image>, f32)>,
}

impl GuiSkin {
    pub fn image_mode(&self, e: Element) -> NodeImageMode {
        let border = self.handles.get(&e).map_or(0.0, |h| h.1);
        match e {
            Element::Background => NodeImageMode::Tiled { tile_x: true, tile_y: true, stretch_value: 1.0 },
            _ if border <= 0.0 => NodeImageMode::Stretch,
            _ => NodeImageMode::Sliced(TextureSlicer {
                border: BorderRect::all(border),
                center_scale_mode: SliceScaleMode::Tile { stretch_value: 1.0 },
                sides_scale_mode: SliceScaleMode::Tile { stretch_value: 1.0 },
                max_corner_scale: 1.0,
            }),
        }
    }

    pub fn handle(&self, e: Element) -> Option<Handle<Image>> {
        self.handles.get(&e).map(|h| h.0.clone())
    }

    /// An `ImageNode` showing `e`, tinted.
    pub fn node(&self, e: Element, tint: Color) -> Option<ImageNode> {
        let image = self.handle(e)?;
        Some(ImageNode { image, color: tint, image_mode: self.image_mode(e), ..default() })
    }
}

pub fn upload_skin(images: HashMap<Element, SkinImage>, assets: &mut Assets<Image>) -> GuiSkin {
    let mut skin = GuiSkin::default();
    for (e, img) in images {
        let border = img.border_px(e);
        let mut image = Image::new(
            Extent3d { width: img.w, height: img.h, depth_or_array_layers: 1 },
            TextureDimension::D2,
            img.rgba,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
        );
        image.sampler = ImageSampler::nearest();
        skin.handles.insert(e, (assets.add(image), border));
    }
    skin
}

fn build_skin(
    mut commands: Commands,
    assets: Option<ResMut<Assets<Image>>>,
    installed: Option<Res<crate::mods::InstalledMods>>,
    selection: Option<Res<crate::packs::PackSelection>>,
    installed_packs: Option<Res<crate::packs::InstalledPacks>>,
) {
    // Apps with no renderer (tests) have no image assets and so no skin; every user copes.
    let Some(mut assets) = assets else { return };
    let mods_dir = installed.map(|i| i.0.clone()).unwrap_or_else(crate::mods::installed_mods_dir);
    let packs = crate::packs::active_pack_paths(selection.as_deref(), installed_packs.as_deref());
    let images = build_skin_images(&gui_texture_dirs(&mods_dir, &packs));
    commands.insert_resource(upload_skin(images, &mut assets));
}

/// Puts an element's image on a node. `hover` is shown while the node is hovered
/// (any node with an `Interaction`).
#[derive(Component, Clone, Copy, Debug)]
pub struct Skinned {
    pub element: Element,
    pub hover: Option<Element>,
    pub tint: Color,
}

impl Skinned {
    pub fn new(element: Element) -> Self {
        Self { element, hover: None, tint: Color::WHITE }
    }
    pub fn hover(mut self, e: Element) -> Self {
        self.hover = Some(e);
        self
    }
    pub fn tint(mut self, c: Color) -> Self {
        self.tint = c;
        self
    }
}

pub fn apply_skin(
    skin: Option<Res<GuiSkin>>,
    focus: Option<Res<crate::menu_nav::MenuFocus>>,
    mut nodes: Query<(&Skinned, Option<&Interaction>, Option<&mut ImageNode>, Entity)>,
    mut commands: Commands,
) {
    let Some(skin) = skin else { return };
    for (s, interaction, existing, entity) in &mut nodes {
        // A node the keyboard or controller has highlighted looks hovered.
        let keyed = focus.as_ref().is_some_and(|f| f.entity == Some(entity));
        let e = match (s.hover, interaction) {
            (Some(h), _) if keyed => h,
            (Some(h), Some(Interaction::Hovered | Interaction::Pressed)) => h,
            _ => s.element,
        };
        let Some(want) = skin.handle(e) else { continue };
        match existing {
            Some(mut node) => {
                if node.image != want || node.color != s.tint {
                    if let Some(new) = skin.node(e, s.tint) {
                        *node = new;
                    }
                }
            }
            None => {
                if let Some(new) = skin.node(e, s.tint) {
                    commands.entity(entity).try_insert(new);
                }
            }
        }
    }
}

pub struct GuiSkinPlugin;

impl Plugin for GuiSkinPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, build_skin).add_systems(Update, apply_skin);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let d = std::env::temp_dir().join(format!("craftmjne-skin-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn every_element_paints_at_its_logical_size_with_a_valid_slice() {
        for e in Element::ALL {
            let c = paint(e);
            assert_eq!((c.w, c.h), e.logical_size(), "{e:?}");
            assert_eq!(c.px.len(), (c.w * c.h) as usize);
            assert!(c.px.iter().all(|p| p[3] == 255), "{e:?} is fully opaque");
            assert!(e.border() * 2 < c.w.min(c.h) || e.border() == 0, "{e:?}: slice borders must leave a centre");
        }
    }

    #[test]
    fn the_gui_texture_readme_names_every_element() {
        let readme = std::fs::read_to_string("textures/gui/README.md").expect("textures/gui/README.md");
        for e in Element::ALL {
            let name = e.name();
            // The table groups state variants ("button_primary, _hover, _pressed"), so check the stem.
            let stem = name.split("_hover").next().unwrap().split("_pressed").next().unwrap().split("_focus").next().unwrap().split("_selected").next().unwrap();
            assert!(readme.contains(stem), "{name} is not described in textures/gui/README.md");
        }
    }

    #[test]
    fn names_are_unique() {
        let mut names: Vec<_> = Element::ALL.iter().map(|e| e.name()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), Element::ALL.len());
    }

    #[test]
    fn painting_is_deterministic() {
        for e in Element::ALL {
            assert_eq!(paint(e).px, paint(e).px, "{e:?}");
        }
    }

    #[test]
    fn button_states_are_visibly_different_textures() {
        let groups = [
            [Element::Button, Element::ButtonHover, Element::ButtonPressed, Element::ButtonSelected],
            [Element::ButtonPrimary, Element::ButtonPrimaryHover, Element::ButtonPrimaryPressed, Element::ButtonPrimary],
            [Element::ButtonDanger, Element::ButtonDangerHover, Element::ButtonDangerPressed, Element::ButtonDanger],
        ];
        for g in groups {
            for i in 0..3 {
                for j in (i + 1)..3 {
                    assert_ne!(paint(g[i]).px, paint(g[j]).px, "{:?} vs {:?}", g[i], g[j]);
                }
            }
        }
        assert_ne!(paint(Element::Field).px, paint(Element::FieldFocus).px);
        assert_ne!(paint(Element::Slot).px, paint(Element::SlotHover).px);
        assert_ne!(paint(Element::Card).px, paint(Element::CardAlt).px);
    }

    #[test]
    fn the_procedural_skin_has_no_files_and_scales_up() {
        let dir = tmp("none");
        let skin = build_skin_images(&[dir.join("textures").join("gui")]);
        assert_eq!(skin.len(), Element::ALL.len());
        let b = &skin[&Element::Button];
        assert_eq!((b.w, b.h, b.scale, b.custom), (16 * PAINT_SCALE, 12 * PAINT_SCALE, PAINT_SCALE, false));
        assert_eq!(b.rgba.len(), (b.w * b.h * 4) as usize);
        assert_eq!(b.border_px(Element::Button), (4 * PAINT_SCALE) as f32);
        let _ = std::fs::remove_dir_all(dir);
    }

    fn write_png(path: &Path, w: u32, h: u32, color: [u8; 4]) {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba(color));
        img.save(path).unwrap();
    }

    #[test]
    fn a_texture_file_replaces_an_element_and_its_slice_scales_with_it() {
        let dir = tmp("override");
        write_png(&dir.join("button.png"), 64, 48, [10, 200, 10, 255]); // 4x the 16x12 logical size
        let skin = build_skin_images(&[dir.clone()]);
        let b = &skin[&Element::Button];
        assert!(b.custom);
        assert_eq!((b.w, b.h, b.scale), (64, 48, 4));
        assert_eq!(b.rgba[..4], [10, 200, 10, 255]);
        assert_eq!(b.border_px(Element::Button), 16.0, "the 4-pixel border is 4x as wide in a 4x texture");
        assert!(!skin[&Element::ButtonHover].custom, "only the named element changes");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_wrongly_shaped_or_corrupt_file_is_ignored_not_trusted() {
        let dir = tmp("bad");
        write_png(&dir.join("button.png"), 17, 12, [1, 2, 3, 255]); // not a multiple
        write_png(&dir.join("panel.png"), 48, 24, [1, 2, 3, 255]); // not square-scaled
        std::fs::write(dir.join("card.png"), b"not a png").unwrap();
        let skin = build_skin_images(&[dir.clone()]);
        for e in [Element::Button, Element::Panel, Element::Card] {
            assert!(!skin[&e].custom, "{e:?} must fall back to the built-in texture");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_later_directory_wins() {
        let (a, b) = (tmp("a"), tmp("b"));
        write_png(&a.join("slot.png"), 14, 14, [1, 1, 1, 255]);
        write_png(&b.join("slot.png"), 28, 28, [9, 9, 9, 255]);
        let skin = build_skin_images(&[a.clone(), b.clone()]);
        assert_eq!(skin[&Element::Slot].rgba[0], 9);
        let skin = build_skin_images(&[b.clone(), a.clone()]);
        assert_eq!(skin[&Element::Slot].rgba[0], 1);
        let _ = std::fs::remove_dir_all(a);
        let _ = std::fs::remove_dir_all(b);
    }

    #[test]
    fn installed_mods_add_their_gui_folders_after_the_games() {
        let dir = tmp("mods");
        std::fs::create_dir_all(dir.join("zeta")).unwrap();
        std::fs::create_dir_all(dir.join("alpha")).unwrap();
        let dirs = gui_texture_dirs(&dir, &[PathBuf::from("/p/low"), PathBuf::from("/p/high")]);
        assert!(dirs[0].ends_with("gui") && !dirs[0].starts_with(&dir));
        assert!(dirs[1].starts_with(dir.join("alpha")) && dirs[2].starts_with(dir.join("zeta")));
        assert!(dirs[1].ends_with("resourcepack/textures/gui"), "a mod's art lives in its resource pack: {:?}", dirs[1]);
        assert!(dirs[3].starts_with("/p/low") && dirs[4].starts_with("/p/high"), "the player's packs go on top, in order");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_skinned_node_gets_its_image_and_hover_swaps_it() {
        let mut app = App::new();
        app.init_resource::<Assets<Image>>();
        let skin = {
            let mut assets = app.world_mut().resource_mut::<Assets<Image>>();
            upload_skin(build_skin_images(&[]), &mut assets)
        };
        let normal = skin.handle(Element::Slot).unwrap();
        let hover = skin.handle(Element::SlotHover).unwrap();
        app.insert_resource(skin).add_systems(Update, apply_skin);
        let e = app.world_mut().spawn((Node::default(), Interaction::None, Skinned::new(Element::Slot).hover(Element::SlotHover))).id();
        app.update();
        assert_eq!(app.world().get::<ImageNode>(e).unwrap().image, normal);
        *app.world_mut().get_mut::<Interaction>(e).unwrap() = Interaction::Hovered;
        app.update();
        assert_eq!(app.world().get::<ImageNode>(e).unwrap().image, hover);
        // A sliced element is drawn sliced; the background tiles.
        assert!(matches!(app.world().resource::<GuiSkin>().image_mode(Element::Panel), NodeImageMode::Sliced(_)));
        assert!(matches!(app.world().resource::<GuiSkin>().image_mode(Element::Background), NodeImageMode::Tiled { .. }));
    }
}
