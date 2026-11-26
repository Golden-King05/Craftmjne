//! Flags: every country and territory flag (the 258 regional-indicator pairs), the England,
//! Scotland and Wales tag sequences, the flag-on-a-pole emoji, the rainbow, transgender and pirate
//! flags, and the Texas flag.
//!
//! A flag is a 12x8 picture built from a few drawing calls (`h("rwb")` is three horizontal bands,
//! `.d(..)` a disc, `.cross(..)` a Nordic cross ...), so the table below is a design per code and
//! not 258 hand-typed grids. At this size an emblem is a mark, not a coat of arms: the colours,
//! layout and the main charge (star, crescent, disc, cross, triangle) are what carry a flag.
//!
//! In text, a flag is two regional-indicator letters (`U` `S`). The font has a ligature that
//! swaps the pair for the flag glyph, which is named by a private-use code point
//! ([`FLAG_BASE`] + index); the regional-indicator letters themselves have drawings too, so a
//! pair with no flag (or a font without the ligature) shows as two letter tiles.

use super::{emoji, Emoji, Grid, SIZE};

const W: usize = 12;
const H: usize = 8;
/// Private-use code points that name the flag glyphs (plane 15).
pub const FLAG_BASE: u32 = 0xF0000;
/// `U+1F1E6` (regional indicator A).
const RI_A: u32 = 0x1F1E6;

#[derive(Clone)]
struct F([[char; W]; H]);

fn u(x: i32, y: i32) -> f32 {
    (x as f32 + 0.5) / W as f32 + (y as f32 + 0.5) / H as f32
}

impl F {
    fn empty() -> F {
        F([['.'; W]; H])
    }
    fn solid(c: char) -> F {
        F([[c; W]; H])
    }
    /// Horizontal bands of equal height, top to bottom.
    fn h(s: &str) -> F {
        let cs: Vec<char> = s.chars().collect();
        let mut f = F::empty();
        for y in 0..H {
            let band = (((y as f32 + 0.5) * cs.len() as f32) / H as f32) as usize;
            for x in 0..W {
                f.0[y][x] = cs[band.min(cs.len() - 1)];
            }
        }
        f
    }
    /// Vertical bands of equal width, left to right.
    fn v(s: &str) -> F {
        let cs: Vec<char> = s.chars().collect();
        let mut f = F::empty();
        for x in 0..W {
            let band = (((x as f32 + 0.5) * cs.len() as f32) / W as f32) as usize;
            for y in 0..H {
                f.0[y][x] = cs[band.min(cs.len() - 1)];
            }
        }
        f
    }
    fn px(mut self, x: i32, y: i32, c: char) -> F {
        if (0..W as i32).contains(&x) && (0..H as i32).contains(&y) {
            self.0[y as usize][x as usize] = c;
        }
        self
    }
    /// A rectangle, `x1`/`y1` exclusive.
    fn bx(mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: char) -> F {
        for y in y0..y1 {
            for x in x0..x1 {
                self = self.px(x, y, c);
            }
        }
        self
    }
    /// A disc centred on `(cx, cy)` (pixel centres are at +0.5).
    fn d(mut self, cx: f32, cy: f32, r: f32, c: char) -> F {
        for y in 0..H as i32 {
            for x in 0..W as i32 {
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                if dx * dx + dy * dy <= r * r {
                    self = self.px(x, y, c);
                }
            }
        }
        self
    }
    /// A plus-shaped star.
    fn star(self, x: i32, y: i32, c: char) -> F {
        self.px(x, y, c).px(x - 1, y, c).px(x + 1, y, c).px(x, y - 1, c).px(x, y + 1, c)
    }
    /// A one-pixel line.
    fn ln(mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: char) -> F {
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let (mut x, mut y, mut err) = (x0, y0, dx + dy);
        loop {
            self = self.px(x, y, c);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
        self
    }
    /// A triangle on the hoist (left) edge, `w` columns deep at its tip.
    fn tri(mut self, w: i32, c: char) -> F {
        for y in 0..H as i32 {
            let half = H as f32 / 2.0;
            let reach = w as f32 * (1.0 - ((y as f32 + 0.5 - half).abs() / half));
            for x in 0..reach.round() as i32 {
                self = self.px(x, y, c);
            }
        }
        self
    }
    /// A cross: a vertical bar `t` wide at column `x` and a horizontal one `t` tall at row `y`.
    fn cross(self, x: i32, y: i32, t: i32, c: char) -> F {
        self.bx(x, 0, x + t, H as i32, c).bx(0, y, W as i32, y + t, c)
    }
    fn paste(mut self, sub: &[Vec<char>], ox: i32, oy: i32) -> F {
        for (y, row) in sub.iter().enumerate() {
            for (x, c) in row.iter().enumerate() {
                self = self.px(ox + x as i32, oy + y as i32, *c);
            }
        }
        self
    }
    /// Paints every pixel the function gives a colour for.
    fn fill(mut self, f: impl Fn(i32, i32) -> Option<char>) -> F {
        for y in 0..H as i32 {
            for x in 0..W as i32 {
                if let Some(c) = f(x, y) {
                    self = self.px(x, y, c);
                }
            }
        }
        self
    }
    /// `n` points on an ellipse.
    fn ring(mut self, cx: f32, cy: f32, rx: f32, ry: f32, n: i32, c: char) -> F {
        for k in 0..n {
            let a = k as f32 / n as f32 * std::f32::consts::TAU;
            self = self.px((cx + rx * a.cos()).round() as i32, (cy + ry * a.sin()).round() as i32, c);
        }
        self
    }
}

/// The Union Jack at `w` x `h`.
fn uj(w: usize, h: usize) -> Vec<Vec<char>> {
    let mut g = vec![vec!['B'; w]; h];
    let mut set = |g: &mut Vec<Vec<char>>, x: i32, y: i32, c: char| {
        if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
            g[y as usize][x as usize] = c;
        }
    };
    for y in 0..h as i32 {
        let x = ((y as f32 + 0.5) * w as f32 / h as f32) as i32;
        for dx in -1..=0 {
            set(&mut g, x + dx, y, 'w');
            set(&mut g, w as i32 - 1 - (x + dx), y, 'w');
        }
    }
    let (rw, rh) = (if w >= 10 { 2 } else { 1 }, if h >= 6 { 2 } else { 1 });
    let (vx, vy) = ((w as i32 - rw) / 2, (h as i32 - rh) / 2);
    for y in 0..h as i32 {
        for x in vx - 1..vx + rw + 1 {
            set(&mut g, x, y, 'w');
        }
    }
    for x in 0..w as i32 {
        for y in vy - 1..vy + rh + 1 {
            set(&mut g, x, y, 'w');
        }
    }
    for y in 0..h as i32 {
        for x in vx..vx + rw {
            set(&mut g, x, y, 'r');
        }
    }
    for x in 0..w as i32 {
        for y in vy..vy + rh {
            set(&mut g, x, y, 'r');
        }
    }
    g
}

