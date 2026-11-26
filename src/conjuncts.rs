//! Conjuncts for the scripts whose consonants fit together: Bengali, Gujarati, Odia, Gurmukhi,
//! Kannada, Telugu, Malayalam, Khmer, Burmese, Javanese, Balinese and Tibetan.
//!
//! Unifont draws each consonant once, full size, in its own cell, and its OpenType build has no
//! `GSUB` at all (checked), so nothing could be taken from it: the joined shapes are *derived* from
//! the consonant bitmaps, the way `scripts::forms` derives cursive forms. Three kinds:
//!
//! * **Half form** (`half`, Bengali, Gujarati, Odia, Malayalam): consonant + virama is the
//!   consonant squeezed to half width (column pairs merged), so `C1 virama C2` reads as a narrow
//!   C1 touching C2.
//! * **Subjoined form** (`blwf`/`pref`, Kannada, Telugu, Khmer, Burmese, Javanese, Balinese and the
//!   four Gurmukhi letters that take one): virama + consonant is the consonant at quarter size
//!   (2x2 blocks merged), zero advance, tucked under the letter before it.
//! * **Reph** (`rphf`, Bengali, Gujarati, Odia, Kannada): ra + virama at the start of a cluster
//!   is a quarter-size ra over the cluster's last letter.
//!
//! Tibetan needs no ligatures: its subjoined letters are code points of their own. Unifont draws
//! them in the same cell as the letter they belong to, overlapping it, so they are only moved
//! down (`lower_tibetan_subjoined`) to sit under it.
//!
//! These are approximations of real conjuncts - readable stacks, not calligraphy - and they are
//! only as good as squeezing a 16-pixel glyph to 8 can be. Sinhala, Tamil and the Philippine
//! scripts are deliberately left alone: their visible virama (al-lakuna, pulli, pamudpod) is how
//! they are normally written.

use crate::cjk::{spec_of, Glyph};
use crate::font_gen::{rects_from_mask, GlyphSpec, Layer, Ligature, Rect, BASELINE_ROW};

/// Where the derived glyphs live (plane 15 private use; this font has nothing else there).
const BASE: u32 = 0xF4000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Half,
    Below,
}

struct Script {
    name: &'static str,
    /// The script tags the shaper may ask for, newest first.
    tags: &'static [[u8; 4]],
    features: &'static [[u8; 4]],
    virama: u32,
    /// Consonant ranges (inclusive).
    consonants: &'static [(u32, u32)],
    kind: Kind,
    /// Only these take the form (all consonants if empty).
    only: &'static [u32],
    /// The ra that makes a reph, if the script has one.
    reph: Option<u32>,
    /// The form needs a zero-width joiner after the virama (Sinhala writes conjuncts that way).
    zwj: bool,
    /// The script's block, to find its vowel signs in.
    block: (u32, u32),
}

