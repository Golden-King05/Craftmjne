//! Vietnamese, in the game's own 12-row Latin style.
//!
//! Vietnamese stacks up to two marks on a vowel: a shape mark (circumflex, breve or horn) and a tone
//! (acute, grave, hook above, tilde or dot below). The Latin-1 letters it shares (á à ã â é è ê ì í ò ó
//! ô õ ù ú ý) were already here; this module composes the other 100 and draws đ/Đ.
//!
//! * A **lowercase** letter with a shape mark and a tone mark stacks them (tone over the shape).
//! * A **capital** has only two rows above it, so its shape mark and tone mark sit side by side, the
//!   tone to the right of the circumflex or breve - as Vietnamese typography sets them.
//! * The **horn** (ơ ư) is a small stroke at the top right of the letter.
//! * The **dot below** goes under the letter; on a letter with a descender (ỵ) it sits beside the tail on the free side,
//!   since the em has no room under it.

use crate::font_gen::GlyphSpec;
use crate::pixel_font::{accent, art_of, compose, parse, to_glyph, Art};

#[derive(Clone, Copy, Debug)]
enum Shape {
    Circ,
    Breve,
    Horn,
}

#[derive(Clone, Copy, Debug)]
enum Tone {
    Grave,
    Acute,
    Hook,
    Tilde,
    Dot,
}

impl Shape {
    fn mark(self) -> Option<char> {
        match self {
            Shape::Circ => Some('\u{302}'),
            Shape::Breve => Some('\u{306}'),
            Shape::Horn => None,
        }
    }
}

impl Tone {
    fn mark(self) -> Option<char> {
        match self {
            Tone::Grave => Some('\u{300}'),
            Tone::Acute => Some('\u{301}'),
            Tone::Hook => Some('\u{309}'),
            Tone::Tilde => Some('\u{303}'),
            Tone::Dot => None,
        }
    }
}

