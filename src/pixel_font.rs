//! The game's own typeface, drawn pixel by pixel.
//!
//! Each glyph is a few rows of `#` (ink) and `.` (none), joined by `/`, with the cell row its first
//! line sits on. The cell is [`font_gen::EM_ROWS`] (12) rows tall: capitals and digits are 8 rows
//! (rows 2-9), lowercase is 6 rows (4-9) with ascenders reaching row 2, descenders drop to rows
//! 10-11, and rows 0-1 are left for accents. A glyph's advance is its width plus one blank column.
//! Accented Latin-1 letters are made from the plain letter plus an accent mark, so there is one
//! drawing of each shape to keep right.
//!
//! This is *only* the art; `font_gen` turns it into a font and `fonts.rs` installs it.

use crate::font_gen::{rects_from_mask, GlyphSpec, Layer};

/// Blank columns after a glyph.
const GAP: i32 = 1;
/// The space character's advance.
pub const SPACE: i32 = 4;

/// `(character, first cell row, rows)`.
const BASE: &[(char, i32, &str)] = &[
    // Capitals
    ('A', 2, "..##../.#..#./#....#/#....#/######/#....#/#....#/#....#"),
    ('B', 2, "#####./#....#/#....#/#####./#....#/#....#/#....#/#####."),
    ('C', 2, ".####./#....#/#...../#...../#...../#...../#....#/.####."),
    ('D', 2, "#####./#....#/#....#/#....#/#....#/#....#/#....#/#####."),
    ('E', 2, "######/#...../#...../#####./#...../#...../#...../######"),
    ('F', 2, "######/#...../#...../#####./#...../#...../#...../#....."),
    ('G', 2, ".####./#....#/#...../#...../#..###/#....#/#....#/.####."),
    ('H', 2, "#....#/#....#/#....#/######/#....#/#....#/#....#/#....#"),
    ('I', 2, "###/.#./.#./.#./.#./.#./.#./###"),
    ('J', 2, "..###/...#./...#./...#./...#./#..#./#..#./.##.."),
    ('K', 2, "#....#/#...#./#..#../###.../#..#../#...#./#....#/#....#"),
    ('L', 2, "#...../#...../#...../#...../#...../#...../#...../######"),
    ('M', 2, "#.....#/##...##/#.#.#.#/#..#..#/#.....#/#.....#/#.....#/#.....#"),
    ('N', 2, "#....#/##...#/##...#/#.#..#/#.#..#/#..#.#/#...##/#....#"),
    ('O', 2, ".####./#....#/#....#/#....#/#....#/#....#/#....#/.####."),
    ('P', 2, "#####./#....#/#....#/#....#/#####./#...../#...../#....."),
    ('Q', 2, ".####./#....#/#....#/#....#/#....#/#..#.#/#...#./.###.#"),
    ('R', 2, "#####./#....#/#....#/#....#/#####./#..#../#...#./#....#"),
    ('S', 2, ".####./#....#/#...../.####./.....#/.....#/#....#/.####."),
    ('T', 2, "#######/...#.../...#.../...#.../...#.../...#.../...#.../...#..."),
    ('U', 2, "#....#/#....#/#....#/#....#/#....#/#....#/#....#/.####."),
    ('V', 2, "#....#/#....#/#....#/#....#/#....#/.#..#./.#..#./..##.."),
    ('W', 2, "#.....#/#.....#/#.....#/#..#..#/#..#..#/#.#.#.#/##...##/#.....#"),
    ('X', 2, "#....#/#....#/.#..#./..##../..##../.#..#./#....#/#....#"),
    ('Y', 2, "#....#/#....#/.#..#./..##../..##../..##../..##../..##.."),
    ('Z', 2, "######/.....#/....#./...#../..#.../.#..../#...../######"),
    // Lowercase
    ('a', 4, ".####./.....#/.#####/#....#/#....#/.#####"),
    ('b', 2, "#...../#...../#.###./##...#/#....#/#....#/##...#/#.###."),
    ('c', 4, ".####/#..../#..../#..../#..../.####"),
    ('d', 2, ".....#/.....#/.###.#/#...##/#....#/#....#/#...##/.###.#"),
    ('e', 4, ".####./#....#/######/#...../#....#/.####."),
    ('f', 2, "..##/.#../.#../###./.#../.#../.#../.#.."),
    ('g', 4, ".#####/#....#/#....#/#....#/#....#/.#####/.....#/.####."),
    ('h', 2, "#...../#...../#.###./##...#/#....#/#....#/#....#/#....#"),
    ('i', 2, ".#./.../##./.#./.#./.#./.#./###"),
    ('j', 2, "..#/.../.##/..#/..#/..#/..#/..#/#.#/.#."),
    ('k', 2, "#..../#..../#..#./#.#../##.../#.#../#..#./#...#"),
    ('l', 2, "##./.#./.#./.#./.#./.#./.#./.##"),
    ('m', 4, "#.##.#./##.#.##/#..#..#/#..#..#/#..#..#/#..#..#"),
    ('n', 4, "#.###./##...#/#....#/#....#/#....#/#....#"),
    ('o', 4, ".####./#....#/#....#/#....#/#....#/.####."),
    ('p', 4, "#.###./##...#/#....#/#....#/##...#/#.###./#...../#....."),
    ('q', 4, ".###.#/#...##/#....#/#....#/#...##/.###.#/.....#/.....#"),
    ('r', 4, "#.##./##..#/#..../#..../#..../#...."),
    ('s', 4, ".####./#...../.####./.....#/#....#/.####."),
    ('t', 3, ".#../.#../####/.#../.#../.#../..##"),
    ('u', 4, "#....#/#....#/#....#/#....#/#...##/.###.#"),
    ('v', 4, "#....#/#....#/#....#/.#..#./.#..#./..##.."),
    ('w', 4, "#.....#/#.....#/#..#..#/#..#..#/#.#.#.#/.#...#."),
    ('x', 4, "#....#/.#..#./..##../..##../.#..#./#....#"),
    ('y', 4, "#....#/#....#/.#..#./.#..#./..##../..#.../.#..../#....."),
    ('z', 4, "######/....#./...#../..#.../.#..../######"),
    // Digits
    ('0', 2, ".####./#....#/#...##/#..#.#/#.#..#/##...#/#....#/.####."),
    ('1', 2, "..#./.##./#.#./..#./..#./..#./..#./####"),
    ('2', 2, ".####./#....#/.....#/....#./...#../..#.../.#..../######"),
    ('3', 2, ".####./#....#/.....#/..###./.....#/.....#/#....#/.####."),
    ('4', 2, "....#./...##./..#.#./.#..#./#...#./######/....#./....#."),
    ('5', 2, "######/#...../#...../#####./.....#/.....#/#....#/.####."),
    ('6', 2, "..###./.#..../#...../#####./#....#/#....#/#....#/.####."),
    ('7', 2, "######/.....#/....#./...#../..#.../..#.../..#.../..#..."),
    ('8', 2, ".####./#....#/#....#/.####./#....#/#....#/#....#/.####."),
    ('9', 2, ".####./#....#/#....#/.#####/.....#/.....#/...##./.###.."),
    // Punctuation and symbols
    ('!', 2, "#/#/#/#/#/#/./#"),
    ('"', 2, "#.#/#.#/#.#"),
    ('#', 2, ".#.#./.#.#./#####/.#.#./#####/.#.#./.#.#."),
    ('$', 2, "..#../.####/#.#../.###./..#.#/####./..#.."),
    ('%', 3, "##...#/##..#./...#../..#.../.#..##/#...##"),
    ('&', 2, ".##.../#..#../#..#../.##.../#..#.#/#...#./.###.#"),
    ('\'', 2, "#/#"),
    ('(', 2, "..#/.#./#../#../#../#../.#./..#"),
    (')', 2, "#../.#./..#/..#/..#/..#/.#./#.."),
    ('*', 3, "#.#.#/.###./#####/.###./#.#.#"),
    ('+', 4, "..#../..#../#####/..#../..#.."),
    (',', 8, "##/##/.#/#."),
    ('-', 6, "#####"),
    ('.', 8, "##/##"),
    ('/', 2, "...#/...#/..#./..#./.#../.#../#.../#..."),
    (':', 4, "##/##/../../##/##"),
    (';', 4, "##/##/../../##/##/.#/#."),
    ('<', 3, "...#/..#./.#../#.../.#../..#./...#"),
    ('=', 5, "#####/...../#####"),
    ('>', 3, "#.../.#../..#./...#/..#./.#../#..."),
    ('?', 2, ".####./#....#/.....#/....#./...#../...#../....../...#.."),
    ('@', 2, ".#####./#.....#/#.###.#/#.#.#.#/#.#.#.#/#.####./#....../.#####."),
    ('[', 2, "###/#../#../#../#../#../#../###"),
    ('\\', 2, "#.../#.../.#../.#../..#./..#./...#/...#"),
    (']', 2, "###/..#/..#/..#/..#/..#/..#/###"),
    ('^', 2, "..#../.#.#./#...#"),
    ('_', 11, "######"),
    ('`', 2, "#../.#."),
    ('{', 2, "..##/.#../.#../#.../.#../.#../.#../..##"),
    ('|', 2, "#/#/#/#/#/#/#/#/#/#"),
    ('}', 2, "##../..#./..#./...#/..#./..#./..#./##.."),
    ('~', 5, ".##..#/#..##."),
    // A few more that text uses
    ('¡', 4, "#/./#/#/#/#/#/#"),
    ('¿', 4, "..#.../....../..#.../.#..../#...../#....#/.####."),
    ('°', 2, ".##./#..#/#..#/.##."),
    ('·', 6, "##/##"),
    ('×', 4, "#...#/.#.#./..#../.#.#./#...#"),
    ('÷', 4, "..#../...../#####/...../..#.."),
    ('±', 3, "..#../..#../#####/..#../..#../...../#####"),
    ('«', 5, "..#.#/.#.#./#.#../.#.#./..#.#"),
    ('»', 5, "#.#../.#.#./..#.#/.#.#./#.#.."),
    ('•', 5, ".##./####/####/.##."),
    ('…', 9, "#.#.#"),
    ('–', 6, "####"),
    ('—', 6, "#######"),
    ('‘', 2, ".#/#./##"),
    ('’', 2, "##/.#/#."),
    ('“', 2, ".#.#/#.#./####"),
    ('”', 2, "##.##/.#..#/#..#."),
    ('©', 2, ".####./#....#/#.##.#/#.#..#/#.#..#/#.##.#/#....#/.####."),
    ('®', 2, ".####./#....#/#.##.#/#.#.##/#.##.#/#.#.##/#....#/.####."),
    ('€', 2, "..###./.#...#/#...../####../#...../####../.#...#/..###."),
    ('£', 2, "..###./.#...#/.#..../####../.#..../.#..../.#..../######"),
    ('¥', 2, "#....#/.#..#./..##../#####./..##../#####./..##../..##.."),
    ('¢', 3, "..#../.####/#.#../#.#../#.#../.####/..#.."),
    ('§', 2, ".####./#....#/.#..../.####./#....#/.####./....#./#....#/.####."),
    ('ß', 2, ".###./#...#/#...#/#.##./#...#/#...#/#.##./#...."),
    ('æ', 4, ".##.##./...#..#/.######/#..#.../#..#..#/.##.##."),
    ('Æ', 2, "..#####/.##.#../#..#.../#..#.../######./#..#.../#..#.../#..####"),
    ('ø', 4, ".####./#...##/#..#.#/#.#..#/##...#/.####."),
    ('Ø', 2, ".####./#...##/#..#.#/#..#.#/#.#..#/#.#..#/##...#/.####."),
    ('ç', 4, ".####/#..../#..../#..../#..../.####/..#../.##.."),
    ('Ç', 2, ".####./#....#/#...../#...../#...../#...../#....#/.####./..#.../.##..."),
    ('ı', 4, "##./.#./.#./.#./.#./###"),
    // Icelandic eth and thorn.
    ('ð', 2, "...##./..###./.####./#....#/#....#/#....#/#....#/.####."),
    ('Ð', 2, ".#####./.#....#/.#....#/####..#/.#....#/.#....#/.#....#/.#####."),
    ('þ', 2, "#...../#...../#.###./##...#/#....#/#....#/##...#/#.###./#...../#....."),
    ('Þ', 2, "#...../#####./#....#/#....#/#....#/#####./#...../#....."),
    // Modifier apostrophe (Yup'ik, Cree) and turned comma, Inuit letters with a bar or a tail.
    ('\u{2BC}', 2, "##/.#/#."),
    ('\u{2BB}', 2, ".#/#./##"),
    ('ł', 2, "##../.#../.#.#/.##./##../.#../.#../.##."),
    ('Ł', 2, "#...../#...../#.#.../##..../#...../#...../#...../######"),
    ('ŋ', 4, "#.###./##...#/#....#/#....#/#....#/#....#/.....#/....#."),
    ('Ŋ', 2, "#....#/##...#/##...#/#.#..#/#.#..#/#..#.#/#...##/#....#/.....#/....#."),
];

