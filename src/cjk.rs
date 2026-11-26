//! Chinese and Japanese text: kana, Bopomofo, CJK punctuation and the ideographs.
//!
//! The glyphs are GNU Unifont's 16x16 bitmaps (SIL OFL 1.1, see `third_party/unifont/`), kept in
//! three compact blobs made by `tools/build_cjk_data.py`:
//!
//! * `zh.bin` - the base set: the Simplified-leaning forms, all of the Basic Multilingual Plane's
//!   CJK (20,992 unified ideographs, Extension A, compatibility ideographs, kana, Bopomofo, symbols,
//!   fullwidth forms) plus the part of Extensions B-H that Unifont draws;
//! * `zh_hant.bin` - only the glyphs that differ in Traditional Chinese;
//! * `ja.bin` - only the glyphs that differ in Japanese (and extra rare kanji).
//!
//! The same code point is drawn differently by language (Han unification), so a [`Style`] picks which
//! overlay goes on the base; translations will set it from the player's language. [`font_bytes`]
//! turns a style into a real font file in memory with the same writer as the rest of the game's fonts.
//!
//! **Geometry.** A glyph is 16 pixels tall (its baseline 14 rows down, as in Unifont) and 16 wide
//! (8 for halfwidth forms), drawn at the same pixel size as the Latin font - so it is 4/3 the Latin
//! text's height and lands on whole screen pixels whenever the Latin does. That is taller than the
//! Latin line; a line of pure CJK text overlaps its neighbours by a pixel or two at size 12.
//!
//! **Not covered:** Unifont draws about 8,000 of the ~70,000 rare ideographs of Extensions B-I (names,
//! historical texts); everything in the Basic Multilingual Plane is covered. Hangul is the game's own
//! font. Language-specific glyph variants beyond the three styles (e.g. Hong Kong forms) are not made.

use std::collections::BTreeMap;
use std::io::Read;

use crate::font_gen::{build_ttf, rects_from_mask, FontSpec, GlyphSpec, Layer, BASELINE_ROW, EM_ROWS};

const ZH: &[u8] = include_bytes!("cjk/zh.bin");
const ZH_HANT: &[u8] = include_bytes!("cjk/zh_hant.bin");
const JA: &[u8] = include_bytes!("cjk/ja.bin");

/// Rows in a glyph, and how many of them are above the baseline.
const ROWS: usize = 16;
const ABOVE_BASELINE: i32 = 14;

/// Which regional forms the shared ideographs are drawn in.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, bevy::prelude::Resource)]
pub struct CjkStyle(pub Style);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum Style {
    /// Chinese as written in the mainland (the base set).
    #[default]
    Chinese,
    /// Chinese as written in Taiwan, Hong Kong and Macau.
    TraditionalChinese,
    Japanese,
}

impl Style {
    pub const ALL: [Style; 3] = [Style::Chinese, Style::TraditionalChinese, Style::Japanese];

    /// The language tag the style is for.
    pub fn id(self) -> &'static str {
        match self {
            Style::Chinese => "zh-Hans",
            Style::TraditionalChinese => "zh-Hant",
            Style::Japanese => "ja",
        }
    }

    /// The style for a language or locale tag (`ja`, `ja_JP.UTF-8`, `zh-TW`, `zh_Hant`, `zh-CN`...);
    /// `None` for a language this module does not draw.
    pub fn from_language(tag: &str) -> Option<Style> {
        let tag = tag.split(['.', '@']).next().unwrap_or("").replace('_', "-").to_lowercase();
        let mut parts = tag.split('-');
        match parts.next()? {
            "ja" => Some(Style::Japanese),
            "zh" => {
                let rest: Vec<&str> = parts.collect();
                if rest.iter().any(|p| matches!(*p, "hant" | "tw" | "hk" | "mo")) {
                    Some(Style::TraditionalChinese)
                } else {
                    Some(Style::Chinese)
                }
            }
            _ => None,
        }
    }
}

/// Whether `c` is drawn by this module's font (kana, Bopomofo, CJK symbols, ideographs, fullwidth forms).
pub fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x2E80..=0x9FFF | 0xF900..=0xFAFF | 0xFE10..=0xFE6F | 0xFF00..=0xFFEF | 0x1B000..=0x1B2FF | 0x1F200..=0x1F2FF | 0x20000..=0x3FFFF)
}

/// One glyph: 16 rows, each a bit mask with the leftmost pixel in the top bit of `width` pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Glyph {
    /// 8 or 16 pixels.
    pub width: usize,
    pub rows: [u16; ROWS],
}

impl Glyph {
    pub fn ink(&self, x: usize, y: usize) -> bool {
        x < self.width && (self.rows[y] >> (self.width - 1 - x)) & 1 == 1
    }
}

