//! The game's emoji: 12x12 pixel art in the typeface's own style (a dark outline, flat colours).
//!
//! Every emoji is a grid of palette letters (see [`PALETTE`]; `.` is clear). Three kinds of source:
//! - **drawn**: a grid written out in `emoji/*.rs`;
//! - **composed**: faces are a round (or cat) head plus an eyes overlay, a mouth overlay and extras,
//!   so ninety-odd faces are a few dozen small drawings combined;
//! - **generated**: colour circles and squares, clock faces, moon phases and arrows come from rules.
//!
//! `fonts.rs` turns the set into a colour font (`font_gen`), and the chat turns `:shortcode:` into the
//! character. Flags are drawn by `flags.rs`. Sequences (flags, keycaps, toned people) are `GSUB` ligatures,
//! written by `font_gen`; the joiners themselves take no room.
//!
//! The set is data: adding an emoji is one table row.

use crate::font_gen::{rects_from_mask, GlyphSpec, Layer};

mod drawn;
mod faces;
mod flags;
mod generated;

pub const SIZE: usize = 12;
/// Advance of an emoji: the art plus one blank column.
pub const ADVANCE: i32 = 13;

/// `(key, rgba)`: the colours emoji are drawn in. `.` is not a colour.
pub const PALETTE: &[(char, [u8; 4])] = &[
    ('k', [34, 28, 40, 255]),    // outline
    ('w', [255, 255, 255, 255]), // white
    ('l', [214, 216, 226, 255]), // light gray
    ('s', [150, 152, 164, 255]), // gray
    ('S', [90, 92, 104, 255]),   // dark gray
    ('y', [255, 205, 66, 255]),  // yellow
    ('Y', [232, 160, 36, 255]),  // dark yellow
    ('o', [244, 134, 42, 255]),  // orange
    ('r', [226, 52, 56, 255]),   // red
    ('R', [150, 30, 44, 255]),   // dark red
    ('p', [255, 150, 186, 255]), // pink
    ('P', [150, 84, 204, 255]),  // purple
    ('b', [62, 112, 224, 255]),  // blue
    ('B', [30, 62, 152, 255]),   // dark blue
    ('c', [112, 204, 244, 255]), // light blue
    ('g', [72, 184, 84, 255]),   // green
    ('G', [34, 112, 54, 255]),   // dark green
    ('n', [152, 94, 54, 255]),   // brown
    ('N', [98, 58, 38, 255]),    // dark brown
    ('t', [242, 192, 142, 255]), // skin
    ('T', [214, 150, 104, 255]), // skin shadow
    ('e', [250, 236, 202, 255]), // cream
];

pub type Grid = [[char; SIZE]; SIZE];

/// Fits one authored row to [`SIZE`] columns: a row that is too wide loses blank columns (from its
/// longest run of blanks first), one that is too narrow is padded on the right. Returns the row and
/// whether any ink had to be cut (a drawing that does is worth fixing: see `drawn::tests`).
pub fn fit_row(row: &str) -> (Vec<char>, bool) {
    let mut cells: Vec<char> = row.chars().collect();
    while cells.len() > SIZE {
        // The longest run of blanks.
        let (mut best, mut best_len, mut i) = (None, 0, 0);
        while i < cells.len() {
            if cells[i] == '.' {
                let start = i;
                while i < cells.len() && cells[i] == '.' {
                    i += 1;
                }
                if i - start > best_len {
                    best_len = i - start;
                    best = Some(start);
                }
            } else {
                i += 1;
            }
        }
        match best {
            Some(at) => {
                cells.remove(at);
            }
            None => {
                cells.truncate(SIZE);
                return (cells, true);
            }
        }
    }
    cells.resize(SIZE, '.');
    (cells, false)
}

/// Parses up to 12 rows of palette letters joined by `/`, fitting each row ([`fit_row`]) and
/// padding missing rows with blanks at the bottom.
pub fn parse(art: &str) -> Grid {
    let mut g = [['.'; SIZE]; SIZE];
    for (y, row) in art.split('/').enumerate().take(SIZE) {
        let (cells, _) = fit_row(row);
        for (x, ch) in cells.into_iter().enumerate() {
            g[y][x] = ch;
        }
    }
    g
}

