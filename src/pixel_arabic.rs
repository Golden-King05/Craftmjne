//! Arabic letters, with the joining forms a reader expects.
//!
//! An Arabic letter has up to four shapes: **isolated**, **initial** (joined to the letter after
//! it), **medial** (joined both sides) and **final** (joined to the letter before it). The text
//! engine picks them through the font's `GSUB` table (`init`, `medi`, `fina`), so this module draws
//! all four shapes of every letter and tells `font_gen` which substitutions to write.
//!
//! * The *isolated* shape is the glyph the letter's own code point maps to.
//! * The other shapes have no code point of their own, so each gets a private-use one
//!   (`PUA_BASE + (letter - U+0600) * 4 + form`), which is only a name for the substitution.
//! * Letters share *families* of bodies (ب ت ث ن ي are one body with different dots), and dots and
//!   hamza marks are added by rule, so a family is drawn once.
//! * The joining stroke is always on cell row 9, running to the glyph's edge on every side that
//!   joins, so neighbouring forms meet in a continuous line.
//! * Right-joining letters (ا د ذ ر ز و ...) have only isolated and final shapes.
//!
//! Not done: the lam-alef ligature (ل followed by ا is drawn as the two letters, joined), vowel
//! marks (tashkeel), Persian/Urdu letters, and Arabic-Indic digits.

use crate::font_gen::{rects_from_mask, GlyphSpec, Layer, Substitution};
use crate::pixel_font::art_of;

/// Where a letter's form-naming private-use code points start.
const PUA_BASE: u32 = 0xE000;
const ROWS: usize = 12;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Form {
    Initial = 0,
    Medial = 1,
    Final = 2,
    Isolated = 3,
}