/// A blue ensign: the Union Jack in the corner of a dark blue field.
fn ens() -> F {
    F::solid('B').paste(&uj(6, 4), 0, 0)
}

/// Four triangles meeting in the middle (top/bottom `a`, left/right `b`), optionally with a saltire.
fn quad(a: char, b: char, saltire: Option<char>) -> F {
    F::empty().fill(|x, y| {
        let (dx, dy) = ((x as f32 + 0.5 - 6.0) / 6.0, (y as f32 + 0.5 - 4.0) / 4.0);
        if let Some(s) = saltire {
            if (dx.abs() - dy.abs()).abs() < 0.2 {
                return Some(s);
            }
        }
        Some(if dx.abs() < dy.abs() { a } else { b })
    })
}

/// A diagonal split from the bottom hoist to the top fly: `a` above it, `b` below, optional
/// bands between (`(width, colour)` in `u` units, widest last).
fn diag(a: char, bands: &[(f32, char)], b: char) -> F {
    F::empty().fill(|x, y| {
        let t = u(x, y);
        let mut edge = 1.0 - bands.iter().map(|(w, _)| w).sum::<f32>() / 2.0;
        if t < edge {
            return Some(a);
        }
        for (w, c) in bands {
            edge += w;
            if t < edge {
                return Some(*c);
            }
        }
        Some(b)
    })
}

fn us() -> F {
    F::h("rwrwrwrw").bx(0, 0, 5, 4, 'B').px(1, 1, 'w').px(3, 1, 'w').px(2, 2, 'w').px(1, 3, 'w').px(3, 3, 'w')
}

fn au() -> F {
    ens().star(3, 6, 'w').px(8, 1, 'w').px(10, 3, 'w').px(8, 5, 'w').px(9, 4, 'w')
}

fn nordic(bg: char, outer: char, inner: Option<char>) -> F {
    let f = F::solid(bg);
    match inner {
        Some(i) => f.cross(3, 2, 3, outer).cross(4, 3, 1, i),
        None => f.cross(3, 3, 2, outer),
    }
}

fn french() -> F {
    F::v("bwr")
}

/// A flag with a blue ensign and a shield (the British territories).
fn ensign_shield(top: char, bottom: char) -> F {
    ens().bx(8, 2, 11, 6, 'w').bx(8, 2, 11, 4, top).bx(8, 4, 11, 6, bottom)
}

