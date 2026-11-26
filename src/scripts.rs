//! More scripts, from GNU Unifont's bitmaps (SIL OFL 1.1, see `third_party/unifont/`):
//! Armenian, Georgian (Mkhedruli, Mtavruli, Asomtavruli and Nuskhuri), Syriac, Mandaic, Coptic,
//! Ethiopic (Ge'ez, Amharic, Tigrinya, with its supplements and extensions), Tifinagh (Tuareg and
//! the neo-Tifinagh of the Berber Academy are one Unicode block), N'Ko and Vai - plus Greek Extended
//! (polytonic) and the archaic letters of the Greek and Coptic block. Everyday modern Greek is drawn
//! in the game's own Latin style (`pixel_greek.rs`); this font is the fallback for the rest.
//!
//! Same geometry as [`crate::cjk`]: glyphs are 16 pixels tall, baseline-aligned, so they land on whole
//! screen pixels with the Latin text and are 4/3 as tall (text containing them gets the taller line).
//! The data is `cjk/scripts.bin` (made by `tools/build_cjk_data.py`).
//!
//! **Combining marks** (Syriac points, N'Ko tone marks, Coptic and Mandaic marks) take no room; the
//! text engine places them over the letter before.
//!
//! **Joining.** Syriac, Mandaic and N'Ko are cursive: a letter changes shape by where it sits in a
//! word. Unifont draws only the isolated shape, so the initial, medial and final forms are *derived*:
//! the isolated letter with a connecting stroke added on the baseline, on the side(s) that join. They
//! are `GSUB` `init`, `medi` and `fina` substitutions (script tags `syrc`, `mand`, `nko `), which the
//! shaper picks from each letter's Unicode joining type. This is an approximation of real cursive
//! writing - good enough to read, not calligraphy - and Syriac's special forms (the final Alaph
//! variants, ligatures) are not made.

use std::collections::BTreeMap;

use crate::cjk::{decode, font_from_all, spec_of, specs_from, Glyph, Join};
use crate::font_gen::{rects_from_mask, GlyphSpec, Layer, Substitution, BASELINE_ROW};
use crate::mongolian::{piece, Form, Piece};

const BLOB: &[u8] = include_bytes!("cjk/scripts.bin");

/// Where the derived joining forms live (private use; this font has no other private-use letters).
const FORM_BASE: u32 = 0xE000;

/// Whether `c` is drawn by this module's font. Modern Greek (`U+0370-03FF`) is the game's own font.
pub fn is_bitmap_script(c: char) -> bool {
    matches!(c as u32,
        0x0530..=0x058F | 0xFB13..=0xFB17 | 0x10A0..=0x10FF | 0x1C90..=0x1CBF | 0x2D00..=0x2D2F
        | 0x0700..=0x074F | 0x0860..=0x086F | 0x0840..=0x085F | 0x2C80..=0x2CFF
        | 0x1200..=0x139F | 0x2D80..=0x2DDF | 0xAB00..=0xAB2F | 0x1E7E0..=0x1E7FF
        | 0x2D30..=0x2D7F | 0x07C0..=0x07FF | 0xA500..=0xA63F | 0x102E0..=0x102FF | 0x1F00..=0x1FFF
        | 0x1800..=0x18AF | 0x11660..=0x1167F | 0xA840..=0xA87F | 0x11A00..=0x11AAF
        | 0x0900..=0x097F | 0xA8E0..=0xA8FF | 0x0980..=0x0DFF | 0x111E0..=0x111FF | 0x0F00..=0x0FFF | 0x0780..=0x07BF | 0x1C50..=0x1C7F
        | 0x1000..=0x109F | 0xA9E0..=0xA9FF | 0xAA60..=0xAA7F | 0x1B170..=0x1B2FF | 0xA000..=0xA4FF
        | 0x16F00..=0x16F9F | 0x0E00..=0x0EFF | 0x1780..=0x17FF | 0x19E0..=0x19FF | 0x1700..=0x177F
        | 0x1A00..=0x1A1F | 0x1B00..=0x1BFF | 0xA980..=0xA9DF | 0x1CC0..=0x1CCF
        | 0x02C7 | 0x02C9..=0x02CB | 0x02D9)
}

pub fn glyphs() -> BTreeMap<u32, Glyph> {
    decode(BLOB)
}

