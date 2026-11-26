//! Hindi (Devanagari): consonants, independent vowels, vowel signs, marks and the danda.
//!
//! Every consonant is 6 columns wide with the *shirorekha* (the headline) on cell row 2 running the
//! full width, so a word's letters hang from one continuous line. Letters are strokes in a small box
//! under the headline (the same stroke rasteriser idea as Hangul).
//!
//! Vowel signs: ा ि ी ो ौ ः are spacing glyphs (the text engine itself puts ि *before* its
//! consonant). The marks that sit above or below the consonant (ु ू ृ े ै ं ँ ़ ्) have zero advance
//! and are drawn at negative x, over the six columns of the consonant just before them, so they land
//! in the right place without any positioning table.
//!
//! Not done: conjuncts. क्ष, त्र, ज्ञ and other consonant clusters need a `GSUB` table full of
//! ligatures; without it the halant (्) is shown under the first consonant and the two letters stay
//! separate, which is how a reader sees it written out in full. Devanagari digits are drawn like 0-9.

use crate::font_gen::{rects_from_mask, GlyphSpec, Layer};
use crate::pixel_font::art_of;

const ROWS: usize = 12;
/// Width of a consonant, headline included.
const WIDTH: usize = 6;
const HEADLINE_ROW: usize = 2;

type Stroke = &'static [(f32, f32)];
type Pixels = Vec<Vec<bool>>;

const ST: Stroke = &[(1., 0.), (1., 1.)];