const SCRIPTS: &[Script] = &[
    Script { name: "Bengali", tags: &[*b"bng2", *b"beng"], features: &[*b"half"], virama: 0x9CD, consonants: &[(0x995, 0x9B9)], kind: Kind::Half, only: &[], reph: Some(0x9B0), zwj: false, block: (0x980, 0x9FF) },
    Script { name: "Gujarati", tags: &[*b"gjr2", *b"gujr"], features: &[*b"half"], virama: 0xACD, consonants: &[(0xA95, 0xAB9)], kind: Kind::Half, only: &[], reph: Some(0xAB0), zwj: false, block: (0xA80, 0xAFF) },
    Script { name: "Odia", tags: &[*b"ory2", *b"orya"], features: &[*b"half"], virama: 0xB4D, consonants: &[(0xB15, 0xB39)], kind: Kind::Half, only: &[], reph: Some(0xB30), zwj: false, block: (0xB00, 0xB7F) },
    Script { name: "Malayalam", tags: &[*b"mlm2", *b"mlym"], features: &[*b"half"], virama: 0xD4D, consonants: &[(0xD15, 0xD3A)], kind: Kind::Half, only: &[], reph: None, zwj: false, block: (0xD00, 0xD7F) },
    Script { name: "Gurmukhi", tags: &[*b"gur2", *b"guru"], features: &[*b"blwf"], virama: 0xA4D, consonants: &[(0xA2F, 0xA39)], kind: Kind::Below, only: &[0xA2F, 0xA30, 0xA35, 0xA39], reph: None, zwj: false, block: (0xA00, 0xA7F) },
    Script { name: "Kannada", tags: &[*b"knd2", *b"knda"], features: &[*b"blwf"], virama: 0xCCD, consonants: &[(0xC95, 0xCB9)], kind: Kind::Below, only: &[], reph: Some(0xCB0), zwj: false, block: (0xC80, 0xCFF) },
    Script { name: "Telugu", tags: &[*b"tel2", *b"telu"], features: &[*b"blwf"], virama: 0xC4D, consonants: &[(0xC15, 0xC39)], kind: Kind::Below, only: &[], reph: None, zwj: false, block: (0xC00, 0xC7F) },
    Script { name: "Khmer", tags: &[*b"khmr"], features: &[*b"pref", *b"blwf", *b"pstf"], virama: 0x17D2, consonants: &[(0x1780, 0x17A2)], kind: Kind::Below, only: &[], reph: None, zwj: false, block: (0x1780, 0x17FF) },
    Script { name: "Burmese", tags: &[*b"mym2", *b"mymr"], features: &[*b"blwf", *b"pstf"], virama: 0x1039, consonants: &[(0x1000, 0x1021)], kind: Kind::Below, only: &[], reph: None, zwj: false, block: (0x1000, 0x109F) },
    Script { name: "Javanese", tags: &[*b"java"], features: &[*b"blwf", *b"pstf"], virama: 0xA9C0, consonants: &[(0xA98F, 0xA9B2)], kind: Kind::Below, only: &[], reph: None, zwj: false, block: (0xA980, 0xA9DF) },
    Script { name: "Balinese", tags: &[*b"bali"], features: &[*b"blwf", *b"pstf"], virama: 0x1B44, consonants: &[(0x1B13, 0x1B33)], kind: Kind::Below, only: &[], reph: None, zwj: false, block: (0x1B00, 0x1B7F) },
    // Sinhala writes conjuncts with a zero-width joiner after the al-lakuna: yansaya, rakaransaya and the
    // traditional stacked forms. Without the joiner the al-lakuna stays visible, as it normally is.
    Script { name: "Sinhala", tags: &[*b"sinh"], features: &[*b"vatu", *b"blwf", *b"pstf", *b"cjct", *b"akhn"], virama: 0xDCA, consonants: &[(0xD9A, 0xDC6)], kind: Kind::Below, only: &[], reph: None, zwj: true, block: (0xD80, 0xDFF) },
    Script { name: "Sundanese", tags: &[*b"sund"], features: &[*b"blwf", *b"pstf"], virama: 0x1BAB, consonants: &[(0x1B8A, 0x1BA0)], kind: Kind::Below, only: &[], reph: None, zwj: false, block: (0x1B80, 0x1BBF) },
];

/// Rows of the em the subjoined forms start at (just under the letter before: its ink ends near row 9).
const BELOW_TOP: i32 = 10;
/// Em row of a reph's top (a quarter-size ra of eight rows ends just over the headline).
const REPH_TOP: i32 = -8;
/// How many rows Tibetan's subjoined letters are lowered to clear the letter above.
const TIBETAN_DROP: i32 = 3;

fn rows_of(g: &Glyph) -> Vec<Vec<bool>> {
    (0..16).map(|y| (0..g.width).map(|x| g.ink(x, y)).collect()).collect()
}

