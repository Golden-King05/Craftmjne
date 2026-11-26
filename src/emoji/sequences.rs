//! Emoji that are built from others or from rules: skin-toned hands, keycaps and the zodiac.
//!
//! * **Skin tones.** A hand or gesture followed by one of the five Fitzpatrick modifiers
//!   (`U+1F3FB..=1F3FF`) is a ligature to a recoloured copy of the picture. The default (no
//!   modifier) keeps the game's own skin colour.
//! * **Keycaps.** `0`-`9`, `#` or `*`, optionally `U+FE0F`, then `U+20E3` is a key with that
//!   symbol on it. The digits themselves need a glyph in the emoji font for the ligature to match,
//!   so they are drawn too (plain, in case anything ever picks the emoji font for a digit).
//! * **Zodiac.** The twelve signs and Ophiuchus on purple tiles.

use super::{emoji, recolor, Emoji, Grid, SIZE};

/// The emoji that take a skin tone.
const TONEABLE: &[&str] = &[
    "thumbs_up", "thumbs_down", "victory", "fist", "wave", "clap", "pray", "muscle", "point_up", "raised_hand", "ok_hand",
    "point_right", "point_down", "point_left", "backhand_up",
];

/// The five modifier code points and the palette keys `(light, shadow)` of their skin.
const TONES: [(char, char, char); 5] = [
    ('\u{1F3FB}', '1', '2'),
    ('\u{1F3FC}', '3', '4'),
    ('\u{1F3FD}', '5', '6'),
    ('\u{1F3FE}', '7', '8'),
    ('\u{1F3FF}', '9', '0'),
];

/// Private-use code points that name these glyphs (plane 15, after the flags).
const BASE: u32 = 0xF1000;

fn named(name: &str, index: u32, grid: Grid, sequences: Vec<Vec<char>>) -> Emoji {
    let mut e = emoji(name, "", grid);
    e.chars = vec![char::from_u32(BASE + index).unwrap()];
    e.sequences = sequences;
    e
}

/// A tile with a symbol on it. `art` rows are `#` for the symbol, up to 8 rows of up to 8 columns,
/// centred.
fn tile(bg: char, fg: char, art: &[&str]) -> Grid {
    let mut g = [['.'; SIZE]; SIZE];
    for row in g.iter_mut().skip(1).take(10) {
        for c in row.iter_mut().skip(1).take(10) {
            *c = bg;
        }
    }
    let h = art.len();
    let w = art.iter().map(|r| r.len()).max().unwrap_or(0);
    let (ox, oy) = ((SIZE - w) / 2, (SIZE - h) / 2);
    for (y, row) in art.iter().enumerate() {
        for (x, ch) in row.chars().enumerate() {
            if ch == '#' {
                g[oy + y][ox + x] = fg;
            }
        }
    }
    g
}

fn glyph_art(c: char) -> Vec<String> {
    match crate::pixel_font::art_of(c) {
        Some((_, _, rows)) => rows.iter().map(|r| r.iter().map(|&on| if on { '#' } else { '.' }).collect()).collect(),
        None => Vec::new(),
    }
}

/// Zodiac symbols, 7 columns wide.
const ZODIAC: &[(&str, char, &[&str])] = &[
    ("aries", '\u{2648}', &[".#...#.", "#.#.#.#", "#..#..#", "...#...", "...#...", "...#...", "...#..."]),
    ("taurus", '\u{2649}', &["#.....#", ".#...#.", "..###..", ".#...#.", "#.....#", "#.....#", ".#...#.", "..###.."]),
    ("gemini", '\u{264A}', &["#######", ".#...#.", ".#...#.", ".#...#.", ".#...#.", ".#...#.", "#######"]),
    ("cancer", '\u{264B}', &["..###..", ".#...##", "#..#..#", "#.....#", "#..#..#", "##...#.", "..###.."]),
    ("leo", '\u{264C}', &["..##...", ".#..#..", ".#..#..", "..##.#.", "....#..", "...#...", "...##.."]),
    ("virgo", '\u{264D}', &["#.#.#..", "#.#.##.", "#.#.#.#", "#.#.#.#", "#.#.###", "....#.#", "....###"]),
    ("libra", '\u{264E}', &["..###..", ".#...#.", "#.....#", ".......", "#######", ".......", "#######"]),
    ("scorpio", '\u{264F}', &["#.#.#..", "#.#.#..", "#.#.#..", "#.#.#..", "#.#.#.#", "....###", ".....#."]),
    ("sagittarius", '\u{2650}', &["....###", "....#.#", "...#..#", "#.#....", ".#.....", "#.#....", "#......"]),
    ("capricorn", '\u{2651}', &["#......", "#.##...", "##..#..", "#...#.#", "....##.", "....#.#", ".....##"]),
    ("aquarius", '\u{2652}', &[".#.#.#.", "#.#.#.#", ".......", ".#.#.#.", "#.#.#.#"]),
    ("pisces", '\u{2653}', &["##...##", "..#.#..", "..#.#..", "#######", "..#.#..", "..#.#..", "##...##"]),
    ("ophiuchus", '\u{26CE}', &["#.....#", "#.#.#.#", "#.#.#.#", ".#####.", "..###..", "..#.#..", ".##.##."]),
];