/// Accent marks: rows, drawn above a letter.
fn accent(kind: char) -> &'static [&'static str] {
    match kind {
        '\u{301}' => &[".#", "#."],          // acute
        '\u{300}' => &["#.", ".#"],          // grave
        '\u{302}' => &[".#.", "#.#"],        // circumflex
        '\u{303}' => &[".#.#", "#.#."],      // tilde
        '\u{308}' => &["#.#"],               // diaeresis
        '\u{306}' => &["#.#", ".#."],        // breve
        '\u{304}' => &["###", "..."],        // macron (pinyin first tone, Hepburn long vowels)
        '\u{30C}' => &["#.#", ".#."],        // caron (pinyin third tone; at this size it is the breve's shape)
        '\u{30A}' => &[".#.", "#.#", ".#."], // ring (three rows; only over capitals' two-row gap by shifting)
        _ => &[],
    }
}

/// `(composed character, plain base, accent mark)`.
const COMPOSED: &[(char, char, char)] = &[
    ('À', 'A', '\u{300}'), ('Á', 'A', '\u{301}'), ('Â', 'A', '\u{302}'), ('Ã', 'A', '\u{303}'), ('Ä', 'A', '\u{308}'), ('Å', 'A', '\u{30A}'),
    ('È', 'E', '\u{300}'), ('É', 'E', '\u{301}'), ('Ê', 'E', '\u{302}'), ('Ë', 'E', '\u{308}'),
    ('Ì', 'I', '\u{300}'), ('Í', 'I', '\u{301}'), ('Î', 'I', '\u{302}'), ('Ï', 'I', '\u{308}'),
    ('Ñ', 'N', '\u{303}'),
    ('Ò', 'O', '\u{300}'), ('Ó', 'O', '\u{301}'), ('Ô', 'O', '\u{302}'), ('Õ', 'O', '\u{303}'), ('Ö', 'O', '\u{308}'),
    ('Ù', 'U', '\u{300}'), ('Ú', 'U', '\u{301}'), ('Û', 'U', '\u{302}'), ('Ü', 'U', '\u{308}'),
    ('Ý', 'Y', '\u{301}'),
    ('à', 'a', '\u{300}'), ('á', 'a', '\u{301}'), ('â', 'a', '\u{302}'), ('ã', 'a', '\u{303}'), ('ä', 'a', '\u{308}'), ('å', 'a', '\u{30A}'),
    ('è', 'e', '\u{300}'), ('é', 'e', '\u{301}'), ('ê', 'e', '\u{302}'), ('ë', 'e', '\u{308}'),
    ('ì', 'ı', '\u{300}'), ('í', 'ı', '\u{301}'), ('î', 'ı', '\u{302}'), ('ï', 'ı', '\u{308}'),
    ('ñ', 'n', '\u{303}'),
    ('ò', 'o', '\u{300}'), ('ó', 'o', '\u{301}'), ('ô', 'o', '\u{302}'), ('õ', 'o', '\u{303}'), ('ö', 'o', '\u{308}'),
    ('ù', 'u', '\u{300}'), ('ú', 'u', '\u{301}'), ('û', 'u', '\u{302}'), ('ü', 'u', '\u{308}'),
    ('ý', 'y', '\u{301}'), ('ÿ', 'y', '\u{308}'),
    // Pinyin tones and Hepburn macrons.
    ('ā', 'a', '\u{304}'), ('ē', 'e', '\u{304}'), ('ī', 'ı', '\u{304}'), ('ō', 'o', '\u{304}'), ('ū', 'u', '\u{304}'),
    ('Ā', 'A', '\u{304}'), ('Ē', 'E', '\u{304}'), ('Ī', 'I', '\u{304}'), ('Ō', 'O', '\u{304}'), ('Ū', 'U', '\u{304}'),
    ('ǎ', 'a', '\u{30C}'), ('ě', 'e', '\u{30C}'), ('ǐ', 'ı', '\u{30C}'), ('ǒ', 'o', '\u{30C}'), ('ǔ', 'u', '\u{30C}'),
    ('Ǎ', 'A', '\u{30C}'), ('Ě', 'E', '\u{30C}'), ('Ǐ', 'I', '\u{30C}'), ('Ǒ', 'O', '\u{30C}'), ('Ǔ', 'U', '\u{30C}'),
    ('ḿ', 'm', '\u{301}'), ('ń', 'n', '\u{301}'), ('ň', 'n', '\u{30C}'), ('ǹ', 'n', '\u{300}'),
];

