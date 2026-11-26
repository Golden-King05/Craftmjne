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

use crate::cjk::Glyph;
use crate::font_gen::{rects_from_mask, GlyphSpec, Layer, Ligature, BASELINE_ROW};

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
}

const SCRIPTS: &[Script] = &[
    Script { name: "Bengali", tags: &[*b"bng2", *b"beng"], features: &[*b"half"], virama: 0x9CD, consonants: &[(0x995, 0x9B9)], kind: Kind::Half, only: &[], reph: Some(0x9B0) },
    Script { name: "Gujarati", tags: &[*b"gjr2", *b"gujr"], features: &[*b"half"], virama: 0xACD, consonants: &[(0xA95, 0xAB9)], kind: Kind::Half, only: &[], reph: Some(0xAB0) },
    Script { name: "Odia", tags: &[*b"ory2", *b"orya"], features: &[*b"half"], virama: 0xB4D, consonants: &[(0xB15, 0xB39)], kind: Kind::Half, only: &[], reph: Some(0xB30) },
    Script { name: "Malayalam", tags: &[*b"mlm2", *b"mlym"], features: &[*b"half"], virama: 0xD4D, consonants: &[(0xD15, 0xD3A)], kind: Kind::Half, only: &[], reph: None },
    Script { name: "Gurmukhi", tags: &[*b"gur2", *b"guru"], features: &[*b"blwf"], virama: 0xA4D, consonants: &[(0xA2F, 0xA39)], kind: Kind::Below, only: &[0xA2F, 0xA30, 0xA35, 0xA39], reph: None },
    Script { name: "Kannada", tags: &[*b"knd2", *b"knda"], features: &[*b"blwf"], virama: 0xCCD, consonants: &[(0xC95, 0xCB9)], kind: Kind::Below, only: &[], reph: Some(0xCB0) },
    Script { name: "Telugu", tags: &[*b"tel2", *b"telu"], features: &[*b"blwf"], virama: 0xC4D, consonants: &[(0xC15, 0xC39)], kind: Kind::Below, only: &[], reph: None },
    Script { name: "Khmer", tags: &[*b"khmr"], features: &[*b"pref", *b"blwf", *b"pstf"], virama: 0x17D2, consonants: &[(0x1780, 0x17A2)], kind: Kind::Below, only: &[], reph: None },
    Script { name: "Burmese", tags: &[*b"mym2", *b"mymr"], features: &[*b"blwf", *b"pstf"], virama: 0x1039, consonants: &[(0x1000, 0x1021)], kind: Kind::Below, only: &[], reph: None },
    Script { name: "Javanese", tags: &[*b"java"], features: &[*b"blwf", *b"pstf"], virama: 0xA9C0, consonants: &[(0xA98F, 0xA9B2)], kind: Kind::Below, only: &[], reph: None },
    Script { name: "Balinese", tags: &[*b"bali"], features: &[*b"blwf", *b"pstf"], virama: 0x1B44, consonants: &[(0x1B13, 0x1B33)], kind: Kind::Below, only: &[], reph: None },
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
    for s in SCRIPTS {
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
                    specs.push(spec(to, 0, &m, -16 + (16 - w) / 2, BELOW_TOP));
                }
            }
            for &script in s.tags {
                for &feature in s.features {
                    let components = if s.kind == Kind::Half { vec![c, virama] } else { vec![virama, c] };
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
    (specs, ligs)
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
                    let want = if s.kind == Kind::Half { vec![c, virama] } else { vec![virama, c] };
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
    fn tibetan_subjoined_letters_are_moved_down_and_nothing_else_is() {
        let before = crate::cjk::specs_from(&crate::scripts::glyphs());
        let after = crate::scripts::glyph_specs();
        let top = |specs: &[GlyphSpec], c: char| specs.iter().find(|s| s.chars == [c]).unwrap().layers[0].rects.iter().map(|r| r.y0).min().unwrap();
        assert_eq!(top(&after, '\u{F90}'), top(&before, '\u{F90}') + TIBETAN_DROP);
        assert_eq!(top(&after, '\u{F40}'), top(&before, '\u{F40}'), "the letter above must stay put");
    }
}
