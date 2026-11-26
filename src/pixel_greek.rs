//! Greek (monotonic), drawn in the game's own 12-row Latin style.
//!
//! Most capitals are the Latin letter's shape (Α Β Ε Ζ Η Ι Κ Μ Ν Ο Ρ Τ Υ Χ), so those reuse the Latin
//! drawing; the rest are drawn here, as are the lowercase letters. The tonos is the acute mark over
//! the letter (a capital's tonos belongs to its left in print; here it sits above, like every other
//! accent in this font) and the dialytika is the diaeresis; ΐ and ΰ carry both, stacked.
//!
//! **Not here:** the polytonic letters of Greek Extended (`U+1F00-1FFF`), the archaic and Coptic
//! letters of the Greek and Coptic block, and the combining marks - those come from the bitmap
//! fallback font ([`crate::scripts`]). Symbol forms (ϑ ϕ ϖ ...) too.

use crate::font_gen::GlyphSpec;
use crate::pixel_font::{art_of, compose, parse, to_glyph, Art};

/// `(character, first cell row, rows)`; same format as `pixel_font`'s table.
const GREEK: &[(char, i32, &str)] = &[
    // Capitals (rows 2-9)
    ('Γ', 2, "#####./#...../#...../#...../#...../#...../#...../#....."),
    ('Δ', 2, "..##../.#..#./.#..#./.#..#./#....#/#....#/#....#/######"),
    ('Θ', 2, ".####./#....#/#....#/######/#....#/#....#/#....#/.####."),
    ('Λ', 2, "..##../..##../.#..#./.#..#./#....#/#....#/#....#/#....#"),
    ('Ξ', 2, "######/....../....../.####./....../....../....../######"),
    ('Π', 2, "######/#....#/#....#/#....#/#....#/#....#/#....#/#....#"),
    ('Σ', 2, "######/#...../.#..../..#.../..#.../.#..../#...../######"),
    ('Φ', 2, "..##../.####./#.##.#/#.##.#/#.##.#/.####./..##../..##.."),
    ('Ψ', 2, "#.##.#/#.##.#/#.##.#/#.##.#/.####./..##../..##../..##.."),
    ('Ω', 2, ".####./#....#/#....#/#....#/#....#/.#..#./.#..#./##..##"),
    // Lowercase
    ('α', 4, ".####./#....#/#....#/#....#/#...##/.###.#"),
    ('β', 2, ".###../#...#./#...#./#.###./#...#./#...#./#...#./#.###./#...../#....."),
    ('γ', 4, "#....#/#....#/.#..#./.#..#./..##../..#.../..#.../..#..."),
    ('δ', 2, ".####./..#.../.#..../.####./#....#/#....#/#....#/.####."),
    ('ε', 4, ".####./#...../.###../#...../#...../.####."),
    ('ζ', 2, "######/#...../#...../.#..../..#.../.#..../#...../#...../#....#/.####."),
    ('η', 4, "#.###./##...#/#....#/#....#/#....#/#....#/.....#/.....#"),
    ('θ', 2, ".###./#...#/#...#/#####/#...#/#...#/#...#/.###."),
    ('κ', 4, "#...#./#..#../###.../#.#.../#..#../#...#."),
    ('λ', 2, "#...../.#..../.#..../..#.../..#.../.#.#../.#.#../#...#."),
    ('μ', 4, "#....#/#....#/#....#/#....#/#...##/#.##.#/#...../#....."),
    ('ξ', 2, "######/#...../.###../#...../#...../.###../#...../#...../#....#/.####."),
    ('π', 4, "######/.#..#./.#..#./.#..#./.#..#./##..##"),
    ('σ', 4, ".#####/#...#./#....#/#....#/#...#./.###.."),
    ('ς', 4, ".####./#...../#...../#...../.####./...#../..#.../.###.."),
    ('τ', 4, "######/..#.../..#.../..#.../..#.../...##."),
    ('υ', 4, "#....#/#....#/#....#/#....#/#....#/.####."),
    ('φ', 2, "..#.../..#.../.####./#.#..#/#.#..#/#.#..#/.####./..#.../..#.../..#..."),
    ('χ', 4, "#...#./.#.#../..#.../..#.../.#.#../#...#./#...../#....."),
    ('ψ', 2, "#.##.#/#.##.#/#.##.#/#.##.#/.####./..##../..##../..##.."),
    ('ω', 4, "#....#/#....#/#.##.#/#.##.#/##..##/.#..#."),
];

/// Letters drawn exactly like a Latin one.
const LOOKALIKES: &[(char, char)] = &[
    ('Α', 'A'), ('Β', 'B'), ('Ε', 'E'), ('Ζ', 'Z'), ('Η', 'H'), ('Ι', 'I'), ('Κ', 'K'), ('Μ', 'M'), ('Ν', 'N'),
    ('Ο', 'O'), ('Ρ', 'P'), ('Τ', 'T'), ('Υ', 'Y'), ('Χ', 'X'),
    ('ο', 'o'), ('ρ', 'p'), ('ν', 'v'), ('ι', 'ı'),
    // The Greek question mark and the ano teleia are a semicolon and a middle dot; the micro sign is mu.
    ('\u{37E}', ';'), ('\u{387}', '·'),
];

