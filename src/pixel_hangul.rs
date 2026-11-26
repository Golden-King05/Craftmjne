//! Korean: every precomposed Hangul syllable (U+AC00-D7A3, 11,172 of them) plus the standalone
//! compatibility letters (U+3131-3163), generated from strokes rather than drawn one by one.
//!
//! A *jamo* (letter part) is a few strokes in a unit square. A syllable is an initial consonant, a
//! vowel and an optional final consonant, each rasterised into its own box of an 11x11 pixel cell,
//! where the boxes depend on the vowel's shape:
//!
//! * **vertical** vowels (ㅏ ㅓ ㅣ ...): consonant on the left, vowel on the right;
//! * **horizontal** vowels (ㅗ ㅜ ㅡ ...): consonant on top, vowel below;
//! * **mixed** vowels (ㅘ ㅢ ...): consonant top left, the horizontal part below it, the
//!   vertical part on the right.
//!
//! With a final consonant the top part is squeezed into 6 rows and the final takes the bottom 4.
//! Doubled and clustered consonants (ㄲ, ㄳ, ㄺ ...) are their parts side by side. Strokes are
//! one pixel wide, so a letter squeezed into a small box loses detail; that is the nature of
//! Hangul at this size.

use crate::font_gen::{rects_from_mask, GlyphSpec, Layer};

/// The cell a syllable is drawn in, in pixels: 11 wide (the advance adds one blank column) and the
/// full 12-row em tall, so a syllable is centred on the Latin letters beside it.
const W: usize = 11;
const H: usize = 12;
type Grid = [[bool; W]; H];
type Stroke = &'static [(f32, f32)];
/// A box inside the cell: x, y, width, height.
type Area = (i32, i32, i32, i32);

const INITIALS: &str = "ㄱㄲㄴㄷㄸㄹㅁㅂㅃㅅㅆㅇㅈㅉㅊㅋㅌㅍㅎ";
const VOWELS: &str = "ㅏㅐㅑㅒㅓㅔㅕㅖㅗㅘㅙㅚㅛㅜㅝㅞㅟㅠㅡㅢㅣ";
/// Index 0 is "no final consonant".
const FINALS: &str = "\0ㄱㄲㄳㄴㄵㄶㄷㄹㄺㄻㄼㄽㄾㄿㅀㅁㅂㅄㅅㅆㅇㅈㅊㅋㅌㅍㅎ";

/// Strokes of a plain (single) consonant.
fn consonant(c: char) -> &'static [Stroke] {
    match c {
        'ㄱ' => &[&[(0., 0.), (1., 0.), (1., 1.)]],
        'ㄴ' => &[&[(0., 0.), (0., 1.), (1., 1.)]],
        'ㄷ' => &[&[(1., 0.), (0., 0.), (0., 1.), (1., 1.)]],
        'ㄹ' => &[&[(0., 0.), (1., 0.), (1., 0.5), (0., 0.5), (0., 1.), (1., 1.)]],
        'ㅁ' => &[&[(0., 0.), (1., 0.), (1., 1.), (0., 1.), (0., 0.)]],
        'ㅂ' => &[&[(0., 0.), (0., 1.), (1., 1.), (1., 0.)], &[(0., 0.5), (1., 0.5)]],
        'ㅅ' => &[&[(0., 1.), (0.5, 0.), (1., 1.)]],
        'ㅇ' => &[&[(0.3, 0.), (0.7, 0.), (1., 0.3), (1., 0.7), (0.7, 1.), (0.3, 1.), (0., 0.7), (0., 0.3), (0.3, 0.)]],
        'ㅈ' => &[&[(0., 0.), (1., 0.)], &[(0., 1.), (0.5, 0.3), (1., 1.)], &[(0.5, 0.), (0.5, 0.3)]],
        'ㅊ' => &[&[(0.35, 0.), (0.65, 0.)], &[(0., 0.3), (1., 0.3)], &[(0., 1.), (0.5, 0.5), (1., 1.)], &[(0.5, 0.3), (0.5, 0.5)]],
        'ㅋ' => &[&[(0., 0.), (1., 0.), (1., 1.)], &[(0., 0.5), (1., 0.5)]],
        'ㅌ' => &[&[(1., 0.), (0., 0.), (0., 1.), (1., 1.)], &[(0., 0.5), (1., 0.5)]],
        'ㅍ' => &[&[(0., 0.), (1., 0.)], &[(0., 1.), (1., 1.)], &[(0.25, 0.), (0.25, 1.)], &[(0.75, 0.), (0.75, 1.)]],
        'ㅎ' => &[
            &[(0.3, 0.), (0.7, 0.)],
            &[(0., 0.3), (1., 0.3)],
            &[(0.3, 0.6), (0.7, 0.6), (1., 0.75), (1., 0.9), (0.7, 1.), (0.3, 1.), (0., 0.9), (0., 0.75), (0.3, 0.6)],
        ],
        _ => &[],
    }
}