/// Four drawings of one body: `(first cell row, rows)` for initial, medial, final, isolated.
struct Family {
    forms: [(i32, &'static str); 4],
}

/// Dots and marks added to a body.
#[derive(Clone, Copy)]
enum Marks {
    None,
    Above(u8),
    Below(u8),
    /// One dot inside the body's hollow (ج).
    Inside,
    /// A hamza above (ئ ؤ).
    Hamza,
}

const TOOTH: Family = Family {
    forms: [(7, "...#/...#/####"), (8, "..#./####"), (7, "#...../#...../######"), (7, "#....#/#....#/######")],
};
const JEEM: Family = Family {
    forms: [
        (6, "#####/#..../#..../#...."),
        (6, "#####/#..../#..../#####"),
        (6, "#####./#...../#...../######/#....."),
        (6, "#####/#..../#..../#####/..#.."),
    ],
};
const SEEN: Family = Family {
    forms: [
        (7, "#.#.#/#.#.#/#####"),
        (8, "#.#.#/#####"),
        (7, "..#.#.#/..#.#.#/#######/#......"),
        (7, "..#.#.#/..#.#.#/#######/#.....#"),
    ],
};
const SAD: Family = Family {
    forms: [
        (6, "..###/..#.#/..###/#####"),
        (6, ".###./.#.#./.###./#####"),
        (6, "....###/....#.#/....###/#######/#......"),
        (6, "....###/....#.#/....###/#######/#.....#"),
    ],
};
const TAH: Family = Family {
    forms: [
        (2, "...#./...#./...#./...#./...#./..###/..#.#/#####"),
        (2, "..#../..#../..#../..#../..#../.###./.#.#./#####"),
        (2, ".....#./.....#./.....#./.....#./.....#./....###/....#.#/#######/#......"),
        (2, ".....#./.....#./.....#./.....#./.....#./....###/....#.#/#######/#.....#"),
    ],
};
const AIN: Family = Family {
    forms: [
        (7, "..###/...#./#####"),
        (7, ".###./..#../#####"),
        (7, "....###/.....#./#######/#......"),
        (7, "....###/.....#./#######/#.....#"),
    ],
};
const FEH: Family = Family {
    forms: [(8, "..##/####"), (8, ".##./####"), (8, "...##./######/#....."), (8, "...##./######/#....#")],
};
const QAF: Family = Family {
    forms: [(8, "..##/####"), (8, ".##./####"), (8, "...##./######/#....#/.####."), (8, "...##./#....#/#....#/.####.")],
};
const KAF: Family = Family {
    forms: [
        (2, "...#./...#./..##./...#./...#./...#./...#./#####"),
        (2, "..#../..#../.##../..#../..#../..#../..#../#####"),
        (2, "....#./....#./...##./....#./....#./....#./....#./######/#....."),
        (2, "....#./....#./...##./....#./....#./....#./....#./#....#/.####."),
    ],
};
const LAM: Family = Family {
    forms: [
        (2, "...#./...#./...#./...#./...#./...#./...#./#####"),
        (2, "..#../..#../..#../..#../..#../..#../..#../#####"),
        (2, "....#./....#./....#./....#./....#./....#./....#./######/#....."),
        (2, ".....#/.....#/.....#/.....#/.....#/.....#/.....#/#....#/.####."),
    ],
};
const MEEM: Family = Family {
    forms: [
        (7, "..###/..#.#/#####"),
        (7, ".###./.#.#./#####"),
        (7, "...###/...#.#/######/..#..."),
        (7, ".####./.#..#./.####./..#..."),
    ],
};
const HEH: Family = Family {
    forms: [
        (6, ".###./.#.#./.###./#####"),
        (6, ".###./.#.#./.###./#####"),
        (6, "..###./..#.#./..###./######"),
        (6, ".##./#..#/#..#/.##."),
    ],
};
const TATWEEL: Family = Family { forms: [(9, "###"), (9, "###"), (9, "###"), (9, "###")] };

// Right-joining bodies: only the final and isolated forms are used.
const ALEF: Family = Family { forms: [(2, "#/#/#/#/#/#/#/#"), (2, "#/#/#/#/#/#/#/#"), (2, "#/#/#/#/#/#/#/#"), (2, "#/#/#/#/#/#/#/#")] };
const DAL: Family = Family { forms: [(7, "###./..#./####"), (7, "###./..#./####"), (7, "###./..#./####"), (7, "##./..#/##.")] };
const REH: Family = Family { forms: [(9, "....#/...#./..#.."), (9, "....#/...#./..#.."), (9, "....#/...#./..#.."), (7, "..#/..#/.#./#..")] };
const WAW: Family = Family {
    forms: [
        (7, "..##./.#..#/..###/....#."),
        (7, "..##./.#..#/..###/....#."),
        (7, "..##./.#..#/..###/....#."),
        (7, ".##./#..#/.###/...#/..#."),
    ],
};
const TEH_MARBUTA: Family = Family {
    forms: [(6, "..###./..#.#./..###./######"), (6, "..###./..#.#./..###./######"), (6, "..###./..#.#./..###./######"), (6, ".##./#..#/#..#/.##.")],
};
const HAMZA: Family = Family { forms: [(6, ".##/#../.#./#../.##"); 4] };

/// Letters whose forms are the four of a family, with the marks that tell them apart.
const DUAL: &[(char, &Family, Marks)] = &[
    ('ب', &TOOTH, Marks::Below(1)),
    ('ت', &TOOTH, Marks::Above(2)),
    ('ث', &TOOTH, Marks::Above(3)),
    ('ن', &TOOTH, Marks::Above(1)),
    ('ي', &TOOTH, Marks::Below(2)),
    ('ى', &TOOTH, Marks::None),
    ('ئ', &TOOTH, Marks::Hamza),
    ('ج', &JEEM, Marks::Inside),
    ('ح', &JEEM, Marks::None),
    ('خ', &JEEM, Marks::Above(1)),
    ('س', &SEEN, Marks::None),
    ('ش', &SEEN, Marks::Above(3)),
    ('ص', &SAD, Marks::None),
    ('ض', &SAD, Marks::Above(1)),
    ('ط', &TAH, Marks::None),
    ('ظ', &TAH, Marks::Above(1)),
    ('ع', &AIN, Marks::None),
    ('غ', &AIN, Marks::Above(1)),
    ('ف', &FEH, Marks::Above(1)),
    ('ق', &QAF, Marks::Above(2)),
    ('ك', &KAF, Marks::None),
    ('ل', &LAM, Marks::None),
    ('م', &MEEM, Marks::None),
    ('ه', &HEH, Marks::None),
    ('\u{640}', &TATWEEL, Marks::None),
];

/// Letters that join only to the letter before them (isolated and final forms).
const RIGHT: &[(char, &Family, Marks)] = &[
    ('ا', &ALEF, Marks::None),
    ('د', &DAL, Marks::None),
    ('ذ', &DAL, Marks::Above(1)),
    ('ر', &REH, Marks::None),
    ('ز', &REH, Marks::Above(1)),
    ('و', &WAW, Marks::None),
    ('ؤ', &WAW, Marks::Hamza),
    ('ة', &TEH_MARBUTA, Marks::Above(2)),
];

/// An alef with a mark: `(letter, first cell row, rows)`; the same drawing serves both its shapes.
const ALEF_MARKED: &[(char, i32, &str)] = &[
    ('أ', 0, "##./.#./.#./.#./.#./.#./.#./.#./.#./.#."),
    ('إ', 2, ".#./.#./.#./.#./.#./.#./.#./.#./##./.#."),
    ('آ', 0, "##./.##/.#./.#./.#./.#./.#./.#./.#./.#."),
];

/// The private-use code point that names `letter`'s `form`.
fn form_char(letter: char, form: Form) -> char {
    char::from_u32(PUA_BASE + (letter as u32 - 0x600) * 4 + form as u32).unwrap()
}

type Pixels = Vec<Vec<bool>>;

/// Draws a body (and its marks) into a 12-row grid, padded with a blank column on every side that
/// does not join.
fn render(top: i32, rows: &str, marks: Marks, form: Form) -> Pixels {
    let art: Vec<&str> = if rows.is_empty() { Vec::new() } else { rows.split('/').collect() };
    let width = art.iter().map(|r| r.len()).max().unwrap_or(0).max(1);
    let mut g: Pixels = vec![vec![false; width]; ROWS];
    let (mut min_x, mut max_x) = (width, 0);
    for (i, r) in art.iter().enumerate() {
        for (x, c) in r.chars().enumerate() {
            let y = top + i as i32;
            if c == '#' && (0..ROWS as i32).contains(&y) {
                g[y as usize][x] = true;
                min_x = min_x.min(x);
                max_x = max_x.max(x);
            }
        }
    }
    let cx = ((min_x + max_x) / 2) as i32;
    let dot = |g: &mut Pixels, x: i32, y: i32| {
        if (0..width as i32).contains(&x) && (0..ROWS as i32).contains(&y) {
            g[y as usize][x as usize] = true;
        }
    };
    match marks {
        Marks::None => {}
        Marks::Above(1) => dot(&mut g, cx, top - 2),
        Marks::Above(2) => {
            dot(&mut g, cx - 1, top - 2);
            dot(&mut g, cx + 1, top - 2);
        }
        Marks::Above(_) => {
            dot(&mut g, cx, top - 3);
            dot(&mut g, cx - 1, top - 2);
            dot(&mut g, cx + 1, top - 2);
        }
        Marks::Below(1) => dot(&mut g, cx, 11),
        Marks::Below(_) => {
            dot(&mut g, cx - 1, 11);
            dot(&mut g, cx + 1, 11);
        }
        Marks::Inside => dot(&mut g, cx, top + 2),
        Marks::Hamza => {
            for (dx, dy) in [(0, -3), (1, -3), (0, -2)] {
                dot(&mut g, cx + dx, top + dy);
            }
        }
    }
    // A blank column on each side that doesn't join: the left of a final/isolated form, the right of an
    // initial/isolated one.
    let pad_left = matches!(form, Form::Final | Form::Isolated);
    let pad_right = matches!(form, Form::Initial | Form::Isolated);
    for row in &mut g {
        if pad_left {
            row.insert(0, false);
        }
        if pad_right {
            row.push(false);
        }
    }
    g
}

fn glyph(chars: Vec<char>, px: &Pixels) -> GlyphSpec {
    let width = px[0].len();
    GlyphSpec { chars, advance: width as i32, colour: false, layers: vec![Layer { color: 0, rects: rects_from_mask(width, px, 0) }] }
}

/// Every Arabic glyph.
pub fn glyphs() -> Vec<GlyphSpec> {
    let mut out = Vec::new();
    for (letter, family, marks) in DUAL {
        for form in [Form::Initial, Form::Medial, Form::Final, Form::Isolated] {
            let (top, rows) = family.forms[form as usize];
            let px = render(top, rows, *marks, form);
            let chars = if form == Form::Isolated { vec![*letter] } else { vec![form_char(*letter, form)] };
            out.push(glyph(chars, &px));
        }
    }
    for (letter, family, marks) in RIGHT {
        for form in [Form::Final, Form::Isolated] {
            let (top, rows) = family.forms[form as usize];
            let px = render(top, rows, *marks, form);
            let chars = if form == Form::Isolated { vec![*letter] } else { vec![form_char(*letter, form)] };
            out.push(glyph(chars, &px));
        }
    }
    for (letter, top, rows) in ALEF_MARKED {
        for form in [Form::Final, Form::Isolated] {
            let px = render(*top, rows, Marks::None, form);
            let chars = if form == Form::Isolated { vec![*letter] } else { vec![form_char(*letter, form)] };
            out.push(glyph(chars, &px));
        }
    }
    // Hamza alone doesn't join.
    out.push(glyph(vec!['ء'], &render(HAMZA.forms[3].0, HAMZA.forms[3].1, Marks::None, Form::Isolated)));
    // Arabic punctuation: the comma, semicolon and question mark are the Latin ones (the question
    // mark turned around).
    for (c, latin, flip) in [('،', ',', false), ('؛', ';', false), ('؟', '?', true)] {
        if let Some((top, w, rows)) = art_of(latin) {
            let rows: Pixels = rows.iter().map(|r| if flip { r.iter().rev().copied().collect() } else { r.clone() }).collect();
            let mut px: Pixels = vec![vec![false; w]; ROWS];
            for (i, r) in rows.iter().enumerate() {
                px[(top as usize) + i] = r.clone();
            }
            for row in &mut px {
                row.push(false);
            }
            out.push(glyph(vec![c], &px));
        }
    }
    out
}

/// The `GSUB` substitutions that pick each letter's joining form.
pub fn substitutions() -> Vec<Substitution> {
    let mut out = Vec::new();
    let mut add = |letter: char, form: Form, feature: &[u8; 4]| {
        out.push(Substitution { script: *b"arab", feature: *feature, from: letter, to: form_char(letter, form) });
    };
    for (letter, _, _) in DUAL {
        add(*letter, Form::Initial, b"init");
        add(*letter, Form::Medial, b"medi");
        add(*letter, Form::Final, b"fina");
    }
    for (letter, _, _) in RIGHT {
        add(*letter, Form::Final, b"fina");
    }
    for (letter, _, _) in ALEF_MARKED {
        add(*letter, Form::Final, b"fina");
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

    /// Shapes `text` (right to left) and returns the glyph ids in logical order.
    fn shape(font: &[u8], text: &str) -> Vec<u32> {
        let face = rustybuzz::Face::from_slice(font, 0).expect("the font parses");
        let mut buf = rustybuzz::UnicodeBuffer::new();
        buf.push_str(text);
        buf.set_direction(rustybuzz::Direction::RightToLeft);
        buf.set_script(rustybuzz::script::ARABIC);
        let out = rustybuzz::shape(&face, &[], buf);
        let mut ids: Vec<u32> = out.glyph_infos().iter().map(|i| i.glyph_id).collect();
        ids.reverse(); // visual (left to right) -> logical
        ids
    }

    fn gid(font: &[u8], c: char) -> u32 {
        ttf_parser::Face::parse(font, 0).unwrap().glyph_index(c).unwrap_or_else(|| panic!("no glyph for U+{:04X}", c as u32)).0 as u32
    }

    #[test]
    fn the_whole_arabic_alphabet_is_drawn() {
        let have: HashSet<char> = glyphs().iter().flat_map(|g| g.chars.clone()).collect();
        for c in ('\u{621}'..='\u{63A}').chain('\u{641}'..='\u{64A}') {
            assert!(have.contains(&c), "missing Arabic U+{:04X}", c as u32);
        }
    }

    #[test]
    fn every_form_is_a_nonblank_glyph_inside_the_cell() {
        for g in glyphs() {
            assert!(!g.layers[0].rects.is_empty(), "{:?} is blank", g.chars);
            for r in &g.layers[0].rects {
                assert!(r.y0 >= 0 && r.y1 <= 12, "{:?} leaves the cell", g.chars);
            }
        }
    }

    #[test]
    fn form_code_points_do_not_collide() {
        let mut seen = HashSet::new();
        for g in glyphs() {
            for c in g.chars {
                assert!(seen.insert(c), "U+{:04X} is used twice", c as u32);
            }
        }
    }

    #[test]
    fn joined_letters_take_their_joining_forms() {
        let f = font();
        // بيت: beh, yeh, teh -> initial, medial, final.
        let ids = shape(&f, "بيت");
        assert_eq!(ids, vec![gid(&f, form_char('ب', Form::Initial)), gid(&f, form_char('ي', Form::Medial)), gid(&f, form_char('ت', Form::Final))]);
    }

    #[test]
    fn a_letter_on_its_own_is_isolated() {
        let f = font();
        assert_eq!(shape(&f, "ب"), vec![gid(&f, 'ب')]);
    }

    #[test]
    fn a_right_joining_letter_ends_the_connection() {
        let f = font();
        // باب: beh (initial), alef (final: joins the beh but not the next letter), beh (isolated again,
        // since alef doesn't join forward... so it starts a new group: isolated).
        let ids = shape(&f, "باب");
        assert_eq!(ids[0], gid(&f, form_char('ب', Form::Initial)));
        assert_eq!(ids[1], gid(&f, form_char('ا', Form::Final)));
        assert_eq!(ids[2], gid(&f, 'ب'));
    }

    #[test]
    fn joining_letters_meet_in_a_continuous_line() {
        // The joining stroke is on row 9 and must reach the edge on every joining side.
        for (letter, family, marks) in DUAL {
            for (form, joins_left, joins_right) in [(Form::Initial, true, false), (Form::Medial, true, true), (Form::Final, false, true)] {
                let (top, rows) = family.forms[form as usize];
                let px = render(top, rows, *marks, form);
                let w = px[0].len();
                if joins_left {
                    assert!(px[9][0], "{letter} {:?}: no joining stroke at the left edge", form as u8);
                }
                if joins_right {
                    assert!(px[9][w - 1], "{letter} {:?}: no joining stroke at the right edge", form as u8);
                }
            }
        }
    }
}
