//! Emoji made by rule rather than drawn: colour circles and squares, clocks, moon phases, arrows,
//! the rainbow, the skin-tone swatches.

use super::{emoji, flip_horizontal, parse, rotate_cw, Emoji, Grid, SIZE};

const BLANK: Grid = [['.'; SIZE]; SIZE];

/// A filled disc of `body` with the dark outline.
fn disc(body: char) -> Grid {
    let mut g = BLANK;
    let c = (SIZE as f32 - 1.0) / 2.0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
            if d <= 5.7 {
                g[y][x] = if d > 4.7 { 'k' } else { body };
            }
        }
    }
    g
}

/// A filled `size`-square of `body` with the dark outline, centred.
fn square(body: char, size: usize) -> Grid {
    let mut g = BLANK;
    let off = (SIZE - size) / 2;
    for y in 0..size {
        for x in 0..size {
            let edge = x == 0 || y == 0 || x == size - 1 || y == size - 1;
            g[off + y][off + x] = if edge { 'k' } else { body };
        }
    }
    g
}

/// Rotates a picture by `degrees` clockwise about its centre (nearest pixel).
fn rotate_by(g: &Grid, degrees: f32) -> Grid {
    let a = degrees.to_radians();
    let (s, c) = (a.sin(), a.cos());
    let ctr = (SIZE as f32 - 1.0) / 2.0;
    let mut out = BLANK;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (dx, dy) = (x as f32 - ctr, y as f32 - ctr);
            // Inverse rotation.
            let sx = (c * dx + s * dy + ctr).round();
            let sy = (-s * dx + c * dy + ctr).round();
            if sx >= 0.0 && sy >= 0.0 && (sx as usize) < SIZE && (sy as usize) < SIZE {
                out[y][x] = g[sy as usize][sx as usize];
            }
        }
    }
    out
}

fn clock(hour: u32, half: bool) -> Grid {
    let mut g = disc('w');
    let minute_angle = if half { 180.0f32 } else { 0.0 };
    let hour_angle = ((hour % 12) as f32 + if half { 0.5 } else { 0.0 }) * 30.0;
    let ctr = (SIZE as f32 - 1.0) / 2.0;
    for (angle, len) in [(hour_angle, 2.6f32), (minute_angle, 4.0)] {
        let a = angle.to_radians();
        let mut t = 0.0;
        while t <= len {
            let x = (ctr + a.sin() * t).round() as usize;
            let y = (ctr - a.cos() * t).round() as usize;
            g[y.min(SIZE - 1)][x.min(SIZE - 1)] = 'k';
            t += 0.5;
        }
    }
    g
}

/// Moon phase 0..8: new, waxing crescent, first quarter, waxing gibbous, full, waning gibbous,
/// last quarter, waning crescent.
fn moon(phase: usize) -> Grid {
    let mut g = disc('S');
    let c = (SIZE as f32 - 1.0) / 2.0;
    let (threshold, right): (f32, bool) = match phase {
        0 => (2.0, true),
        1 => (0.35, true),
        2 => (-0.05, true),
        3 => (-0.45, true),
        4 => (-2.0, true),
        5 => (-0.45, false),
        6 => (-0.05, false),
        _ => (0.35, false),
    };
    for y in 0..SIZE {
        for x in 0..SIZE {
            if g[y][x] == 'S' {
                let u = (x as f32 - c) / 5.0;
                let lit = if right { u >= threshold } else { -u >= threshold };
                if lit {
                    g[y][x] = 'y';
                }
            }
        }
    }
    g
}

fn crescent() -> Grid {
    let mut g = BLANK;
    let c = (SIZE as f32 - 1.0) / 2.0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let a = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
            let b = ((x as f32 - c - 3.0).powi(2) + (y as f32 - c + 1.0).powi(2)).sqrt();
            if a <= 5.7 && b > 4.6 {
                g[y][x] = if a > 4.7 || b < 5.6 { 'k' } else { 'y' };
            }
        }
    }
    g
}

fn rainbow() -> Grid {
    let mut g = BLANK;
    let bands = ['r', 'o', 'y', 'g', 'b', 'P'];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let d = ((x as f32 - 5.5).powi(2) + (y as f32 - 10.5).powi(2)).sqrt();
            if (3.5..9.5).contains(&d) && y <= 10 {
                let i = ((9.5 - d).floor() as usize).min(5);
                g[y][x] = bands[i];
            }
        }
    }
    g
}