/// Consonants: the strokes under the headline, in a box 5 columns wide and 7 rows tall.
const CONSONANTS: &[(char, &[Stroke])] = &[
    ('क', &[ST, &[(1., 0.35), (0.45, 0.25), (0., 0.5), (0.2, 0.85), (0.7, 0.95), (1., 0.7)]]),
    ('ख', &[ST, &[(0., 0.2), (0.55, 0.2), (0.55, 0.8), (1., 0.8)], &[(0., 0.55), (0.55, 0.55)]]),
    ('ग', &[ST, &[(1., 0.45), (0.1, 0.45), (0.1, 0.9), (0.6, 0.9)]]),
    ('घ', &[ST, &[(0., 0.), (0., 0.9), (0.45, 0.9), (0.45, 0.45), (1., 0.45)]]),
    ('ङ', &[&[(0.7, 0.), (0.7, 0.45), (0.1, 0.45), (0.1, 0.95), (0.7, 0.95)]]),
    ('च', &[ST, &[(0., 0.3), (0.5, 0.3), (0.5, 0.6), (0., 0.95), (0.6, 0.95)]]),
    ('छ', &[&[(0., 0.05), (0., 0.9), (0.6, 0.9)], &[(0.4, 0.45), (0.9, 0.45), (0.9, 0.9)]]),
    ('ज', &[ST, &[(0., 0.3), (0.5, 0.3), (0.5, 0.65), (0., 0.95)]]),
    ('झ', &[ST, &[(0., 0.2), (0.45, 0.2), (0.45, 0.9), (0., 0.9)], &[(0., 0.55), (0.45, 0.55)]]),
    ('ञ', &[&[(0.1, 0.), (0.1, 0.95)], &[(0.1, 0.5), (0.8, 0.5), (0.8, 0.95)]]),
    ('ट', &[&[(0.1, 0.1), (0.1, 0.5), (0.7, 0.6), (0.1, 0.9), (0.9, 0.95)]]),
    ('ठ', &[&[(0.2, 0.2), (0.7, 0.2), (0.95, 0.5), (0.7, 0.95), (0.2, 0.95), (0., 0.55), (0.2, 0.2)]]),
    ('ड', &[&[(0.1, 0.), (0.1, 0.5), (0.8, 0.5), (0.8, 0.9), (0.1, 0.9)]]),
    ('ढ', &[&[(0.1, 0.), (0.1, 0.9), (0.7, 0.9), (0.7, 0.4), (0.1, 0.4)], &[(0.9, 0.), (0.9, 0.45)]]),
    ('ण', &[ST, &[(0., 0.1), (0.55, 0.1), (0., 0.9)]]),
    ('त', &[ST, &[(0.1, 0.35), (0.55, 0.35), (0.55, 0.95), (0.1, 0.95)]]),
    ('थ', &[ST, &[(0.1, 0.2), (0.1, 0.9), (0.55, 0.95), (0.55, 0.45)]]),
    ('द', &[&[(0., 0.15), (0., 0.9), (0.7, 0.9), (0.7, 0.45), (0.2, 0.45)]]),
    ('ध', &[ST, &[(0., 0.), (0., 0.9), (0.5, 0.9), (0.5, 0.4), (1., 0.4)]]),
    ('न', &[ST, &[(0., 0.), (0., 0.6), (0.4, 0.95), (1., 0.9)]]),
    ('प', &[ST, &[(0., 0.4), (0., 0.95), (0.5, 0.95), (0.5, 0.4)]]),
    ('फ', &[ST, &[(0., 0.25), (0.55, 0.25), (0.55, 0.9), (0., 0.9)]]),
    ('ब', &[ST, &[(0., 0.15), (0.5, 0.15), (0.5, 0.5), (0., 0.5), (0., 0.95), (0.5, 0.95)]]),
    ('भ', &[ST, &[(0., 0.2), (0., 0.95), (0.5, 0.95), (0.5, 0.5)]]),
    ('म', &[ST, &[(0., 0.15), (0., 0.95), (0.6, 0.95)], &[(0.6, 0.5), (1., 0.5)]]),
    ('य', &[ST, &[(0., 0.3), (0., 0.9), (0.55, 0.9)], &[(0., 0.55), (0.55, 0.55)]]),
    ('र', &[&[(0.5, 0.), (0.5, 0.55), (0.05, 0.95)]]),
    ('ल', &[ST, &[(0., 0.2), (0.5, 0.2), (0.5, 0.9), (0., 0.9)]]),
    ('ळ', &[ST, &[(0., 0.2), (0.5, 0.2), (0.5, 0.9), (0., 0.9)], &[(0.25, 0.4), (0.25, 0.7)]]),
    ('व', &[ST, &[(0.1, 0.3), (0.5, 0.7), (0.1, 0.95)]]),
    ('श', &[ST, &[(0., 0.3), (0.5, 0.3), (0.5, 0.7), (0., 0.95)], &[(0.5, 0.3), (1., 0.3)]]),
    ('ष', &[ST, &[(0., 0.3), (0.5, 0.3), (0.5, 0.95)], &[(0., 0.3), (0., 0.65)]]),
    ('स', &[ST, &[(0., 0.3), (0.5, 0.3), (0., 0.95)]]),
    ('ह', &[&[(0., 0.25), (0.7, 0.25), (0.7, 0.95)], &[(0.2, 0.25), (0.2, 0.7), (0.7, 0.7)]]),
];