/// `(character, base, mark)`: the tonos (acute) and dialytika (diaeresis).
const COMPOSED: &[(char, char, char)] = &[
    ('ά', 'α', '\u{301}'), ('έ', 'ε', '\u{301}'), ('ή', 'η', '\u{301}'), ('ί', 'ι', '\u{301}'),
    ('ό', 'ο', '\u{301}'), ('ύ', 'υ', '\u{301}'), ('ώ', 'ω', '\u{301}'),
    ('Ά', 'Α', '\u{301}'), ('Έ', 'Ε', '\u{301}'), ('Ή', 'Η', '\u{301}'), ('Ί', 'Ι', '\u{301}'),
    ('Ό', 'Ο', '\u{301}'), ('Ύ', 'Υ', '\u{301}'), ('Ώ', 'Ω', '\u{301}'),
    ('ϊ', 'ι', '\u{308}'), ('ϋ', 'υ', '\u{308}'), ('Ϊ', 'Ι', '\u{308}'), ('Ϋ', 'Υ', '\u{308}'),
];

/// Dialytika with tonos, stacked: ΐ ΰ.
const STACKED: &[(char, char)] = &[('ΐ', 'ι'), ('ΰ', 'υ')];

/// The art for a Greek letter: this module's table, a Latin lookalike, or Latin itself.
fn art_for(c: char) -> Option<Art> {
    if let Some(g) = GREEK.iter().find(|g| g.0 == c) {
        return Some(parse(g.1, g.2));
    }
    if let Some((_, latin)) = LOOKALIKES.iter().find(|(g, _)| *g == c) {
        return art_of(*latin);
    }
    art_of(c)
}

/// Every Greek glyph of the typeface.
pub fn glyphs() -> Vec<GlyphSpec> {
    let mut out = Vec::new();
    for (c, top, rows) in GREEK {
        out.push(to_glyph(*c, &parse(*top, rows)));
    }
    for (c, latin) in LOOKALIKES {
        if let Some(art) = art_of(*latin) {
            out.push(to_glyph(*c, &art));
        }
    }
    // The micro sign is a mu (it is not a Latin letter).
    out.push(to_glyph('\u{B5}', &parse(4, GREEK.iter().find(|g| g.0 == 'μ').unwrap().2)));
    for (c, base, mark) in COMPOSED {
        if let Some(b) = art_for(*base) {
            out.push(to_glyph(*c, &compose(&b, *mark)));
        }
    }
    for (c, base) in STACKED {
        if let Some(b) = art_for(*base) {
            out.push(to_glyph(*c, &compose(&compose(&b, '\u{308}'), '\u{301}')));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn rows_of_a_glyph_are_all_the_same_width() {
        for (c, _, rows) in GREEK {
            let widths: HashSet<usize> = rows.split('/').map(str::len).collect();
            assert_eq!(widths.len(), 1, "{c}: rows differ in width");
        }
    }

    #[test]
    fn the_whole_modern_greek_alphabet_with_its_accents_is_there() {
        let have: HashSet<char> = crate::pixel_font::glyphs().iter().flat_map(|g| g.chars.clone()).collect();
        let want = "ΑΒΓΔΕΖΗΘΙΚΛΜΝΞΟΠΡΣΤΥΦΧΨΩαβγδεζηθικλμνξοπρσςτυφχψωάέήίόύώΐΰΆΈΉΊΌΎΏϊϋΪΫ;·µ\u{37E}\u{387}";
        for c in want.chars() {
            assert!(have.contains(&c), "missing Greek {c} U+{:04X}", c as u32);
        }
    }

    #[test]
    fn a_greek_letter_is_not_drawn_like_the_latin_letter_it_could_be_mistaken_for_unless_it_is_that_letter() {
        // Letters that merely resemble a Latin one must have their own drawing.
        let all = crate::pixel_font::glyphs();
        let ink = |c: char| {
            let g = all.iter().find(|g| g.chars == [c]).unwrap();
            let mut r: Vec<_> = g.layers.iter().flat_map(|l| l.rects.iter().map(|r| (r.x0, r.y0, r.x1, r.y1))).collect();
            r.sort();
            r
        };
        for (g, l) in [('α', 'a'), ('β', 'B'), ('γ', 'y'), ('ε', 'e'), ('η', 'n'), ('κ', 'k'), ('σ', 'o'), ('υ', 'u'), ('ω', 'w'), ('Π', 'N'), ('Λ', 'A'), ('μ', 'u')] {
            assert_ne!(ink(g), ink(l), "{g} is drawn exactly like {l}");
        }
        // And the distinct capitals differ from each other.
        let caps: Vec<_> = "ΓΔΘΛΞΠΣΦΨΩ".chars().map(ink).collect();
        for i in 0..caps.len() {
            for j in i + 1..caps.len() {
                assert_ne!(caps[i], caps[j]);
            }
        }
    }

    #[test]
    fn tonos_sits_above_the_letter() {
        let top = |c: char| glyphs().into_iter().find(|g| g.chars == [c]).unwrap().layers.iter().flat_map(|l| l.rects.iter()).map(|r| r.y0).min().unwrap();
        assert!(top('ά') < top('α'));
        assert!(top('ΐ') < top('ϊ'));
        assert!(top('Ά') >= 0 && top('ΐ') >= 0);
    }
}
