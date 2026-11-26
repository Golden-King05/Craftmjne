//! Letters for scripts other than Latin, drawn in the same 12-row grid as `pixel_font`.
//!
//! * **Cyrillic**: Russian, Ukrainian, Belarusian and Serbian letters. Many are the Latin letter's
//!   shape (А, В, Е, К, М, Н, О, Р, С, Т, Х, а, е, о, р, с, у, х), so those reuse the Latin drawing;
//!   the rest are drawn here. Ё, Й, Ї, Ў are a letter plus an accent mark, like the Latin accents.
//! * **Hebrew**: the 22 letters and the five final forms. Right-to-left layout is the text
//!   engine's job; the font just has the letters. No vowel points.
//!
//! Hangul, Arabic and Devanagari are in their own modules because they need more than a table.

use crate::font_gen::GlyphSpec;
use crate::pixel_font::{art_of, compose, parse, to_glyph, Art};

/// `(character, first cell row, rows)`; same format as `pixel_font`'s table.
const CYRILLIC: &[(char, i32, &str)] = &[
    // Capitals (rows 2-9)
    ('Б', 2, "######/#...../#...../#####./#....#/#....#/#....#/#####."),
    ('Г', 2, "######/#...../#...../#...../#...../#...../#...../#....."),
    ('Ґ', 2, "....#./######/#...../#...../#...../#...../#...../#....."),
    ('Д', 2, ".####./.#..#./.#..#./.#..#./.#..#./######/#....#/#....#"),
    ('Ж', 2, "#..#..#/#..#..#/.#.#.#./..###../..###../.#.#.#./#..#..#/#..#..#"),
    ('З', 2, ".####./#....#/.....#/..###./.....#/.....#/#....#/.####."),
    ('И', 2, "#....#/#....#/#...##/#..#.#/#.#..#/##...#/#....#/#....#"),
    ('Л', 2, ".#####/.#...#/.#...#/.#...#/.#...#/.#...#/#....#/#....#"),
    ('П', 2, "######/#....#/#....#/#....#/#....#/#....#/#....#/#....#"),
    ('У', 2, "#....#/#....#/#....#/.#####/.....#/.....#/#....#/.####."),
    ('Ф', 2, "...#.../.#####./#..#..#/#..#..#/#..#..#/.#####./...#.../...#..."),
    ('Ц', 2, "#....#/#....#/#....#/#....#/#....#/#....#/######/.....#"),
    ('Ч', 2, "#....#/#....#/#....#/.#####/.....#/.....#/.....#/.....#"),
    ('Ш', 2, "#..#..#/#..#..#/#..#..#/#..#..#/#..#..#/#..#..#/#..#..#/#######"),
    ('Щ', 2, "#..#..#/#..#..#/#..#..#/#..#..#/#..#..#/#..#..#/#######/......#"),
    ('Ъ', 2, "##..../.#..../.#..../.####./.#...#/.#...#/.#...#/.####."),
    ('Ы', 2, "#.....#/#.....#/#.....#/####..#/#...#.#/#...#.#/#...#.#/####..#"),
    ('Ь', 2, "#...../#...../#...../#####./#....#/#....#/#....#/#####."),
    ('Э', 2, ".####./#....#/.....#/..####/.....#/.....#/#....#/.####."),
    ('Ю', 2, "#..###./#.#...#/#.#...#/###...#/#.#...#/#.#...#/#.#...#/#..###."),
    ('Я', 2, ".#####/#....#/#....#/#....#/.#####/..#..#/.#...#/#....#"),
    ('Є', 2, ".####./#....#/#...../####../#...../#...../#....#/.####."),
    // Lowercase (rows 4-9, ascenders from row 2, descenders to row 11)
    ('б', 2, "..####/.#..../#...../#####./#....#/#....#/#....#/.####."),
    ('в', 4, "#####./#....#/#####./#....#/#....#/#####."),
    ('г', 4, "#####/#..../#..../#..../#..../#...."),
    ('ґ', 4, "..#../#####/#..../#..../#..../#...."),
    ('д', 4, ".####./.#..#./.#..#./.#..#./.#..#./######/#....#"),
    ('ж', 4, "#..#..#/.#.#.#./..###../..###../.#.#.#./#..#..#"),
    ('з', 4, ".####./.....#/..###./.....#/#....#/.####."),
    ('и', 4, "#....#/#...##/#..#.#/#.#..#/##...#/#....#"),
    ('к', 4, "#...#./#..#../###.../#..#../#...#./#....#"),
    ('л', 4, ".#####/.#...#/.#...#/.#...#/.#...#/#....#"),
    ('м', 4, "#....#/##..##/#.##.#/#.##.#/#....#/#....#"),
    ('н', 4, "#....#/#....#/######/#....#/#....#/#....#"),
    ('п', 4, "######/#....#/#....#/#....#/#....#/#....#"),
    ('т', 4, "#####/..#../..#../..#../..#../..#.."),
    ('ф', 2, "...#.../...#.../.#####./#..#..#/#..#..#/#..#..#/#..#..#/.#####./...#.../...#..."),
    ('ц', 4, "#....#/#....#/#....#/#....#/#....#/######/.....#"),
    ('ч', 4, "#....#/#....#/#....#/.#####/.....#/.....#"),
    ('ш', 4, "#..#..#/#..#..#/#..#..#/#..#..#/#..#..#/#######"),
    ('щ', 4, "#..#..#/#..#..#/#..#..#/#..#..#/#..#..#/#######/......#"),
    ('ъ', 4, "##..../.#..../.####./.#...#/.#...#/.####."),
    ('ы', 4, "#.....#/#.....#/####..#/#...#.#/#...#.#/####..#"),
    ('ь', 4, "#...../#...../#####./#....#/#....#/#####."),
    ('э', 4, ".####./.....#/..####/.....#/#....#/.####."),
    ('ю', 4, "#..##./#.#..#/###..#/#.#..#/#.#..#/#..##."),
    ('я', 4, ".####/#...#/#...#/.####/..#.#/#...#"),
    ('є', 4, ".####/#..../###../#..../#..../.####"),
];

