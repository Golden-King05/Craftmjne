//! The game's fonts: one pixel typeface for everything, and the way packs change or add to it.
//!
//! **The default font** is the game's own pixel typeface (`pixel_font.rs`) and **the emoji font** is
//! its colour emoji (`emoji.rs`); both are written as real font files in memory (`font_gen.rs`) and
//! handed to Bevy. There is no other font in the game: the built-in one replaces Bevy's.
//!
//! **Crisp text.** Pixel fonts blur at the wrong size, so a text's `font_size` is snapped to a
//! multiple of the font's pixel grid (12 for ours: 12, 24, 36...) and drawn without smoothing. That
//! happens for every text with a pixel font, in one system ([`apply_fonts`]), so no screen has to
//! know.
//!
//! **Packs** (`fonts/` in any resource pack, mod pack or the game folder; the highest layer wins):
//! - `fonts/<name>.ttf` / `.otf`: a vector font, added as the font `<name>` (not snapped).
//! - `fonts/<name>.png`: a bitmap font sheet - 16 x 16 cells, cell = code point (row-major), ink
//!   where the pixel is not clear - turned into a pixel font the same way ours is. The cell is
//!   `width / 16` pixels square; the baseline sits 7/8 down. Glyph width is measured from the ink.
//! - `fonts/fonts.json`: `{ "default": "name", "chat": "other" }` maps *roles* to font names. The
//!   role `default` replaces the typeface everywhere; the others are used by whatever carries that
//!   role as a [`FontTag`].
//!
//! **The font tag.** Any text can carry a [`FontTag`]: a role (`title`, `button`, `chat`,
//! `tooltip`...) or a font name. A block or item file may also say `"font": "name"`, which its name
//! uses in tooltips. A tag that names nothing falls back to the default font, never to nothing.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use bevy::prelude::*;
use bevy::text::{CosmicFontSystem, Font, FontSmoothing, LineHeight};

use crate::font_gen::{build_ttf, rects_from_mask, FontSpec, GlyphSpec, Layer, BASELINE_ROW, EM_ROWS};

/// Roles the game itself uses; a pack's `fonts.json` may map any of them.
pub const ROLES: &[&str] = &["default", "title", "button", "chat", "tooltip", "hud"];

/// The default text font as file bytes.
pub fn default_font_bytes() -> Vec<u8> {
    build_ttf(&FontSpec {
        family: "Craftmjne".into(),
        palette: vec![],
        glyphs: crate::pixel_font::glyphs(),
        em_rows: EM_ROWS,
        baseline_row: BASELINE_ROW,
        space_advance: crate::pixel_font::SPACE,
        substitutions: crate::pixel_font::substitutions(),
        ligatures: crate::pixel_font::ligatures(),
    })
}

/// The emoji font as file bytes.
pub fn emoji_font_bytes() -> Vec<u8> {
    build_ttf(&FontSpec {
        family: "Craftmjne Emoji".into(),
        palette: crate::emoji::palette_rgba(),
        glyphs: crate::emoji::glyph_specs(),
        em_rows: EM_ROWS,
        baseline_row: BASELINE_ROW,
        space_advance: crate::pixel_font::SPACE,
        substitutions: vec![],
        ligatures: crate::emoji::ligatures(),
    })
}

/// Rounds `size` to the nearest multiple of `unit` (at least one), so a pixel is a whole number of
/// screen pixels.
pub fn snap_size(size: f32, unit: f32) -> f32 {
    if unit <= 0.0 {
        return size;
    }
    ((size / unit).round().max(1.0)) * unit
}

/// A font a pack provides.
pub struct PackFont {
    pub bytes: Vec<u8>,
    /// The pixel grid of a bitmap font (its cell height); `None` for a vector font.
    pub unit: Option<f32>,
}

#[derive(Default)]
pub struct PackFonts {
    pub fonts: BTreeMap<String, PackFont>,
    pub roles: HashMap<String, String>,
    /// Problems with files that were skipped, for the log.
    pub notes: Vec<String>,
}

