//! Coverage of the whole Unicode emoji list.
//!
//! `data/standard.txt` is every *fully-qualified* emoji of the official `emoji-test.txt`
//! (`group|code points|CLDR name`). [`all`] looks at which of them the hand-drawn and rule-made
//! sets already show, and makes a glyph for each of the rest, in this order of preference:
//!
//! 1. **A recoloured copy** of a picture that already exists (skin tones).
//! 2. **A drawn-by-rule person or group**: [`Person`] sprites for people, couples and families,
//!    with a small prop for a profession, a heart, kiss or handshake between two of them.
//! 3. **A badge**: a drawn emoji with a smaller picture in its corner (`black cat`, `heart on fire`).
//! 4. **A labelled placeholder**: a tile in its group's colour with two letters from its name
//!    (a skin-tone one takes the tone as its colour). This is *not* art. It exists so every standard
//!    emoji shows as one glyph, can be told apart, and is listed by `/emoji-report` for replacing.
//!
//! Every glyph is reachable by the standard sequence (and the same without `U+FE0F`), and by
//! `:cldr_name:` in chat.

use std::collections::{HashMap, HashSet};

use super::{emoji, recolor, Emoji, Grid, SIZE};

const DATA: &str = include_str!("data/standard.txt");
const ZWJ: char = '\u{200D}';
const VS16: char = '\u{FE0F}';
/// Private-use code points naming the generated glyphs (plane 15, after the flags and tones).
const BASE: u32 = 0xF2000;

/// One entry of the standard list.
#[derive(Clone, Debug)]
pub struct Entry {
    pub group: &'static str,
    pub cps: Vec<char>,
    pub name: &'static str,
}

/// Every fully-qualified standard emoji.
pub fn entries() -> Vec<Entry> {
    DATA.lines()
        .filter_map(|l| {
            let mut p = l.splitn(3, '|');
            let (group, cps, name) = (p.next()?, p.next()?, p.next()?);
            let cps: Option<Vec<char>> = cps.split(' ').map(|h| u32::from_str_radix(h, 16).ok().and_then(char::from_u32)).collect();
            Some(Entry { group, cps: cps?, name })
        })
        .collect()
}

fn is_tone(c: char) -> bool {
    ('\u{1F3FB}'..='\u{1F3FF}').contains(&c)
}

/// The palette keys `(light, shadow)` of a skin-tone modifier.
fn tone_keys(c: char) -> (char, char) {
    match c {
        '\u{1F3FB}' => ('1', '2'),
        '\u{1F3FC}' => ('3', '4'),
        '\u{1F3FD}' => ('5', '6'),
        '\u{1F3FE}' => ('7', '8'),
        _ => ('9', '0'),
    }
}

fn strip_vs16(cps: &[char]) -> Vec<char> {
    cps.iter().copied().filter(|c| *c != VS16).collect()
}

// ---------------------------------------------------------------------------------------------
// Placeholders

/// 3x5 capital letters for the placeholder labels.
fn letter(c: char) -> [&'static str; 5] {
    const ART: [[&str; 5]; 26] = [
        [".#.", "#.#", "###", "#.#", "#.#"],
        ["##.", "#.#", "##.", "#.#", "##."],
        [".##", "#..", "#..", "#..", ".##"],
        ["##.", "#.#", "#.#", "#.#", "##."],
        ["###", "#..", "##.", "#..", "###"],
        ["###", "#..", "##.", "#..", "#.."],
        [".##", "#..", "#.#", "#.#", ".##"],
        ["#.#", "#.#", "###", "#.#", "#.#"],
        ["###", ".#.", ".#.", ".#.", "###"],
        ["..#", "..#", "..#", "#.#", ".#."],
        ["#.#", "#.#", "##.", "#.#", "#.#"],
        ["#..", "#..", "#..", "#..", "###"],
        ["#.#", "###", "###", "#.#", "#.#"],
        ["##.", "#.#", "#.#", "#.#", "#.#"],
        [".#.", "#.#", "#.#", "#.#", ".#."],
        ["##.", "#.#", "##.", "#..", "#.."],
        [".#.", "#.#", "#.#", "##.", ".##"],
        ["##.", "#.#", "##.", "#.#", "#.#"],
        [".##", "#..", ".#.", "..#", "##."],
        ["###", ".#.", ".#.", ".#.", ".#."],
        ["#.#", "#.#", "#.#", "#.#", "###"],
        ["#.#", "#.#", "#.#", "#.#", ".#."],
        ["#.#", "#.#", "###", "###", "#.#"],
        ["#.#", "#.#", ".#.", "#.#", "#.#"],
        ["#.#", "#.#", ".#.", ".#.", ".#."],
        ["###", "..#", ".#.", "#..", "###"],
    ];
    ART[(c.to_ascii_uppercase() as u8 - b'A') as usize]
}