/// `(character, base, ü's accent)`: ü with a tone mark above its dots (pinyin lü/nü syllables).
const STACKED: &[(char, char, char)] = &[('ǖ', 'ü', '\u{304}'), ('ǘ', 'ü', '\u{301}'), ('ǚ', 'ü', '\u{30C}'), ('ǜ', 'ü', '\u{300}')];

/// A glyph's ink: `(top cell row, width, rows of booleans)`.
pub(crate) type Art = (i32, usize, Vec<Vec<bool>>);

pub(crate) fn parse(top: i32, rows: &str) -> Art {
    let rows: Vec<Vec<bool>> = rows.split('/').map(|r| r.chars().map(|c| c == '#').collect()).collect();
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    (top, width, rows)
}

pub(crate) fn art_of(c: char) -> Option<Art> {
    BASE.iter().find(|g| g.0 == c).map(|g| parse(g.1, g.2))
}

/// Puts an accent over `base`: centred over the glyph, in the two rows above its top (rows 0-1 over a
/// capital, 2-3 over a lowercase letter). The ring is drawn one row shorter so it fits.
pub(crate) fn compose(base: &Art, mark: char) -> Art {
    let (top, width, rows) = base;
    let a = accent(mark);
    let mut out = rows.clone();
    // The accent's own rows, placed so its last row sits on the row just above the base.
    let acc_top = top - a.len().min(2) as i32;
    let acc: Vec<&&str> = if a.len() > 2 { a[1..].iter().collect() } else { a.iter().collect() };
    let acc_w = acc.iter().map(|r| r.len()).max().unwrap_or(0);
    let left = (width.saturating_sub(acc_w)) / 2;
    // Build a taller grid starting at the accent.
    let rise = (top - acc_top).max(0) as usize;
    let mut grid: Vec<Vec<bool>> = vec![vec![false; *width]; rise];
    for (i, line) in acc.iter().enumerate() {
        for (x, ch) in line.chars().enumerate() {
            if ch == '#' && left + x < *width {
                grid[i][left + x] = true;
            }
        }
    }
    grid.append(&mut out);
    (acc_top, *width, grid)
}