/// Draws `rows` onto `g` starting at row `top`; `.` leaves what is there.
pub fn overlay(g: &mut Grid, top: usize, rows: &[&str]) {
    for (i, row) in rows.iter().enumerate() {
        for (x, ch) in row.chars().enumerate().take(SIZE) {
            if ch != '.' && top + i < SIZE {
                g[top + i][x] = ch;
            }
        }
    }
}

pub fn recolor(g: &mut Grid, from: char, to: char) {
    for row in g.iter_mut() {
        for c in row.iter_mut() {
            if *c == from {
                *c = to;
            }
        }
    }
}

pub fn flip_vertical(g: &Grid) -> Grid {
    let mut out = *g;
    for y in 0..SIZE {
        out[y] = g[SIZE - 1 - y];
    }
    out
}

pub fn flip_horizontal(g: &Grid) -> Grid {
    let mut out = *g;
    for row in out.iter_mut() {
        row.reverse();
    }
    out
}

/// Turns the picture a quarter turn clockwise.
pub fn rotate_cw(g: &Grid) -> Grid {
    let mut out = [['.'; SIZE]; SIZE];
    for y in 0..SIZE {
        for x in 0..SIZE {
            out[x][SIZE - 1 - y] = g[y][x];
        }
    }
    out
}

/// One emoji: its short name, the characters that show it, and its picture.
#[derive(Clone)]
pub struct Emoji {
    pub name: String,
    pub chars: Vec<char>,
    pub grid: Grid,
    /// Character sequences that a ligature turns into this glyph (flags, keycaps, toned people ...).
    pub sequences: Vec<Vec<char>>,
}

pub(crate) fn emoji(name: &str, chars: &str, grid: Grid) -> Emoji {
    Emoji { name: name.to_string(), chars: chars.chars().collect(), grid, sequences: Vec::new() }
}

/// Every emoji, drawn, composed and generated.
pub fn all() -> Vec<Emoji> {
    let mut out = Vec::new();
    out.extend(faces::all());
    out.extend(drawn::all());
    out.extend(generated::all());
    out.extend(flags::all());
    // The first drawing of a character wins; a name is unique.
    let mut seen_chars = std::collections::HashSet::new();
    let mut seen_names = std::collections::HashSet::new();
    out.retain_mut(|e| {
        e.chars.retain(|c| seen_chars.insert(*c));
        !e.chars.is_empty() && seen_names.insert(e.name.clone())
    });
    out
}

/// The characters that take no room: variation selectors and the zero-width joiner.
pub const ZERO_WIDTH: &[char] = &['\u{FE0F}', '\u{FE0E}', '\u{200D}', '\u{20E3}'];

/// Every character that takes no room: the joiners above and the tag letters of subdivision flags
/// (`U+E0061..=U+E007A`, `U+E007F`). Each gets its own glyph so a ligature can tell them apart.
pub fn joiners() -> Vec<char> {
    let mut v: Vec<char> = ZERO_WIDTH.to_vec();
    v.extend(('\u{E0061}'..='\u{E007A}').chain(['\u{E007F}']));
    v
}

/// The font glyphs for the set (colour layers by palette index).
pub fn glyph_specs() -> Vec<GlyphSpec> {
    let mut specs: Vec<GlyphSpec> = all()
        .iter()
        .map(|e| {
            let mut layers = Vec::new();
            for (i, (key, _)) in PALETTE.iter().enumerate() {
                let mask: Vec<Vec<bool>> = e.grid.iter().map(|row| row.iter().map(|c| c == key).collect()).collect();
                let rects = rects_from_mask(SIZE, &mask, 0);
                if !rects.is_empty() {
                    layers.push(Layer { color: i as u16, rects });
                }
            }
            GlyphSpec { chars: e.chars.clone(), advance: ADVANCE, layers, colour: true }
        })
        .collect();
    for c in joiners() {
        specs.push(GlyphSpec { chars: vec![c], advance: 0, layers: vec![Layer { color: 0, rects: vec![] }], colour: false });
    }
    specs
}

/// The `ccmp` ligatures that turn sequences (flags, keycaps, toned people) into their glyphs.
pub fn ligatures() -> Vec<crate::font_gen::Ligature> {
    let mut out = Vec::new();
    for e in all() {
        for seq in &e.sequences {
            out.push(crate::font_gen::Ligature { script: *b"DFLT", feature: *b"ccmp", components: seq.clone(), to: e.chars[0] });
        }
    }
    out
}