/// Syriac, Mandaic and N'Ko: the scripts whose letters get derived joining forms.
fn is_cursive_script(c: char) -> bool {
    matches!(c as u32, 0x0700..=0x074F | 0x0860..=0x086F | 0x0840..=0x085F | 0x07C0..=0x07FF)
}

/// The script tag the joining forms of `c` are registered under.
fn script_tag(c: char) -> [u8; 4] {
    match c as u32 {
        0x0840..=0x085F => *b"mand",
        0x07C0..=0x07FF => *b"nko ",
        _ => *b"syrc",
    }
}

/// `g` with a connecting stroke on the baseline row, on the left and/or right edge. The stroke runs from
/// the letter's outermost ink on that row to the edge of its cell, so neighbours touch.
fn with_stroke(g: &Glyph, left: bool, right: bool) -> Glyph {
    let mut out = *g;
    // The lowest row with ink, but never below the baseline row (descenders do not connect).
    let Some(row) = (0..=13).rev().find(|&y| g.rows[y] != 0) else { return out };
    let ink: Vec<usize> = (0..g.width).filter(|&x| g.ink(x, row)).collect();
    let (first, last) = (*ink.first().unwrap(), *ink.last().unwrap());
    let bit = |x: usize| 1u16 << (g.width - 1 - x);
    if left {
        for x in 0..first {
            out.rows[row] |= bit(x);
        }
    }
    if right {
        for x in last + 1..g.width {
            out.rows[row] |= bit(x);
        }
    }
    out
}

/// The joining forms of every cursive letter: `(base, final, initial, medial)`, the last two only for
/// dual-joining letters, with the private-use letters that name them.
fn forms() -> Vec<(char, [Option<(char, Glyph)>; 3])> {
    let mut out = Vec::new();
    let mut next = FORM_BASE;
    let mut take = || {
        let c = char::from_u32(next).unwrap();
        next += 1;
        c
    };
    for (&cp, g) in &glyphs() {
        let Some(base) = char::from_u32(cp) else { continue };
        // Only the scripts written cursively in horizontal lines get derived forms. Mongolian joins in
        // principle, but it is written downward and Unifont draws it as upright separate letters.
        if g.combining || g.join == Join::None || !is_cursive_script(base) {
            continue;
        }
        // In a right-to-left script the letter before is on the right, the one after on the left.
        let fina = Some((take(), with_stroke(g, false, true)));
        let (init, medi) = if g.join == Join::Dual {
            (Some((take(), with_stroke(g, true, false))), Some((take(), with_stroke(g, true, true))))
        } else {
            (None, None)
        };
        out.push((base, [fina, init, medi]));
    }
    out
}

/// Mongolian-block letters (not marks): drawn rotated, with their own joining forms, below.
fn is_mongolian_letter(cp: u32, g: &Glyph) -> bool {
    (0x1800..=0x18AF).contains(&cp) && !g.combining
}

/// Where the rotated Mongolian forms (and their spines) sit: the row of the 16-row rotated glyph the spine is on.
const MONGOLIAN_SPINE_ROW: i32 = 12;
/// Private-use letters naming the rotated Mongolian forms (after the cursive scripts' at `FORM_BASE`).
const MONGOLIAN_FORM_BASE: u32 = 0xE800;

/// A Mongolian piece turned a quarter turn so it reads left to right: the upright letter's top becomes its
/// left, its spine runs along one row, and the spine rows of all letters line up. `gap` is the space after
/// it; joined forms have none, so their strokes meet.
fn rotated_spec(c: char, p: &Piece, gap: i32) -> GlyphSpec {
    let h = p.rows.len();
    let w = p.rows[0].len();
    // Counter-clockwise: the right edge becomes the top, the top the left.
    let mask: Vec<Vec<bool>> = (0..w).map(|nr| (0..h).map(|nc| p.rows[nc][w - 1 - nr]).collect()).collect();
    let spine_row = (w - 1 - p.spine) as i32;
    let y_offset = (BASELINE_ROW - 14) + (MONGOLIAN_SPINE_ROW - spine_row);
    let rects = rects_from_mask(h, &mask, y_offset);
    GlyphSpec { chars: vec![c], advance: h as i32 + gap, layers: vec![Layer { color: 0, rects }], colour: false }
}