/// Up to two letters standing for a CLDR name: the first letters of its first two meaningful
/// words, or the first two letters of a single word.
pub fn abbreviation(name: &str) -> [char; 2] {
    const SKIP: &[&str] = &["with", "of", "the", "and", "in", "a", "on", "for", "to"];
    let words: Vec<String> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !SKIP.contains(&w.to_lowercase().as_str()))
        .map(|w| w.chars().filter(|c| c.is_ascii_alphabetic()).collect::<String>())
        .filter(|w| !w.is_empty())
        .collect();
    let mut it = words.iter();
    match (it.next(), it.next()) {
        (Some(a), Some(b)) => [a.chars().next().unwrap().to_ascii_uppercase(), b.chars().next().unwrap().to_ascii_uppercase()],
        (Some(a), None) => {
            let mut c = a.chars();
            let first = c.next().unwrap().to_ascii_uppercase();
            [first, c.next().map_or(first, |x| x.to_ascii_uppercase())]
        }
        _ => ['X', 'X'],
    }
}

fn group_colour(group: &str) -> char {
    match group {
        "Smileys & Emotion" => 'Y',
        "People & Body" => 'o',
        "Animals & Nature" => 'g',
        "Food & Drink" => 'r',
        "Travel & Places" => 'b',
        "Activities" => 'P',
        "Objects" => 's',
        "Symbols" => 'c',
        _ => 'l',
    }
}

/// A labelled tile: `bg` body with a dark outline and two letters.
pub fn placeholder_tile(bg: char, letters: [char; 2]) -> Grid {
    let mut g = [['.'; SIZE]; SIZE];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let corner = (x == 0 || x == SIZE - 1) && (y == 0 || y == SIZE - 1);
            if corner {
                continue;
            }
            let edge = x == 0 || y == 0 || x == SIZE - 1 || y == SIZE - 1;
            g[y][x] = if edge { 'k' } else { bg };
        }
    }
    let fg = if "rbPsSBR789 0".contains(bg) { 'w' } else { 'k' };
    for (i, c) in letters.iter().enumerate() {
        let art = letter(*c);
        for (y, row) in art.iter().enumerate() {
            for (x, ch) in row.chars().enumerate() {
                if ch == '#' {
                    g[3 + y][2 + i * 4 + x] = fg;
                }
            }
        }
    }
    g
}

// ---------------------------------------------------------------------------------------------
// People