/// Turns a bitmap sheet (16 x 16 cells, cell = code point) into a font. `None` if the picture
/// isn't a usable sheet.
pub fn sheet_to_font(name: &str, img: &image::RgbaImage) -> Result<(Vec<u8>, f32), String> {
    let (w, h) = img.dimensions();
    if w == 0 || w % 16 != 0 || h < w / 16 * 8 || h % (w / 16) != 0 {
        return Err(format!("{name}.png must be 16 cells wide, each cell square (it is {w}x{h})"));
    }
    let cell = (w / 16) as i32;
    if !(4..=64).contains(&cell) {
        return Err(format!("{name}.png cells are {cell}px; 4 to 64 is supported"));
    }
    let baseline = (cell * 7 / 8).max(1);
    let rows_of_cells = (h / w * 16) as u32;
    let mut glyphs = Vec::new();
    for index in 0x21u32..(16 * rows_of_cells).min(0x100) {
        let Some(c) = char::from_u32(index) else { continue };
        let (cx, cy) = ((index % 16) * cell as u32, (index / 16) * cell as u32);
        let mask: Vec<Vec<bool>> = (0..cell as u32).map(|y| (0..cell as u32).map(|x| img.get_pixel(cx + x, cy + y)[3] >= 128).collect()).collect();
        let width = (0..cell as usize).rev().find(|x| mask.iter().any(|row| row[*x])).map(|x| x + 1);
        let Some(width) = width else { continue };
        glyphs.push(GlyphSpec {
            chars: vec![c],
            advance: width as i32 + 1,
            colour: false,
            layers: vec![Layer { color: 0, rects: rects_from_mask(width, &mask, 0) }],
        });
    }
    if glyphs.is_empty() {
        return Err(format!("{name}.png has no glyphs (every cell is clear)"));
    }
    let bytes = build_ttf(&FontSpec { family: name.to_string(), palette: vec![], glyphs, em_rows: cell, baseline_row: baseline, space_advance: (cell / 2).max(2), substitutions: vec![], ligatures: vec![] });
    Ok((bytes, cell as f32))
}

/// Reads every `fonts/` folder in `dirs` (lowest priority first; later ones win).
pub fn load_pack_fonts(dirs: &[PathBuf]) -> PackFonts {
    let mut out = PackFonts::default();
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
        files.sort();
        for path in files {
            let (Some(stem), Some(ext)) = (path.file_stem().and_then(|s| s.to_str()), path.extension().and_then(|e| e.to_str())) else { continue };
            let name = stem.to_string();
            match ext.to_ascii_lowercase().as_str() {
                "ttf" | "otf" => match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| Font::try_from_bytes(b.clone()).map(|_| b).map_err(|e| format!("{e:?}"))) {
                    Ok(bytes) => {
                        out.fonts.insert(name, PackFont { bytes, unit: None });
                    }
                    Err(e) => out.notes.push(format!("{}: not a usable font ({e})", path.display())),
                },
                "png" => match image::open(&path).map(|i| i.to_rgba8()).map_err(|e| e.to_string()).and_then(|img| sheet_to_font(&name, &img)) {
                    Ok((bytes, unit)) => {
                        out.fonts.insert(name, PackFont { bytes, unit: Some(unit) });
                    }
                    Err(e) => out.notes.push(format!("{}: {e}", path.display())),
                },
                "json" if stem == "fonts" => match std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<HashMap<String, String>>(&t).ok()) {
                    Some(roles) => out.roles.extend(roles),
                    None => out.notes.push(format!("{}: expected an object of role -> font name", path.display())),
                },
                _ => {}
            }
        }
    }
    out
}

/// The fonts in force: which handle is "the default", which are named, and each one's pixel grid.
#[derive(Resource)]
pub struct FontBook {
    /// Used by every text that doesn't say otherwise.
    pub default: Handle<Font>,
    named: HashMap<String, Handle<Font>>,
    roles: HashMap<String, String>,
    units: HashMap<AssetId<Font>, f32>,
    /// Every handle that has been "the default" - texts holding one of these follow the current one.
    managed: HashSet<AssetId<Font>>,
    emoji_faces: Vec<bevy::text::cosmic_text::fontdb::ID>,
}

impl Default for FontBook {
    fn default() -> Self {
        let base = Handle::<Font>::default();
        let mut units = HashMap::new();
        units.insert(base.id(), EM_ROWS as f32);
        Self { default: base.clone(), named: HashMap::new(), roles: HashMap::new(), units, managed: HashSet::from([base.id()]), emoji_faces: Vec::new() }
    }
}

impl FontBook {
    /// The font a tag (a role or a font name) means; the default if it means nothing.
    pub fn resolve(&self, tag: &str) -> Handle<Font> {
        let name = self.roles.get(tag).map(String::as_str).unwrap_or(tag);
        if name == "default" {
            return self.default.clone();
        }
        self.named.get(name).cloned().unwrap_or_else(|| self.default.clone())
    }