/// The design for a two-letter code.
fn design(code: &str) -> Option<F> {
    Some(match code {
        "AC" => ensign_shield('c', 'y'),
        "SH" => ensign_shield('y', 'c'),
        "TA" => ensign_shield('w', 'n'),
        "AD" => F::v("byr").px(6, 3, 'y').px(6, 4, 'R'),
        "AE" => F::h("gwk").bx(0, 0, 3, 8, 'r'),
        "AF" => F::v("krg").d(6.0, 4.0, 1.5, 'w'),
        "AG" => F::solid('r').fill(|x, y| (y < 5 && x >= y && x < 12 - y).then_some('k')).px(5, 1, 'y').px(6, 1, 'y').px(5, 3, 'b').px(6, 3, 'b').px(5, 4, 'w').px(6, 4, 'w'),
        "AI" => ens().bx(8, 2, 11, 6, 'w').bx(8, 4, 11, 6, 'c'),
        "AL" => F::solid('r').d(6.0, 4.0, 2.0, 'k'),
        "AM" => F::h("rbo"),
        "AO" => F::h("rk").d(6.0, 4.0, 1.6, 'y'),
        "AQ" => F::solid('b').d(6.0, 4.0, 2.8, 'w'),
        "AR" => F::h("cwc").d(6.0, 4.0, 1.0, 'y'),
        "AS" => F::solid('B').fill(|x, y| {
            let reach = 9.0 * (1.0 - ((y as f32 + 0.5 - 4.0).abs() / 4.0));
            let from_right = 11 - x;
            if (from_right as f32) < reach.round() - 2.0 {
                Some('w')
            } else if (from_right as f32) < reach.round() {
                Some('r')
            } else {
                None
            }
        }),
        "AT" => F::h("rwr"),
        "AU" | "HM" => au(),
        "AW" => F::solid('c').bx(0, 5, 12, 6, 'y').bx(0, 7, 12, 8, 'y').star(2, 2, 'r'),
        "AX" => nordic('b', 'y', Some('r')),
        "AZ" => F::h("brg").d(5.5, 4.0, 1.4, 'w').d(6.2, 4.0, 1.1, 'r'),
        "BA" => F::solid('B').fill(|x, y| (x >= 3 && x < 10 - y).then_some('y')).px(9, 0, 'w').px(8, 1, 'w').px(7, 2, 'w').px(6, 3, 'w'),
        "BB" => F::v("bYb").bx(5, 2, 7, 6, 'k'),
        "BD" => F::solid('G').d(5.0, 4.0, 2.6, 'r'),
        "BE" => F::v("kyr"),
        "BF" => F::h("rg").star(6, 4, 'y'),
        "BG" => F::h("wgr"),
        "BH" => F::solid('r').bx(0, 0, 3, 8, 'w').px(3, 1, 'w').px(3, 3, 'w').px(3, 5, 'w').px(3, 7, 'w'),
        "BI" => quad('r', 'G', Some('w')).d(6.0, 4.0, 1.6, 'w'),
        "BJ" => F::h("yr").bx(0, 0, 5, 8, 'g'),
        "BL" | "MF" | "GP" | "MQ" | "GF" | "RE" | "YT" | "PM" | "CP" => french(),
        "BM" => ens().bx(8, 2, 11, 5, 'r').bx(8, 5, 11, 6, 'w'),
        "BN" => diag('y', &[(0.15, 'w'), (0.2, 'k')], 'y').d(6.0, 4.0, 1.3, 'r'),
        "BO" => F::h("ryg"),
        "BQ" | "NL" => F::h("rwb"),
        "BR" => F::solid('G').fill(|x, y| (((x as f32 + 0.5 - 6.0).abs() / 5.5 + (y as f32 + 0.5 - 4.0).abs() / 3.5) < 1.0).then_some('y')).d(6.0, 4.0, 1.7, 'B'),
        "BS" => F::h("cyc").tri(5, 'k'),
        "BT" => diag('y', &[], 'o').d(6.0, 4.0, 1.3, 'w'),
        "BV" | "SJ" | "NO" => nordic('r', 'w', Some('B')),
        "BW" => F::solid('c').bx(0, 2, 12, 6, 'w').bx(0, 3, 12, 5, 'k'),
        "BY" => F::solid('r').bx(0, 5, 12, 8, 'g').bx(0, 0, 2, 8, 'w').px(0, 1, 'r').px(1, 3, 'r').px(0, 5, 'r'),
        "BZ" => F::solid('B').bx(0, 0, 12, 1, 'r').bx(0, 7, 12, 8, 'r').d(6.0, 4.0, 2.6, 'w').d(6.0, 4.0, 1.6, 'g'),
        "CA" => F::solid('r').bx(3, 0, 9, 8, 'w').px(6, 2, 'r').bx(5, 3, 8, 5, 'r').px(6, 5, 'r'),
        "CC" => F::solid('G').d(3.0, 3.0, 1.6, 'y').px(9, 2, 'y').px(10, 4, 'y').px(8, 5, 'y'),
        "CD" => diag('c', &[(0.2, 'y'), (0.25, 'r'), (0.2, 'y')], 'c').px(1, 1, 'y'),
        "CF" => F::h("bwgy").bx(5, 0, 7, 8, 'r').px(1, 1, 'y'),
        "CG" => diag('g', &[(0.3, 'y')], 'r'),
        "CH" => F::solid('r').bx(5, 1, 7, 7, 'w').bx(3, 3, 9, 5, 'w'),
        "CI" => F::v("owg"),
        "CK" => F::solid('B').paste(&uj(6, 4), 0, 0).ring(8.5, 5.5, 2.0, 1.5, 8, 'w'),
        "CL" => F::h("wr").bx(0, 0, 4, 4, 'B').px(2, 2, 'w').px(1, 2, 'w').px(2, 1, 'w'),
        "CM" => F::v("gry").px(6, 4, 'y').px(6, 3, 'y').px(5, 4, 'y').px(7, 4, 'y').px(6, 5, 'y'),
        "CN" => F::solid('r').star(2, 2, 'y').px(5, 1, 'y').px(6, 2, 'y').px(6, 4, 'y').px(5, 5, 'y'),
        "CO" => F::solid('y').bx(0, 4, 12, 6, 'B').bx(0, 6, 12, 8, 'r'),
        "CR" => F::solid('B').bx(0, 1, 12, 2, 'w').bx(0, 2, 12, 6, 'r').bx(0, 6, 12, 7, 'w'),
        "CU" => F::h("BwBwB").tri(5, 'r').px(1, 4, 'w'),
        "CV" => F::solid('B').bx(0, 4, 12, 5, 'w').bx(0, 5, 12, 6, 'r').bx(0, 6, 12, 7, 'w').ring(6.0, 4.0, 3.0, 2.2, 10, 'y'),
        "CW" => F::solid('B').bx(0, 5, 12, 6, 'y').px(2, 2, 'w').px(3, 3, 'w'),
        "CX" => F::solid('G').fill(|x, y| (u(x, y) >= 1.0).then_some('B')).d(6.0, 4.0, 1.3, 'y'),
        "CY" => F::solid('w').d(6.0, 3.5, 2.5, 'o').bx(4, 6, 8, 7, 'g'),
        "CZ" => F::h("wr").tri(7, 'b'),
        "DE" => F::h("kry"),
        "DG" | "IO" => F::h("wBwBwBwB").paste(&uj(6, 4), 0, 0),
        "DJ" => F::h("cg").tri(6, 'w').px(1, 4, 'r'),
        "DK" => nordic('r', 'w', None),
        "DM" => F::solid('G').bx(4, 0, 5, 8, 'y').bx(5, 0, 6, 8, 'k').bx(6, 0, 7, 8, 'w').bx(0, 2, 12, 3, 'y').bx(0, 3, 12, 4, 'k').bx(0, 4, 12, 5, 'w').d(5.5, 3.5, 1.5, 'r'),
        "DO" => F::empty().fill(|x, y| Some(if (x < 6) == (y < 4) { 'B' } else { 'r' })).bx(5, 0, 7, 8, 'w').bx(0, 3, 12, 5, 'w'),
        "DZ" => F::v("gw").d(6.0, 4.0, 2.0, 'r').d(6.8, 4.0, 1.6, 'w').px(7, 4, 'r'),
        "EA" | "ES" => F::solid('r').bx(0, 2, 12, 6, 'y').px(3, 3, 'R'),
        "EC" => F::solid('y').bx(0, 4, 12, 6, 'B').bx(0, 6, 12, 8, 'r').d(6.0, 4.0, 1.2, 'c'),
        "EE" => F::h("bkw"),
        "EG" => F::h("rwk").d(6.0, 4.0, 0.9, 'y'),
        "EH" => F::h("kwg").tri(4, 'r').px(8, 4, 'r').px(9, 3, 'r'),
        "ER" => F::h("gb").tri(12, 'r').px(3, 4, 'y'),
        "ET" => F::h("gyr").d(6.0, 4.0, 1.8, 'b').px(6, 4, 'y'),
        "EU" => F::solid('B').ring(6.0, 4.0, 3.0, 2.5, 12, 'y'),
        "FI" => nordic('w', 'b', None),
        "FJ" => F::solid('c').paste(&uj(6, 4), 0, 0).bx(8, 2, 11, 6, 'w').bx(8, 2, 11, 3, 'r'),
        "FK" => ensign_shield('g', 'w'),
        "GS" => ensign_shield('n', 'y'),
        "MS" => ensign_shield('g', 'c'),
        "PN" => ensign_shield('c', 'g'),
        "TC" => ensign_shield('y', 'g'),
        "VG" => ensign_shield('g', 'y'),
        "FM" => F::solid('c').px(6, 1, 'w').px(3, 4, 'w').px(9, 4, 'w').px(6, 7, 'w'),
        "FO" => nordic('w', 'b', Some('r')),
        "FR" => french(),
        "GA" => F::h("gyb"),
        "GB" => F::empty().paste(&uj(12, 8), 0, 0),
        "GD" => F::empty().fill(|x, y| {
            if x == 0 || y == 0 || x == 11 || y == 7 {
                return Some('r');
            }
            let (dx, dy) = ((x as f32 + 0.5 - 6.0) / 6.0, (y as f32 + 0.5 - 4.0) / 4.0);
            Some(if dx.abs() < dy.abs() { 'y' } else { 'g' })
        }).d(6.0, 4.0, 1.3, 'r').px(6, 4, 'y'),
        "GE" => F::solid('w').bx(5, 0, 7, 8, 'r').bx(0, 3, 12, 5, 'r').px(2, 1, 'r').px(9, 1, 'r').px(2, 6, 'r').px(9, 6, 'r'),
        "GG" => F::solid('w').cross(5, 3, 2, 'r'),
        "GH" => F::h("ryg").px(6, 4, 'k'),
        "GI" => F::solid('w').bx(0, 5, 12, 8, 'r').bx(5, 2, 8, 6, 'r'),
        "GL" => F::h("wr").fill(|x, y| {
            let (dx, dy) = (x as f32 + 0.5 - 4.5, y as f32 + 0.5 - 4.0);
            (dx * dx + dy * dy < 2.4 * 2.4).then_some(if y < 4 { 'r' } else { 'w' })
        }),
        "GM" => F::solid('r').bx(0, 2, 12, 3, 'w').bx(0, 3, 12, 5, 'b').bx(0, 5, 12, 6, 'w').bx(0, 6, 12, 8, 'g'),
        "GN" => F::v("ryg"),
        "ML" => F::v("gyr"),
        "GQ" => F::h("gwr").tri(3, 'b'),
        "GR" => F::h("bwbwbwbwb").bx(0, 0, 5, 4, 'b').bx(2, 0, 3, 4, 'w').bx(0, 1, 5, 2, 'w'),
        "GT" => F::v("cwc").px(6, 4, 'g'),
        "GU" => F::solid('r').bx(1, 1, 11, 7, 'B').d(6.0, 4.0, 2.0, 'c'),
        "GW" => F::h("yg").bx(0, 0, 4, 8, 'r').px(2, 4, 'k'),
        "GY" => F::solid('G').tri(12, 'w').tri(11, 'y').tri(5, 'k').tri(4, 'r'),
        "HK" => F::solid('r').d(6.0, 4.0, 2.0, 'w').px(6, 4, 'r'),
        "HN" => F::h("bwb").px(4, 4, 'B').px(6, 4, 'B').px(8, 4, 'B'),
        "HR" => F::h("rwb").bx(5, 2, 7, 6, 'r').px(5, 3, 'w').px(6, 2, 'w').px(5, 5, 'w').px(6, 4, 'w'),
        "HT" => F::h("Br").bx(4, 2, 8, 6, 'w').px(6, 4, 'G'),
        "HU" => F::h("rwg"),
        "IC" => F::v("wby"),
        "ID" => F::h("rw"),
        "IE" => F::v("gwo"),
        "IL" => F::solid('w').bx(0, 1, 12, 2, 'b').bx(0, 6, 12, 7, 'b').star(6, 4, 'b'),
        "IM" => F::solid('r').d(6.0, 4.0, 1.8, 'w'),
        "IN" => F::h("owg").star(6, 4, 'B'),
        "IQ" => F::h("rwk").bx(4, 3, 8, 5, 'g'),
        "IR" => F::h("gwr").d(6.0, 4.0, 0.9, 'r'),
        "IS" => nordic('b', 'w', Some('r')),
        "IT" => F::v("gwr"),
        "JE" => F::solid('w').fill(|x, y| {
            let (a, b) = (y as f32 + 0.5, x as f32 * 8.0 / 12.0 + 0.33);
            ((a - b).abs() < 0.9 || (a - (8.0 - b)).abs() < 0.9).then_some('r')
        }).px(6, 1, 'y'),
        "JM" => quad('g', 'k', Some('y')),
        "JO" => F::h("kwg").tri(6, 'r').px(2, 4, 'w'),
        "JP" => F::solid('w').d(6.0, 4.0, 2.2, 'r'),
        "KE" => F::solid('k').bx(0, 2, 12, 3, 'w').bx(0, 3, 12, 5, 'r').bx(0, 5, 12, 6, 'w').bx(0, 6, 12, 8, 'g').bx(5, 2, 7, 6, 'R'),
        "KG" => F::solid('r').d(6.0, 4.0, 1.8, 'y').px(6, 4, 'r'),
        "KH" => F::solid('B').bx(0, 2, 12, 6, 'r').bx(4, 3, 8, 5, 'w'),
        "KI" => F::solid('r').bx(0, 5, 12, 8, 'b').bx(0, 5, 12, 6, 'w').bx(0, 7, 12, 8, 'w').d(6.0, 3.5, 1.5, 'y'),
        "KM" => F::h("ywrb").tri(5, 'g'),
        "KN" => diag('g', &[(0.1, 'y'), (0.2, 'k'), (0.1, 'y')], 'r'),
        "KP" => F::solid('B').bx(0, 1, 12, 2, 'w').bx(0, 2, 12, 6, 'r').bx(0, 6, 12, 7, 'w').d(4.0, 4.0, 1.6, 'w').px(4, 4, 'r'),
        "KR" => F::solid('w').fill(|x, y| {
            let (dx, dy) = (x as f32 + 0.5 - 6.0, y as f32 + 0.5 - 4.0);
            (dx * dx + dy * dy < 4.0).then_some(if y < 4 { 'r' } else { 'b' })
        }).px(1, 1, 'k').px(10, 1, 'k').px(1, 6, 'k').px(10, 6, 'k'),
        "KW" => F::h("gwr").bx(0, 0, 2, 8, 'k').tri(5, 'k'),
        "KY" => ens().bx(8, 2, 11, 4, 'r').bx(8, 4, 11, 6, 'b'),
        "KZ" => F::solid('c').d(6.0, 3.0, 1.6, 'y').bx(3, 6, 9, 7, 'y'),
        "LA" => F::solid('r').bx(0, 2, 12, 6, 'B').d(6.0, 4.0, 1.5, 'w'),
        "LB" => F::solid('r').bx(0, 2, 12, 6, 'w').star(6, 4, 'g'),
        "LC" => F::solid('c').fill(|x, y| {
            let dist = (x as f32 + 0.5 - 6.0).abs();
            ((1..=6).contains(&y) && dist < (y - 1) as f32 * 0.8 + 0.4).then_some(if y >= 4 { 'y' } else { 'k' })
        }),
        "LI" => F::h("br").bx(2, 1, 4, 2, 'y').px(3, 2, 'y'),
        "LK" => F::solid('y').bx(1, 1, 11, 7, 'R').bx(1, 1, 2, 7, 'G').bx(2, 1, 3, 7, 'o'),
        "LR" => F::h("rwrwrwrwrwr").bx(0, 0, 4, 4, 'B').px(2, 2, 'w').px(1, 2, 'w').px(2, 1, 'w'),
        "LS" => F::h("bwg").px(6, 4, 'k'),
        "LT" => F::h("ygr"),
        "LU" => F::h("rwc"),
        "LV" => F::solid('R').bx(0, 3, 12, 5, 'w'),
        "LY" => F::solid('r').bx(0, 2, 12, 6, 'k').bx(0, 6, 12, 8, 'g').d(6.0, 4.0, 1.2, 'w'),
        "MA" => F::solid('r').px(6, 3, 'G').px(5, 4, 'G').px(6, 4, 'G').px(7, 4, 'G').px(5, 5, 'G').px(7, 5, 'G'),
        "MC" => F::h("rw"),
        "MD" => F::v("byr").px(6, 4, 'n'),
        "ME" => F::solid('y').bx(1, 1, 11, 7, 'R').px(6, 4, 'y').px(5, 4, 'y').px(7, 4, 'y'),
        "MG" => F::solid('r').bx(4, 4, 12, 8, 'g').bx(0, 0, 4, 8, 'w'),
        "MH" => F::solid('B').ln(0, 7, 11, 3, 'o').ln(0, 7, 11, 4, 'w').star(2, 2, 'w'),
        "MK" => F::solid('r').bx(5, 0, 6, 8, 'y').bx(0, 3, 12, 4, 'y').ln(6, 4, 0, 0, 'y').ln(6, 4, 11, 0, 'y').ln(6, 4, 0, 7, 'y').ln(6, 4, 11, 7, 'y').d(6.0, 4.0, 1.5, 'y'),
        "MM" => F::h("ygr").star(6, 4, 'w'),
        "MN" => F::v("rBr").bx(1, 2, 2, 6, 'y').px(2, 3, 'y'),
        "MO" => F::solid('G').bx(5, 4, 8, 5, 'w').px(6, 3, 'y').px(6, 5, 'w'),
        "MP" => F::solid('b').d(6.0, 4.0, 2.2, 'c').bx(5, 3, 7, 6, 'w'),
        "MR" => F::solid('G').bx(0, 0, 12, 1, 'r').bx(0, 7, 12, 8, 'r').d(6.0, 4.0, 2.0, 'y').d(6.0, 3.2, 1.8, 'G'),
        "MT" => F::v("wr").star(2, 1, 's'),
        "MU" => F::h("rbyg"),
        "MV" => F::solid('r').bx(2, 2, 10, 6, 'G').d(6.0, 4.0, 1.5, 'w').d(6.8, 4.0, 1.2, 'G'),
        "MW" => F::h("krg").d(6.0, 1.0, 1.5, 'r'),
        "MX" => F::v("gwr").bx(5, 3, 7, 5, 'n'),
        "MY" => F::h("rwrwrwrw").bx(0, 0, 6, 4, 'B').d(2.5, 2.0, 1.3, 'y').px(2, 2, 'B'),
        "MZ" => F::solid('g').bx(0, 2, 12, 3, 'w').bx(0, 3, 12, 5, 'k').bx(0, 5, 12, 6, 'w').bx(0, 6, 12, 8, 'y').tri(5, 'r').px(1, 4, 'y'),
        "NA" => diag('b', &[(0.1, 'w'), (0.3, 'r'), (0.1, 'w')], 'g').px(1, 1, 'y'),
        "NC" => F::h("brg").d(4.0, 4.0, 1.8, 'y'),
        "NE" => F::h("owg").d(6.0, 4.0, 1.0, 'o'),
        "NF" => F::v("gwg").bx(5, 2, 7, 6, 'G'),
        "NG" => F::v("gwg"),
        "NI" => F::h("bwb").px(6, 4, 'g'),
        "NP" => F::empty().fill(|x, y| {
            let upper = y < 4 && x <= 9 - y * 2;
            let lower = y >= 4 && (x as f32) <= 7.0 - (y - 4) as f32 * 1.5;
            (upper || lower).then_some('r')
        }),
        "NR" => F::solid('B').bx(0, 3, 12, 4, 'y').star(3, 6, 'w'),
        "NU" => F::solid('y').paste(&uj(6, 4), 0, 0),
        "NZ" => ens().px(8, 2, 'r').px(10, 4, 'r').px(8, 6, 'r').px(6, 4, 'r'),
        "OM" => F::solid('r').bx(3, 0, 12, 3, 'w').bx(3, 5, 12, 8, 'g').px(1, 1, 'w'),
        "PA" => F::empty().fill(|x, y| Some(match (x < 6, y < 4) {
            (true, true) | (false, false) => 'w',
            (false, true) => 'r',
            (true, false) => 'b',
        })).px(3, 2, 'b').px(9, 6, 'r'),
        "PE" => F::v("rwr"),
        "PF" => F::solid('r').bx(0, 2, 12, 6, 'w').d(6.0, 4.0, 1.5, 'y'),
        "PG" => F::empty().fill(|x, y| Some(if (y as f32) * 12.0 > (x as f32) * 8.0 { 'k' } else { 'r' })).px(9, 2, 'y').px(2, 6, 'w').px(3, 5, 'w'),
        "PH" => F::h("br").tri(6, 'w').px(1, 4, 'y'),
        "PK" => F::solid('G').bx(0, 0, 3, 8, 'w').d(7.0, 4.0, 1.8, 'w').d(7.7, 3.7, 1.5, 'G'),
        "PL" => F::h("wr"),
        "PR" => F::h("rwrwr").tri(5, 'B').px(1, 4, 'w'),
        "PS" => F::h("kwg").tri(5, 'r'),
        "PT" => F::solid('r').bx(0, 0, 5, 8, 'G').d(5.0, 4.0, 1.3, 'y'),
        "PW" => F::solid('c').d(5.0, 4.0, 2.2, 'y'),
        "PY" => F::h("rwb").px(6, 4, 'G'),
        "QA" => F::solid('R').bx(0, 0, 3, 8, 'w').px(3, 1, 'w').px(3, 3, 'w').px(3, 5, 'w').px(3, 7, 'w').px(4, 2, 'w').px(4, 4, 'w').px(4, 6, 'w'),
        "RO" => F::v("byr"),
        "RS" => F::h("rbw").bx(2, 2, 4, 5, 'R'),
        "RU" => F::h("wbr"),
        "RW" => F::solid('c').bx(0, 4, 12, 6, 'y').bx(0, 6, 12, 8, 'g').d(9.5, 1.5, 1.2, 'y'),
        "SA" => F::solid('G').bx(2, 2, 10, 3, 'w').px(3, 3, 'w').px(6, 3, 'w').px(8, 3, 'w').bx(2, 5, 10, 6, 'w'),
        "SB" => diag('c', &[(0.2, 'y')], 'g').px(1, 1, 'w').px(3, 2, 'w').px(2, 3, 'w'),
        "SC" => F::empty().fill(|x, y| {
            let a = ((8.0 - (y as f32 + 0.5)).atan2(x as f32 + 0.5)).to_degrees();
            Some(if a > 72.0 { 'b' } else if a > 54.0 { 'y' } else if a > 36.0 { 'r' } else if a > 18.0 { 'w' } else { 'g' })
        }),
        "SD" => F::h("rwk").tri(5, 'g'),
        "SE" => nordic('b', 'y', None),
        "SG" => F::h("rw").d(2.0, 2.0, 1.6, 'w').d(2.8, 2.0, 1.3, 'r').px(5, 1, 'w').px(6, 2, 'w').px(5, 3, 'w'),
        "SI" => F::h("wbr").bx(1, 1, 3, 4, 'b'),
        "SK" => F::h("wbr").bx(2, 2, 4, 5, 'r').px(3, 3, 'w'),
        "SL" => F::h("gwb"),
        "SM" => F::h("wc").px(6, 4, 'y'),
        "SN" => F::v("gyr").star(6, 4, 'g'),
        "SO" => F::solid('c').star(6, 4, 'w'),
        "SR" => F::solid('g').bx(0, 1, 12, 2, 'w').bx(0, 2, 12, 6, 'r').bx(0, 6, 12, 7, 'w').px(6, 4, 'y'),
        "SS" => F::solid('k').bx(0, 2, 12, 3, 'w').bx(0, 3, 12, 5, 'r').bx(0, 5, 12, 6, 'w').bx(0, 6, 12, 8, 'g').tri(5, 'b').px(1, 4, 'y'),
        "ST" => F::solid('g').bx(0, 3, 12, 5, 'y').tri(4, 'r').px(6, 4, 'k').px(8, 4, 'k'),
        "SV" => F::h("bwb"),
        "SX" => F::h("rB").tri(6, 'w'),
        "SY" => F::h("rwk").px(4, 4, 'g').px(8, 4, 'g'),
        "SZ" => F::solid('B').bx(0, 1, 12, 2, 'y').bx(0, 2, 12, 6, 'R').bx(0, 6, 12, 7, 'y').bx(4, 3, 8, 5, 'w'),
        "TD" => F::v("byr"),
        "TF" => F::solid('B').bx(0, 0, 2, 4, 'b').bx(2, 0, 4, 4, 'w').bx(4, 0, 6, 4, 'r').px(8, 5, 'w').px(10, 3, 'w').px(7, 3, 'w'),
        "TG" => F::h("gygyg").bx(0, 0, 5, 5, 'r').px(2, 2, 'w'),
        "TH" => F::solid('r').bx(0, 1, 12, 2, 'w').bx(0, 2, 12, 6, 'B').bx(0, 6, 12, 7, 'w'),
        "TJ" => F::solid('r').bx(0, 2, 12, 6, 'w').bx(0, 6, 12, 8, 'g').px(6, 4, 'y'),
        "TK" => F::solid('B').ln(2, 7, 9, 3, 'y').ln(3, 7, 9, 4, 'y').px(2, 2, 'w').px(4, 1, 'w').px(6, 2, 'w'),
        "TL" => F::solid('r').tri(9, 'y').tri(5, 'k').px(2, 4, 'w'),
        "TM" => F::solid('G').bx(1, 0, 3, 8, 'R').px(2, 2, 'y').px(2, 5, 'y').px(6, 2, 'w').px(7, 3, 'w'),
        "TN" => F::solid('r').d(6.0, 4.0, 2.5, 'w').d(6.5, 4.0, 1.6, 'r').px(7, 4, 'w'),
        "TO" => F::solid('r').bx(0, 0, 6, 4, 'w').bx(2, 0, 4, 4, 'r').bx(0, 1, 6, 3, 'r'),
        "TR" => F::solid('r').d(5.0, 4.0, 2.3, 'w').d(5.8, 4.0, 1.9, 'r').px(8, 4, 'w'),
        "TT" => F::solid('r').fill(|x, y| {
            let d = ((y as f32 + 0.5) * 12.0 / 8.0 - (x as f32 + 0.5)).abs();
            if d < 1.2 { Some('k') } else if d < 2.6 { Some('w') } else { None }
        }),
        "TV" => F::solid('c').paste(&uj(6, 4), 0, 0).px(8, 2, 'y').px(10, 4, 'y').px(8, 6, 'y').px(6, 5, 'y'),
        "TW" => F::solid('r').bx(0, 0, 6, 4, 'B').d(3.0, 2.0, 1.2, 'w'),
        "TZ" => diag('g', &[(0.1, 'y'), (0.2, 'k'), (0.1, 'y')], 'c'),
        "UA" => F::h("by"),
        "UG" => F::h("kyrkyr").d(6.0, 4.0, 1.5, 'w'),
        "UM" | "US" => us(),
        "VI" => F::solid('w').bx(5, 2, 7, 6, 'y').px(3, 3, 'b').px(8, 3, 'b'),
        "UN" => F::solid('c').d(6.0, 4.0, 3.0, 'w').d(6.0, 4.0, 2.0, 'c').d(6.0, 4.0, 1.0, 'w'),
        "UY" => F::h("wbwbwbwbw").bx(0, 0, 4, 4, 'w').px(2, 2, 'y'),
        "UZ" => F::solid('b').bx(0, 2, 12, 3, 'r').bx(0, 3, 12, 5, 'w').bx(0, 5, 12, 6, 'r').bx(0, 6, 12, 8, 'g').px(2, 1, 'w'),
        "VA" => F::v("yw").px(9, 4, 'y').px(9, 3, 'y'),
        "VC" => F::solid('B').bx(4, 0, 8, 8, 'y').bx(8, 0, 12, 8, 'g').px(5, 3, 'g').px(6, 2, 'g').px(6, 4, 'g'),
        "VE" => F::h("ybr").px(4, 3, 'w').px(6, 3, 'w').px(8, 3, 'w'),
        "VN" => F::solid('r').star(6, 4, 'y'),
        "VU" => F::h("rg").tri(7, 'y').tri(5, 'k'),
        "WF" => F::solid('r').bx(0, 0, 5, 4, 'w').bx(1, 1, 4, 3, 'r'),
        "WS" => F::solid('r').bx(0, 0, 6, 4, 'B').px(2, 1, 'w').px(3, 2, 'w').px(1, 2, 'w'),
        "XK" => F::solid('B').bx(4, 3, 9, 6, 'y').px(3, 1, 'w').px(5, 1, 'w').px(7, 1, 'w').px(9, 1, 'w'),
        "YE" => F::h("rwk"),
        "ZA" => F::h("rb").bx(0, 2, 12, 3, 'w').bx(0, 5, 12, 6, 'w').bx(0, 3, 12, 5, 'g').tri(4, 'k'),
        "ZM" => F::solid('g').bx(8, 4, 9, 8, 'r').bx(9, 4, 10, 8, 'k').bx(10, 4, 11, 8, 'o').px(8, 1, 'o'),
        "ZW" => F::h("gyrkryg").tri(5, 'k').tri(4, 'w').px(1, 4, 'r'),
        _ => return None,
    })
}