/// The rotated Mongolian glyphs and the substitutions that choose their forms: `(glyphs, substitutions)`.
fn mongolian() -> (Vec<GlyphSpec>, Vec<Substitution>) {
    let (mut specs, mut subs) = (Vec::new(), Vec::new());
    let mut next = MONGOLIAN_FORM_BASE;
    for (&cp, g) in &glyphs() {
        if !is_mongolian_letter(cp, g) {
            continue;
        }
        let Some(c) = char::from_u32(cp) else { continue };
        let isolated = piece(g, Form::Isolated);
        if isolated.rows.is_empty() {
            continue;
        }
        specs.push(rotated_spec(c, &isolated, 1));
        if g.join != Join::Dual {
            continue;
        }
        for (form, feature) in [(Form::Final, *b"fina"), (Form::Initial, *b"init"), (Form::Medial, *b"medi")] {
            let p = piece(g, form);
            if p.rows.is_empty() {
                continue;
            }
            let to = char::from_u32(next).unwrap();
            next += 1;
            specs.push(rotated_spec(to, &p, 0));
            subs.push(Substitution { script: *b"mong", feature, from: c, to });
        }
    }
    (specs, subs)
}

/// Devanagari's right-hand vowel signs and the visarga. Unifont draws each in a cell of its own with the ink in the
/// right-hand columns, meant to sit over the consonant's cell (the consonant's headline runs on into the sign's);
/// given a cell of their own they leave a gap of a whole letter. So they take no room and hang over the letter before.
/// The left-hand ones (the i sign, which comes before its consonant) keep their cell.
const OVERLAID_SIGNS: &[u32] = &[0x903, 0x93B, 0x93E, 0x940, 0x949, 0x94A, 0x94B, 0x94C, 0x94F];

fn overlay_spacing_signs(specs: &mut [GlyphSpec]) {
    for s in specs {
        if s.chars.first().is_some_and(|c| OVERLAID_SIGNS.contains(&(*c as u32))) {
            for l in &mut s.layers {
                for r in &mut l.rects {
                    r.x0 -= s.advance;
                    r.x1 -= s.advance;
                }
            }
            s.advance = 0;
        }
    }
}

pub fn glyph_specs() -> Vec<GlyphSpec> {
    let rest: BTreeMap<u32, Glyph> = glyphs().into_iter().filter(|(cp, g)| !is_mongolian_letter(*cp, g)).collect();
    let mut specs = specs_from(&rest);
    specs.extend(mongolian().0);
    for (_, fs) in forms() {
        for (c, g) in fs.iter().flatten() {
            specs.push(spec_of(*c, g));
        }
    }
    crate::conjuncts::lower_tibetan_subjoined(&mut specs);
    overlay_spacing_signs(&mut specs);
    specs.extend(crate::conjuncts::derived(&glyphs()).0);
    specs
}

/// The `GSUB` ligatures that make conjuncts (half forms, subjoined letters, reph).
pub fn ligatures() -> Vec<crate::font_gen::Ligature> {
    crate::conjuncts::derived(&glyphs()).1
}

/// The `GSUB` substitutions that pick a cursive letter's form.
pub fn substitutions() -> Vec<Substitution> {
    let mut out = mongolian().1;
    for (base, [fina, init, medi]) in forms() {
        let script = script_tag(base);
        for (feature, form) in [(*b"fina", fina), (*b"init", init), (*b"medi", medi)] {
            if let Some((to, _)) = form {
                out.push(Substitution { script, feature, from: base, to });
            }
        }
    }
    out
}