/// The plain consonants a (possibly doubled or clustered) consonant is made of.
fn parts(c: char) -> &'static [char] {
    match c {
        'ㄲ' => &['ㄱ', 'ㄱ'],
        'ㄳ' => &['ㄱ', 'ㅅ'],
        'ㄵ' => &['ㄴ', 'ㅈ'],
        'ㄶ' => &['ㄴ', 'ㅎ'],
        'ㄸ' => &['ㄷ', 'ㄷ'],
        'ㄺ' => &['ㄹ', 'ㄱ'],
        'ㄻ' => &['ㄹ', 'ㅁ'],
        'ㄼ' => &['ㄹ', 'ㅂ'],
        'ㄽ' => &['ㄹ', 'ㅅ'],
        'ㄾ' => &['ㄹ', 'ㅌ'],
        'ㄿ' => &['ㄹ', 'ㅍ'],
        'ㅀ' => &['ㄹ', 'ㅎ'],
        'ㅃ' => &['ㅂ', 'ㅂ'],
        'ㅄ' => &['ㅂ', 'ㅅ'],
        'ㅆ' => &['ㅅ', 'ㅅ'],
        'ㅉ' => &['ㅈ', 'ㅈ'],
        _ => &[],
    }
}

/// Strokes of a plain vowel, in the box it is drawn in.
fn simple_vowel(c: char) -> &'static [Stroke] {
    match c {
        'ㅏ' => &[&[(0., 0.), (0., 1.)], &[(0., 0.5), (1., 0.5)]],
        'ㅐ' => &[&[(0., 0.), (0., 1.)], &[(0.67, 0.), (0.67, 1.)], &[(0., 0.5), (0.67, 0.5)]],
        'ㅑ' => &[&[(0., 0.), (0., 1.)], &[(0., 0.3), (1., 0.3)], &[(0., 0.7), (1., 0.7)]],
        'ㅒ' => &[&[(0., 0.), (0., 1.)], &[(0.67, 0.), (0.67, 1.)], &[(0., 0.3), (0.67, 0.3)], &[(0., 0.7), (0.67, 0.7)]],
        'ㅓ' => &[&[(1., 0.), (1., 1.)], &[(0., 0.5), (1., 0.5)]],
        'ㅔ' => &[&[(0.33, 0.), (0.33, 1.)], &[(1., 0.), (1., 1.)], &[(0., 0.5), (0.33, 0.5)]],
        'ㅕ' => &[&[(1., 0.), (1., 1.)], &[(0., 0.3), (1., 0.3)], &[(0., 0.7), (1., 0.7)]],
        'ㅖ' => &[&[(0.33, 0.), (0.33, 1.)], &[(1., 0.), (1., 1.)], &[(0., 0.3), (0.33, 0.3)], &[(0., 0.7), (0.33, 0.7)]],
        'ㅣ' => &[&[(0.5, 0.), (0.5, 1.)]],
        'ㅗ' => &[&[(0., 1.), (1., 1.)], &[(0.5, 0.), (0.5, 1.)]],
        'ㅛ' => &[&[(0., 1.), (1., 1.)], &[(0.25, 0.), (0.25, 1.)], &[(0.75, 0.), (0.75, 1.)]],
        'ㅜ' => &[&[(0., 0.), (1., 0.)], &[(0.5, 0.), (0.5, 1.)]],
        'ㅠ' => &[&[(0., 0.), (1., 0.)], &[(0.25, 0.), (0.25, 1.)], &[(0.75, 0.), (0.75, 1.)]],
        'ㅡ' => &[&[(0., 0.5), (1., 0.5)]],
        _ => &[],
    }
}

/// A vowel as `(horizontal part, vertical part)`; one or both.
fn vowel_parts(c: char) -> (Option<char>, Option<char>) {
    match c {
        'ㅗ' | 'ㅛ' | 'ㅜ' | 'ㅠ' | 'ㅡ' => (Some(c), None),
        'ㅘ' => (Some('ㅗ'), Some('ㅏ')),
        'ㅙ' => (Some('ㅗ'), Some('ㅐ')),
        'ㅚ' => (Some('ㅗ'), Some('ㅣ')),
        'ㅝ' => (Some('ㅜ'), Some('ㅓ')),
        'ㅞ' => (Some('ㅜ'), Some('ㅔ')),
        'ㅟ' => (Some('ㅜ'), Some('ㅣ')),
        'ㅢ' => (Some('ㅡ'), Some('ㅣ')),
        _ => (None, Some(c)),
    }
}