/// Drops blank columns on both sides, keeping every row.
fn trim_columns(rows: Vec<Vec<bool>>) -> Vec<Vec<bool>> {
    let w = rows.first().map_or(0, Vec::len);
    let first = (0..w).find(|&x| rows.iter().any(|r| r[x]));
    let last = (0..w).rev().find(|&x| rows.iter().any(|r| r[x]));
    match (first, last) {
        (Some(a), Some(b)) => rows.iter().map(|r| r[a..=b].to_vec()).collect(),
        _ => rows,
    }
}

/// `g`'s ink box squeezed to half width: each pair of columns becomes one that is inked if either
/// was. Letters that are narrow inside their cell are squeezed from their ink, not from the cell.
fn half_mask(g: &Glyph) -> Vec<Vec<bool>> {
    trim_columns(rows_of(g)).iter().map(|r| r.chunks(2).map(|p| p.iter().any(|&b| b)).collect()).collect()
}

/// `g` at quarter size (2x2 blocks merged), trimmed of blank rows and columns so it can be placed by its ink.
fn quarter_mask(g: &Glyph) -> Vec<Vec<bool>> {
    let src = rows_of(g);
    let out: Vec<Vec<bool>> = src
        .chunks(2)
        .map(|pair| (0..(g.width + 1) / 2).map(|x| pair.iter().any(|r| r[2 * x] || r.get(2 * x + 1).copied().unwrap_or(false))).collect())
        .collect();
    let first = out.iter().position(|r| r.iter().any(|&b| b));
    let last = out.iter().rposition(|r| r.iter().any(|&b| b));
    match (first, last) {
        (Some(a), Some(b)) => trim_columns(out[a..=b].to_vec()),
        _ => vec![],
    }
}

fn spec(c: char, advance: i32, mask: &[Vec<bool>], x_shift: i32, top: i32) -> GlyphSpec {
    let width = mask.first().map_or(0, Vec::len);
    let mut rects = rects_from_mask(width, mask, top);
    for r in &mut rects {
        r.x0 += x_shift;
        r.x1 += x_shift;
    }
    GlyphSpec { chars: vec![c], advance, layers: vec![Layer { color: 0, rects }], colour: false }
}

fn wanted(s: &Script, cp: u32) -> bool {
    s.consonants.iter().any(|&(a, b)| (a..=b).contains(&cp)) && (s.only.is_empty() || s.only.contains(&cp))
}

/// The derived glyphs and the ligatures that choose them: `(glyphs, ligatures)`.
pub fn derived(set: &std::collections::BTreeMap<u32, Glyph>) -> (Vec<GlyphSpec>, Vec<Ligature>) {
    let (mut specs, mut ligs) = (Vec::new(), Vec::new());
    let mut next = BASE;
    let mut alloc = || {
        let c = char::from_u32(next).unwrap();
        next += 1;
        c
    };
    let mut subs: Vec<(usize, char, GlyphSpec)> = Vec::new();
    for (si, s) in SCRIPTS.iter().enumerate() {
        let Some(virama) = char::from_u32(s.virama) else { continue };
        for (&cp, g) in set {
            if !wanted(s, cp) || g.combining || g.width != 16 {
                continue;
            }
            let Some(c) = char::from_u32(cp) else { continue };
            let to = alloc();
            match s.kind {
                Kind::Half => {
                    let m = half_mask(g);
                    let w = m.first().map_or(0, Vec::len) as i32;
                    // A column of air after it, so it does not touch the letter it leads into.
                    specs.push(spec(to, w + 1, &m, 0, BASELINE_ROW - 14));
                }
                Kind::Below => {
                    let m = quarter_mask(g);
                    // Zero advance, drawn centred under the 16-wide letter before it.
                    let w = m.first().map_or(0, Vec::len) as i32;
                    let below = spec(to, 0, &m, -16 + (16 - w) / 2, BELOW_TOP);
                    subs.push((si, to, below.clone()));
                    specs.push(below);
                }
            }
            for &script in s.tags {
                for &feature in s.features {
                    let components = components_of(s, c, virama);
                    ligs.push(Ligature { script, feature, components, to });
                }
            }
            if s.reph == Some(cp) {
                let reph = alloc();
                let m = quarter_mask(g);
                let w = m.first().map_or(0, Vec::len) as i32;
                specs.push(spec(reph, 0, &m, -16 + (16 - w) / 2 + 2, REPH_TOP));
                for &script in s.tags {
                    ligs.push(Ligature { script, feature: *b"rphf", components: vec![c, virama], to: reph });
                }
            }
        }
    }
    lowered_vowels(set, &subs, &mut specs, &mut ligs, &mut alloc);
    tibetan_stacks(set, &mut specs, &mut ligs, &mut alloc);
    chillus(set, &mut ligs);
    tamil(set, &mut specs, &mut ligs, &mut alloc);
    ya_phala(set, &mut specs, &mut ligs, &mut alloc);
    // Sinhala and Malayalam write some forms with a joiner after the virama; the font needs a glyph for
    // it or the shaper cannot match the sequence (and would draw a box for it).
    specs.push(GlyphSpec { chars: vec![ZWJ], advance: 0, layers: vec![Layer { color: 0, rects: vec![] }], colour: false });
    (specs, ligs)
}