#[derive(Clone, Copy, PartialEq, Debug)]
enum Sex {
    Male,
    Female,
    Neutral,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Age {
    Adult,
    Old,
    Child,
    Baby,
}

#[derive(Clone, Copy, Debug)]
struct Person {
    sex: Sex,
    age: Age,
    skin: (char, char),
    hair: char,
    curly: bool,
    bald: bool,
}

fn person_base(c: char) -> Option<(Sex, Age)> {
    Some(match c {
        '\u{1F468}' => (Sex::Male, Age::Adult),
        '\u{1F469}' => (Sex::Female, Age::Adult),
        '\u{1F9D1}' => (Sex::Neutral, Age::Adult),
        '\u{1F466}' => (Sex::Male, Age::Child),
        '\u{1F467}' => (Sex::Female, Age::Child),
        '\u{1F9D2}' => (Sex::Neutral, Age::Child),
        '\u{1F476}' => (Sex::Neutral, Age::Baby),
        '\u{1F474}' => (Sex::Male, Age::Old),
        '\u{1F475}' => (Sex::Female, Age::Old),
        '\u{1F9D3}' => (Sex::Neutral, Age::Old),
        _ => return None,
    })
}

impl Person {
    fn new(sex: Sex, age: Age) -> Self {
        let hair = match (age, sex) {
            (Age::Old, _) => 'l',
            (_, Sex::Male) => 'N',
            (_, Sex::Female) => 'Y',
            _ => 'n',
        };
        Person { sex, age, skin: ('t', 'T'), hair, curly: false, bald: false }
    }
}

/// Draws `p` in the box `x0, y0, w, h`; `shirt` is the colour of its top.
fn draw_person(g: &mut Grid, x0: usize, y0: usize, w: usize, h: usize, p: &Person, shirt: char) {
    let head_w = match w {
        0..=3 => 2,
        4..=5 => 3,
        _ => 4,
    };
    let head_h = head_w.min(h.saturating_sub(2)).max(2);
    let hx = x0 + (w - head_w) / 2;
    let mut set = |x: usize, y: usize, c: char| {
        if x < SIZE && y < SIZE {
            g[y][x] = c;
        }
    };
    for dy in 0..head_h {
        for dx in 0..head_w {
            let shadow = head_w >= 3 && dx == head_w - 1;
            set(hx + dx, y0 + dy, if shadow { p.skin.1 } else { p.skin.0 });
        }
    }
    if !p.bald {
        for dx in 0..head_w {
            let c = if p.curly && dx % 2 == 1 { p.skin.0 } else { p.hair };
            set(hx + dx, y0, c);
        }
        let long = matches!(p.sex, Sex::Female) || p.curly;
        if head_w >= 3 || long {
            if hx > x0 || long {
                for dy in 1..(head_h + usize::from(long)) {
                    if hx > 0 {
                        set(hx - 1, y0 + dy, p.hair);
                    }
                    if long {
                        set(hx + head_w, y0 + dy, p.hair);
                    }
                }
            }
            if head_w == 4 && !long {
                set(hx, y0 + 1, p.hair);
                set(hx + head_w - 1, y0 + 1, p.hair);
            }
        }
    }
    if head_w == 4 && head_h >= 3 {
        set(hx + 1, y0 + 1, 'k');
        set(hx + 2, y0 + 1, 'k');
    }
    let body_top = y0 + head_h;
    let body_h = h.saturating_sub(head_h);
    let torso = (body_h * 5 / 8).max(1);
    let dress = p.sex == Sex::Female;
    for dy in 0..body_h {
        let y = body_top + dy;
        if dy < torso {
            let (a, b) = if dress && w >= 5 { (x0 + 1, x0 + w - 1) } else { (x0, x0 + w) };
            for x in a..b {
                set(x, y, shirt);
            }
        } else if dress {
            for x in x0..x0 + w {
                set(x, y, shirt);
            }
        } else {
            let pants = if p.sex == Sex::Neutral { 'S' } else { 'B' };
            let half = w / 2;
            for x in x0..x0 + w {
                if w >= 4 && w % 2 == 0 && x == x0 + half - 1 + 1 && dy > torso {
                    continue;
                }
                set(x, y, pants);
            }
        }
    }
}

/// The picture shrunk to 5x5: each cell is the commonest colour of its block, blank if it is mostly so.
fn shrink(g: &Grid) -> [[char; 5]; 5] {
    let mut out = [['.'; 5]; 5];
    for (dy, row) in out.iter_mut().enumerate() {
        for (dx, cell) in row.iter_mut().enumerate() {
            let (y0, y1, x0, x1) = (dy * SIZE / 5, (dy + 1) * SIZE / 5, dx * SIZE / 5, (dx + 1) * SIZE / 5);
            let mut counts: HashMap<char, usize> = HashMap::new();
            let mut total = 0;
            for y in y0..y1 {
                for x in x0..x1 {
                    total += 1;
                    if g[y][x] != '.' {
                        *counts.entry(g[y][x]).or_default() += 1;
                    }
                }
            }
            let best = counts.iter().max_by_key(|(c, n)| (**n, **c)).map(|(c, n)| (*c, *n));
            if let Some((c, n)) = best {
                if n * 4 >= total {
                    *cell = c;
                }
            }
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq)]
enum Link {
    Heart,
    Kiss,
    Handshake,
}

/// What a sequence of people is made of.
struct Scene {
    people: Vec<Person>,
    link: Option<Link>,
    prop: Option<Vec<char>>,
}

/// Single code points that stand for a group of people.
fn expand(key: &[char]) -> Vec<char> {
    let (tone, rest): (Vec<char>, Vec<char>) = key.iter().partition(|c| is_tone(**c));
    if rest.len() != 1 {
        return key.to_vec();
    }
    let seq: &[char] = match rest[0] {
        '\u{1F46B}' => &['\u{1F469}', ZWJ, '\u{1F91D}', ZWJ, '\u{1F468}'],
        '\u{1F46C}' => &['\u{1F468}', ZWJ, '\u{1F91D}', ZWJ, '\u{1F468}'],
        '\u{1F46D}' => &['\u{1F469}', ZWJ, '\u{1F91D}', ZWJ, '\u{1F469}'],
        '\u{1F46A}' => &['\u{1F468}', ZWJ, '\u{1F469}', ZWJ, '\u{1F466}'],
        '\u{1F491}' => &['\u{1F9D1}', ZWJ, '\u{2764}', ZWJ, '\u{1F9D1}'],
        '\u{1F48F}' => &['\u{1F9D1}', ZWJ, '\u{2764}', ZWJ, '\u{1F48B}', ZWJ, '\u{1F9D1}'],
        _ => return key.to_vec(),
    };
    // A modifier on the single code point goes on every person.
    let mut out = Vec::new();
    for &c in seq {
        out.push(c);
        if person_base(c).is_some() {
            if let Some(&t) = tone.first() {
                out.push(t);
            }
        }
    }
    out
}

/// Reads a sequence as people (+ link, + prop), or `None` when it is not about people.
fn scene(key: &[char]) -> Option<Scene> {
    let key = expand(key);
    let parts: Vec<&[char]> = key.split(|c| *c == ZWJ).collect();
    person_base(*parts.first()?.first()?)?;
    let mut people: Vec<Person> = Vec::new();
    let mut link = None;
    let mut prop = None;
    for part in parts {
        let c = part[0];
        let tone = part.iter().copied().find(|c| is_tone(*c));
        if let Some((sex, age)) = person_base(c) {
            let mut p = Person::new(sex, age);
            if let Some(t) = tone {
                p.skin = tone_keys(t);
            }
            people.push(p);
            continue;
        }
        match c {
            '\u{2764}' => link = Some(Link::Heart),
            '\u{1F48B}' => link = Some(Link::Kiss),
            '\u{1F91D}' => link = Some(Link::Handshake),
            '\u{1F9B0}' => people.last_mut()?.hair = 'o',
            '\u{1F9B1}' => {
                let p = people.last_mut()?;
                p.curly = true;
            }
            '\u{1F9B2}' => people.last_mut()?.bald = true,
            '\u{1F9B3}' => people.last_mut()?.hair = 'w',
            '\u{2640}' => people.last_mut()?.sex = Sex::Female,
            '\u{2642}' => people.last_mut()?.sex = Sex::Male,
            _ => prop = Some(part.iter().copied().filter(|c| !is_tone(*c)).collect()),
        }
    }
    if people.is_empty() || people.len() > 4 {
        return None;
    }
    Some(Scene { people, link, prop })
}

const SHIRTS: [char; 4] = ['b', 'r', 'g', 'P'];

fn draw_scene(s: &Scene, prop_art: Option<[[char; 5]; 5]>) -> Grid {
    let mut g = [['.'; SIZE]; SIZE];
    let n = s.people.len();
    let width = match n {
        1 => {
            if prop_art.is_some() {
                7
            } else {
                8
            }
        }
        2 => 5,
        3 => 4,
        _ => 3,
    };
    let start = match n {
        1 => {
            if prop_art.is_some() {
                0
            } else {
                2
            }
        }
        2 => 0,
        _ => 0,
    };
    for (i, p) in s.people.iter().enumerate() {
        let h = match p.age {
            Age::Adult | Age::Old => 12,
            Age::Child => 8,
            Age::Baby => 5,
        };
        let x0 = start + i * (width + usize::from(n == 2));
        draw_person(&mut g, x0, 12 - h, width, h, p, SHIRTS[i % SHIRTS.len()]);
    }
    if n == 2 {
        match s.link {
            Some(Link::Heart) | Some(Link::Kiss) => {
                let c = if s.link == Some(Link::Heart) { 'r' } else { 'p' };
                for (x, y) in [(4, 0), (6, 0), (4, 1), (5, 1), (6, 1), (5, 2)] {
                    g[y][x] = c;
                }
            }
            Some(Link::Handshake) => {
                for x in 4..7 {
                    g[6][x] = 'y';
                }
            }
            None => {}
        }
    }
    if n == 1 {
        if let Some(art) = prop_art {
            for (dy, row) in art.iter().enumerate() {
                for (dx, c) in row.iter().enumerate() {
                    if *c != '.' {
                        g[6 + dy][7 + dx] = *c;
                    }
                }
            }
        }
    }
    g
}

// ---------------------------------------------------------------------------------------------
// The generator

/// What became of the standard list.
#[derive(Clone, Debug, Default)]
pub struct Summary {
    pub total: usize,
    /// Already shown by the drawn / rule-made sets.
    pub existing: usize,
    /// Recoloured copies, people and badges made by rule.
    pub generated: usize,
    /// Labelled tiles, by CLDR name.
    pub placeholders: Vec<String>,
}

impl Summary {
    /// How many are labelled tiles rather than pictures.
    pub fn placeholder_count(&self) -> usize {
        self.placeholders.len()
    }
}

fn shortcode(name: &str) -> String {
    let mut s = String::new();
    for c in name.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else if !s.ends_with('_') && !s.is_empty() {
            s.push('_');
        }
    }
    s.trim_end_matches('_').to_string()
}

enum Made {
    Art(Grid),
    Placeholder(Grid),
}

/// The emoji that make the list complete, and a summary of how.
pub fn all(existing: &[Emoji]) -> (Vec<Emoji>, Summary) {
    let list = entries();
    let mut known: HashMap<Vec<char>, (Grid, bool)> = HashMap::new();
    let mut have_chars: HashSet<char> = HashSet::new();
    let mut have_seqs: HashSet<Vec<char>> = HashSet::new();
    let mut names: HashSet<String> = existing.iter().map(|e| e.name.clone()).collect();
    for e in existing {
        for &c in &e.chars {
            have_chars.insert(c);
            known.entry(vec![c]).or_insert((e.grid, false));
        }
        for s in &e.sequences {
            let key = strip_vs16(s);
            have_seqs.insert(key.clone());
            known.entry(key).or_insert((e.grid, false));
        }
    }
    // The four hair components (not in the fully-qualified list, but the people sequences use them).
    let mut components = Vec::new();
    for (c, name, hair, curly, bald) in [
        ('\u{1F9B0}', "red_hair", 'o', false, false),
        ('\u{1F9B1}', "curly_hair", 'N', true, false),
        ('\u{1F9B2}', "bald", 'N', false, true),
        ('\u{1F9B3}', "white_hair", 'w', false, false),
    ] {
        if have_chars.contains(&c) || names.contains(name) {
            continue;
        }
        let mut p = Person::new(Sex::Neutral, Age::Adult);
        p.hair = hair;
        p.curly = curly;
        p.bald = bald;
        let mut g = [['.'; SIZE]; SIZE];
        draw_person(&mut g, 3, 3, 6, 6, &p, 'b');
        have_chars.insert(c);
        known.insert(vec![c], (g, false));
        let mut m = emoji(name, "", g);
        m.chars = vec![c];
        components.push(m);
    }
    let covered = |key: &[char]| if key.len() == 1 { have_chars.contains(&key[0]) } else { have_seqs.contains(key) };

    let mut summary = Summary { total: list.len(), ..Summary::default() };
    // Pass order: untoned single, untoned longer, toned single, toned longer.
    let mut todo: Vec<usize> = Vec::new();
    let mut seen_keys: HashSet<Vec<char>> = HashSet::new();
    for (i, e) in list.iter().enumerate() {
        let key = strip_vs16(&e.cps);
        if covered(&key) || !seen_keys.insert(key) {
            continue;
        }
        todo.push(i);
    }
    summary.existing = list.len() - todo.len();
    let rank = |i: &usize| {
        let key = strip_vs16(&list[*i].cps);
        (key.iter().any(|c| is_tone(*c)), key.len() > 1)
    };
    let mut order = todo.clone();
    order.sort_by_key(|i| (rank(i), *i));

    let mut made: HashMap<usize, Made> = HashMap::new();
    for &i in &order {
        let e = &list[i];
        let key = strip_vs16(&e.cps);
        let tones: Vec<char> = key.iter().copied().filter(|c| is_tone(*c)).collect();
        let plain: Vec<char> = key.iter().copied().filter(|c| !is_tone(*c)).collect();
        let mut tone_set = tones.clone();
        tone_set.sort();
        tone_set.dedup();

        let label = abbreviation(e.name);
        let base_known = known.get(&plain).cloned();
        let mut result: Option<Made> = None;

        // 1. A recoloured copy.
        if tone_set.len() == 1 {
            if let Some((g, false)) = &base_known {
                let (light, shadow) = tone_keys(tone_set[0]);
                let mut r = *g;
                recolor(&mut r, 't', light);
                recolor(&mut r, 'T', shadow);
                if r != *g {
                    result = Some(Made::Art(r));
                }
            }
        }
        // 2. People.
        if result.is_none() {
            if let Some(sc) = scene(&key) {
                let prop_art = sc.prop.as_ref().and_then(|p| known.get(p)).map(|(g, _)| shrink(g));
                // A profession whose prop we cannot draw is a labelled tile instead.
                if sc.prop.is_none() || prop_art.is_some() {
                    result = Some(Made::Art(draw_scene(&sc, prop_art)));
                }
            }
        }
        // 3. A badge on a drawn emoji.
        if result.is_none() && tones.is_empty() {
            let parts: Vec<&[char]> = plain.split(|c| *c == ZWJ).collect();
            if parts.len() == 2 {
                let (a, b) = (known.get(parts[0]), known.get(parts[1]));
                if let (Some((ga, false)), Some((gb, _))) = (a, b) {
                    let mut g = *ga;
                    let small = shrink(gb);
                    for (dy, row) in small.iter().enumerate() {
                        for (dx, c) in row.iter().enumerate() {
                            if *c != '.' {
                                g[7 + dy][7 + dx] = *c;
                            }
                        }
                    }
                    result = Some(Made::Art(g));
                }
            }
        }
        // 4. A labelled tile; a skin tone is its colour.
        let made_one = result.unwrap_or_else(|| {
            let bg = if let Some(&t) = tone_set.first() { tone_keys(t).0 } else { group_colour(e.group) };
            Made::Placeholder(placeholder_tile(bg, label))
        });
        let (grid, is_placeholder) = match &made_one {
            Made::Art(g) => (*g, false),
            Made::Placeholder(g) => (*g, true),
        };
        known.insert(plain.clone(), known.get(&plain).cloned().unwrap_or((grid, is_placeholder)));
        known.insert(key, (grid, is_placeholder));
        made.insert(i, made_one);
    }

    let mut out = components;
    let mut index = 0u32;
    for &i in &todo {
        let e = &list[i];
        let key = strip_vs16(&e.cps);
        let (grid, placeholder) = match &made[&i] {
            Made::Art(g) => (*g, false),
            Made::Placeholder(g) => (*g, true),
        };
        let mut name = shortcode(e.name);
        if name.is_empty() || names.contains(&name) {
            name = format!("{name}_{:x}", e.cps[0] as u32);
        }
        while names.contains(&name) {
            name.push('_');
        }
        names.insert(name.clone());
        let mut m = emoji(&name, "", grid);
        m.placeholder = placeholder;
        if key.len() == 1 {
            m.chars = vec![key[0]];
        } else {
            m.chars = vec![char::from_u32(BASE + index).unwrap()];
            index += 1;
            m.sequences.push(e.cps.clone());
            if e.cps != key {
                m.sequences.push(key);
            }
        }
        if placeholder {
            summary.placeholders.push(e.name.to_string());
        } else {
            summary.generated += 1;
        }
        out.push(m);
    }
    (out, summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_has_every_standard_emoji() {
        let list = entries();
        assert!(list.len() > 3900, "{} entries", list.len());
        assert!(list.iter().any(|e| e.name == "grinning face"));
        assert!(list.iter().any(|e| e.cps == vec!['\u{1F469}', '\u{1F3FD}', ZWJ, '\u{1F692}']));
    }

    #[test]
    fn labels_come_from_the_name() {
        assert_eq!(abbreviation("melting face"), ['M', 'F']);
        assert_eq!(abbreviation("flag: Rwanda"), ['F', 'R']);
        assert_eq!(abbreviation("face with tears of joy"), ['F', 'T']);
        assert_eq!(abbreviation("piñata"), ['P', 'I']);
        assert_eq!(abbreviation("a"), ['X', 'X']);
    }

    #[test]
    fn shortcodes_are_plain_names() {
        assert_eq!(shortcode("woman firefighter: medium skin tone"), "woman_firefighter_medium_skin_tone");
        assert_eq!(shortcode("flag: São Tomé & Príncipe"), "flag_s_o_tom_pr_ncipe");
    }

    #[test]
    fn a_family_of_three_is_three_people() {
        let s = scene(&['\u{1F468}', ZWJ, '\u{1F469}', ZWJ, '\u{1F466}']).unwrap();
        assert_eq!(s.people.len(), 3);
        assert!(s.people[2].age == Age::Child);
    }

    #[test]
    fn two_skin_tones_in_a_couple_land_on_the_right_people() {
        let seq = ['\u{1F9D1}', '\u{1F3FB}', ZWJ, '\u{1F91D}', ZWJ, '\u{1F9D1}', '\u{1F3FF}'];
        let s = scene(&seq).unwrap();
        assert_eq!(s.people[0].skin, tone_keys('\u{1F3FB}'));
        assert_eq!(s.people[1].skin, tone_keys('\u{1F3FF}'));
        assert!(s.link == Some(Link::Handshake));
    }

    #[test]
    fn placeholders_with_different_names_differ() {
        assert_ne!(placeholder_tile('o', ['M', 'F']), placeholder_tile('o', ['R', 'F']));
    }
}