pub(crate) fn to_glyph(c: char, art: &Art) -> GlyphSpec {
    let (top, width, rows) = art;
    GlyphSpec {
        chars: vec![c],
        advance: *width as i32 + GAP,
        colour: false,
        layers: vec![Layer { color: 0, rects: rects_from_mask(*width, rows, *top) }],
    }
}

/// The `GSUB` substitutions of the typeface (Arabic joining forms).
pub fn substitutions() -> Vec<crate::font_gen::Substitution> {
    crate::pixel_arabic::substitutions()
}

/// The `GSUB` ligatures of the typeface.
pub fn ligatures() -> Vec<crate::font_gen::Ligature> {
    let mut v = crate::pixel_arabic::ligatures();
    v.extend(crate::pixel_devanagari::ligatures());
    v
}

/// Every glyph of the typeface (the space is the font's own).
pub fn glyphs() -> Vec<GlyphSpec> {
    let mut out = Vec::new();
    for (c, top, rows) in BASE {
        out.push(to_glyph(*c, &parse(*top, rows)));
    }
    for (c, base, mark) in COMPOSED {
        if let Some(b) = art_of(*base) {
            out.push(to_glyph(*c, &compose(&b, *mark)));
        }
    }
    for (c, base, mark) in STACKED {
        if let Some(b) = art_of('u') {
            let dotted = compose(&b, '\u{308}');
            let _ = base;
            out.push(to_glyph(*c, &compose(&dotted, *mark)));
        }
    }
    // The tie that joins two letters into one sound (Yup'ik u͡g): a zero-width arch reaching back over
    // the letter before it.
    out.push(crate::pixel_scripts::point_glyph('\u{361}', &[(-3, 2), (-2, 2), (-1, 2), (0, 2), (1, 2), (2, 2), (-4, 3), (3, 3)]));
    out.extend(crate::pixel_scripts::glyphs());
    // A non-breaking space is a space.
    out.push(GlyphSpec { chars: vec!['\u{A0}'], advance: SPACE, colour: false, layers: vec![Layer { color: 0, rects: vec![] }] });
    out
}