/// Independent vowels: strokes in a box 6 columns wide (the headline included).
const VOWELS: &[(char, &[Stroke])] = &[
    ('अ', &[&[(0.8, 0.), (0.8, 1.)], &[(0., 0.3), (0.5, 0.3), (0.5, 0.95), (0., 0.95)], &[(0., 0.6), (0.5, 0.6)]]),
    ('आ', &[&[(0.8, 0.), (0.8, 1.)], &[(1., 0.), (1., 1.)], &[(0., 0.3), (0.5, 0.3), (0.5, 0.95), (0., 0.95)], &[(0., 0.6), (0.5, 0.6)]]),
    ('इ', &[&[(0.1, 0.1), (0.1, 0.8), (0.4, 0.95), (0.7, 0.8)], &[(0.8, 0.), (0.8, 0.95)]]),
    ('ई', &[&[(0.1, 0.1), (0.1, 0.8), (0.4, 0.95), (0.7, 0.8)], &[(0.8, 0.), (0.8, 0.95)], &[(0.4, -0.15), (0.8, -0.15)]]),
    ('उ', &[&[(0.8, 0.), (0.8, 0.9)], &[(0.1, 0.3), (0.1, 0.9), (0.5, 0.9)], &[(0.1, 0.6), (0.5, 0.6)]]),
    ('ऊ', &[&[(0.8, 0.), (0.8, 0.9)], &[(0.1, 0.3), (0.1, 0.9), (0.5, 0.9)], &[(0.1, 0.6), (0.5, 0.6)], &[(1., 0.9), (1., 1.)]]),
    ('ऋ', &[&[(0.1, 0.2), (0.1, 0.95)], &[(0.1, 0.5), (0.6, 0.5), (0.6, 0.95)], &[(0.85, 0.), (0.85, 0.95)]]),
    ('ए', &[&[(0.8, 0.), (0.8, 1.)], &[(0.1, 0.35), (0.55, 0.35), (0.1, 0.95)]]),
    ('ऐ', &[&[(0.8, 0.), (0.8, 1.)], &[(0.1, 0.35), (0.55, 0.35), (0.1, 0.95)], &[(0.3, -0.2), (0.6, -0.2)]]),
    ('ओ', &[&[(0.8, 0.), (0.8, 1.)], &[(1., 0.), (1., 1.)], &[(0.1, 0.35), (0.55, 0.35), (0.1, 0.95)]]),
    ('औ', &[&[(0.8, 0.), (0.8, 1.)], &[(1., 0.), (1., 1.)], &[(0.1, 0.35), (0.55, 0.35), (0.1, 0.95)], &[(0.3, -0.2), (0.6, -0.2)]]),
];

fn plot(g: &mut Pixels, x: i32, y: i32) {
    if (0..g.len() as i32).contains(&y) && (0..g[0].len() as i32).contains(&x) {
        g[y as usize][x as usize] = true;
    }
}