const ZWJ: char = '\u{200D}';

fn components_of(s: &Script, c: char, virama: char) -> Vec<char> {
    match (s.kind, s.zwj) {
        (Kind::Half, _) => vec![c, virama],
        (Kind::Below, false) => vec![virama, c],
        (Kind::Below, true) => vec![virama, ZWJ, c],
    }
}

/// `a` and `b` as one glyph (no advance): `b`'s ink moved down so its top sits on `a`'s bottom.
fn stacked(to: char, a: &GlyphSpec, b: &GlyphSpec) -> GlyphSpec {
    let bottom = a.layers[0].rects.iter().map(|r| r.y1).max().unwrap_or(0);
    let top = b.layers[0].rects.iter().map(|r| r.y0).min().unwrap_or(0);
    let dy = (bottom - top).max(0);
    let mut rects = a.layers[0].rects.clone();
    rects.extend(b.layers[0].rects.iter().map(|r| Rect { x0: r.x0, x1: r.x1, y0: r.y0 + dy, y1: r.y1 + dy }));
    GlyphSpec { chars: vec![to], advance: 0, layers: vec![Layer { color: 0, rects }], colour: false }
}

/// A vowel sign that hangs below its letter: a combining mark whose ink starts in the lower rows of its cell.
fn is_below_mark(g: &Glyph) -> bool {
    g.combining && (0..16).find(|&y| (0..g.width).any(|x| g.ink(x, y))).is_some_and(|top| top >= 10)
}

/// Vowel signs under a subjoined letter. Both sit just under the letter before them, so they would overprint
/// each other; the pair becomes one glyph with the sign moved down below the subjoined letter. Shapers order
/// the two differently (Kannada puts the sign first, Khmer the letter first), so both orders are matched.
fn lowered_vowels(
    set: &std::collections::BTreeMap<u32, Glyph>,
    subs: &[(usize, char, GlyphSpec)],
    specs: &mut Vec<GlyphSpec>,
    ligs: &mut Vec<Ligature>,
    alloc: &mut impl FnMut() -> char,
) {
    for (si, s) in SCRIPTS.iter().enumerate() {
        let marks: Vec<(char, GlyphSpec)> = set
            .iter()
            .filter(|(cp, g)| (s.block.0..=s.block.1).contains(cp) && is_below_mark(g))
            .filter_map(|(cp, g)| Some((char::from_u32(*cp)?, spec_of(char::from_u32(*cp)?, g))))
            .collect();
        for (_, sub, sub_spec) in subs.iter().filter(|(i, _, _)| *i == si) {
            for (mark, mark_spec) in &marks {
                let to = alloc();
                specs.push(stacked(to, sub_spec, mark_spec));
                for &script in s.tags {
                    for feature in [*b"blws", *b"psts"] {
                        ligs.push(Ligature { script, feature, components: vec![*sub, *mark], to });
                        ligs.push(Ligature { script, feature, components: vec![*mark, *sub], to });
                    }
                }
            }
        }
    }
}