/// `(character, base letter, shape mark, tone mark)`.
const LETTERS: &[(char, char, Option<Shape>, Option<Tone>)] = &[
    ('Ă', 'A', Some(Shape::Breve), None),
    ('ă', 'a', Some(Shape::Breve), None),
    ('Ĩ', 'I', None, Some(Tone::Tilde)),
    ('ĩ', 'i', None, Some(Tone::Tilde)),
    ('Ũ', 'U', None, Some(Tone::Tilde)),
    ('ũ', 'u', None, Some(Tone::Tilde)),
    ('Ơ', 'O', Some(Shape::Horn), None),
    ('ơ', 'o', Some(Shape::Horn), None),
    ('Ư', 'U', Some(Shape::Horn), None),
    ('ư', 'u', Some(Shape::Horn), None),
    ('Ạ', 'A', None, Some(Tone::Dot)),
    ('ạ', 'a', None, Some(Tone::Dot)),
    ('Ả', 'A', None, Some(Tone::Hook)),
    ('ả', 'a', None, Some(Tone::Hook)),
    ('Ấ', 'A', Some(Shape::Circ), Some(Tone::Acute)),
    ('ấ', 'a', Some(Shape::Circ), Some(Tone::Acute)),
    ('Ầ', 'A', Some(Shape::Circ), Some(Tone::Grave)),
    ('ầ', 'a', Some(Shape::Circ), Some(Tone::Grave)),
    ('Ẩ', 'A', Some(Shape::Circ), Some(Tone::Hook)),
    ('ẩ', 'a', Some(Shape::Circ), Some(Tone::Hook)),
    ('Ẫ', 'A', Some(Shape::Circ), Some(Tone::Tilde)),
    ('ẫ', 'a', Some(Shape::Circ), Some(Tone::Tilde)),
    ('Ậ', 'A', Some(Shape::Circ), Some(Tone::Dot)),
    ('ậ', 'a', Some(Shape::Circ), Some(Tone::Dot)),
    ('Ắ', 'A', Some(Shape::Breve), Some(Tone::Acute)),
    ('ắ', 'a', Some(Shape::Breve), Some(Tone::Acute)),
    ('Ằ', 'A', Some(Shape::Breve), Some(Tone::Grave)),
    ('ằ', 'a', Some(Shape::Breve), Some(Tone::Grave)),
    ('Ẳ', 'A', Some(Shape::Breve), Some(Tone::Hook)),
    ('ẳ', 'a', Some(Shape::Breve), Some(Tone::Hook)),
    ('Ẵ', 'A', Some(Shape::Breve), Some(Tone::Tilde)),
    ('ẵ', 'a', Some(Shape::Breve), Some(Tone::Tilde)),
    ('Ặ', 'A', Some(Shape::Breve), Some(Tone::Dot)),
    ('ặ', 'a', Some(Shape::Breve), Some(Tone::Dot)),
    ('Ẹ', 'E', None, Some(Tone::Dot)),
    ('ẹ', 'e', None, Some(Tone::Dot)),
    ('Ẻ', 'E', None, Some(Tone::Hook)),
    ('ẻ', 'e', None, Some(Tone::Hook)),
    ('Ẽ', 'E', None, Some(Tone::Tilde)),
    ('ẽ', 'e', None, Some(Tone::Tilde)),
    ('Ế', 'E', Some(Shape::Circ), Some(Tone::Acute)),
    ('ế', 'e', Some(Shape::Circ), Some(Tone::Acute)),
    ('Ề', 'E', Some(Shape::Circ), Some(Tone::Grave)),
    ('ề', 'e', Some(Shape::Circ), Some(Tone::Grave)),
    ('Ể', 'E', Some(Shape::Circ), Some(Tone::Hook)),
    ('ể', 'e', Some(Shape::Circ), Some(Tone::Hook)),
    ('Ễ', 'E', Some(Shape::Circ), Some(Tone::Tilde)),
    ('ễ', 'e', Some(Shape::Circ), Some(Tone::Tilde)),
    ('Ệ', 'E', Some(Shape::Circ), Some(Tone::Dot)),
    ('ệ', 'e', Some(Shape::Circ), Some(Tone::Dot)),
    ('Ỉ', 'I', None, Some(Tone::Hook)),
    ('ỉ', 'i', None, Some(Tone::Hook)),
    ('Ị', 'I', None, Some(Tone::Dot)),
    ('ị', 'i', None, Some(Tone::Dot)),
    ('Ọ', 'O', None, Some(Tone::Dot)),
    ('ọ', 'o', None, Some(Tone::Dot)),
    ('Ỏ', 'O', None, Some(Tone::Hook)),
    ('ỏ', 'o', None, Some(Tone::Hook)),
    ('Ố', 'O', Some(Shape::Circ), Some(Tone::Acute)),
    ('ố', 'o', Some(Shape::Circ), Some(Tone::Acute)),
    ('Ồ', 'O', Some(Shape::Circ), Some(Tone::Grave)),
    ('ồ', 'o', Some(Shape::Circ), Some(Tone::Grave)),
    ('Ổ', 'O', Some(Shape::Circ), Some(Tone::Hook)),
    ('ổ', 'o', Some(Shape::Circ), Some(Tone::Hook)),
    ('Ỗ', 'O', Some(Shape::Circ), Some(Tone::Tilde)),
    ('ỗ', 'o', Some(Shape::Circ), Some(Tone::Tilde)),
    ('Ộ', 'O', Some(Shape::Circ), Some(Tone::Dot)),
    ('ộ', 'o', Some(Shape::Circ), Some(Tone::Dot)),
    ('Ớ', 'O', Some(Shape::Horn), Some(Tone::Acute)),
    ('ớ', 'o', Some(Shape::Horn), Some(Tone::Acute)),
    ('Ờ', 'O', Some(Shape::Horn), Some(Tone::Grave)),
    ('ờ', 'o', Some(Shape::Horn), Some(Tone::Grave)),
    ('Ở', 'O', Some(Shape::Horn), Some(Tone::Hook)),
    ('ở', 'o', Some(Shape::Horn), Some(Tone::Hook)),
    ('Ỡ', 'O', Some(Shape::Horn), Some(Tone::Tilde)),
    ('ỡ', 'o', Some(Shape::Horn), Some(Tone::Tilde)),
    ('Ợ', 'O', Some(Shape::Horn), Some(Tone::Dot)),
    ('ợ', 'o', Some(Shape::Horn), Some(Tone::Dot)),
    ('Ụ', 'U', None, Some(Tone::Dot)),
    ('ụ', 'u', None, Some(Tone::Dot)),
    ('Ủ', 'U', None, Some(Tone::Hook)),
    ('ủ', 'u', None, Some(Tone::Hook)),
    ('Ứ', 'U', Some(Shape::Horn), Some(Tone::Acute)),
    ('ứ', 'u', Some(Shape::Horn), Some(Tone::Acute)),
    ('Ừ', 'U', Some(Shape::Horn), Some(Tone::Grave)),
    ('ừ', 'u', Some(Shape::Horn), Some(Tone::Grave)),
    ('Ử', 'U', Some(Shape::Horn), Some(Tone::Hook)),
    ('ử', 'u', Some(Shape::Horn), Some(Tone::Hook)),
    ('Ữ', 'U', Some(Shape::Horn), Some(Tone::Tilde)),
    ('ữ', 'u', Some(Shape::Horn), Some(Tone::Tilde)),
    ('Ự', 'U', Some(Shape::Horn), Some(Tone::Dot)),
    ('ự', 'u', Some(Shape::Horn), Some(Tone::Dot)),
    ('Ỳ', 'Y', None, Some(Tone::Grave)),
    ('ỳ', 'y', None, Some(Tone::Grave)),
    ('Ỵ', 'Y', None, Some(Tone::Dot)),
    ('ỵ', 'y', None, Some(Tone::Dot)),
    ('Ỷ', 'Y', None, Some(Tone::Hook)),
    ('ỷ', 'y', None, Some(Tone::Hook)),
    ('Ỹ', 'Y', None, Some(Tone::Tilde)),
    ('ỹ', 'y', None, Some(Tone::Tilde)),
];