fn line(g: &mut Pixels, (mut x0, mut y0): (i32, i32), (x1, y1): (i32, i32)) {
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

/// Draws strokes (unit-square polylines) into the box `(x, y, w, h)`.
fn draw(g: &mut Pixels, strokes: &[Stroke], (ax, ay, aw, ah): (i32, i32, i32, i32)) {
    let map = |(nx, ny): (f32, f32)| (ax + (nx * (aw - 1) as f32).round() as i32, ay + (ny * (ah - 1) as f32).round() as i32);
    for s in strokes {
        for w in s.windows(2) {
            line(g, map(w[0]), map(w[1]));
        }
    }
}

fn blank(width: usize) -> Pixels {
    vec![vec![false; width]; ROWS]
}

fn headline(g: &mut Pixels, from: usize, to: usize) {
    for x in from..to {
        g[HEADLINE_ROW][x] = true;
    }
}

fn glyph(c: char, g: &Pixels, advance: usize, origin: i32) -> GlyphSpec {
    // `origin` is the column of the pixel grid that sits at the glyph's x = 0 (marks sit left of it).
    let rects = rects_from_mask(g[0].len(), g, 0)
        .into_iter()
        .map(|mut r| {
            r.x0 -= origin;
            r.x1 -= origin;
            r
        })
        .collect();
    GlyphSpec { chars: vec![c], advance: advance as i32, colour: false, layers: vec![Layer { color: 0, rects }] }
}

fn consonant(c: char, strokes: &[Stroke]) -> GlyphSpec {
    let mut g = blank(WIDTH);
    headline(&mut g, 0, WIDTH);
    draw(&mut g, strokes, (0, 3, 5, 7));
    glyph(c, &g, WIDTH, 0)
}

fn vowel(c: char, strokes: &[Stroke]) -> GlyphSpec {
    let mut g = blank(WIDTH);
    headline(&mut g, 0, WIDTH);
    draw(&mut g, strokes, (0, 3, 6, 7));
    glyph(c, &g, WIDTH, 0)
}

/// A spacing vowel sign from rows of art (`top`, rows), its headline joined to the word's.
fn spacing_sign(c: char, top: i32, rows: &str, extra: &[(i32, i32)]) -> GlyphSpec {
    let art: Vec<&str> = rows.split('/').collect();
    let w = art.iter().map(|r| r.len()).max().unwrap_or(1);
    // Extra pixels (an above-mark belonging to the sign) are at negative x; make room on the left.
    let left = extra.iter().map(|(x, _)| (-x).max(0)).max().unwrap_or(0) as usize;
    let mut g = blank(left + w);
    for (i, r) in art.iter().enumerate() {
        for (x, ch) in r.chars().enumerate() {
            if ch == '#' {
                plot(&mut g, (left + x) as i32, top + i as i32);
            }
        }
    }
    for (x, y) in extra {
        plot(&mut g, left as i32 + x, *y);
    }
    glyph(c, &g, w, left as i32)
}

/// A zero-advance mark made of pixels at the given (x, y), x counted from the end of the consonant
/// it follows (so `-6..0` is over that consonant's six columns).
fn mark(c: char, pixels: &[(i32, i32)]) -> GlyphSpec {
    let left = pixels.iter().map(|(x, _)| -x).max().unwrap_or(0).max(0) as usize;
    let mut g = blank(left + 1);
    for (x, y) in pixels {
        plot(&mut g, left as i32 + x, *y);
    }
    glyph(c, &g, 0, left as i32)
}


const VIRAMA: char = '\u{94D}';
/// Private-use code points naming the conjunct glyphs: the half forms, then reph and the below-base ra.
const HALF_BASE: u32 = 0xE900;
const REPH: char = '\u{E9F0}';
const RAKAR: char = '\u{E9F1}';

fn has_stem(strokes: &[Stroke]) -> bool {
    strokes.iter().any(|s| *s == ST)
}

/// The consonants that have a half form: the ones with a stem, which is what the half form drops.
fn stemmed() -> impl Iterator<Item = (usize, char, &'static [Stroke])> {
    CONSONANTS.iter().filter(|(_, s)| has_stem(s)).enumerate().map(|(i, (c, s))| (i, *c, *s))
}

fn half_char(index: usize) -> char {
    char::from_u32(HALF_BASE + index as u32).unwrap()
}

/// A consonant's half form: its strokes without the stem, in a narrower letter with the headline
/// running on, so it hangs from the word's line and the next consonant starts where the stem was.
fn half_glyph(index: usize, strokes: &[Stroke]) -> GlyphSpec {
    let mut g = blank(5);
    headline(&mut g, 0, 5);
    let rest: Vec<Stroke> = strokes.iter().copied().filter(|s| *s != ST).collect();
    draw(&mut g, &rest, (0, 3, 4, 7));
    glyph(half_char(index), &g, 5, 0)
}

/// `half`, `rphf` and `blwf`: a consonant + halant is its half form, ra + halant at the start of a
/// syllable is a reph over the base, and halant + ra after a consonant is the ra stroke below it.
/// Written for both script tags the shaper may ask for.
pub fn ligatures() -> Vec<crate::font_gen::Ligature> {
    let mut out = Vec::new();
    for script in [*b"dev2", *b"deva"] {
        let lig = |feature: &[u8; 4], components: Vec<char>, to: char| crate::font_gen::Ligature { script, feature: *feature, components, to };
        for (i, c, _) in stemmed() {
            out.push(lig(b"half", vec![c, VIRAMA], half_char(i)));
        }
        out.push(lig(b"rphf", vec!['र', VIRAMA], REPH));
        out.push(lig(b"blwf", vec![VIRAMA, 'र'], RAKAR));
    }
    out
}

/// Every Devanagari glyph.
pub fn glyphs() -> Vec<GlyphSpec> {
    let mut out = Vec::new();
    for (c, s) in CONSONANTS {
        out.push(consonant(*c, s));
    }
    for (c, s) in VOWELS {
        out.push(vowel(*c, s));
    }
    for (i, _, strokes) in stemmed() {
        out.push(half_glyph(i, strokes));
    }
    out.push(mark(REPH, &[(-6, 1), (-5, 0), (-4, 0), (-3, 1)]));
    out.push(mark(RAKAR, &[(-3, 7), (-4, 8), (-5, 9), (-5, 10)]));
    // Spacing signs (headline joined).
    out.push(spacing_sign('ा', 2, "##/.#/.#/.#/.#/.#/.#/.#", &[]));
    out.push(spacing_sign('ि', 2, "###/#../#../#../#../#../#../#..", &[]));
    out.push(spacing_sign('ी', 1, "#./##/.#/.#/.#/.#/.#/.#/.#", &[]));
    out.push(spacing_sign('ो', 2, "##/.#/.#/.#/.#/.#/.#/.#", &[(-3, 0), (-4, 1)]));
    out.push(spacing_sign('ौ', 2, "##/.#/.#/.#/.#/.#/.#/.#", &[(-3, 0), (-4, 1), (-2, 0), (-3, 1)]));
    out.push(spacing_sign('ः', 4, "#/././#/././#", &[]));
    // Marks: zero advance, over/under the consonant before.
    out.push(mark('ु', &[(-4, 10), (-3, 10), (-3, 11)]));
    out.push(mark('ू', &[(-4, 10), (-3, 10), (-4, 11), (-3, 11)]));
    out.push(mark('ृ', &[(-5, 10), (-3, 10), (-4, 11)]));
    out.push(mark('े', &[(-3, 0), (-4, 1)]));
    out.push(mark('ै', &[(-3, 0), (-4, 1), (-2, 0), (-3, 1)]));
    out.push(mark('ं', &[(-3, 0)]));
    out.push(mark('ँ', &[(-3, 0), (-4, 1), (-2, 1)]));
    out.push(mark('़', &[(-3, 10)]));
    out.push(mark('्', &[(-3, 10), (-4, 11)]));
    // Danda and double danda.
    out.push(spacing_sign('।', 2, "#/#/#/#/#/#/#/#", &[]));
    out.push(spacing_sign('॥', 2, "#.#/#.#/#.#/#.#/#.#/#.#/#.#/#.#", &[]));
    // Digits ०-९ are the Latin digits.
    for (i, c) in ('०'..='९').enumerate() {
        if let Some((top, w, rows)) = art_of(char::from_digit(i as u32, 10).unwrap()) {
            let mut g = blank(w + 1);
            for (r, row) in rows.iter().enumerate() {
                for (x, &on) in row.iter().enumerate() {
                    if on {
                        plot(&mut g, x as i32, top + r as i32);
                    }
                }
            }
            out.push(glyph(c, &g, w + 1, 0));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn font() -> Vec<u8> {
        crate::fonts::default_font_bytes()
    }

    fn run(font: &[u8], text: &str) -> Vec<(u32, i32, i32)> {
        let face = rustybuzz::Face::from_slice(font, 0).expect("the font parses");
        let mut buf = rustybuzz::UnicodeBuffer::new();
        buf.push_str(text);
        buf.set_script(rustybuzz::script::DEVANAGARI);
        buf.set_direction(rustybuzz::Direction::LeftToRight);
        let out = rustybuzz::shape(&face, &[], buf);
        out.glyph_infos().iter().zip(out.glyph_positions()).map(|(i, p)| (i.glyph_id, p.x_advance, p.x_offset)).collect()
    }

    fn gid(font: &[u8], c: char) -> u32 {
        ttf_parser::Face::parse(font, 0).unwrap().glyph_index(c).unwrap_or_else(|| panic!("no glyph for U+{:04X}", c as u32)).0 as u32
    }

    #[test]
    fn the_letters_are_all_there() {
        let have: HashSet<char> = glyphs().iter().flat_map(|g| g.chars.clone()).collect();
        for c in ('\u{915}'..='\u{939}').chain('\u{905}'..='\u{914}').filter(|c| ![0x90C, 0x90D, 0x90E, 0x911, 0x912, 0x929, 0x931, 0x934].contains(&(*c as u32))) {
            assert!(have.contains(&c), "missing U+{:04X}", c as u32);
        }
        for c in ['ा', 'ि', 'ी', 'ु', 'ू', 'ृ', 'े', 'ै', 'ो', 'ौ', 'ं', 'ँ', 'ः', '़', '्', '।'] {
            assert!(have.contains(&c), "missing U+{:04X}", c as u32);
        }
    }

    #[test]
    fn every_glyph_has_ink_inside_the_cell_and_consonants_carry_the_headline() {
        for g in glyphs() {
            assert!(!g.layers[0].rects.is_empty(), "{:?} is blank", g.chars);
            for r in &g.layers[0].rects {
                assert!(r.y0 >= 0 && r.y1 <= 12, "{:?} leaves the cell", g.chars);
            }
        }
        for (c, s) in CONSONANTS {
            let g = consonant(*c, s);
            let headline: i32 = g.layers[0].rects.iter().filter(|r| r.y0 <= 2 && r.y1 > 2).map(|r| r.x1 - r.x0).max().unwrap();
            assert_eq!(headline, WIDTH as i32, "{c}: the headline doesn't span the whole letter");
        }
    }

    #[test]
    fn consonants_are_distinct_shapes() {
        let mut seen = HashSet::new();
        for (c, s) in CONSONANTS {
            let key: Vec<_> = consonant(*c, s).layers[0].rects.iter().map(|r| (r.x0, r.y0, r.x1, r.y1)).collect();
            assert!(seen.insert(key), "{c} looks like another consonant");
        }
    }

    #[test]
    fn the_i_sign_is_put_before_its_consonant() {
        // कि is typed ka + i-sign but reads i-sign then ka.
        let f = font();
        let glyphs = run(&f, "कि");
        assert_eq!(glyphs.iter().map(|g| g.0).collect::<Vec<_>>(), vec![gid(&f, 'ि'), gid(&f, 'क')]);
    }

    #[test]
    fn a_mark_takes_no_room_and_stays_where_it_was_drawn() {
        let f = font();
        for text in ["कु", "कं", "के", "क्", "क़"] {
            let g = run(&f, text);
            assert_eq!(g.len(), 2, "{text}");
            assert_eq!(g[1].1, 0, "{text}: the mark should have no advance");
            assert_eq!(g[1].2, 0, "{text}: the shaper moved the mark ({}); the art assumes it doesn't", g[1].2);
        }
    }

    #[test]
    fn a_stemmed_consonant_before_a_halant_takes_its_half_form() {
        let f = font();
        // क्ष: half ka then ssa, not ka, halant, ssa.
        let g = run(&f, "क्ष");
        assert_eq!(g.len(), 2, "{g:?}");
        assert_eq!(g[1].0, gid(&f, 'ष'));
        assert_ne!(g[0].0, gid(&f, 'क'));
        // ज्ञ the same.
        assert_eq!(run(&f, "ज्ञ").len(), 2);
        // A consonant with no stem (ट) keeps its halant.
        let g = run(&f, "ट्ट");
        assert_eq!(g.len(), 3, "{g:?}");
    }

    #[test]
    fn ra_with_a_halant_is_a_reph_before_a_consonant_and_a_stroke_below_after_one() {
        let f = font();
        // र्क: the reph goes over ka (it is moved after it).
        let g = run(&f, "र्क");
        assert_eq!(g.len(), 2, "{g:?}");
        assert_eq!(g.iter().filter(|x| x.1 == 0).count(), 1, "the reph takes no room");
        // क्र: ka with ra below it.
        let g = run(&f, "क्र");
        assert_eq!(g.len(), 2, "{g:?}");
        assert_eq!(g[0].0, gid(&f, 'क'));
        assert_eq!(g[1].1, 0);
    }
}