/// Cyrillic letters that are drawn exactly like a Latin one.
const LOOKALIKES: &[(char, char)] = &[
    ('А', 'A'), ('В', 'B'), ('Е', 'E'), ('К', 'K'), ('М', 'M'), ('Н', 'H'), ('О', 'O'), ('Р', 'P'),
    ('С', 'C'), ('Т', 'T'), ('Х', 'X'), ('І', 'I'), ('Ј', 'J'),
    ('а', 'a'), ('е', 'e'), ('о', 'o'), ('р', 'p'), ('с', 'c'), ('у', 'y'), ('х', 'x'), ('і', 'i'), ('ј', 'j'),
];

/// `(character, base, accent mark)`, like `pixel_font`'s composed Latin letters.
const COMPOSED: &[(char, char, char)] = &[
    ('Ё', 'E', '\u{308}'), ('ё', 'e', '\u{308}'),
    ('Й', 'И', '\u{306}'), ('й', 'и', '\u{306}'),
    ('Ї', 'I', '\u{308}'), ('ї', 'ı', '\u{308}'),
    ('Ў', 'У', '\u{306}'), ('ў', 'у', '\u{306}'),
];

/// Hebrew, rows 3-9 (finals drop below the baseline). Written as the letters are read; the layout
/// engine puts them right to left.
const HEBREW: &[(char, i32, &str)] = &[
    ('א', 3, "#...##/#..#../#.#.../.##..#/..#..#/.#...#/#....#"),
    ('ב', 3, "#####./....#./....#./....#./....#./....#./######"),
    ('ג', 3, ".####./....#./...##./..#.#./.#..#./#...#./....#."),
    ('ד', 3, "######/....#./....#./....#./....#./....#./....#."),
    ('ה', 3, "#####./....#./#...#./#...#./#...#./#...#./#...#."),
    ('ו', 3, "..##../...#../...#../...#../...#../...#../...#.."),
    ('ז', 3, "#####./...#../...#../...#../...#../...#../...#.."),
    ('ח', 3, "######/#....#/#....#/#....#/#....#/#....#/#....#"),
    ('ט', 3, "#....#/#....#/#.##.#/#.#..#/#.#..#/#....#/.####."),
    ('י', 3, "##./.#./.#."),
    ('כ', 3, "#####./....#./....#./...#../....#./....#./#####."),
    ('ך', 3, "######/....#./....#./....#./....#./....#./....#./....#./....#."),
    ('ל', 2, "#...../#...../#####./....#./....#./...#../..#.../.#...."),
    ('מ', 3, "#####./#...#./#...#./#..##./#.#.#./#...#./#####."),
    ('ם', 3, "#####./#....#/#....#/#....#/#....#/#....#/######"),
    ('נ', 3, "..####/....#./....#./....#./....#./#...#./.####."),
    ('ן', 3, "..##../...#../...#../...#../...#../...#../...#../...#../...#.."),
    ('ס', 3, ".####./#....#/#....#/#....#/#....#/#....#/.####."),
    ('ע', 3, "#....#/#....#/#....#/.#..#./..##../.#..#./##..##"),
    ('פ', 3, "#####./#....#/#.####/#.#..#/#.#..#/#.#..#/#####."),
    ('ף', 3, "#####./#....#/#.####/#.#..#/#.#..#/#.#..#/#.#..#/....#./....#."),
    ('צ', 3, "#...#./#...#./.#.#../..##../.#.#../#..#../.####."),
    ('ץ', 3, "#...#./#...#./.#.#../..##../..#.../..#.../..#.../..#.../..#..."),
    ('ק', 3, "#####./....#./#...#./#...#./#...#./....#./....#."),
    ('ר', 3, "####./...#./...#./...#./...#./...#./...#."),
    ('ש', 3, "#.#.#/#.#.#/#.#.#/#.#.#/#.#.#/#..##/.####"),
    ('ת', 3, "#####./#...#./#...#./#...#./#...#./#...#./#...##"),
];