/// The font file ("Craftmjne Scripts").
pub fn font_bytes() -> Vec<u8> {
    font_from_all("Craftmjne Scripts", glyph_specs(), substitutions(), ligatures())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every assigned code point (Unicode 14) of the blocks this module promises, as (first, last).
    const PROMISED: &[(u32, u32)] = &[
        (0x0370, 0x0373), (0x0375, 0x0377), (0x037A, 0x037F), (0x0384, 0x038A), (0x038C, 0x038C), (0x038E, 0x03A1),
        (0x03A3, 0x03FF), (0x1F00, 0x1F15), (0x1F18, 0x1F1D), (0x1F20, 0x1F45), (0x1F48, 0x1F4D), (0x1F50, 0x1F57),
        (0x1F59, 0x1F59), (0x1F5B, 0x1F5B), (0x1F5D, 0x1F5D), (0x1F5F, 0x1F7D), (0x1F80, 0x1FB4), (0x1FB6, 0x1FC4),
        (0x1FC6, 0x1FD3), (0x1FD6, 0x1FDB), (0x1FDD, 0x1FEF), (0x1FF2, 0x1FF4), (0x1FF6, 0x1FFE),
        (0x0531, 0x0556), (0x0559, 0x058A), (0x058D, 0x058F), (0xFB13, 0xFB17),
        (0x10A0, 0x10C5), (0x10C7, 0x10C7), (0x10CD, 0x10CD), (0x10D0, 0x10FF), (0x1C90, 0x1CBA), (0x1CBD, 0x1CBF),
        (0x2D00, 0x2D25), (0x2D27, 0x2D27), (0x2D2D, 0x2D2D),
        (0x0700, 0x070D), (0x070F, 0x074A), (0x074D, 0x074F), (0x0860, 0x086A),
        (0x0840, 0x085B), (0x085E, 0x085E),
        (0x2C80, 0x2CF3), (0x2CF9, 0x2CFF),
        (0x1200, 0x1248), (0x124A, 0x124D), (0x1250, 0x1256), (0x1258, 0x1258), (0x125A, 0x125D), (0x1260, 0x1288),
        (0x128A, 0x128D), (0x1290, 0x12B0), (0x12B2, 0x12B5), (0x12B8, 0x12BE), (0x12C0, 0x12C0), (0x12C2, 0x12C5),
        (0x12C8, 0x12D6), (0x12D8, 0x1310), (0x1312, 0x1315), (0x1318, 0x135A), (0x135D, 0x137C), (0x1380, 0x1399),
        (0x2D80, 0x2D96), (0x2DA0, 0x2DA6), (0x2DA8, 0x2DAE), (0x2DB0, 0x2DB6), (0x2DB8, 0x2DBE), (0x2DC0, 0x2DC6),
        (0x2DC8, 0x2DCE), (0x2DD0, 0x2DD6), (0x2DD8, 0x2DDE), (0xAB01, 0xAB06), (0xAB09, 0xAB0E), (0xAB11, 0xAB16),
        (0xAB20, 0xAB26), (0xAB28, 0xAB2E),
        (0x2D30, 0x2D67), (0x2D6F, 0x2D70), (0x2D7F, 0x2D7F),
        (0x07C0, 0x07FA), (0x07FD, 0x07FF),
        (0xA500, 0xA62B),
        // Mongolian and relatives, South and Southeast Asia, Yi, Miao, Nushu, the Philippines, Indonesia;
        // and the zhuyin tone marks.
    (0x1800, 0x1819), (0x1820, 0x1878), (0x1880, 0x18AA), (0x11660, 0x1166C), (0xA840, 0xA877),
    (0x11A50, 0x11AA2), (0x11A00, 0x11A47), (0x0900, 0x097F), (0xA8E0, 0xA8FF), (0x0980, 0x0983), (0x0985, 0x098C), (0x098F, 0x0990),
    (0x0993, 0x09A8), (0x09AA, 0x09B0), (0x09B2, 0x09B2), (0x09B6, 0x09B9), (0x09BC, 0x09C4),
    (0x09C7, 0x09C8), (0x09CB, 0x09CE), (0x09D7, 0x09D7), (0x09DC, 0x09DD), (0x09DF, 0x09E3),
    (0x09E6, 0x09FE), (0x0A01, 0x0A03), (0x0A05, 0x0A0A), (0x0A0F, 0x0A10), (0x0A13, 0x0A28),
    (0x0A2A, 0x0A30), (0x0A32, 0x0A33), (0x0A35, 0x0A36), (0x0A38, 0x0A39), (0x0A3C, 0x0A3C),
    (0x0A3E, 0x0A42), (0x0A47, 0x0A48), (0x0A4B, 0x0A4D), (0x0A51, 0x0A51), (0x0A59, 0x0A5C),
    (0x0A5E, 0x0A5E), (0x0A66, 0x0A76), (0x0A81, 0x0A83), (0x0A85, 0x0A8D), (0x0A8F, 0x0A91),
    (0x0A93, 0x0AA8), (0x0AAA, 0x0AB0), (0x0AB2, 0x0AB3), (0x0AB5, 0x0AB9), (0x0ABC, 0x0AC5),
    (0x0AC7, 0x0AC9), (0x0ACB, 0x0ACD), (0x0AD0, 0x0AD0), (0x0AE0, 0x0AE3), (0x0AE6, 0x0AF1),
    (0x0AF9, 0x0AFF), (0x0B01, 0x0B03), (0x0B05, 0x0B0C), (0x0B0F, 0x0B10), (0x0B13, 0x0B28),
    (0x0B2A, 0x0B30), (0x0B32, 0x0B33), (0x0B35, 0x0B39), (0x0B3C, 0x0B44), (0x0B47, 0x0B48),
    (0x0B4B, 0x0B4D), (0x0B55, 0x0B57), (0x0B5C, 0x0B5D), (0x0B5F, 0x0B63), (0x0B66, 0x0B77),
    (0x0B82, 0x0B83), (0x0B85, 0x0B8A), (0x0B8E, 0x0B90), (0x0B92, 0x0B95), (0x0B99, 0x0B9A),
    (0x0B9C, 0x0B9C), (0x0B9E, 0x0B9F), (0x0BA3, 0x0BA4), (0x0BA8, 0x0BAA), (0x0BAE, 0x0BB9),
    (0x0BBE, 0x0BC2), (0x0BC6, 0x0BC8), (0x0BCA, 0x0BCD), (0x0BD0, 0x0BD0), (0x0BD7, 0x0BD7),
    (0x0BE6, 0x0BFA), (0x0C00, 0x0C0C), (0x0C0E, 0x0C10), (0x0C12, 0x0C28), (0x0C2A, 0x0C39),
    (0x0C3C, 0x0C44), (0x0C46, 0x0C48), (0x0C4A, 0x0C4D), (0x0C55, 0x0C56), (0x0C58, 0x0C5A),
    (0x0C5D, 0x0C5D), (0x0C60, 0x0C63), (0x0C66, 0x0C6F), (0x0C77, 0x0C7F), (0x0C80, 0x0C8C),
    (0x0C8E, 0x0C90), (0x0C92, 0x0CA8), (0x0CAA, 0x0CB3), (0x0CB5, 0x0CB9), (0x0CBC, 0x0CC4),
    (0x0CC6, 0x0CC8), (0x0CCA, 0x0CCD), (0x0CD5, 0x0CD6), (0x0CDD, 0x0CDE), (0x0CE0, 0x0CE3),
    (0x0CE6, 0x0CEF), (0x0CF1, 0x0CF2), (0x0D00, 0x0D0C), (0x0D0E, 0x0D10), (0x0D12, 0x0D44),
    (0x0D46, 0x0D48), (0x0D4A, 0x0D4F), (0x0D54, 0x0D63), (0x0D66, 0x0D7F), (0x0D81, 0x0D83),
    (0x0D85, 0x0D96), (0x0D9A, 0x0DB1), (0x0DB3, 0x0DBB), (0x0DBD, 0x0DBD), (0x0DC0, 0x0DC6),
    (0x0DCA, 0x0DCA), (0x0DCF, 0x0DD4), (0x0DD6, 0x0DD6), (0x0DD8, 0x0DDF), (0x0DE6, 0x0DEF),
    (0x0DF2, 0x0DF4), (0x111E1, 0x111F4), (0x0F00, 0x0F47), (0x0F49, 0x0F6C), (0x0F71, 0x0F97),
    (0x0F99, 0x0FBC), (0x0FBE, 0x0FCC), (0x0FCE, 0x0FDA), (0x0780, 0x07B1), (0x1C50, 0x1C7F),
    (0x1000, 0x109F), (0xA9E0, 0xA9FE), (0xAA60, 0xAA7F), (0x1B170, 0x1B2FB), (0xA000, 0xA48C),
    (0xA490, 0xA4C6), (0x16F00, 0x16F4A), (0x16F4F, 0x16F87), (0x16F8F, 0x16F9F), (0xA4D0, 0xA4FF),
    (0x0E01, 0x0E3A), (0x0E3F, 0x0E5B), (0x0E81, 0x0E82), (0x0E84, 0x0E84), (0x0E86, 0x0E8A),
    (0x0E8C, 0x0EA3), (0x0EA5, 0x0EA5), (0x0EA7, 0x0EBD), (0x0EC0, 0x0EC4), (0x0EC6, 0x0EC6),
    (0x0EC8, 0x0ECD), (0x0ED0, 0x0ED9), (0x0EDC, 0x0EDF), (0x1780, 0x17DD), (0x17E0, 0x17E9),
    (0x17F0, 0x17F9), (0x19E0, 0x19FF), (0x1700, 0x1715), (0x171F, 0x1736), (0x1740, 0x1753),
    (0x1760, 0x176C), (0x176E, 0x1770), (0x1772, 0x1773), (0x1A00, 0x1A1B), (0x1A1E, 0x1A1F),
    (0x1B00, 0x1B4C), (0x1B50, 0x1B7E), (0xA980, 0xA9CD), (0xA9CF, 0xA9D9), (0xA9DE, 0xA9DF),
    (0x1B80, 0x1BBF), (0x1CC0, 0x1CC7), (0x1BC0, 0x1BF3), (0x1BFC, 0x1BFF), (0x02C7, 0x02C7),
    (0x02C9, 0x02CB), (0x02D9, 0x02D9),
    ];

    #[test]
    fn every_assigned_character_of_every_promised_script_has_a_glyph() {
        let set = glyphs();
        let mut missing = Vec::new();
        for &(a, b) in PROMISED {
            for cp in a..=b {
                if !set.contains_key(&cp) {
                    missing.push(format!("{cp:X}"));
                }
            }
        }
        assert!(missing.is_empty(), "{} missing, e.g. {:?}", missing.len(), &missing[..missing.len().min(8)]);
        assert!(set.len() > 1800, "only {} glyphs", set.len());
    }

    #[test]
    fn the_font_maps_them_all_and_leaves_modern_greek_and_latin_alone() {
        let bytes = font_bytes();
        let face = ttf_parser::Face::parse(&bytes, 0).expect("the scripts font parses");
        for &(a, b) in PROMISED {
            for cp in (a..=b).step_by(7) {
                assert!(face.glyph_index(char::from_u32(cp).unwrap()).is_some(), "U+{cp:X} is not in the font");
            }
        }
        // Emoji and joiners belong to the emoji font - except the zero-width joiner itself, which Sinhala and
        // Malayalam conjuncts need in this font too (an engine-level test checks emoji sequences still hold).
        for j in crate::emoji::joiners().into_iter().filter(|c| *c != '\u{200D}') {
            assert!(face.glyph_index(j).is_none(), "U+{:X} must stay the emoji font's", j as u32);
        }
    }

    #[test]
    fn combining_marks_take_no_room_and_hang_left_of_their_origin() {
        let set = glyphs();
        let marks: Vec<u32> = set.iter().filter(|(_, g)| g.combining).map(|(c, _)| *c).collect();
        assert!(marks.len() >= 40, "{} combining marks", marks.len());
        let bytes = font_bytes();
        let face = ttf_parser::Face::parse(&bytes, 0).unwrap();
        // Syriac pthaha above (U+0730) and N'Ko short high tone (U+07EB).
        for cp in [0x0730u32, 0x07EB, 0x0741] {
            assert!(set[&cp].combining, "U+{cp:X} should be a mark");
            let g = face.glyph_index(char::from_u32(cp).unwrap()).unwrap();
            assert_eq!(face.glyph_hor_advance(g), Some(0), "U+{cp:X} takes room");
            let bbox = face.glyph_bounding_box(g).unwrap();
            assert!(bbox.x_max <= 0, "U+{cp:X} is not drawn left of its origin: {bbox:?}");
        }
        // Letters do take room.
        let beth = face.glyph_index('\u{712}').unwrap();
        assert!(face.glyph_hor_advance(beth).unwrap() > 0);
    }

    /// Shapes `text` and returns the glyph ids.
    fn shape(font: &[u8], text: &str) -> Vec<u32> {
        let face = rustybuzz::Face::from_slice(font, 0).expect("the font parses");
        let mut buf = rustybuzz::UnicodeBuffer::new();
        buf.push_str(text);
        rustybuzz::shape(&face, &[], buf).glyph_infos().iter().map(|i| i.glyph_id).collect()
    }

    /// The glyph id of `c`'s joining forms `(final, initial, medial)` in `face`.
    fn form_ids(face: &ttf_parser::Face, c: char) -> (u32, Option<u32>, Option<u32>) {
        let f = forms().into_iter().find(|(b, _)| *b == c).unwrap().1;
        let id = |x: &Option<(char, Glyph)>| x.as_ref().map(|(ch, _)| face.glyph_index(*ch).unwrap().0 as u32);
        (id(&f[0]).unwrap(), id(&f[1]), id(&f[2]))
    }

    #[test]
    fn cursive_letters_take_the_form_their_position_in_the_word_calls_for() {
        let font = font_bytes();
        let face = ttf_parser::Face::parse(&font, 0).unwrap();
        let gid = |c: char| face.glyph_index(c).unwrap().0 as u32;
        // The shaper returns right-to-left text in visual order, so reverse to read it in typing order.
        let logical = |text: &str| -> Vec<u32> {
            let mut v = shape(&font, text);
            v.reverse();
            v
        };
        for (name, text) in [("syriac", "\u{712}\u{713}\u{71A}"), ("nko", "\u{7CA}\u{7CB}\u{7CC}"), ("mandaic", "\u{841}\u{842}\u{843}")] {
            let chars: Vec<char> = text.chars().collect();
            let ids = logical(text);
            assert_eq!(ids.len(), 3, "{name}: {ids:?}");
            // Typed first = rightmost = starts the word: initial. Last: final. Middle: medial.
            assert_eq!(Some(ids[0]), form_ids(&face, chars[0]).1, "{name}: the first letter should be initial");
            assert_eq!(Some(ids[1]), form_ids(&face, chars[1]).2, "{name}: the middle letter should be medial");
            assert_eq!(ids[2], form_ids(&face, chars[2]).0, "{name}: the last letter should be final");
            // Alone, a letter keeps its isolated shape; two letters are initial then final.
            assert_eq!(logical(&chars[0].to_string()), vec![gid(chars[0])], "{name}: alone");
            let two = logical(&text.chars().take(2).collect::<String>());
            assert_eq!((Some(two[0]), two[1]), (form_ids(&face, chars[0]).1, form_ids(&face, chars[1]).0), "{name}: a pair");
        }
        // A right-joining letter (Alaph) joins only the letter before it, so the one after it starts a word.
        let ids = logical("\u{710}\u{712}");
        assert_eq!(ids, vec![gid('\u{710}'), gid('\u{712}')], "Alaph then Beth: both isolated");
        // ...and after a dual-joining letter Alaph takes its final form.
        let ids = logical("\u{712}\u{710}");
        assert_eq!(ids, vec![form_ids(&face, '\u{712}').1.unwrap(), form_ids(&face, '\u{710}').0]);
    }

    #[test]
    fn a_joined_form_reaches_the_edge_of_its_cell_where_it_joins() {
        let f = forms().into_iter().find(|(b, _)| *b == '\u{712}').unwrap().1;
        let reaches = |g: &Glyph, left: bool| (0..16).any(|y| g.ink(if left { 0 } else { g.width - 1 }, y));
        let fina = f[0].as_ref().unwrap().1;
        let init = f[1].as_ref().unwrap().1;
        let medi = f[2].as_ref().unwrap().1;
        // Joining to the previous letter touches the right edge; to the next, the left edge.
        assert!(reaches(&fina, false) && reaches(&init, true) && reaches(&medi, false) && reaches(&medi, true));
    }

    #[test]
    fn devanagari_signs_that_belong_over_the_letter_take_no_room_and_the_i_sign_keeps_its_cell() {
        let specs = glyph_specs();
        let spec = |cp: u32| specs.iter().find(|s| s.chars == [char::from_u32(cp).unwrap()]).unwrap();
        for cp in OVERLAID_SIGNS {
            let s = spec(*cp);
            assert_eq!(s.advance, 0, "U+{cp:04X}");
            assert!(s.layers[0].rects.iter().all(|r| r.x1 <= 0), "U+{cp:04X} hangs left of its origin, over the letter before");
        }
        // The i sign is drawn before its consonant, in a cell of its own; a letter is one too.
        assert_eq!(spec(0x93F).advance, 16);
        assert_eq!(spec(0x915).advance, 16);
    }

    #[test]
    fn is_bitmap_script_matches_the_data() {
        for c in "ԱաႠაܐࡀⲀሀⴰߊꔀἀ".chars() {
            assert!(is_bitmap_script(c), "{c}");
        }
        for c in "aαß日Ꮳᐃðא".chars() {
            assert!(!is_bitmap_script(c), "{c}");
        }
        assert!(glyphs().keys().filter(|c| char::from_u32(**c).is_some_and(|c| !(0x370..=0x3FF).contains(&(c as u32)))).all(|c| is_bitmap_script(char::from_u32(*c).unwrap())));
    }
}