    pub fn unit_of(&self, font: &Handle<Font>) -> Option<f32> {
        self.units.get(&font.id()).copied()
    }

    /// Names of the fonts packs have added.
    pub fn names(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.named.keys().map(String::as_str).collect();
        v.sort();
        v
    }
}

/// Tags a text with a font: a role (`chat`, `title`...) or a font name. See the module docs.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct FontTag(pub String);

impl FontTag {
    pub fn new(tag: &str) -> Self {
        Self(tag.to_string())
    }
}

/// PreStartup: the built-in typeface replaces Bevy's default font, and the emoji go in as the fallback.
fn install_builtin(mut fonts: ResMut<Assets<Font>>, mut book: ResMut<FontBook>, cosmic: Option<ResMut<CosmicFontSystem>>) {
    if let Ok(font) = Font::try_from_bytes(default_font_bytes()) {
        let _ = fonts.insert(&Handle::<Font>::default(), font);
    }
    if let Some(mut cosmic) = cosmic {
        book.emoji_faces = register_face(&mut cosmic, emoji_font_bytes());
    }
}

fn register_face(cosmic: &mut CosmicFontSystem, bytes: Vec<u8>) -> Vec<bevy::text::cosmic_text::fontdb::ID> {
    cosmic.0.db_mut().load_font_source(bevy::text::cosmic_text::fontdb::Source::Binary(std::sync::Arc::new(bytes))).to_vec()
}

/// The folders to read fonts from, lowest layer first: the game's own, then every layer in force.
fn font_dirs(layers: &[crate::load_order::LayerSource]) -> Vec<PathBuf> {
    let mut dirs = vec![crate::packs::core_root().join("fonts")];
    for layer in layers {
        dirs.push(match layer {
            crate::load_order::LayerSource::Mod(m) => m.path.join(crate::packs::MOD_PACK_DIR).join("fonts"),
            crate::load_order::LayerSource::Pack(p) => p.join("fonts"),
        });
    }
    dirs
}

/// Re-reads the packs' fonts when the layers in force change or a reload is asked for.
fn refresh_pack_fonts(
    layers: Option<Res<crate::packs::EffectiveLayers>>,
    reload: Option<Res<crate::packs::PackReload>>,
    mut fonts: ResMut<Assets<Font>>,
    mut book: ResMut<FontBook>,
    cosmic: Option<ResMut<CosmicFontSystem>>,
    mut texts: Query<&mut TextFont>,
    mut last: Local<Option<(Vec<crate::load_order::LayerSource>, u64)>>,
) {
    let Some(layers) = layers else { return };
    let now = (layers.0.clone(), reload.map_or(0, |r| r.0));
    if last.as_ref() == Some(&now) {
        return;
    }
    let first = last.is_none();
    *last = Some(now);
    let found = load_pack_fonts(&font_dirs(&layers.0));
    if first && found.fonts.is_empty() && found.roles.is_empty() && found.notes.is_empty() {
        return; // nothing to change from the built-in
    }
    for note in &found.notes {
        warn!("Font skipped: {note}");
    }
    // Fresh handles: Bevy caches a font by its handle, so a changed file needs a new one.
    let mut named = HashMap::new();
    let mut units = HashMap::new();
    units.insert(Handle::<Font>::default().id(), EM_ROWS as f32);
    let mut emoji_bytes = None;
    for (name, f) in &found.fonts {
        if name == "emoji" {
            emoji_bytes = Some(f.bytes.clone());
        }
        let Ok(font) = Font::try_from_bytes(f.bytes.clone()) else { continue };
        let handle = fonts.add(font);
        if let Some(u) = f.unit {
            units.insert(handle.id(), u);
        }
        named.insert(name.clone(), handle);
    }
    book.roles = found.roles.clone();
    book.named = named;
    book.units.retain(|id, _| *id == Handle::<Font>::default().id());
    book.units.extend(units);
    // The new default: the `default` role, a font called `default`, or the built-in.
    let want = book.roles.get("default").cloned().unwrap_or_else(|| "default".to_string());
    let new_default = book.named.get(&want).or_else(|| book.named.get("default")).cloned().unwrap_or_else(Handle::<Font>::default);
    let old = book.default.id();
    book.managed.insert(old);
    book.managed.insert(new_default.id());
    book.default = new_default;
    // The emoji: a pack's own, or the built-in again.
    if let Some(mut cosmic) = cosmic {
        for id in book.emoji_faces.drain(..) {
            cosmic.0.db_mut().remove_face(id);
        }
        book.emoji_faces = register_face(&mut cosmic, emoji_bytes.unwrap_or_else(emoji_font_bytes));
    }
    // Make every text lay itself out again with whatever changed.
    for mut tf in &mut texts {
        tf.set_changed();
    }
}