/// Every flag code, in alphabetical order (the 258 regional-indicator pairs).
const CODES: &str = "AC AD AE AF AG AI AL AM AO AQ AR AS AT AU AW AX AZ BA BB BD BE BF BG BH BI BJ BL BM BN BO BQ BR BS BT BV BW BY BZ CA CC CD CF CG CH CI CK CL CM CN CO CP CR CU CV CW CX CY CZ DE DG DJ DK DM DO DZ EA EC EE EG EH ER ES ET EU FI FJ FK FM FO FR GA GB GD GE GF GG GH GI GL GM GN GP GQ GR GS GT GU GW GY HK HM HN HR HT HU IC ID IE IL IM IN IO IQ IR IS IT JE JM JO JP KE KG KH KI KM KN KP KR KW KY KZ LA LB LC LI LK LR LS LT LU LV LY MA MC MD ME MF MG MH MK ML MM MN MO MP MQ MR MS MT MU MV MW MX MY MZ NA NC NE NF NG NI NL NO NP NR NU NZ OM PA PE PF PG PH PK PL PM PN PR PS PT PW PY QA RE RO RS RU RW SA SB SC SD SE SG SH SI SJ SK SL SM SN SO SR SS ST SV SX SY SZ TA TC TD TF TG TH TJ TK TL TM TN TO TR TT TV TW TZ UA UG UM UN US UY UZ VA VC VE VG VI VN VU WF WS XK YE YT ZA ZM ZW";