/// Tibetan stacks with a vowel sign under them: the lowered subjoined letter and the sign, one glyph.
fn tibetan_stacks(set: &std::collections::BTreeMap<u32, Glyph>, specs: &mut Vec<GlyphSpec>, ligs: &mut Vec<Ligature>, alloc: &mut impl FnMut() -> char) {
    let marks: Vec<(char, GlyphSpec)> = set
        .iter()
        .filter(|(cp, g)| (0xF00..=0xFFF).contains(*cp) && !(0xF90..=0xFBC).contains(*cp) && is_below_mark(g))
        .filter_map(|(cp, g)| Some((char::from_u32(*cp)?, spec_of(char::from_u32(*cp)?, g))))
        .collect();
    for (&cp, g) in set.range(0xF90..=0xFBC) {
        let Some(sub) = char::from_u32(cp) else { continue };
        let mut sub_spec = spec_of(sub, g);
        lower_tibetan_subjoined(std::slice::from_mut(&mut sub_spec));
        if sub_spec.layers[0].rects.is_empty() {
            continue;
        }
        for (mark, mark_spec) in &marks {
            let to = alloc();
            specs.push(stacked(to, &sub_spec, mark_spec));
            for feature in [*b"ccmp", *b"blws"] {
                ligs.push(Ligature { script: *b"tibt", feature, components: vec![sub, *mark], to });
            }
        }
    }
}

/// Malayalam's old way of writing a chillu: consonant + virama + joiner is the chillu letter that already has
/// its own code point.
fn chillus(set: &std::collections::BTreeMap<u32, Glyph>, ligs: &mut Vec<Ligature>) {
    for (consonant, chillu) in [(0xD23, 0xD7A), (0xD28, 0xD7B), (0xD30, 0xD7C), (0xD32, 0xD7D), (0xD33, 0xD7E), (0xD15, 0xD7F)] {
        if !set.contains_key(&chillu) {
            continue;
        }
        let (Some(c), Some(to)) = (char::from_u32(consonant), char::from_u32(chillu)) else { continue };
        for script in [*b"mlm2", *b"mlym"] {
            for feature in [*b"akhn", *b"cjct", *b"calt", *b"haln"] {
                ligs.push(Ligature { script, feature, components: vec![c, '\u{D4D}', ZWJ], to });
            }
        }
    }
}

/// Side by side: each glyph's ink (the first squeezed to half width when `squeeze_first`), as one glyph.
fn joined_glyph(to: char, parts: &[&Glyph], squeeze_first: bool) -> GlyphSpec {
    let masks: Vec<Vec<Vec<bool>>> = parts.iter().enumerate().map(|(i, g)| if i == 0 && squeeze_first { half_mask(g) } else { rows_of(g) }).collect();
    let rows: Vec<Vec<bool>> = (0..16).map(|y| masks.iter().flat_map(|m| m[y].iter().copied()).collect()).collect();
    let width = rows[0].len() as i32;
    spec(to, width, &rows, 0, BASELINE_ROW - 14)
}