/// Hebrew points (niqqud), dagesh and the shin/sin dots: zero-advance marks. The shaper centres
/// them over the letter they follow, so only their own shape matters. `(code point, pixels)`.
const HEBREW_POINTS: &[(char, &[(i32, i32)])] = &[
    ('\u{5B0}', &[(-3, 10), (-3, 11)]),                               // sheva
    ('\u{5B1}', &[(-5, 11), (-4, 10), (-2, 10), (-3, 11)]),          // hataf segol
    ('\u{5B2}', &[(-5, 11), (-4, 10), (-3, 10), (-2, 10)]),          // hataf patah
    ('\u{5B3}', &[(-5, 11), (-4, 10), (-3, 10), (-2, 10), (-3, 11)]), // hataf qamats
    ('\u{5B4}', &[(-3, 10)]),                                         // hiriq
    ('\u{5B5}', &[(-4, 10), (-2, 10)]),                               // tsere
    ('\u{5B6}', &[(-4, 10), (-2, 10), (-3, 11)]),                     // segol
    ('\u{5B7}', &[(-4, 10), (-3, 10), (-2, 10)]),                     // patah
    ('\u{5B8}', &[(-4, 10), (-3, 10), (-2, 10), (-3, 11)]),           // qamats
    ('\u{5B9}', &[(-3, 1)]),                                          // holam
    ('\u{5BA}', &[(-3, 1)]),                                          // holam haser for vav
    ('\u{5BB}', &[(-4, 9), (-3, 10), (-2, 11)]),                      // qubuts
    ('\u{5BC}', &[(-3, 6)]),                                          // dagesh
    ('\u{5BD}', &[(-3, 10), (-3, 11)]),                               // meteg
    ('\u{5BF}', &[(-4, 1), (-3, 1), (-2, 1)]),                        // rafe
    ('\u{5C1}', &[(-2, 1)]),                                          // shin dot
    ('\u{5C2}', &[(-4, 1)]),                                          // sin dot
];

fn point_glyph(c: char, pixels: &[(i32, i32)]) -> GlyphSpec {
    let left = pixels.iter().map(|(x, _)| -x).max().unwrap_or(0).max(0) as usize;
    let mut g = vec![vec![false; left + 1]; 12];
    for (x, y) in pixels {
        g[*y as usize][(left as i32 + x) as usize] = true;
    }
    let rects = crate::font_gen::rects_from_mask(left + 1, &g, 0)
        .into_iter()
        .map(|mut r| {
            r.x0 -= left as i32;
            r.x1 -= left as i32;
            r
        })
        .collect();
    GlyphSpec { chars: vec![c], advance: 0, colour: false, layers: vec![crate::font_gen::Layer { color: 0, rects }] }
}