/// Every frame, cheaply: gives tagged texts their font, moves untagged texts that hold a default
/// handle onto the current default, and makes pixel-font texts crisp (size snapped to the pixel grid,
/// no smoothing, a line height that is a whole number of pixels).
fn apply_fonts(book: Res<FontBook>, mut texts: Query<(&mut TextFont, Option<&FontTag>)>) {
    for (mut tf, tag) in &mut texts {
        let want = match tag {
            Some(t) => Some(book.resolve(&t.0)),
            None if book.managed.contains(&tf.font.id()) => Some(book.default.clone()),
            None => None,
        };
        if let Some(want) = want {
            if tf.font != want {
                tf.font = want;
            }
        }
        if let Some(unit) = book.unit_of(&tf.font) {
            let size = snap_size(tf.font_size, unit);
            if tf.font_size != size {
                tf.font_size = size;
            }
            if tf.font_smoothing != FontSmoothing::None {
                tf.font_smoothing = FontSmoothing::None;
            }
            if !matches!(tf.line_height, LineHeight::RelativeToFont(r) if (r - 1.25).abs() < 1e-6) {
                tf.line_height = LineHeight::RelativeToFont(1.25);
            }
        }
    }
}

pub struct FontPlugin;

impl Plugin for FontPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FontBook>()
            .add_systems(PreStartup, install_builtin)
            .add_systems(Update, refresh_pack_fonts)
            .add_systems(PostUpdate, apply_fonts.before(bevy::ui::UiSystem::Prepare));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let d = std::env::temp_dir().join(format!("craftmjne-fonts-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn sizes_snap_to_whole_multiples_of_the_pixel_grid() {
        assert_eq!(snap_size(12.0, 12.0), 12.0);
        assert_eq!(snap_size(14.0, 12.0), 12.0);
        assert_eq!(snap_size(16.0, 12.0), 12.0);
        assert_eq!(snap_size(18.0, 12.0), 24.0);
        assert_eq!(snap_size(20.0, 12.0), 24.0);
        assert_eq!(snap_size(56.0, 12.0), 60.0);
        assert_eq!(snap_size(3.0, 12.0), 12.0, "never below one pixel per pixel");
        assert_eq!(snap_size(15.0, 0.0), 15.0, "no grid, no snapping");
    }

    #[test]
    fn the_built_in_fonts_are_valid_and_the_emoji_font_has_colour() {
        let text_bytes = default_font_bytes();
        let text = ttf_parser::Face::parse(&text_bytes, 0).expect("text font");
        assert!(text.glyph_index('A').is_some() && text.glyph_index('é').is_some());
        assert!(text.tables().colr.is_none());
        let bytes = emoji_font_bytes();
        let emoji = ttf_parser::Face::parse(&bytes, 0).expect("emoji font");
        assert!(emoji.glyph_index('😀').is_some());
        assert!(emoji.tables().colr.is_some());
        // Bevy accepts both.
        assert!(Font::try_from_bytes(default_font_bytes()).is_ok());
        assert!(Font::try_from_bytes(bytes).is_ok());
    }

    #[test]
    fn a_png_sheet_becomes_a_pixel_font() {
        let cell = 8u32;
        let mut img = image::RgbaImage::new(cell * 16, cell * 16);
        // 'A' (0x41) is cell column 1, row 4: a 3-wide, 5-tall block.
        for y in 1..6 {
            for x in 0..3 {
                img.put_pixel(cell + x, 4 * cell + y, image::Rgba([255, 255, 255, 255]));
            }
        }
        let (bytes, unit) = sheet_to_font("sheetfont", &img).expect("a usable sheet");
        assert_eq!(unit, 8.0);
        let face = ttf_parser::Face::parse(&bytes, 0).unwrap();
        let a = face.glyph_index('A').expect("A drawn");
        assert_eq!(face.glyph_hor_advance(a), Some(4 * crate::font_gen::PIXEL as u16), "ink is 3 wide, plus one blank column");
        assert!(face.glyph_index('B').is_none(), "clear cells make no glyph");
        assert_eq!(face.units_per_em(), 8 * crate::font_gen::PIXEL as u16);
    }

    #[test]
    fn a_badly_shaped_or_blank_sheet_is_refused_with_a_reason() {
        let wrong = image::RgbaImage::new(100, 50);
        assert!(sheet_to_font("x", &wrong).unwrap_err().contains("16 cells wide"));
        let blank = image::RgbaImage::new(128, 128);
        assert!(sheet_to_font("x", &blank).unwrap_err().contains("no glyphs"));
    }

    #[test]
    fn pack_fonts_are_read_with_the_highest_folder_winning_and_roles_mapped() {
        let (low, high) = (tmp("low"), tmp("high"));
        let ttf = default_font_bytes();
        std::fs::write(low.join("mono.ttf"), &ttf).unwrap();
        std::fs::write(low.join("fonts.json"), r#"{"chat": "mono", "title": "low_title"}"#).unwrap();
        std::fs::write(high.join("fonts.json"), r#"{"title": "fancy"}"#).unwrap();
        std::fs::write(high.join("broken.ttf"), b"not a font").unwrap();
        std::fs::write(high.join("sheet.png"), b"not a png").unwrap();
        let found = load_pack_fonts(&[low.clone(), high.clone()]);
        assert!(found.fonts.contains_key("mono") && !found.fonts.contains_key("broken"));
        assert_eq!(found.roles.get("chat").map(String::as_str), Some("mono"));
        assert_eq!(found.roles.get("title").map(String::as_str), Some("fancy"), "the higher folder's mapping wins");
        assert_eq!(found.notes.len(), 2, "{:?}", found.notes);
        assert!(found.fonts["mono"].unit.is_none(), "a vector font isn't snapped");
        let _ = std::fs::remove_dir_all(low);
        let _ = std::fs::remove_dir_all(high);
    }

    #[test]
    fn a_tag_resolves_through_roles_to_a_named_font_and_never_to_nothing() {
        let mut book = FontBook::default();
        let mut fonts = Assets::<Font>::default();
        let mono = fonts.add(Font::try_from_bytes(default_font_bytes()).unwrap());
        book.named.insert("mono".into(), mono.clone());
        book.roles.insert("chat".into(), "mono".into());
        assert_eq!(book.resolve("chat"), mono, "a role maps to its font");
        assert_eq!(book.resolve("mono"), mono, "a font name works as a tag too");
        assert_eq!(book.resolve("tooltip"), book.default, "an unmapped role is the default");
        assert_eq!(book.resolve("no_such_font"), book.default);
        assert_eq!(book.unit_of(&book.default), Some(12.0));
    }

    #[test]
    fn the_system_tags_snaps_and_swaps_defaults() {
        let mut app = App::new();
        let mut book = FontBook::default();
        let mut fonts = Assets::<Font>::default();
        let mono = fonts.add(Font::try_from_bytes(default_font_bytes()).unwrap());
        book.named.insert("mono".into(), mono.clone());
        book.roles.insert("chat".into(), "mono".into());
        book.units.insert(mono.id(), 8.0);
        // A pack replaced the default: the old default handle is still "managed".
        let new_default = fonts.add(Font::try_from_bytes(default_font_bytes()).unwrap());
        book.units.insert(new_default.id(), 12.0);
        book.managed.insert(new_default.id());
        book.default = new_default.clone();
        app.insert_resource(book).add_systems(Update, apply_fonts);
        let plain = app.world_mut().spawn(TextFont { font_size: 14.0, ..default() }).id();
        let chat = app.world_mut().spawn((TextFont { font_size: 17.0, ..default() }, FontTag::new("chat"))).id();
        let vector = app.world_mut().spawn(TextFont { font: Handle::weak_from_u128(7), font_size: 15.0, ..default() }).id();
        app.update();
        let tf = |app: &App, e: Entity| app.world().entity(e).get::<TextFont>().unwrap().clone();
        let p = tf(&app, plain);
        assert_eq!(p.font, new_default, "an untagged text on the old default follows the new default");
        assert_eq!(p.font_size, 12.0, "snapped to the 12-pixel grid");
        assert_eq!(p.font_smoothing, FontSmoothing::None);
        let c = tf(&app, chat);
        assert_eq!(c.font, mono, "the chat role gives the chat font");
        assert_eq!(c.font_size, 16.0, "snapped to the *chat font's* 8-pixel grid");
        let v = tf(&app, vector);
        assert_eq!(v.font_size, 15.0, "a font the book doesn't know is left alone");
    }
}