/// A 12x8 flag as a 12x12 emoji grid (rows 2-9).
fn to_grid(f: &F) -> Grid {
    let mut g = [['.'; SIZE]; SIZE];
    for y in 0..H {
        for x in 0..W {
            g[y + 2][x] = f.0[y][x];
        }
    }
    g
}

/// A flag on a pole: `body` is drawn at the top right of a short brown pole.
fn pole_flag(body: &F) -> Grid {
    let mut g = [['.'; SIZE]; SIZE];
    for row in g.iter_mut().skip(1).take(10) {
        row[1] = 'n';
    }
    for y in 0..6 {
        for x in 0..9 {
            g[y + 1][x + 2] = body.0[y][x];
        }
    }
    g
}

fn regional_indicators() -> Vec<Emoji> {
    ('A'..='Z')
        .enumerate()
        .map(|(i, letter)| {
            let mut g = [['.'; SIZE]; SIZE];
            for y in 1..11 {
                for x in 1..11 {
                    g[y][x] = 'l';
                }
            }
            if let Some((top, w, rows)) = crate::pixel_font::art_of(letter) {
                let ox = (SIZE - w) / 2;
                for (r, row) in rows.iter().enumerate() {
                    for (x, &on) in row.iter().enumerate() {
                        if on {
                            let y = (top as usize + r).saturating_sub(1).min(SIZE - 1);
                            g[y][ox + x] = 'S';
                        }
                    }
                }
            }
            let c = char::from_u32(RI_A + i as u32).unwrap();
            emoji(&format!("regional_indicator_{}", letter.to_ascii_lowercase()), &c.to_string(), g)
        })
        .collect()
}