/// The art for a letter wherever it lives: this module's tables, a Latin lookalike, or Latin itself.
fn art_for(c: char) -> Option<Art> {
    if let Some(g) = CYRILLIC.iter().chain(HEBREW).find(|g| g.0 == c) {
        return Some(parse(g.1, g.2));
    }
    if let Some((_, latin)) = LOOKALIKES.iter().find(|(cy, _)| *cy == c) {
        return art_of(*latin);
    }
    art_of(c)
}

/// Every glyph this module draws.
pub fn glyphs() -> Vec<GlyphSpec> {
    let mut out = Vec::new();
    for (c, top, rows) in CYRILLIC.iter().chain(HEBREW) {
        out.push(to_glyph(*c, &parse(*top, rows)));
    }
    for (c, latin) in LOOKALIKES {
        if let Some(art) = art_of(*latin) {
            out.push(to_glyph(*c, &art));
        }
    }
    for (c, base, mark) in COMPOSED {
        if let Some(b) = art_for(*base) {
            out.push(to_glyph(*c, &compose(&b, *mark)));
        }
    }
    for (c, px) in HEBREW_POINTS {
        out.push(point_glyph(*c, px));
    }
    out.extend(crate::pixel_hangul::glyphs());
    out.extend(crate::pixel_arabic::glyphs());
    out.extend(crate::pixel_devanagari::glyphs());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn rows_of_a_glyph_are_all_the_same_width() {
        for (c, _, rows) in CYRILLIC.iter().chain(HEBREW) {
            let widths: HashSet<usize> = rows.split('/').map(str::len).collect();
            assert_eq!(widths.len(), 1, "{c}: rows differ in width");
        }
    }

    #[test]
    fn the_whole_russian_and_hebrew_alphabets_are_there() {
        let have: HashSet<char> = glyphs().iter().flat_map(|g| g.chars.clone()).collect();
        for c in ('\u{410}'..='\u{44F}').chain(['Ё', 'ё', 'І', 'і', 'Ї', 'ї', 'Є', 'є', 'Ґ', 'ґ', 'Ў', 'ў']) {
            assert!(have.contains(&c), "missing Cyrillic {c} U+{:04X}", c as u32);
        }
        for c in '\u{5D0}'..='\u{5EA}' {
            assert!(have.contains(&c), "missing Hebrew U+{:04X}", c as u32);
        }
    }

    #[test]
    fn no_character_is_drawn_twice() {
        let mut seen = HashSet::new();
        for g in crate::pixel_font::glyphs() {
            for c in &g.chars {
                assert!(seen.insert(*c), "{c} U+{:04X} has two drawings", *c as u32);
            }
        }
    }

    #[test]
    fn every_glyph_stays_inside_the_cell() {
        for g in glyphs() {
            for layer in &g.layers {
                for r in &layer.rects {
                    assert!(r.y0 >= 0 && r.y1 <= 12, "{:?} leaves rows 0-11", g.chars);
                }
            }
        }
    }

    #[test]
    fn hebrew_points_take_no_room_and_stay_with_their_letter() {
        let font = crate::fonts::default_font_bytes();
        let face = rustybuzz::Face::from_slice(&font, 0).unwrap();
        let tt = ttf_parser::Face::parse(&font, 0).unwrap();
        for (mark, _) in HEBREW_POINTS {
            let mut buf = rustybuzz::UnicodeBuffer::new();
            buf.push_str(&format!("ב{mark}"));
            buf.set_direction(rustybuzz::Direction::RightToLeft);
            buf.set_script(rustybuzz::script::HEBREW);
            let out = rustybuzz::shape(&face, &[], buf);
            assert_eq!(out.len(), 2, "U+{:04X}", *mark as u32);
            let gid = tt.glyph_index(*mark).unwrap().0 as u32;
            let at = out.glyph_infos().iter().position(|i| i.glyph_id == gid).expect("the point is drawn");
            assert_eq!(out.glyph_positions()[at].x_advance, 0, "U+{:04X} takes room", *mark as u32);
        }
    }
}