/// The emoji built from `existing`.
pub fn all(existing: &[Emoji]) -> Vec<Emoji> {
    let mut out = Vec::new();
    let mut index = 0;
    let mut next = || {
        index += 1;
        index - 1
    };

    // Skin tones.
    for name in TONEABLE {
        let Some(base) = existing.iter().find(|e| e.name == *name) else { continue };
        let base_char = base.chars[0];
        for (n, (modifier, light, shadow)) in TONES.iter().enumerate() {
            let mut g = base.grid;
            recolor(&mut g, 't', *light);
            recolor(&mut g, 'T', *shadow);
            out.push(named(&format!("{name}_tone_{}", n + 1), next(), g, vec![vec![base_char, *modifier], vec![base_char, '\u{FE0F}', *modifier]]));
        }
    }

    // Keycaps, and the plain digits their ligatures start from.
    for c in "0123456789#*".chars() {
        let art = glyph_art(c);
        let rows: Vec<&str> = art.iter().map(String::as_str).collect();
        let label = match c {
            '#' => "hash".to_string(),
            '*' => "asterisk".to_string(),
            d => d.to_string(),
        };
        let mut plain = [['.'; SIZE]; SIZE];
        for (y, row) in rows.iter().enumerate() {
            for (x, ch) in row.chars().enumerate() {
                if ch == '#' && y + 2 < SIZE && x + 3 < SIZE {
                    plain[y + 2][x + 3] = 'k';
                }
            }
        }
        out.push(emoji(&format!("digit_{label}"), &c.to_string(), plain));
        out.push(named(&format!("keycap_{label}"), next(), tile('l', 'k', &rows), vec![vec![c, '\u{20E3}'], vec![c, '\u{FE0F}', '\u{20E3}']]));
    }
    // Keycap ten.
    {
        let one = glyph_art('1');
        let zero = glyph_art('0');
        let mut rows: Vec<String> = Vec::new();
        for (a, b) in one.iter().zip(zero.iter()) {
            let a = &a[..a.len().min(3)];
            rows.push(format!("{a}.{}", &b[..b.len().min(4)]));
        }
        let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
        let mut e = emoji("keycap_ten", "\u{1F51F}", tile('l', 'k', &rows));
        e.sequences = Vec::new();
        out.push(e);
    }

    // Zodiac.
    for (name, c, art) in ZODIAC {
        out.push(emoji(name, &c.to_string(), tile('P', 'w', art)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_toneable_emoji_exists_and_has_five_tones() {
        let set = super::super::all();
        for name in TONEABLE {
            assert!(set.iter().any(|e| e.name == *name), "{name} is not an emoji");
            for n in 1..=5 {
                let e = set.iter().find(|e| e.name == format!("{name}_tone_{n}")).unwrap_or_else(|| panic!("{name} tone {n}"));
                assert!(e.grid.iter().flatten().all(|c| *c != 't' && *c != 'T'), "{name} tone {n} still has the default skin");
            }
        }
    }

    #[test]
    fn the_five_tones_look_different() {
        let set = super::super::all();
        let a: Vec<_> = (1..=5).map(|n| set.iter().find(|e| e.name == format!("thumbs_up_tone_{n}")).unwrap().grid).collect();
        for i in 0..5 {
            for j in i + 1..5 {
                assert_ne!(a[i], a[j]);
            }
        }
    }

    #[test]
    fn there_are_keycaps_and_thirteen_signs() {
        let set = super::super::all();
        for c in "0123456789".chars().map(|c| c.to_string()).chain(["hash".to_string(), "asterisk".to_string()]) {
            assert!(set.iter().any(|e| e.name == format!("keycap_{c}")), "keycap {c}");
        }
        for (name, _, _) in ZODIAC {
            assert!(set.iter().any(|e| e.name == *name), "{name}");
        }
    }
}