/// A tag character (`U+E0000 + ascii`), as used by subdivision flag sequences.
fn tag(c: char) -> char {
    char::from_u32(0xE0000 + c as u32).unwrap()
}

/// A flag glyph with the sequences that produce it.
fn flag(name: &str, index: u32, grid: Grid, sequences: Vec<Vec<char>>) -> Emoji {
    let mut e = emoji(name, "", grid);
    e.chars = vec![char::from_u32(FLAG_BASE + index).unwrap()];
    // A one-character "sequence" is just the character: map it straight to the glyph.
    for seq in sequences {
        if seq.len() == 1 {
            e.chars.push(seq[0]);
        } else {
            e.sequences.push(seq);
        }
    }
    e
}

/// The flag-on-a-pole and special flags and the subdivision flags.
fn specials(mut index: u32) -> Vec<Emoji> {
    let pole = |c: char| pole_flag(&F::solid(c));
    let mut out = Vec::new();
    let mut add = |name: &str, grid: Grid, seqs: Vec<Vec<char>>| {
        out.push(flag(name, index, grid, seqs));
        index += 1;
    };
    let zwj = '\u{200D}';
    let vs = '\u{FE0F}';
    let flag_black = '\u{1F3F4}';
    let flag_white = '\u{1F3F3}';

    add("white_flag", pole_flag(&F::solid('w')), vec![vec![flag_white], vec![flag_white, vs]]);
    add("black_flag", pole('k'), vec![vec![flag_black]]);
    add("triangular_flag", {
        let mut g = [['.'; SIZE]; SIZE];
        for row in g.iter_mut().skip(1).take(10) {
            row[3] = 'n';
        }
        for y in 1..6 {
            for x in 4..(4 + 7 - (y as i32 - 3).unsigned_abs() as usize * 2).min(11) {
                g[y][x] = 'r';
            }
        }
        g
    }, vec![vec!['\u{1F6A9}']]);
    add("checkered_flag", {
        let body = F::empty().fill(|x, y| Some(if (x + y) % 2 == 0 { 'k' } else { 'w' }));
        pole_flag(&body)
    }, vec![vec!['\u{1F3C1}']]);
    add("crossed_flags", {
        let mut g = [['.'; SIZE]; SIZE];
        for i in 0..9 {
            g[1 + i][1 + i / 2] = 'n';
            g[1 + i][10 - i / 2] = 'n';
        }
        for y in 0..4 {
            for x in 0..4 {
                g[1 + y][2 + x] = 'r';
                g[1 + y][6 + x] = 'b';
            }
        }
        g
    }, vec![vec!['\u{1F38C}']]);
    add("rainbow_flag", pole_flag(&F::h("royPgb")), vec![vec![flag_white, vs, zwj, '\u{1F308}'], vec![flag_white, zwj, '\u{1F308}']]);
    add("transgender_flag", pole_flag(&F::h("cpwpc")), vec![vec![flag_white, vs, zwj, '\u{26A7}', vs], vec![flag_white, vs, zwj, '\u{26A7}'], vec![flag_white, zwj, '\u{26A7}']]);
    add("pirate_flag", pole_flag(&F::solid('k').d(4.5, 2.5, 1.3, 'w').px(3, 4, 'w').px(5, 4, 'w')), vec![vec![flag_black, zwj, '\u{2620}', vs], vec![flag_black, zwj, '\u{2620}']]);

    let seq = |code: &str| {
        let mut s = vec![flag_black];
        s.extend(code.chars().map(tag));
        s.push('\u{E007F}');
        s
    };
    add("flag_england", to_grid(&F::solid('w').cross(5, 3, 2, 'r')), vec![seq("gbeng")]);
    add("flag_scotland", to_grid(&F::solid('b').fill(|x, y| {
        let (a, b) = (y as f32 + 0.5, x as f32 * 8.0 / 12.0 + 0.33);
        ((a - b).abs() < 0.9 || (a - (8.0 - b)).abs() < 0.9).then_some('w')
    })), vec![seq("gbsct")]);
    add("flag_wales", to_grid(&F::h("wg").d(6.0, 4.0, 1.8, 'r')), vec![seq("gbwls")]);
    // The flag of the Republic of Texas, the Lone Star flag: a blue bar with a white star, white
    // over red. It has no flag of its own in Unicode, so it uses the tag sequence its code
    // (US-TX) would have, and the shortcode `:flag_texas:`.
    add("flag_texas", to_grid(&F::h("wr").bx(0, 0, 4, 8, 'B').star(1, 3, 'w').px(2, 3, 'w')), vec![seq("ustx")]);
    out
}