/// A stroke at the top right: the horn of ơ and ư.
fn horn(base: &Art) -> Art {
    let (top, width, rows) = base;
    let w = width + 1;
    let mut rows: Vec<Vec<bool>> = rows.iter().map(|r| {
        let mut r = r.clone();
        r.resize(w, false);
        r
    }).collect();
    rows[0][w - 2] = true;
    rows[0][w - 1] = true;
    if rows.len() > 1 {
        rows[1][w - 1] = true;
    }
    (*top, w, rows)
}

/// A dot under the letter (two pixels, on the last row of the em); beside the tail when the letter has one.
fn dot_below(base: &Art) -> Art {
    let (top, width, rows) = base;
    let mut rows = rows.clone();
    let bottom = *top + rows.len() as i32; // first row below the ink
    if bottom <= 10 {
        while *top + (rows.len() as i32) < 11 {
            rows.push(vec![false; *width]);
        }
        let mut dot = vec![false; *width];
        let x = (width / 2).saturating_sub(1);
        dot[x] = true;
        if x + 1 < *width {
            dot[x + 1] = true;
        }
        rows.push(dot);
    } else {
        // A descender already fills the last rows: put the dot on the free side of the tail, on row 11,
        // at the right edge (or the nearest free pixel to it).
        let idx = (11 - *top) as usize;
        if idx < rows.len() {
            if let Some(x) = (0..*width).rev().find(|&x| !rows[idx][x] && (x == 0 || !rows[idx][x - 1]) && (x + 1 >= *width || !rows[idx][x + 1])) {
                rows[idx][x] = true;
            } else if let Some(x) = (0..*width).rev().find(|&x| !rows[idx][x]) {
                rows[idx][x] = true;
            }
        }
    }
    (*top, *width, rows)
}

/// Two marks side by side in the two rows above a capital: `left` then `right`.
fn pair_above(base: &Art, left: char, right: char) -> Art {
    let (top, width, rows) = base;
    let (la, ra) = (accent(left), accent(right));
    let lw = la.iter().map(|r| r.len()).max().unwrap_or(0);
    let rw = ra.iter().map(|r| r.len()).max().unwrap_or(0);
    let w = (*width).max(lw + rw);
    let mut grid = vec![vec![false; w]; 2];
    for (set, offset) in [(&la, 0), (&ra, lw)] {
        for (i, line) in set.iter().take(2).enumerate() {
            for (x, ch) in line.chars().enumerate() {
                if ch == '#' {
                    grid[i][offset + x] = true;
                }
            }
        }
    }
    for r in rows {
        let mut r = r.clone();
        r.resize(w, false);
        grid.push(r);
    }
    (*top - 2, w, grid)
}

fn letter_art(base: char, shape: Option<Shape>, tone: Option<Tone>) -> Option<Art> {
    // A tone above an i needs the dotless one.
    let above = matches!(tone, Some(Tone::Grave | Tone::Acute | Tone::Hook | Tone::Tilde));
    let base = if base == 'i' && above { 'ı' } else { base };
    let mut art = art_of(base)?;
    let upper = base.is_uppercase();
    match (shape, tone.and_then(Tone::mark)) {
        (Some(Shape::Horn), t) => {
            art = horn(&art);
            if let Some(t) = t {
                art = compose(&art, t);
            }
        }
        (Some(s), Some(t)) => {
            let m = s.mark()?;
            art = if upper { pair_above(&art, m, t) } else { compose(&compose(&art, m), t) };
        }
        (Some(s), None) => art = compose(&art, s.mark()?),
        (None, Some(t)) => art = compose(&art, t),
        (None, None) => {}
    }
    if matches!(tone, Some(Tone::Dot)) {
        art = dot_below(&art);
    }
    Some(art)
}