pub fn palette_rgba() -> Vec<[u8; 4]> {
    PALETTE.iter().map(|(_, c)| *c).collect()
}

/// `(shortcode, text)` for every emoji, for `:smile:` in chat. A flag's text is its two
/// regional-indicator letters (or tag sequence); the font turns those into the flag.
pub fn shortcodes() -> Vec<(String, String)> {
    all()
        .into_iter()
        .filter_map(|e| {
            let text: String = match e.sequences.first() {
                Some(seq) => seq.iter().collect(),
                None => e.chars.first()?.to_string(),
            };
            Some((e.name, text))
        })
        .collect()
}

/// Replaces `:name:` with the emoji it names, leaving anything else alone (so `:)` or a lone colon
/// stay as typed).
pub fn expand_shortcodes(text: &str) -> String {
    thread_local! {
        static MAP: std::collections::HashMap<String, String> = shortcodes().into_iter().collect();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(':') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        if let Some(end) = after.find(':') {
            let name = &after[..end];
            if let Some(c) = MAP.with(|m| m.get(name).cloned()) {
                out.push_str(&c);
                rest = &after[end + 1..];
                continue;
            }
        }
        out.push(':');
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Renders the whole set to an RGBA sheet for looking at.
pub fn contact_sheet(scale: usize, columns: usize) -> (u32, u32, Vec<u8>) {
    let set = all();
    let rows = set.len().div_ceil(columns);
    let cell = SIZE + 2;
    let (w, h) = (columns * cell * scale, rows * cell * scale);
    let mut px = vec![245u8; w * h * 4];
    for (i, e) in set.iter().enumerate() {
        let (cx, cy) = ((i % columns) * cell + 1, (i / columns) * cell + 1);
        for y in 0..SIZE {
            for x in 0..SIZE {
                let key = e.grid[y][x];
                let Some((_, rgba)) = PALETTE.iter().find(|(k, _)| *k == key) else { continue };
                for sy in 0..scale {
                    for sx in 0..scale {
                        let o = (((cy + y) * scale + sy) * w + (cx + x) * scale + sx) * 4;
                        px[o..o + 4].copy_from_slice(rgba);
                    }
                }
            }
        }
    }
    (w as u32, h as u32, px)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_drawing_is_twelve_by_twelve_of_known_colours_with_ink() {
        for e in all() {
            let mut ink = 0;
            for row in e.grid.iter() {
                for c in row.iter() {
                    if *c != '.' {
                        assert!(PALETTE.iter().any(|(k, _)| k == c), "{} uses the unknown colour {c:?}", e.name);
                        ink += 1;
                    }
                }
            }
            assert!(ink > 8, "{} is nearly blank", e.name);
        }
    }

    #[test]
    fn no_two_emoji_share_a_name_or_a_character() {
        let set = all();
        let mut names = std::collections::HashSet::new();
        let mut chars = std::collections::HashSet::new();
        for e in &set {
            assert!(names.insert(e.name.clone()), "duplicate name {}", e.name);
            for c in &e.chars {
                assert!(chars.insert(*c), "{c:?} drawn twice ({})", e.name);
            }
        }
    }

    #[test]
    fn every_flag_is_in_the_set_and_shows_through_a_ligature() {
        let set = all();
        let flags: Vec<_> = set.iter().filter(|e| e.name.starts_with("flag_")).collect();
        assert!(flags.len() >= 258 + 4, "only {} flags", flags.len());
        for e in &flags {
            assert!(!e.sequences.is_empty(), "{} has no sequence", e.name);
        }
        assert!(set.iter().any(|e| e.name == "flag_us"));
        assert!(set.iter().any(|e| e.name == "flag_texas"));
    }

    #[test]
    fn the_colour_font_builds_and_maps_every_emoji() {
        let spec = crate::font_gen::FontSpec { family: "Craftmjne Emoji".into(), palette: palette_rgba(), glyphs: glyph_specs(), space_advance: 4, em_rows: crate::font_gen::EM_ROWS, baseline_row: crate::font_gen::BASELINE_ROW, substitutions: vec![], ligatures: ligatures() };
        let bytes = crate::font_gen::build_ttf(&spec);
        let face = ttf_parser::Face::parse(&bytes, 0).expect("a valid font");
        let colr = face.tables().colr.expect("COLR");
        for e in all() {
            for c in &e.chars {
                let g = face.glyph_index(*c).unwrap_or_else(|| panic!("{} ({c:?}) is not mapped", e.name));
                assert!(colr.contains(g), "{} has no colour layers", e.name);
            }
        }
        for c in joiners() {
            let g = face.glyph_index(c).expect("joiners are mapped");
            assert_eq!(face.glyph_hor_advance(g), Some(0), "{c:?} takes no room");
        }
    }

    #[test]
    fn shortcodes_expand_and_everything_else_is_left_alone() {
        let first = all().into_iter().find(|e| e.sequences.is_empty()).unwrap();
        let c = first.chars[0];
        assert_eq!(expand_shortcodes(&format!("hi :{}: there", first.name)), format!("hi {c} there"));
        assert_eq!(expand_shortcodes(":flag_us:"), "\u{1F1FA}\u{1F1F8}");
        assert_eq!(expand_shortcodes("a :nosuchemoji: b"), "a :nosuchemoji: b");
        assert_eq!(expand_shortcodes("time 10:30 :) done"), "time 10:30 :) done");
        assert_eq!(expand_shortcodes("lone : colon"), "lone : colon");
    }

    /// Shapes `text` with the emoji font and returns the glyph ids.
    fn shape(font: &[u8], text: &str) -> Vec<u32> {
        let face = rustybuzz::Face::from_slice(font, 0).expect("the font parses");
        let mut buf = rustybuzz::UnicodeBuffer::new();
        buf.push_str(text);
        let out = rustybuzz::shape(&face, &[], buf);
        out.glyph_infos().iter().map(|i| i.glyph_id).collect()
    }

    #[test]
    fn a_pair_of_regional_indicators_becomes_one_flag_glyph() {
        let font = crate::fonts::emoji_font_bytes();
        let face = ttf_parser::Face::parse(&font, 0).unwrap();
        let gid = |c: char| face.glyph_index(c).unwrap().0 as u32;
        let us = all().into_iter().find(|e| e.name == "flag_us").unwrap();
        assert_eq!(shape(&font, "\u{1F1FA}\u{1F1F8}"), vec![gid(us.chars[0])]);
        // A pair that is no country stays two letter tiles.
        assert_eq!(shape(&font, "\u{1F1FF}\u{1F1FF}").len(), 2);
    }

    #[test]
    fn subdivision_and_special_flags_become_one_glyph_too() {
        let font = crate::fonts::emoji_font_bytes();
        let face = ttf_parser::Face::parse(&font, 0).unwrap();
        let gid = |c: char| face.glyph_index(c).unwrap().0 as u32;
        for (name, text) in [
            ("flag_texas", "\u{1F3F4}\u{E0075}\u{E0073}\u{E0074}\u{E0078}\u{E007F}"),
            ("flag_scotland", "\u{1F3F4}\u{E0067}\u{E0062}\u{E0073}\u{E0063}\u{E0074}\u{E007F}"),
            ("rainbow_flag", "\u{1F3F3}\u{FE0F}\u{200D}\u{1F308}"),
            ("pirate_flag", "\u{1F3F4}\u{200D}\u{2620}\u{FE0F}"),
        ] {
            let e = all().into_iter().find(|e| e.name == name).unwrap();
            assert_eq!(shape(&font, text), vec![gid(e.chars[0])], "{name}");
        }
        // The plain black flag is still just itself.
        let black = all().into_iter().find(|e| e.name == "black_flag").unwrap();
        assert_eq!(shape(&font, "\u{1F3F4}"), vec![gid(black.chars[0])]);
    }

    #[test]
    fn the_gun_is_a_water_pistol() {
        let set = all();
        assert!(set.iter().any(|e| e.name == "water_pistol" && e.chars == vec!['\u{1F52B}']));
        assert!(!set.iter().any(|e| e.name == "gun" || e.name == "pistol"));
    }

    #[test]
    fn a_reasonable_number_of_emoji_exist() {
        assert!(all().len() >= 300, "only {}", all().len());
    }

    /// `CRAFT_EMOJI_SHEET=/path.png cargo test emoji::tests::sheet` writes the sheet.
    #[test]
    fn sheet() {
        let (w, h, px) = contact_sheet(6, 20);
        if let Ok(path) = std::env::var("CRAFT_EMOJI_SHEET") {
            image::RgbaImage::from_raw(w, h, px).unwrap().save(path).unwrap();
        }
    }
}