/// Tamil's two real ligatures: க்ஷ (ksha) and ஶ்ரீ / ஸ்ரீ (shri). Everything else in Tamil keeps its visible pulli.
fn tamil(set: &std::collections::BTreeMap<u32, Glyph>, specs: &mut Vec<GlyphSpec>, ligs: &mut Vec<Ligature>, alloc: &mut impl FnMut() -> char) {
    const PULLI: char = '\u{BCD}';
    let get = |cp: u32| set.get(&cp);
    let mut add = |components: Vec<char>, glyph: GlyphSpec| {
        for script in [*b"tml2", *b"taml"] {
            ligs.push(Ligature { script, feature: *b"akhn", components: components.clone(), to: glyph.chars[0] });
        }
        specs.push(glyph);
    };
    if let (Some(ka), Some(ssa)) = (get(0xB95), get(0xBB7)) {
        let to = alloc();
        add(vec!['\u{B95}', PULLI, '\u{BB7}'], joined_glyph(to, &[ka, ssa], true));
    }
    for sa in [0xBB6u32, 0xBB8] {
        if let (Some(g_sa), Some(ra), Some(ii)) = (get(sa), get(0xBB0), get(0xBC0)) {
            let to = alloc();
            add(vec![char::from_u32(sa).unwrap(), PULLI, '\u{BB0}', '\u{BC0}'], joined_glyph(to, &[g_sa, ra, ii], true));
        }
    }
}

/// Bengali ya-phala (the ya after a virama, written as a hook to the right of the letter before): a quarter-size
/// ya standing on the baseline, after the base.
fn ya_phala(set: &std::collections::BTreeMap<u32, Glyph>, specs: &mut Vec<GlyphSpec>, ligs: &mut Vec<Ligature>, alloc: &mut impl FnMut() -> char) {
    let Some(ya) = set.get(&0x9AF) else { return };
    let m = quarter_mask(ya);
    let (h, w) = (m.len() as i32, m.first().map_or(0, Vec::len) as i32);
    let to = alloc();
    specs.push(spec(to, w + 1, &m, 0, BASELINE_ROW - h));
    for script in [*b"bng2", *b"beng"] {
        for feature in [*b"pstf", *b"vatu", *b"cjct"] {
            ligs.push(Ligature { script, feature, components: vec!['\u{9CD}', '\u{9AF}'], to });
        }
    }
}

/// Tibetan's subjoined letters (`U+0F90-0FBC`), moved down to sit under the letter they belong to.
pub fn lower_tibetan_subjoined(specs: &mut [GlyphSpec]) {
    for s in specs {
        if s.chars.first().is_some_and(|c| (0xF90..=0xFBC).contains(&(*c as u32))) {
            for l in &mut s.layers {
                for r in &mut l.rects {
                    r.y0 += TIBETAN_DROP;
                    r.y1 += TIBETAN_DROP;
                }
            }
        }
    }
}