/// Every Vietnamese glyph this module makes.
pub fn glyphs() -> Vec<GlyphSpec> {
    let mut out = Vec::new();
    for (c, base, shape, tone) in LETTERS {
        if let Some(art) = letter_art(*base, *shape, *tone) {
            out.push(to_glyph(*c, &art));
        }
    }
    // d with a bar through its stem; the capital is the eth (the same letter in print).
    out.push(to_glyph('đ', &parse(2, ".....#/...###/.###.#/#...##/#....#/#....#/#...##/.###.#")));
    if let Some(eth) = art_of('Ð') {
        out.push(to_glyph('Đ', &eth));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<GlyphSpec> {
        glyphs()
    }

    #[test]
    fn every_vietnamese_letter_is_there() {
        let have: std::collections::HashSet<char> = crate::pixel_font::glyphs().iter().flat_map(|g| g.chars.clone()).collect();
        let want = "ÀÁÂÃÈÉÊÌÍÒÓÔÕÙÚÝàáâãèéêìíòóôõùúýĂăĐđĨĩŨũƠơƯưẠạẢảẤấẦầẨẩẪẫẬậẮắẰằẲẳẴẵẶặẸẹẺẻẼẽẾếỀềỂểỄễỆệỈỉỊịỌọỎỏỐốỒồỔổỖỗỘộỚớỜờỞởỠỡỢợỤụỦủỨứỪừỬửỮữỰựỲỳỴỵỶỷỸỹ";
        for c in want.chars() {
            assert!(have.contains(&c), "missing Vietnamese {c} U+{:04X}", c as u32);
        }
        assert_eq!(want.chars().count(), 134);
    }

    fn ink(g: &GlyphSpec) -> Vec<(i32, i32, i32, i32)> {
        let mut r: Vec<_> = g.layers.iter().flat_map(|l| l.rects.iter().map(|r| (r.x0, r.y0, r.x1, r.y1))).collect();
        r.sort();
        r
    }

    #[test]
    fn no_two_vietnamese_letters_look_alike_and_none_looks_like_its_base() {
        let all = all();
        let latin = crate::pixel_font::glyphs();
        let mut seen: Vec<(char, Vec<(i32, i32, i32, i32)>)> = Vec::new();
        for g in &all {
            let i = ink(g);
            for (c, other) in &seen {
                assert_ne!(&i, other, "{:?} is drawn exactly like {c}", g.chars);
            }
            seen.push((g.chars[0], i));
        }
        // Each composed letter differs from the plain base it is built on.
        for (c, base, _, _) in LETTERS {
            let a = all.iter().find(|g| g.chars == [*c]).unwrap();
            if let Some(b) = latin.iter().find(|g| g.chars == [*base]) {
                assert_ne!(ink(a), ink(b), "{c} is drawn like {base}");
            }
        }
    }

    #[test]
    fn marks_fit_in_the_em_and_a_capital_keeps_both_marks() {
        for g in all() {
            for r in g.layers.iter().flat_map(|l| l.rects.iter()) {
                assert!(r.y0 >= 0 && r.y1 <= crate::font_gen::EM_ROWS, "{:?} leaves the cell: {r:?}", g.chars);
            }
        }
        // Ấ has a circumflex and an acute in rows 0-1 side by side, over a letter that starts at row 2.
        let a = all().into_iter().find(|g| g.chars == ['Ấ']).unwrap();
        let top = ink(&a).iter().map(|r| r.1).min().unwrap();
        assert_eq!(top, 0);
        let marks: Vec<_> = ink(&a).into_iter().filter(|r| r.1 < 2).collect();
        assert!(marks.len() >= 3, "{marks:?}");
        // ... while the lowercase stacks them.
        let l = all().into_iter().find(|g| g.chars == ['ấ']).unwrap();
        assert_eq!(ink(&l).iter().map(|r| r.1).min().unwrap(), 0);
    }

    #[test]
    fn the_dot_below_clears_the_letter_and_the_horn_reaches_right() {
        let dot = all().into_iter().find(|g| g.chars == ['ạ']).unwrap();
        let bottom = ink(&dot).iter().map(|r| r.3).max().unwrap();
        assert_eq!(bottom, 12, "the dot sits on the last row");
        let plain = crate::pixel_font::glyphs().into_iter().find(|g| g.chars == ['a']).unwrap();
        let plain_bottom = ink(&plain).iter().map(|r| r.3).max().unwrap();
        assert!(plain_bottom <= 10 && bottom - plain_bottom >= 2, "a row of space between letter and dot");
        let horn = all().into_iter().find(|g| g.chars == ['ơ']).unwrap();
        let o = crate::pixel_font::glyphs().into_iter().find(|g| g.chars == ['o']).unwrap();
        let width = |g: &GlyphSpec| ink(g).iter().map(|r| r.2).max().unwrap();
        assert!(width(&horn) > width(&o));
    }
}