/// Reads one blob: a zlib stream of `u32 code point | u8 width/8 | rows` records.
fn decode(blob: &[u8]) -> BTreeMap<u32, Glyph> {
    let mut raw = Vec::new();
    flate2::read::ZlibDecoder::new(blob).read_to_end(&mut raw).expect("the CJK data is a zlib stream");
    let mut out = BTreeMap::new();
    let mut at = 0;
    while at + 5 <= raw.len() {
        let cp = u32::from_le_bytes([raw[at], raw[at + 1], raw[at + 2], raw[at + 3]]);
        let bytes = raw[at + 4] as usize;
        at += 5;
        let mut rows = [0u16; ROWS];
        for row in rows.iter_mut() {
            *row = if bytes == 1 { raw[at] as u16 } else { u16::from_be_bytes([raw[at], raw[at + 1]]) };
            at += bytes;
        }
        out.insert(cp, Glyph { width: bytes * 8, rows });
    }
    out
}

/// Every glyph of a style: the base set with the style's own forms laid over it.
pub fn glyphs(style: Style) -> BTreeMap<u32, Glyph> {
    let mut set = decode(ZH);
    let overlay = match style {
        Style::Chinese => return set,
        Style::TraditionalChinese => ZH_HANT,
        Style::Japanese => JA,
    };
    set.extend(decode(overlay));
    set
}

/// The font glyphs for a style.
pub fn glyph_specs(style: Style) -> Vec<GlyphSpec> {
    // Whatever the emoji font draws stays the emoji font's (a symbol both fonts know must not turn
    // from the colour picture into a plain one, nor split an emoji sequence between two fonts).
    let emoji: std::collections::HashSet<char> = crate::emoji::all().iter().flat_map(|e| e.chars.iter().copied()).collect();
    glyphs(style)
        .into_iter()
        .filter_map(|(cp, g)| {
            let c = char::from_u32(cp)?;
            if emoji.contains(&c) || crate::emoji::joiners().contains(&c) {
                return None;
            }
            let mask: Vec<Vec<bool>> = (0..ROWS).map(|y| (0..g.width).map(|x| g.ink(x, y)).collect()).collect();
            // Row 0 of the glyph sits `ABOVE_BASELINE` rows above the baseline.
            let rects = rects_from_mask(g.width, &mask, BASELINE_ROW - ABOVE_BASELINE);
            Some(GlyphSpec { chars: vec![c], advance: g.width as i32, layers: vec![Layer { color: 0, rects }], colour: false })
        })
        .collect()
}