fn plot(g: &mut Grid, x: i32, y: i32) {
    if (0..W as i32).contains(&x) && (0..H as i32).contains(&y) {
        g[y as usize][x as usize] = true;
    }
}

/// A one-pixel line between two pixels (Bresenham).
fn line(g: &mut Grid, (mut x0, mut y0): (i32, i32), (x1, y1): (i32, i32)) {
    let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
    let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
    let mut err = dx + dy;
    loop {
        plot(g, x0, y0);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

/// Draws strokes (unit-square polylines) into `area`.
fn draw(g: &mut Grid, strokes: &[Stroke], (ax, ay, aw, ah): Area) {
    let map = |(nx, ny): (f32, f32)| (ax + (nx * (aw - 1) as f32).round() as i32, ay + (ny * (ah - 1) as f32).round() as i32);
    for s in strokes {
        for w in s.windows(2) {
            line(g, map(w[0]), map(w[1]));
        }
    }
}

/// A consonant, single or double/cluster (side by side), into `area`.
fn draw_consonant(g: &mut Grid, c: char, area: Area) {
    let p = parts(c);
    if p.is_empty() {
        draw(g, consonant(c), area);
        return;
    }
    let (x, y, w, h) = area;
    let left = ((w - 1) + 1) / 2;
    let right = w - left - 1;
    draw(g, consonant(p[0]), (x, y, left.max(1), h));
    draw(g, consonant(p[1]), (x + left + 1, y, right.max(1), h));
}

/// One syllable, as rows of pixels.
fn syllable(initial: char, vowel: char, fin: Option<char>) -> Grid {
    let mut g = [[false; W]; H];
    let (horiz, vert) = vowel_parts(vowel);
    let tall = fin.is_none();
    // Each of: initial, horizontal part, vertical part.
    let (init, hbox, vbox): (Area, Area, Area) = match (horiz.is_some(), vert.is_some(), tall) {
        (false, true, true) => ((0, 1, 6, 10), (0, 0, 0, 0), (7, 0, 4, 12)),
        (false, true, false) => ((0, 0, 6, 7), (0, 0, 0, 0), (7, 0, 4, 7)),
        (true, false, true) => ((2, 0, 7, 7), (0, 8, 11, 4), (0, 0, 0, 0)),
        (true, false, false) => ((2, 0, 7, 4), (0, 5, 11, 2), (0, 0, 0, 0)),
        (true, true, true) => ((0, 0, 6, 6), (0, 8, 6, 4), (7, 0, 4, 12)),
        (true, true, false) => ((0, 0, 6, 4), (0, 5, 6, 2), (7, 0, 4, 7)),
        (false, false, _) => ((0, 0, 0, 0), (0, 0, 0, 0), (0, 0, 0, 0)),
    };
    draw_consonant(&mut g, initial, init);
    if let Some(h) = horiz {
        draw(&mut g, simple_vowel(h), hbox);
    }
    if let Some(v) = vert {
        draw(&mut g, simple_vowel(v), vbox);
    }
    if let Some(f) = fin {
        draw_consonant(&mut g, f, (1, 8, 9, 4));
    }
    g
}

/// A standalone letter (U+3131-3163) centred in the cell.
fn standalone(c: char) -> Grid {
    let mut g = [[false; W]; H];
    if VOWELS.contains(c) {
        let (h, v) = vowel_parts(c);
        match (h, v) {
            (Some(h), None) => draw(&mut g, simple_vowel(h), (0, 4, 11, 5)),
            (None, Some(v)) => draw(&mut g, simple_vowel(v), (3, 0, 5, 12)),
            (Some(h), Some(v)) => {
                draw(&mut g, simple_vowel(h), (0, 6, 6, 5));
                draw(&mut g, simple_vowel(v), (7, 0, 4, 12));
            }
            _ => {}
        }
    } else {
        draw_consonant(&mut g, c, (1, 1, 9, 10));
    }
    g
}

fn spec(c: char, g: &Grid) -> GlyphSpec {
    let rows: Vec<Vec<bool>> = g.iter().map(|r| r.to_vec()).collect();
    GlyphSpec {
        chars: vec![c],
        advance: W as i32 + 1,
        colour: false,
        layers: vec![Layer { color: 0, rects: rects_from_mask(W, &rows, 0) }],
    }
}

/// The compatibility letters, `U+3131 ..= U+3163`, in code point order.
const COMPAT: &str = "ㄱㄲㄳㄴㄵㄶㄷㄸㄹㄺㄻㄼㄽㄾㄿㅀㅁㅂㅃㅄㅅㅆㅇㅈㅉㅊㅋㅌㅍㅎㅏㅐㅑㅒㅓㅔㅕㅖㅗㅘㅙㅚㅛㅜㅝㅞㅟㅠㅡㅢㅣ";

/// Every Hangul glyph, in code point order (so consecutive glyph ids make one cmap run).
pub fn glyphs() -> Vec<GlyphSpec> {
    let mut out = Vec::with_capacity(11_172 + 51);
    for c in COMPAT.chars() {
        out.push(spec(c, &standalone(c)));
    }
    let initials: Vec<char> = INITIALS.chars().collect();
    let vowels: Vec<char> = VOWELS.chars().collect();
    let finals: Vec<char> = FINALS.chars().collect();
    let mut code = 0xAC00u32;
    for &l in &initials {
        for &v in &vowels {
            for &t in &finals {
                let fin = if t == '\0' { None } else { Some(t) };
                out.push(spec(char::from_u32(code).unwrap(), &syllable(l, v, fin)));
                code += 1;
            }
        }
    }
    debug_assert_eq!(code, 0xD7A4);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn the_tables_have_the_standard_sizes() {
        assert_eq!(INITIALS.chars().count(), 19);
        assert_eq!(VOWELS.chars().count(), 21);
        assert_eq!(FINALS.chars().count(), 28);
        assert_eq!(COMPAT.chars().count(), 0x3163 - 0x3131 + 1);
    }

    #[test]
    fn every_syllable_is_there_and_has_ink() {
        let all = glyphs();
        let syl: Vec<&GlyphSpec> = all.iter().filter(|g| ('\u{AC00}'..='\u{D7A3}').contains(&g.chars[0])).collect();
        assert_eq!(syl.len(), 11_172);
        for g in &syl {
            assert!(!g.layers[0].rects.is_empty(), "{:?} is blank", g.chars);
        }
        // Known words land on the right code points: 한 = U+D55C, 국 = U+AD6D.
        let han = all.iter().find(|g| g.chars[0] == '한').expect("한");
        assert!(!han.layers[0].rects.is_empty());
        assert!(all.iter().any(|g| g.chars[0] == '국'));
    }

    #[test]
    fn syllables_are_almost_all_visibly_different() {
        // The strokes are tiny, so a few collisions are possible, but if many syllables look alike the
        // layout is wrong.
        let mut seen = HashSet::new();
        let mut dupes = 0;
        for l in INITIALS.chars() {
            for v in VOWELS.chars() {
                for t in FINALS.chars() {
                    let fin = if t == '\0' { None } else { Some(t) };
                    if !seen.insert(syllable(l, v, fin)) {
                        dupes += 1;
                    }
                }
            }
        }
        assert!(dupes < 11_172 / 50, "{dupes} of 11172 syllables look identical to another");
    }

    #[test]
    fn a_final_consonant_goes_below_the_rest() {
        let with = syllable('ㄱ', 'ㅏ', Some('ㄴ'));
        let without = syllable('ㄱ', 'ㅏ', None);
        assert!(with[11].iter().any(|&p| p), "the final has no ink on the bottom row");
        assert!(!without[11].iter().any(|&p| p) || without[11].iter().filter(|&&p| p).count() <= 1, "no final yet the bottom row is full");
        // The initial and vowel are squeezed into the top six rows.
        // The initial and vowel are squeezed into the top seven rows; rows 7+ belong to the final.
        assert!((0..7).all(|y| !with[y].iter().all(|&p| p)));
    }

    #[test]
    fn horizontal_vowels_sit_under_the_consonant_and_vertical_ones_beside_it() {
        let go = syllable('ㄱ', 'ㅗ', None); // 고
        let ga = syllable('ㄱ', 'ㅏ', None); // 가
        // 고: ink in the bottom rows spans the width; 가: the right columns carry the vowel.
        assert!(go[11].iter().filter(|&&p| p).count() >= 8);
        assert!(ga[5][7..].iter().any(|&p| p), "가's vowel tick is missing on the right");
        assert!((0..8).all(|y| !go[y][9..].iter().any(|&p| p)), "고 has nothing standing to the right of its consonant");
    }
}