#[allow(dead_code)]
pub(crate) fn script_names() -> Vec<&'static str> {
    SCRIPTS.iter().map(|s| s.name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> (Vec<GlyphSpec>, Vec<Ligature>) {
        derived(&crate::scripts::glyphs())
    }

    fn glyph_of(specs: &[GlyphSpec], c: char) -> &GlyphSpec {
        specs.iter().find(|s| s.chars == [c]).unwrap()
    }

    #[test]
    fn every_listed_script_gets_a_form_for_every_consonant_it_names() {
        let set = crate::scripts::glyphs();
        let ligs = all().1;
        for s in SCRIPTS {
            let virama = char::from_u32(s.virama).unwrap();
            for &(a, b) in s.consonants {
                for cp in a..=b {
                    if !s.only.is_empty() && !s.only.contains(&cp) {
                        continue;
                    }
                    let Some(g) = set.get(&cp) else { continue };
                    if g.combining || g.width != 16 {
                        continue;
                    }
                    let c = char::from_u32(cp).unwrap();
                    let want = components_of(s, c, virama);
                    for tag in s.tags {
                        assert!(
                            ligs.iter().any(|l| &l.script == tag && l.components == want),
                            "{}: no form for U+{cp:04X} under {}",
                            s.name,
                            String::from_utf8_lossy(tag)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_half_form_is_narrower_than_the_letter_and_a_subjoined_form_takes_no_room_and_sits_below() {
        let (specs, ligs) = all();
        for s in SCRIPTS {
            let virama = char::from_u32(s.virama).unwrap();
            let lig = ligs
                .iter()
                .find(|l| l.components.contains(&virama) && s.tags.contains(&l.script) && l.feature != *b"rphf")
                .unwrap_or_else(|| panic!("{} has no conjunct ligature", s.name));
            let g = glyph_of(&specs, lig.to);
            let ys: Vec<i32> = g.layers[0].rects.iter().map(|r| r.y0).collect();
            match s.kind {
                Kind::Half => {
                    assert!(g.advance > 0 && g.advance < 16, "{}: half form advance {}", s.name, g.advance);
                }
                Kind::Below => {
                    assert_eq!(g.advance, 0, "{}", s.name);
                    assert!(ys.iter().all(|&y| y >= BELOW_TOP), "{}: a subjoined letter rises above its row", s.name);
                }
            }
        }
    }

    #[test]
    fn a_reph_sits_above_the_line_and_only_scripts_with_one_get_it() {
        let (specs, ligs) = all();
        let reph: Vec<&Ligature> = ligs.iter().filter(|l| l.feature == *b"rphf").collect();
        for s in SCRIPTS {
            let has = reph.iter().any(|l| s.tags.contains(&l.script));
            assert_eq!(has, s.reph.is_some(), "{}", s.name);
        }
        for l in reph {
            let g = glyph_of(&specs, l.to);
            assert_eq!(g.advance, 0);
            assert!(g.layers[0].rects.iter().all(|r| r.y1 <= 0), "a reph reaches below the top of the line");
        }
    }

    #[test]
    fn a_stacked_pair_puts_the_second_glyph_directly_below_the_first() {
        let r = |y0, y1| Rect { x0: 0, x1: 4, y0, y1 };
        let a = GlyphSpec { chars: vec!['a'], advance: 0, layers: vec![Layer { color: 0, rects: vec![r(10, 15)] }], colour: false };
        let b = GlyphSpec { chars: vec!['b'], advance: 0, layers: vec![Layer { color: 0, rects: vec![r(11, 13)] }], colour: false };
        let s = stacked('c', &a, &b);
        assert_eq!(s.advance, 0);
        let ys: Vec<(i32, i32)> = s.layers[0].rects.iter().map(|r| (r.y0, r.y1)).collect();
        assert_eq!(ys, vec![(10, 15), (15, 17)], "b starts where a ends");
    }

    #[test]
    fn lowered_vowel_ligatures_exist_in_both_orders_for_every_subjoining_script() {
        let ligs = all().1;
        for s in SCRIPTS.iter().filter(|s| s.kind == Kind::Below) {
            let has = |first_is_mark: bool| {
                ligs.iter().any(|l| {
                    s.tags.contains(&l.script) && l.feature == *b"blws" && l.components.len() == 2 && {
                        let c0 = l.components[0] as u32;
                        let in_block = |c: u32| (s.block.0..=s.block.1).contains(&c);
                        // The mark is in the script's block; the subjoined letter is a private-use glyph.
                        if first_is_mark { in_block(c0) } else { c0 >= BASE }
                    }
                })
            };
            // A script with no below-sign in Unifont has none to lower; the rest must have both orders.
            if ligs.iter().any(|l| s.tags.contains(&l.script) && l.feature == *b"blws") {
                assert!(has(true) && has(false), "{}", s.name);
            }
        }
        assert!(ligs.iter().any(|l| l.script == *b"khmr" && l.feature == *b"blws"), "Khmer has below signs");
    }

    #[test]
    fn tibetan_subjoined_letters_are_moved_down_and_nothing_else_is() {
        let before = crate::cjk::specs_from(&crate::scripts::glyphs());
        let after = crate::scripts::glyph_specs();
        let top = |specs: &[GlyphSpec], c: char| specs.iter().find(|s| s.chars == [c]).unwrap().layers[0].rects.iter().map(|r| r.y0).min().unwrap();
        assert_eq!(top(&after, '\u{F90}'), top(&before, '\u{F90}') + TIBETAN_DROP);
        assert_eq!(top(&after, '\u{F40}'), top(&before, '\u{F40}'), "the letter above must stay put");
    }
}