/// The font for a style as file bytes ("Craftmjne CJK" - never "Unifont", which the licence reserves).
pub fn font_bytes(style: Style) -> Vec<u8> {
    build_ttf(&FontSpec {
        family: "Craftmjne CJK".into(),
        palette: vec![],
        glyphs: glyph_specs(style),
        em_rows: EM_ROWS,
        baseline_row: BASELINE_ROW,
        space_advance: 8,
        substitutions: vec![],
        ligatures: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The assigned ranges of the BMP that this module promises (Unicode 15+), as (first, last).
    const PROMISED: &[(u32, u32)] = &[
        (0x3041, 0x3096), // hiragana
        (0x309D, 0x309F),
        (0x30A1, 0x30FF), // katakana
        (0x3105, 0x312F), // bopomofo
        (0x31A0, 0x31BF), // bopomofo extended
        (0x31F0, 0x31FF), // katakana phonetic extensions
        (0x3400, 0x4DBF), // extension A
        (0x4E00, 0x9FFF), // unified ideographs
        (0xF900, 0xFA6D), // compatibility ideographs
        (0xFF01, 0xFF5E), // fullwidth ASCII variants
        (0xFF61, 0xFF9F), // halfwidth katakana and punctuation
    ];

    #[test]
    fn every_kana_bopomofo_and_bmp_ideograph_has_a_glyph_in_every_style() {
        for style in Style::ALL {
            let set = glyphs(style);
            let mut missing = Vec::new();
            for &(a, b) in PROMISED {
                for cp in a..=b {
                    if !set.contains_key(&cp) {
                        missing.push(cp);
                    }
                }
            }
            assert!(missing.is_empty(), "{style:?} lacks {} of the promised characters, e.g. {:X?}", missing.len(), &missing[..missing.len().min(6)]);
        }
    }

    #[test]
    fn cjk_punctuation_and_the_ideographic_space_exist() {
        let set = glyphs(Style::Japanese);
        for c in "、。〃々〆〇〈〉《》「」『』【】〒〓〔〕〜　".chars() {
            assert!(set.contains_key(&(c as u32)), "{c} (U+{:X}) has no glyph", c as u32);
        }
    }

    #[test]
    fn glyphs_are_eight_or_sixteen_wide_and_visible_ones_have_ink() {
        let set = glyphs(Style::Chinese);
        assert!(set.values().all(|g| g.width == 8 || g.width == 16));
        // A character with a shape has ink; only the spaces and fillers may be blank.
        let blank: Vec<u32> = set.iter().filter(|(_, g)| g.rows.iter().all(|r| *r == 0)).map(|(c, _)| *c).collect();
        assert!(blank.iter().all(|c| matches!(c, 0x3000 | 0xFFA0) || char::from_u32(*c).map_or(false, |c| c.is_whitespace())), "blank glyphs: {blank:X?}");
        let han = set[&0x4E00]; // 一: one horizontal stroke
        assert!(han.rows.iter().any(|r| r.count_ones() >= 12), "一 should be a long bar");
    }

    #[test]
    fn the_styles_really_draw_shared_characters_differently() {
        let zh = glyphs(Style::Chinese);
        let ja = glyphs(Style::Japanese);
        let hant = glyphs(Style::TraditionalChinese);
        let differs = |a: &BTreeMap<u32, Glyph>, b: &BTreeMap<u32, Glyph>| (0x4E00..=0x9FFF).filter(|c| a.get(c) != b.get(c)).count();
        assert!(differs(&zh, &ja) > 5_000, "Japanese forms differ on {} ideographs", differs(&zh, &ja));
        assert!(differs(&zh, &hant) > 5_000, "Traditional forms differ on {} ideographs", differs(&zh, &hant));
        // 直 is the textbook case (Japanese and Chinese stroke it differently).
        assert_ne!(zh[&0x76F4], ja[&0x76F4]);
    }

    #[test]
    fn symbols_japanese_and_chinese_text_uses_are_there() {
        let set = glyphs(Style::Japanese);
        for c in "①②③⑩※→←↑↓⇒≠≦≧∞∴∵─│┌┐└┘■□●○◎▲△▼▽◆◇★☆♪♯♭†‡…‥—“”‘’℃№Σπ".chars() {
            assert!(set.contains_key(&(c as u32)), "{c} (U+{:X}) has no glyph", c as u32);
        }
    }

    #[test]
    fn the_cjk_font_never_takes_over_an_emoji_or_a_joiner() {
        let bytes = font_bytes(Style::Chinese);
        let face = ttf_parser::Face::parse(&bytes, 0).unwrap();
        for j in crate::emoji::joiners() {
            assert!(face.glyph_index(j).is_none(), "U+{:X} must stay the emoji font's", j as u32);
        }
        for c in ['\u{2764}', '\u{2600}', '\u{2B50}', '\u{27A1}', '\u{2744}'] {
            assert!(face.glyph_index(c).is_none(), "{c} is an emoji");
        }
        // But a plain symbol that is no emoji is here.
        assert!(face.glyph_index('→').is_some() || face.glyph_index('★').is_some());
    }

    #[test]
    fn is_cjk_tells_han_and_kana_from_latin() {
        for c in "日本語ひらがなカタカナ中文，。ｱ".chars() {
            assert!(is_cjk(c), "{c}");
        }
        for c in "abcXYZ 012äé한😀".chars() {
            assert!(!is_cjk(c), "{c}");
        }
    }

    #[test]
    fn languages_map_to_styles() {
        assert_eq!(Style::from_language("ja"), Some(Style::Japanese));
        assert_eq!(Style::from_language("ja_JP.UTF-8"), Some(Style::Japanese));
        assert_eq!(Style::from_language("zh-CN"), Some(Style::Chinese));
        assert_eq!(Style::from_language("zh"), Some(Style::Chinese));
        assert_eq!(Style::from_language("zh_TW.UTF-8"), Some(Style::TraditionalChinese));
        assert_eq!(Style::from_language("zh-Hant-HK"), Some(Style::TraditionalChinese));
        assert_eq!(Style::from_language("en_US"), None);
        assert_eq!(Style::from_language(""), None);
    }

    #[test]
    fn a_style_builds_a_font_that_maps_its_characters_with_the_right_metrics() {
        let bytes = font_bytes(Style::Japanese);
        let face = ttf_parser::Face::parse(&bytes, 0).expect("the CJK font parses");
        let units = |px: i32| (px * crate::font_gen::PIXEL) as u16;
        let kanji = face.glyph_index('日').expect("日 is mapped");
        assert_eq!(face.glyph_hor_advance(kanji), Some(units(16)));
        let half = face.glyph_index('ｱ').expect("halfwidth katakana is mapped");
        assert_eq!(face.glyph_hor_advance(half), Some(units(8)));
        // Baseline-aligned: the glyph reaches 14 rows above it and 2 below.
        let bbox = face.glyph_bounding_box(kanji).unwrap();
        assert!(bbox.y_max <= (14 * crate::font_gen::PIXEL) as i16 && bbox.y_max >= (12 * crate::font_gen::PIXEL) as i16, "{bbox:?}");
        assert!(bbox.y_min >= -((2 * crate::font_gen::PIXEL) as i16), "{bbox:?}");
    }
}