const UP_ARROW: &str = ".....kk...../....kbbk..../...kbbbbk.../..kbbbbbbk../.kbbbbbbbbk./kkkkbbbbkkkk/...kbbbbk.../...kbbbbk.../...kbbbbk.../...kbbbbk.../...kbbbbk.../...kkkkkk...";
const UP_TRIANGLE: &str = "............/............/.....kk...../....kbbk..../...kbbbbk.../..kbbbbbbk../.kbbbbbbbbk./kbbbbbbbbbbk/kkkkkkkkkkkk/............/............/............";
const EARTH: &str = "....kkkk..../..kkbbbbkk../.kbggbbbbbk./kbgggbbbggbk/kbgggbbggggk/kbbggbbbgggk/kbbbbbbbbggk/kbbbbggbbbbk/.kbbggggbbk./..kkbggbkk.../....kkkk..../............";
const MOUNTAIN: &str = "............/.....kk...../....kwwk..../...kwwwwk.../..kkwwkwwk../.kssksskssk./kssssssssssk/kssSssssSssk/kssssssssssk/kkkkkkkkkkkk/............/............";

pub fn all() -> Vec<Emoji> {
    let mut out = Vec::new();
    for (name, ch, body) in [
        ("red_circle", "🔴", 'r'),
        ("orange_circle", "🟠", 'o'),
        ("yellow_circle", "🟡", 'y'),
        ("green_circle", "🟢", 'g'),
        ("blue_circle", "🔵", 'b'),
        ("purple_circle", "🟣", 'P'),
        ("brown_circle", "🟤", 'n'),
        ("black_circle", "⚫", 'S'),
        ("white_circle", "⚪", 'w'),
    ] {
        out.push(emoji(name, ch, disc(body)));
    }
    for (name, ch, body) in [
        ("red_square", "🟥", 'r'),
        ("orange_square", "🟧", 'o'),
        ("yellow_square", "🟨", 'y'),
        ("green_square", "🟩", 'g'),
        ("blue_square", "🟦", 'b'),
        ("purple_square", "🟪", 'P'),
        ("brown_square", "🟫", 'n'),
        ("black_large_square", "⬛", 'S'),
        ("white_large_square", "⬜", 'w'),
    ] {
        out.push(emoji(name, ch, square(body, 10)));
    }
    // The skin-tone modifiers show as swatches (there is no ligature to apply them to a hand).
    for (name, ch, body) in [("tone_1", "\u{1F3FB}", 'e'), ("tone_2", "\u{1F3FC}", 't'), ("tone_3", "\u{1F3FD}", 'T'), ("tone_4", "\u{1F3FE}", 'n'), ("tone_5", "\u{1F3FF}", 'N')] {
        out.push(emoji(name, ch, square(body, 8)));
    }
    // Clocks on the hour (U+1F550..) and the half hour (U+1F55C..).
    for h in 1..=12u32 {
        let on = char::from_u32(0x1F54F + h).unwrap();
        let half = char::from_u32(0x1F55B + h).unwrap();
        out.push(emoji(&format!("clock{h}"), &on.to_string(), clock(h, false)));
        out.push(emoji(&format!("clock{h}30"), &half.to_string(), clock(h, true)));
    }
    // Moon phases U+1F311..1F318.
    for p in 0..8usize {
        let ch = char::from_u32(0x1F311 + p as u32).unwrap();
        let name = ["new_moon", "waxing_crescent_moon", "first_quarter_moon", "waxing_gibbous_moon", "full_moon", "waning_gibbous_moon", "last_quarter_moon", "waning_crescent_moon"][p];
        out.push(emoji(name, &ch.to_string(), moon(p)));
    }
    out.push(emoji("crescent_moon", "🌙", crescent()));
    out.push(emoji("rainbow", "🌈", rainbow()));
    out.push(emoji("earth", "🌍🌎🌏", parse(EARTH)));
    out.push(emoji("mountain", "⛰🏔", parse(MOUNTAIN)));

    // Arrows from one drawing, turned.
    let up = parse(UP_ARROW);
    let right = rotate_cw(&up);
    let down = rotate_cw(&right);
    let left = flip_horizontal(&right);
    out.push(emoji("arrow_up", "⬆", up));
    out.push(emoji("arrow_right", "➡", right));
    out.push(emoji("arrow_down", "⬇", down));
    out.push(emoji("arrow_left", "⬅", left));
    out.push(emoji("arrow_upper_right", "↗", rotate_by(&up, 45.0)));
    out.push(emoji("arrow_lower_right", "↘", rotate_by(&up, 135.0)));
    out.push(emoji("arrow_lower_left", "↙", rotate_by(&up, 225.0)));
    out.push(emoji("arrow_upper_left", "↖", rotate_by(&up, 315.0)));
    // Up-down and left-right: the two halves of an arrow each way.
    let mut ud = up;
    for y in 6..SIZE {
        ud[y] = down[y];
    }
    out.push(emoji("arrow_up_down", "↕", ud));
    let tri = parse(UP_TRIANGLE);
    let tri_right = rotate_cw(&tri);
    out.push(emoji("arrow_up_small", "🔼", tri));
    out.push(emoji("arrow_down_small", "🔽", rotate_cw(&tri_right)));
    out.push(emoji("play", "▶", tri_right));
    out.push(emoji("reverse", "◀", flip_horizontal(&tri_right)));
    out
}
