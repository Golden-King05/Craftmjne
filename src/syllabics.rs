//! Cherokee and the Canadian Aboriginal syllabics (Cree, Inuktitut, Ojibwe, Blackfoot, ...).
//!
//! Like the Chinese and Japanese glyphs, these are GNU Unifont's 16x16 bitmaps (SIL OFL 1.1, see
//! `third_party/unifont/`), kept in one small blob (`cjk/syllabics.bin`, made by
//! `tools/build_cjk_data.py`) and turned into a fallback font in memory. They are complete for
//! what Unicode encodes:
//!
//! * **Cherokee** `U+13A0-13FF` (the 86 capital syllables and the six lowercase ones) and the
//!   Cherokee Supplement `U+AB70-ABBF` (the 80 small letters Unicode now writes lowercase Cherokee
//!   with);
//! * **Unified Canadian Aboriginal Syllabics** `U+1400-167F` (640) and its Extended block
//!   `U+18B0-18FF` (80) - the writing of Cree, Inuktitut, Ojibwe, Naskapi, Blackfoot, Carrier...
//!
//! The geometry is the same as [`crate::cjk`]: 16 pixels tall, baseline-aligned, so it lands on whole
//! screen pixels with the Latin text and is 4/3 as tall (text containing it gets the taller line).
//! The Latin letters these languages also use (Icelandic ð þ, Yup'ik ḿ ń u͡g, Inuit ł ŋ) are in the
//! game's own Latin font, not here.
//!
//! **Not in Unicode, so not here:** the Afaka syllabary of Ndyuka and the Yugtun script of Yup'ik are
//! not encoded yet.

use std::collections::BTreeMap;

use crate::cjk::{decode, font_from, specs_from, Glyph};
use crate::font_gen::GlyphSpec;

const BLOB: &[u8] = include_bytes!("cjk/syllabics.bin");

/// Whether `c` is drawn by this module's font.
pub fn is_syllabic(c: char) -> bool {
    matches!(c as u32, 0x13A0..=0x13FF | 0xAB70..=0xABBF | 0x1400..=0x167F | 0x18B0..=0x18FF)
}

pub fn glyphs() -> BTreeMap<u32, Glyph> {
    decode(BLOB)
}

pub fn glyph_specs() -> Vec<GlyphSpec> {
    specs_from(&glyphs())
}

/// The font file ("Craftmjne Syllabics").
pub fn font_bytes() -> Vec<u8> {
    font_from("Craftmjne Syllabics", glyph_specs())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every assigned code point of the two scripts' blocks (Unicode 15+).
    fn assigned() -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        v.extend(0x13A0..=0x13F5);
        v.extend(0x13F8..=0x13FD);
        v.extend(0xAB70..=0xABBF);
        v.extend(0x1400..=0x167F);
        v.extend(0x18B0..=0x18F5);
        v
    }

    #[test]
    fn every_assigned_cherokee_and_canadian_syllabic_has_a_glyph() {
        let set = glyphs();
        let missing: Vec<String> = assigned().into_iter().filter(|c| !set.contains_key(c)).map(|c| format!("{c:X}")).collect();
        assert!(missing.is_empty(), "missing: {missing:?}");
        assert!(set.len() >= assigned().len());
    }

    #[test]
    fn the_letters_the_languages_name_are_there() {
        // CLDR's Cherokee main set (lowercase) and index (capitals), and part of Inuktitut's main set.
        let set = glyphs();
        for c in "ꭰꮿᏸᏼᎠᎦᎭᏃᏉᏎᏖᏜᏣᏬᏳᏹ᏿".chars().chain("ᐃᐄᐅᐆᐊᐋᐱᑉᑎᑦᑭᒃᒋᒡᒥᒻᓂᓐᓕᓪᓯᔅᔨᔾᕆᕐᕕᕿᖃᖅᖏᖑᖕᙱᖠ".chars()) {
            assert!(set.contains_key(&(c as u32)), "{c} (U+{:X}) has no glyph", c as u32);
        }
    }

    #[test]
    fn is_syllabic_matches_the_blocks() {
        assert!(is_syllabic('Ꭰ') && is_syllabic('ꭰ') && is_syllabic('ᐃ') && is_syllabic('ᖠ'));
        assert!(!is_syllabic('a') && !is_syllabic('日') && !is_syllabic('ð'));
        // Every glyph this module makes is one it claims.
        assert!(glyphs().keys().all(|c| is_syllabic(char::from_u32(*c).unwrap())));
    }

    #[test]
    fn the_font_maps_every_glyph_with_16_row_geometry_and_leaves_latin_alone() {
        let bytes = font_bytes();
        let face = ttf_parser::Face::parse(&bytes, 0).expect("the syllabics font parses");
        for cp in assigned() {
            assert!(face.glyph_index(char::from_u32(cp).unwrap()).is_some(), "U+{cp:X} is not in the font");
        }
        let g = face.glyph_index('ᐃ').unwrap();
        let bbox = face.glyph_bounding_box(g).unwrap();
        assert!(bbox.y_max <= (14 * crate::font_gen::PIXEL) as i16 && bbox.y_min >= -((2 * crate::font_gen::PIXEL) as i16), "{bbox:?}");
        for c in ['a', 'ð', 'þ', '日'] {
            assert!(face.glyph_index(c).is_none(), "{c} belongs to another font");
        }
    }
}