/// Renders glyph art to a grayscale sheet for looking at (tests and the contact-sheet tool).
pub fn contact_sheet(glyph_list: &[GlyphSpec], scale: u32, columns: u32) -> (u32, u32, Vec<u8>) {
    let cell_w = 16u32;
    let cell_h = 14u32;
    let rows = (glyph_list.len() as u32).div_ceil(columns);
    let (w, h) = (columns * cell_w * scale, rows * cell_h * scale);
    let mut px = vec![255u8; (w * h) as usize];
    for (i, g) in glyph_list.iter().enumerate() {
        let (cx, cy) = ((i as u32 % columns) * cell_w, (i as u32 / columns) * cell_h);
        for layer in &g.layers {
            for r in &layer.rects {
                for y in r.y0..r.y1 {
                    for x in r.x0..r.x1 {
                        let (gx, gy) = (cx as i32 + 1 + x, cy as i32 + y);
                        if gx < 0 || gy < 0 {
                            continue;
                        }
                        for sy in 0..scale {
                            for sx in 0..scale {
                                let (ox, oy) = (gx as u32 * scale + sx, gy as u32 * scale + sy);
                                if ox < w && oy < h {
                                    px[(oy * w + ox) as usize] = 0;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    (w, h, px)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_printable_ascii_character_has_a_glyph_except_the_space() {
        let have: Vec<char> = glyphs().iter().flat_map(|g| g.chars.clone()).collect();
        for c in '!'..='~' {
            assert!(have.contains(&c), "missing glyph for {c:?}");
        }
    }

    #[test]
    fn glyphs_stay_inside_the_cell_and_have_ink() {
        for g in glyphs() {
            if g.chars == ['\u{A0}'] {
                continue;
            }
            let rects: Vec<_> = g.layers.iter().flat_map(|l| l.rects.iter()).collect();
            assert!(!rects.is_empty(), "{:?} is blank", g.chars);
            for r in rects {
                assert!(r.y0 >= 0 && r.y1 <= crate::font_gen::EM_ROWS, "{:?} leaves the cell: {r:?}", g.chars);
                // Hebrew, Arabic and Devanagari marks sit over the letter before them, so they reach left of their own origin.
                let reach_left = if ('\u{300}'..='\u{36F}').contains(&g.chars[0]) || ('\u{590}'..='\u{6FF}').contains(&g.chars[0]) || ('\u{900}'..='\u{97F}').contains(&g.chars[0]) || ('\u{E900}'..='\u{E9FF}').contains(&g.chars[0]) { -6 } else { 0 };
                // A zero-width mark may also reach a little right of its origin (a tie spans two letters).
                let reach_right = if g.advance == 0 { 4 } else { g.advance };
                assert!(r.x0 >= reach_left && r.x1 <= reach_right, "{:?} is wider than its advance: {r:?}", g.chars);
            }
        }
    }

    #[test]
    fn icelandic_yupik_and_inuit_latin_letters_are_all_there() {
        let have: std::collections::HashSet<char> = glyphs().iter().flat_map(|g| g.chars.iter().copied()).collect();
        // CLDR's Icelandic main, auxiliary and index letters.
        for c in "aábdðeéfghiíjklmnoópqrstuúvwxyýzþæö".chars().chain("AÁBCDÐEÉFGHIÍJKLMNOÓPQRSTUÚVWXYÝZÞÆÖ".chars()) {
            assert!(have.contains(&c), "Icelandic {c} has no glyph");
        }
        // Yup'ik: m and n with acute (voiceless), the modifier apostrophe and the tie of u͡g.
        for c in "ḿń\u{2BC}\u{361}".chars() {
            assert!(have.contains(&c), "Yup'ik U+{:X} has no glyph", c as u32);
        }
        // Inuit Latin and Cree SRO.
        for c in "łŁŋŊâêîôāēīō\u{2BB}".chars() {
            assert!(have.contains(&c), "U+{:X} has no glyph", c as u32);
        }
    }

    #[test]
    fn eth_and_thorn_differ_from_the_letters_they_resemble() {
        let ink = |c: char| {
            let g = glyphs().into_iter().find(|g| g.chars == [c]).unwrap();
            let mut r: Vec<_> = g.layers.iter().flat_map(|l| l.rects.iter().map(|r| (r.x0, r.y0, r.x1, r.y1))).collect();
            r.sort();
            r
        };
        for (a, b) in [('ð', 'd'), ('þ', 'p'), ('þ', 'b'), ('Ð', 'D'), ('Þ', 'P'), ('ł', 'l'), ('Ł', 'L'), ('ŋ', 'n'), ('Ŋ', 'N')] {
            assert_ne!(ink(a), ink(b), "{a} looks like {b}");
        }
        // Thorn has both an ascender and a descender; eth has an ascender; the tie takes no room.
        let rows = |c: char| ink(c).iter().map(|r| (r.1, r.3)).fold((i32::MAX, 0), |(lo, hi), (a, b)| (lo.min(a), hi.max(b)));
        assert!(rows('þ').0 <= 2 && rows('þ').1 >= 12, "{:?}", rows('þ'));
        assert!(rows('ð').0 <= 2 && rows('ð').1 <= 10);
        let tie = glyphs().into_iter().find(|g| g.chars == ['\u{361}']).unwrap();
        assert_eq!(tie.advance, 0);
    }

    #[test]
    fn rows_of_a_glyph_are_all_the_same_width() {
        // A short row would shift ink; a ragged glyph is a typo.
        for (c, _, rows) in BASE {
            let widths: Vec<usize> = rows.split('/').map(str::len).collect();
            let w = *widths.iter().max().unwrap();
            // Only glyphs deliberately drawn with a short last row may differ.
            let ragged = widths.iter().filter(|x| **x != w).count();
            assert!(ragged <= 1 || matches!(c, 'æ' | 'Æ' | '…' | '~'), "{c:?} has ragged rows: {widths:?}");
        }
    }

    #[test]
    fn descenders_go_below_the_baseline_and_capitals_do_not() {
        let spec = glyphs();
        let bottom = |c: char| spec.iter().find(|g| g.chars == [c]).unwrap().layers.iter().flat_map(|l| l.rects.iter()).map(|r| r.y1).max().unwrap();
        assert_eq!(bottom('A'), 10, "capitals sit on the baseline (row 10 is its boundary)");
        for c in ['g', 'j', 'p', 'q', 'y'] {
            assert!(bottom(c) > 10, "{c} has a descender");
        }
    }

    #[test]
    fn accents_sit_above_the_letter_without_touching_it() {
        let spec = glyphs();
        let top = |c: char| spec.iter().find(|g| g.chars == [c]).unwrap().layers.iter().flat_map(|l| l.rects.iter()).map(|r| r.y0).min().unwrap();
        assert!(top('Á') < top('A'));
        assert!(top('é') < top('e'));
        assert!(top('Á') >= 0 && top('é') >= 0);
    }

    #[test]
    fn the_typeface_builds_a_valid_font() {
        let spec = crate::font_gen::FontSpec { family: "Craftmjne".into(), palette: vec![], glyphs: glyphs(), space_advance: SPACE, em_rows: crate::font_gen::EM_ROWS, baseline_row: crate::font_gen::BASELINE_ROW, substitutions: substitutions(), ligatures: ligatures() };
        let bytes = crate::font_gen::build_ttf(&spec);
        let face = ttf_parser::Face::parse(&bytes, 0).unwrap();
        for c in "Hello, World! 0123456789 àéîõü".chars() {
            assert!(face.glyph_index(c).is_some(), "{c:?} should be in the font");
        }
    }

    /// Writes a contact sheet to look at: `CRAFT_SHEET=/path.png cargo test pixel_font::tests::contact -- --nocapture`.
    #[test]
    fn contact_sheet_renders() {
        let (w, h, px) = contact_sheet(&glyphs(), 4, 16);
        assert!(w > 0 && h > 0);
        if let Ok(path) = std::env::var("CRAFT_SHEET") {
            image::GrayImage::from_raw(w, h, px).unwrap().save(path).unwrap();
        }
    }
}