/// Every flag, the regional-indicator letters they are written with, and the special flags.
pub fn all() -> Vec<Emoji> {
    let mut out = regional_indicators();
    let codes: Vec<&str> = CODES.split_whitespace().collect();
    for (i, code) in codes.iter().enumerate() {
        let f = design(code).unwrap_or_else(|| panic!("no design for flag {code}"));
        let letters: Vec<char> = code.chars().map(|c| char::from_u32(RI_A + (c as u32 - 'A' as u32)).unwrap()).collect();
        out.push(flag(&format!("flag_{}", code.to_ascii_lowercase()), i as u32, to_grid(&f), vec![letters]));
    }
    out.extend(specials(codes.len() as u32));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_are_258_country_flags_each_with_a_design() {
        let codes: Vec<&str> = CODES.split_whitespace().collect();
        assert_eq!(codes.len(), 258);
        let mut seen = std::collections::HashSet::new();
        for c in &codes {
            assert!(seen.insert(*c), "{c} listed twice");
            assert!(design(c).is_some(), "{c} has no design");
        }
    }

    #[test]
    fn every_flag_is_fully_painted_with_known_colours() {
        for e in all() {
            if !e.name.starts_with("flag_") {
                continue;
            }
            let painted = e.grid.iter().flatten().filter(|c| **c != '.').count();
            // Every flag but the non-rectangular Nepal fills its 12x8 area.
            if e.name != "flag_np" {
                assert!(painted >= 60, "{} has only {painted} painted pixels", e.name);
            }
        }
    }

    #[test]
    fn flags_that_are_different_countries_look_different() {
        // France/Netherlands/Italy/Ireland/Germany etc. are the classic look-alike risks.
        let mut by_look: std::collections::HashMap<Vec<char>, Vec<String>> = Default::default();
        for e in all() {
            if e.name.starts_with("flag_") && e.name.len() == 7 {
                by_look.entry(e.grid.iter().flatten().copied().collect()).or_default().push(e.name.clone());
            }
        }
        let allowed_same: &[&[&str]] = &[
            &["flag_bl", "flag_mf", "flag_gp", "flag_mq", "flag_gf", "flag_re", "flag_yt", "flag_pm", "flag_cp", "flag_fr"],
            &["flag_bv", "flag_sj", "flag_no"],
            &["flag_au", "flag_hm"],
            &["flag_dg", "flag_io"],
            &["flag_bq", "flag_nl"],
            &["flag_id", "flag_mc"],
            &["flag_ea", "flag_es"],
            &["flag_um", "flag_us"],
            &["flag_us", "flag_um"],
            &["flag_td", "flag_ro"],
        ];
        for (_, names) in by_look.iter().filter(|(_, n)| n.len() > 1) {
            assert!(allowed_same.iter().any(|group| names.iter().all(|n| group.contains(&n.as_str()))), "{names:?} look identical");
        }
    }

    #[test]
    fn regional_indicator_letters_are_drawn() {
        let letters: Vec<_> = all().into_iter().filter(|e| e.name.starts_with("regional_indicator_")).collect();
        assert_eq!(letters.len(), 26);
    }

    #[test]
    fn the_subdivision_flags_and_texas_use_tag_sequences() {
        let set = all();
        let tx = set.iter().find(|e| e.name == "flag_texas").expect("Texas");
        assert_eq!(tx.sequences[0][0], '\u{1F3F4}');
        assert_eq!(*tx.sequences[0].last().unwrap(), '\u{E007F}');
        for name in ["flag_england", "flag_scotland", "flag_wales", "rainbow_flag", "transgender_flag", "pirate_flag", "checkered_flag", "crossed_flags", "triangular_flag", "white_flag", "black_flag"] {
            assert!(set.iter().any(|e| e.name == name), "{name} missing");
        }
    }
}
