//! Knapping: shaping a stone into a tool head by striking it.
//!
//! The pure rules live at the top of this file and know nothing about Bevy;
//! the screen and its systems are at the bottom.
//!
//! **The stone** is a [`GRID`] x [`GRID`] grid of cells, each either gone or
//! holding some *integrity* (up to [`INTEGRITY`]). A new stone is a rounded
//! blob whose edge wobbles a little from one seed to the next.
//!
//! **A strike** lands on one cell and is a circle of force: full [`FORCE`]
//! on the cell struck, falling off with distance (`FORCE * (1 - d/RADIUS)^2`)
//! out to [`RADIUS`]. Each cell loses that much integrity and is gone at
//! zero. So a hit clears the cell you aimed at, knocks the cells around it
//! most of the way out, and a second hit nearby finishes them - which is why
//! you pick where to strike with the neighbours in mind.
//!
//! **Cracks** are the risk of striking away from an edge. A strike's *depth*
//! is how many cells it is from open air (the stone's edge, or a hole already
//! chipped). At depth 1 - on the edge - nothing can crack. Deeper, there is a
//! chance ([`crack_chance`]) that a crack runs out of the strike. It heads
//! for the nearest open air, leaning toward it each step with some
//! randomness (so it bends like a real fracture), and when it gets there the
//! cells along it break away with a band of their neighbours. It can also run
//! out of energy first, which only leaves a flaw: weakened cells a later
//! strike will take. Anything cut off from the main body falls away.
//!
//! **Scoring** compares what's left with the tool's outline: an outline cell
//! chipped away costs twice what a leftover cell outside it does, because a
//! chip can't be put back. [`quality_from_score`] turns that into the tool's
//! quality; only an exact match is 100.
//!
//! Everything random comes from a seeded [`Rng`], so a given seed plays out
//! the same every time.

use crate::tools::MAX_QUALITY;

/// The stone is `GRID` cells on a side.
pub const GRID: usize = 16;
/// What an untouched cell can take before it breaks away.
pub const INTEGRITY: u8 = 70;
/// The force of a strike on the cell it lands on.
pub const FORCE: f32 = 160.0;
/// How far, in cells, a strike's force reaches.
pub const RADIUS: f32 = 3.2;

/// Stone isn't uniform: each cell's integrity is [`INTEGRITY`] give or take
/// this much, and nothing on the screen says which is which.
pub const INTEGRITY_SPREAD: u8 = 14;
/// A full strike's force varies by up to this fraction either way.
pub const FORCE_VARIANCE: f32 = 0.3;

/// A light tap (pressure flaking, as opposed to a full strike) is always
/// enough to break the one cell it lands on (`TAP_FORCE` is more than the
/// toughest cell can take) and dents its neighbours only a little. It's how the last cells next to the outline get cleared without
/// the full strike's force wrecking them.
pub const TAP_FORCE: f32 = 84.0;
/// ...but a stone only has so much give: this many taps per stone. It's what
/// keeps a perfect match rare - the full strikes have to leave nearly
/// nothing for the taps to clean up.
pub const MAX_TAPS: u32 = 4;
pub const TAP_RADIUS: f32 = 1.6;

/// A cell outline costs this many "leftover cells" when it's chipped away.
const MISS_WEIGHT: f32 = 2.0;
const EXCESS_WEIGHT: f32 = 1.0;
/// Scores at or below this are quality 0, and a perfect score is 100.
const SCORE_FLOOR: f32 = 0.25;
/// Bends the score-to-quality line so the top end is hard to reach.
const QUALITY_GAMMA: f32 = 1.5;

/// A crack gets this likely per cell of depth beyond the edge (depth 1).
const CRACK_PER_DEPTH: f32 = 0.12;
const CRACK_MAX_CHANCE: f32 = 0.9;
/// Each step a crack takes, the chance it runs out of energy.
const CRACK_ARREST: f32 = 0.06;
/// How much randomness bends a crack off the straight way out.
const CRACK_JITTER: f32 = 1.1;
/// How much a crack prefers to keep going the way it was going.
const CRACK_MOMENTUM: f32 = 0.6;
const CRACK_MAX_STEPS: usize = 24;
/// A flaw (a crack that ran out of energy) leaves its cells this fragile.
pub const FLAW_INTEGRITY: u8 = 25;

/// A small seeded random source (xorshift64*), so a knapping attempt is
/// reproducible from its seed.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number in `0..1`.
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// The shape a tool head is knapped toward: which cells belong to it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Outline {
    cells: [[bool; GRID]; GRID],
    area: u32,
}

impl Outline {
    /// Reads `GRID` rows of `GRID` characters, `#` for a cell of the head and
    /// anything else (`.`) for none. Errors say what's wrong with the rows.
    pub fn parse(rows: &[String]) -> Result<Self, String> {
        if rows.len() != GRID {
            return Err(format!("an outline needs {GRID} rows, this has {}", rows.len()));
        }
        let mut cells = [[false; GRID]; GRID];
        let mut area = 0;
        for (y, row) in rows.iter().enumerate() {
            if row.chars().count() != GRID {
                return Err(format!("outline row {y} needs {GRID} characters, it has {}", row.chars().count()));
            }
            for (x, c) in row.chars().enumerate() {
                if c == '#' {
                    cells[y][x] = true;
                    area += 1;
                }
            }
        }
        if area == 0 {
            return Err("an outline with no cells".into());
        }
        Ok(Self { cells, area })
    }

    pub fn contains(&self, x: usize, y: usize) -> bool {
        self.cells[y][x]
    }

    pub fn area(&self) -> u32 {
        self.area
    }
}

/// A stone being knapped: per-cell integrity, `0` meaning gone.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Stone {
    cells: [[u8; GRID]; GRID],
}

/// What one strike did, for the screen to react to.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Outcome {
    /// Cells the force itself knocked away.
    pub chipped: u32,
    /// A crack ran out of the strike.
    pub cracked: bool,
    /// ...and it ran out of energy before reaching open air, leaving a flaw.
    pub arrested: bool,
    /// Cells a crack broke away.
    pub broken: u32,
    /// Cells cut off from the main body and lost.
    pub fallen: u32,
}

impl Stone {
    /// A rounded blob whose edge wobbles with the seed.
    pub fn blob(rng: &mut Rng) -> Self {
        let (p1, p2) = (rng.unit() * std::f32::consts::TAU, rng.unit() * std::f32::consts::TAU);
        let mut cells = [[0u8; GRID]; GRID];
        let c = (GRID as f32 - 1.0) / 2.0;
        for (y, row) in cells.iter_mut().enumerate() {
            for (x, cell) in row.iter_mut().enumerate() {
                let (dx, dy) = (x as f32 - c, y as f32 - c);
                let angle = dy.atan2(dx);
                let radius = 7.4 + 0.3 * ((3.0 * angle + p1).sin() * 0.6 + (5.0 * angle + p2).sin() * 0.4);
                if dx.hypot(dy) <= radius {
                    // Hidden toughness, INTEGRITY - spread ..= INTEGRITY + spread - 1.
                    *cell = INTEGRITY - INTEGRITY_SPREAD + (rng.unit() * 2.0 * INTEGRITY_SPREAD as f32) as u8;
                }
            }
        }
        Self { cells }
    }

    pub fn integrity(&self, x: usize, y: usize) -> u8 {
        self.cells[y][x]
    }

    pub fn present(&self, x: usize, y: usize) -> bool {
        self.cells[y][x] > 0
    }

    pub fn count(&self) -> u32 {
        self.cells.iter().flatten().filter(|&&c| c > 0).count() as u32
    }

    fn is_present(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as usize) < GRID && (y as usize) < GRID && self.cells[y as usize][x as usize] > 0
    }

    /// For every present cell, how many cells it is from open air: 1 on an
    /// edge (next to a gap or the grid's border), 2 one cell in, and so on.
    /// `0` for a cell that's gone.
    pub fn depths(&self) -> [[u8; GRID]; GRID] {
        let mut depth = [[0u8; GRID]; GRID];
        let mut queue = std::collections::VecDeque::new();
        for y in 0..GRID as i32 {
            for x in 0..GRID as i32 {
                if !self.is_present(x, y) {
                    continue;
                }
                let open = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|&(dx, dy)| !self.is_present(x + dx, y + dy));
                if open {
                    depth[y as usize][x as usize] = 1;
                    queue.push_back((x, y));
                }
            }
        }
        while let Some((x, y)) = queue.pop_front() {
            let d = depth[y as usize][x as usize];
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = (x + dx, y + dy);
                if self.is_present(nx, ny) && depth[ny as usize][nx as usize] == 0 {
                    depth[ny as usize][nx as usize] = d.saturating_add(1);
                    queue.push_back((nx, ny));
                }
            }
        }
        depth
    }

    /// The force part of a strike alone: damages every cell within [`RADIUS`]
    /// of `(x, y)` and clears the ones that break. Returns how many went.
    pub fn apply_force(&mut self, x: usize, y: usize) -> u32 {
        self.apply(x, y, FORCE, RADIUS)
    }

    /// A light tap's force: breaks the cell struck and barely marks the rest.
    pub fn apply_tap(&mut self, x: usize, y: usize) -> u32 {
        self.apply(x, y, TAP_FORCE, TAP_RADIUS)
    }

    fn apply(&mut self, x: usize, y: usize, force: f32, radius: f32) -> u32 {
        let mut chipped = 0;
        let reach = radius.ceil() as i32;
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                let (cx, cy) = (x as i32 + dx, y as i32 + dy);
                if !self.is_present(cx, cy) {
                    continue;
                }
                let d = (dx as f32).hypot(dy as f32);
                if d >= radius {
                    continue;
                }
                let damage = (force * (1.0 - d / radius).powi(2)) as u32;
                let cell = &mut self.cells[cy as usize][cx as usize];
                if damage >= *cell as u32 {
                    *cell = 0;
                    chipped += 1;
                } else {
                    *cell -= damage as u8;
                }
            }
        }
        chipped
    }

    /// Removes everything not connected (4-way) to the largest piece.
    /// Returns how many cells fell away.
    fn drop_fragments(&mut self) -> u32 {
        let mut label = [[0u16; GRID]; GRID];
        let mut sizes = vec![0u32];
        for y in 0..GRID {
            for x in 0..GRID {
                if self.cells[y][x] == 0 || label[y][x] != 0 {
                    continue;
                }
                let id = sizes.len() as u16;
                sizes.push(0);
                let mut stack = vec![(x as i32, y as i32)];
                label[y][x] = id;
                while let Some((cx, cy)) = stack.pop() {
                    sizes[id as usize] += 1;
                    for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                        let (nx, ny) = (cx + dx, cy + dy);
                        if self.is_present(nx, ny) && label[ny as usize][nx as usize] == 0 {
                            label[ny as usize][nx as usize] = id;
                            stack.push((nx, ny));
                        }
                    }
                }
            }
        }
        let Some((keep, _)) = sizes.iter().enumerate().skip(1).max_by_key(|&(_, &n)| n) else { return 0 };
        let mut fallen = 0;
        for y in 0..GRID {
            for x in 0..GRID {
                if self.cells[y][x] > 0 && label[y][x] as usize != keep {
                    self.cells[y][x] = 0;
                    fallen += 1;
                }
            }
        }
        fallen
    }

    /// Lets a crack run out of `(x, y)`, which has depth `depth[y][x]`.
    /// Returns `(broken, arrested)`.
    fn crack(&mut self, x: usize, y: usize, depth: &[[u8; GRID]; GRID], rng: &mut Rng) -> (u32, bool) {
        let mut path = vec![(x as i32, y as i32)];
        let (mut cx, mut cy) = (x as i32, y as i32);
        let mut dir = (0.0f32, 0.0f32);
        let mut arrested = false;
        for _ in 0..CRACK_MAX_STEPS {
            if depth[cy as usize][cx as usize] <= 1 {
                break; // next to open air: through
            }
            if rng.unit() < CRACK_ARREST {
                arrested = true;
                break;
            }
            let mut best: Option<((i32, i32), f32)> = None;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if (dx, dy) == (0, 0) {
                        continue;
                    }
                    let (nx, ny) = (cx + dx, cy + dy);
                    if !self.is_present(nx, ny) || path.contains(&(nx, ny)) {
                        continue;
                    }
                    let step = ((dx as f32), (dy as f32));
                    let len = step.0.hypot(step.1);
                    let along = (dir.0 * step.0 + dir.1 * step.1) / len;
                    // Lean toward open air (lower depth), keep some heading,
                    // and wobble.
                    let score = depth[ny as usize][nx as usize] as f32 - CRACK_MOMENTUM * along + CRACK_JITTER * rng.unit();
                    if best.is_none_or(|(_, s)| score < s) {
                        best = Some(((nx, ny), score));
                    }
                }
            }
            let Some(((nx, ny), _)) = best else { break };
            dir = ((nx - cx) as f32, (ny - cy) as f32);
            (cx, cy) = (nx, ny);
            path.push((cx, cy));
        }
        if arrested || depth[cy as usize][cx as usize] > 1 {
            // Ran out of energy: a flaw, not a break.
            for &(px, py) in &path {
                let cell = &mut self.cells[py as usize][px as usize];
                *cell = (*cell).min(FLAW_INTEGRITY);
            }
            return (0, true);
        }
        // Through: the path and its 4-neighbours break away.
        let mut broken = 0;
        for &(px, py) in &path {
            for (dx, dy) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (bx, by) = (px + dx, py + dy);
                if self.is_present(bx, by) {
                    self.cells[by as usize][bx as usize] = 0;
                    broken += 1;
                }
            }
        }
        (broken, false)
    }
}

/// The chance a strike at this depth starts a crack: none on an edge, rising
/// with each cell deeper.
pub fn crack_chance(depth: u8) -> f32 {
    ((depth as f32 - 1.0) * CRACK_PER_DEPTH).clamp(0.0, CRACK_MAX_CHANCE)
}

/// How well a stone matches an outline, `0.0..=1.0`, `1.0` only when it
/// matches exactly.
pub fn score(stone: &Stone, outline: &Outline) -> f32 {
    raw_score(stone, outline).clamp(0.0, 1.0)
}

/// [`score`] before it's clamped at zero: keeps falling the more of the
/// stone is left over, so something hunting for improvement (a bot, or a
/// hint) still has a slope to follow far from the outline.
fn raw_score(stone: &Stone, outline: &Outline) -> f32 {
    let (mut missing, mut excess) = (0u32, 0u32);
    for y in 0..GRID {
        for x in 0..GRID {
            match (stone.present(x, y), outline.contains(x, y)) {
                (false, true) => missing += 1,
                (true, false) => excess += 1,
                _ => {}
            }
        }
    }
    let penalty = (MISS_WEIGHT * missing as f32 + EXCESS_WEIGHT * excess as f32) / outline.area() as f32;
    1.0 - penalty
}

/// The quality (`0..=100`) of a tool knapped to this score. Rises slowly at
/// first and steeply near the top, and is `100` only for a perfect score.
pub fn quality_from_score(score: f32) -> u8 {
    let t = ((score - SCORE_FLOOR) / (1.0 - SCORE_FLOOR)).clamp(0.0, 1.0);
    ((MAX_QUALITY as f32 * t.powf(QUALITY_GAMMA) + 1e-3).floor() as u8).min(MAX_QUALITY)
}

/// One attempt: a stone, the outline it's being shaped toward, and the
/// seeded randomness its cracks draw on.
#[derive(Clone, Debug)]
pub struct Session {
    pub stone: Stone,
    pub outline: Outline,
    rng: Rng,
    /// Full strikes and taps both.
    pub strikes: u32,
    taps: u32,
}

impl Session {
    pub fn new(outline: Outline, seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let stone = Stone::blob(&mut rng);
        Self { stone, outline, rng, strikes: 0, taps: 0 }
    }

    /// How many light taps are left.
    pub fn taps_left(&self) -> u32 {
        MAX_TAPS - self.taps
    }

    /// Strikes cell `(x, y)`. `None`, and nothing happens, if there's no
    /// stone there.
    pub fn strike(&mut self, x: usize, y: usize) -> Option<Outcome> {
        if x >= GRID || y >= GRID || !self.stone.present(x, y) {
            return None;
        }
        self.strikes += 1;
        let depth = self.stone.depths();
        let d = depth[y][x];
        // Every strike lands a little harder or softer than the last.
        let force = FORCE * (1.0 + (self.rng.unit() * 2.0 - 1.0) * FORCE_VARIANCE);
        let chipped = self.stone.apply(x, y, force, RADIUS);
        let mut outcome = Outcome { chipped, ..Outcome::default() };
        if self.rng.unit() < crack_chance(d) {
            // The crack runs from the strike cell, which the force just
            // cleared, so give it a footing by treating it as stone again for
            // the path (its depth is what it was).
            let mut probe = self.stone.clone();
            probe.cells[y][x] = 1;
            let (broken, arrested) = probe.crack(x, y, &depth, &mut self.rng);
            probe.cells[y][x] = 0;
            self.stone = probe;
            outcome.cracked = true;
            outcome.arrested = arrested;
            outcome.broken = broken;
        }
        outcome.fallen = self.stone.drop_fragments();
        Some(outcome)
    }

    /// A light tap on `(x, y)`: clears that cell, dents the four around it,
    /// and can't start a crack. `None` if there's no stone there or no taps
    /// are left.
    pub fn tap(&mut self, x: usize, y: usize) -> Option<Outcome> {
        if x >= GRID || y >= GRID || !self.stone.present(x, y) || self.taps_left() == 0 {
            return None;
        }
        self.strikes += 1;
        self.taps += 1;
        let chipped = self.stone.apply_tap(x, y);
        let fallen = self.stone.drop_fragments();
        Some(Outcome { chipped, fallen, ..Outcome::default() })
    }

    pub fn score(&self) -> f32 {
        score(&self.stone, &self.outline)
    }

    pub fn quality(&self) -> u8 {
        quality_from_score(self.score())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(lines: &[&str]) -> Vec<String> {
        let mut v: Vec<String> = lines.iter().map(|s| format!("{s:.<16}")).collect();
        while v.len() < GRID {
            v.push(".".repeat(GRID));
        }
        v
    }

    /// A small square in the middle.
    fn square() -> Outline {
        let mut lines = vec![String::new(); 6];
        lines.extend((0..4).map(|_| "......####".to_string()));
        Outline::parse(&rows(&lines.iter().map(|s| s.as_str()).collect::<Vec<_>>())).unwrap()
    }

    #[test]
    fn an_outline_reads_its_rows_and_refuses_a_malformed_one() {
        assert_eq!(square().area(), 16);
        assert!(square().contains(6, 6) && !square().contains(5, 6));
        assert!(Outline::parse(&["#".repeat(GRID)]).is_err(), "too few rows");
        let mut bad = rows(&["#"]);
        bad[3] = "###".into();
        assert!(Outline::parse(&bad).unwrap_err().contains("row 3"));
        assert!(Outline::parse(&rows(&[])).unwrap_err().contains("no cells"));
    }

    #[test]
    fn a_new_stone_is_a_blob_that_varies_with_the_seed() {
        let a = Stone::blob(&mut Rng::new(1));
        let b = Stone::blob(&mut Rng::new(2));
        assert_eq!(a, Stone::blob(&mut Rng::new(1)), "same seed, same stone");
        assert_ne!(a, b);
        assert!((140..=200).contains(&a.count()), "{} cells", a.count());
        assert!(a.present(8, 8) && !a.present(0, 0), "round, with the corners missing");
    }

    #[test]
    fn depth_counts_cells_in_from_open_air() {
        let stone = Stone::blob(&mut Rng::new(3));
        let depth = stone.depths();
        assert_eq!(depth[0][0], 0, "no stone there");
        assert_eq!(depth[8][8].min(9), depth[8][8], "the middle is deep");
        assert!(depth[8][8] >= 6);
        // Every present cell has a depth, and edge cells are exactly 1.
        for y in 0..GRID {
            for x in 0..GRID {
                assert_eq!(stone.present(x, y), depth[y][x] > 0);
            }
        }
        // A hole makes its neighbours edges.
        let mut holed = stone.clone();
        holed.cells[8][8] = 0;
        assert_eq!(holed.depths()[8][9], 1);
    }

    #[test]
    fn a_strike_clears_its_cell_weakens_the_ring_around_it_and_a_second_finishes_it() {
        let mut stone = Stone::blob(&mut Rng::new(4));
        let before = stone.count();
        let far_before = stone.integrity(8, 12);
        let chipped = stone.apply_force(8, 8);
        assert!(!stone.present(8, 8), "the struck cell is gone");
        assert!(stone.present(8, 7) || chipped > 1, "a neighbour was hit hard");
        // Force falls off with distance: a cell right beside takes more than one two away.
        let fresh = Stone::blob(&mut Rng::new(4));
        let near = fresh.integrity(10, 8) as i32 - stone.integrity(10, 8) as i32;
        let far = fresh.integrity(11, 8) as i32 - stone.integrity(11, 8) as i32;
        assert!(near > far, "near {near}, far {far}");
        assert_eq!(stone.integrity(8, 12), far_before, "nothing outside the radius is touched");
        assert_eq!(before - stone.count(), chipped);
        // Hitting beside the damaged cells takes them out.
        let weak: Vec<_> = (0..GRID).flat_map(|y| (0..GRID).map(move |x| (x, y))).filter(|&(x, y)| {
            stone.present(x, y) && stone.integrity(x, y) < INTEGRITY
        }).collect();
        assert!(!weak.is_empty());
        let after_first = stone.count();
        stone.apply_force(8, 9);
        assert!(after_first - stone.count() >= 2, "the second strike finished some of them");
    }

    #[test]
    fn force_reaches_exactly_to_its_radius() {
        let mut stone = Stone { cells: [[INTEGRITY; GRID]; GRID] };
        stone.apply_force(8, 8);
        for y in 0..GRID {
            for x in 0..GRID {
                let d = ((x as f32 - 8.0).hypot(y as f32 - 8.0)).abs();
                if d >= RADIUS {
                    assert_eq!(stone.integrity(x, y), INTEGRITY, "({x},{y}) is {d} away");
                } else if d <= RADIUS - 0.3 {
                    // (Right at the rim the damage rounds down to nothing.)
                    assert!(stone.integrity(x, y) < INTEGRITY, "({x},{y}) is {d} away and untouched");
                }
            }
        }
    }

    #[test]
    fn striking_nothing_does_nothing() {
        let outline = square();
        let mut s = Session::new(outline, 5);
        assert_eq!(s.strike(0, 0), None, "no stone in the corner");
        assert_eq!(s.strike(99, 99), None);
        assert_eq!(s.strikes, 0);
    }

    #[test]
    fn edges_never_crack_and_the_risk_rises_with_depth() {
        assert_eq!(crack_chance(0), 0.0);
        assert_eq!(crack_chance(1), 0.0);
        let mut last = 0.0;
        for depth in 2..=9 {
            let c = crack_chance(depth);
            assert!(c > last, "depth {depth}");
            assert!(c <= CRACK_MAX_CHANCE);
            last = c;
        }
        assert_eq!(crack_chance(30), CRACK_MAX_CHANCE, "it tops out, never certain");
        // And in play: a long run of edge strikes never cracks anything.
        for seed in 0..20 {
            let mut s = Session::new(square(), seed);
            for _ in 0..40 {
                let depth = s.stone.depths();
                let Some((x, y)) = (0..GRID).flat_map(|y| (0..GRID).map(move |x| (x, y))).find(|&(x, y)| depth[y][x] == 1) else { break };
                let outcome = s.strike(x, y).unwrap();
                assert!(!outcome.cracked, "an edge strike cracked (seed {seed})");
            }
        }
    }

    /// Strikes the middle of a fresh stone with seeds until it cracks.
    fn cracked_session() -> (Session, Outcome, Stone) {
        for seed in 0..500 {
            let mut s = Session::new(square(), seed);
            let before = s.stone.clone();
            // Deep, but with the middle ring already open so the crack is short.
            let outcome = s.strike(8, 8).unwrap();
            if outcome.cracked && !outcome.arrested {
                return (s, outcome, before);
            }
        }
        panic!("never saw a crack");
    }

    #[test]
    fn a_crack_through_removes_a_band_and_costs_more_than_the_force_alone() {
        let (s, outcome, before) = cracked_session();
        let mut force_only = before.clone();
        let chipped = force_only.apply_force(8, 8);
        assert!(outcome.broken > 0);
        assert!(before.count() - s.stone.count() > chipped, "the crack took more than the strike");
    }

    #[test]
    fn a_crack_that_runs_out_of_energy_leaves_a_flaw_not_a_break() {
        for seed in 0..2000 {
            let mut s = Session::new(square(), seed);
            let outcome = s.strike(8, 8).unwrap();
            if outcome.cracked && outcome.arrested {
                assert_eq!(outcome.broken, 0);
                let weak = (0..GRID).flat_map(|y| (0..GRID).map(move |x| (x, y))).filter(|&(x, y)| {
                    s.stone.present(x, y) && s.stone.integrity(x, y) <= FLAW_INTEGRITY
                }).count();
                assert!(weak > 0, "an arrested crack should leave weakened cells");
                return;
            }
        }
        panic!("never saw an arrested crack");
    }

    #[test]
    fn a_crack_bends_toward_the_nearest_open_air() {
        // Open a hole just off to the left of centre: a crack out of the
        // middle should mostly end up going that way.
        let (mut to_hole, mut total) = (0, 0);
        for seed in 0..400 {
            let mut rng = Rng::new(seed);
            let mut stone = Stone::blob(&mut rng);
            for y in 6..=10 {
                stone.cells[y][3] = 0; // a gap on the left
            }
            let depth = stone.depths();
            let mut probe = stone.clone();
            let (broken, arrested) = probe.crack(8, 8, &depth, &mut rng);
            if arrested || broken == 0 {
                continue;
            }
            total += 1;
            let removed_left = (0..GRID).flat_map(|y| (0..8).map(move |x| (x, y))).filter(|&(x, y)| stone.present(x, y) && !probe.present(x, y)).count();
            let removed_right = (0..GRID).flat_map(|y| (9..GRID).map(move |x| (x, y))).filter(|&(x, y)| stone.present(x, y) && !probe.present(x, y)).count();
            if removed_left > removed_right {
                to_hole += 1;
            }
        }
        assert!(total > 100, "only {total} cracks got through");
        assert!(to_hole as f32 > 0.8 * total as f32, "{to_hole} of {total} headed for the gap");
    }

    #[test]
    fn a_piece_cut_off_from_the_rest_falls_away() {
        let mut stone = Stone { cells: [[0; GRID]; GRID] };
        for y in 2..6 {
            for x in 2..6 {
                stone.cells[y][x] = INTEGRITY;
            }
        }
        for y in 9..12 {
            for x in 9..12 {
                stone.cells[y][x] = INTEGRITY;
            }
        }
        let fallen = stone.drop_fragments();
        assert_eq!(fallen, 9, "the smaller block is lost");
        assert_eq!(stone.count(), 16);
        assert!(stone.present(3, 3) && !stone.present(10, 10));
        assert_eq!(Stone { cells: [[0; GRID]; GRID] }.drop_fragments(), 0, "nothing to drop from nothing");
    }

    #[test]
    fn scoring_is_one_only_for_an_exact_match_and_chipping_the_outline_costs_double() {
        let outline = square();
        let mut exact = Stone { cells: [[0; GRID]; GRID] };
        for y in 6..10 {
            for x in 6..10 {
                exact.cells[y][x] = INTEGRITY;
            }
        }
        assert_eq!(score(&exact, &outline), 1.0);
        assert_eq!(quality_from_score(1.0), 100);

        let mut one_extra = exact.clone();
        one_extra.cells[0][0] = INTEGRITY;
        let mut one_missing = exact.clone();
        one_missing.cells[6][6] = 0;
        let (extra, missing) = (score(&one_extra, &outline), score(&one_missing, &outline));
        assert!(extra < 1.0 && missing < extra, "{extra} vs {missing}");
        assert!(((1.0 - extra) * 2.0 - (1.0 - missing)).abs() < 1e-5, "a missing cell costs twice an extra one");
        assert_eq!(score(&Stone { cells: [[0; GRID]; GRID] }, &outline), 0.0);
        assert!(score(&Stone::blob(&mut Rng::new(1)), &outline) < 0.5, "an untouched stone is a poor match");
    }

    #[test]
    fn quality_climbs_with_score_and_is_a_hundred_only_at_the_top() {
        let mut last = 0;
        let mut s = 0.0;
        while s < 1.0 {
            let q = quality_from_score(s);
            assert!(q >= last, "quality fell at score {s}");
            assert!(q < 100, "score {s} reached 100");
            last = q;
            s += 0.001;
        }
        assert_eq!(quality_from_score(0.0), 0);
        assert_eq!(quality_from_score(SCORE_FLOOR), 0);
        assert_eq!(quality_from_score(1.0), 100);
        assert!(quality_from_score(0.995) >= 97, "one stray cell is still Epic");
    }

    #[test]
    fn the_same_seed_plays_out_the_same_way() {
        let play = |seed| {
            let mut s = Session::new(square(), seed);
            let mut outcomes = Vec::new();
            for (x, y) in [(8, 8), (5, 5), (10, 9), (3, 8), (8, 3)] {
                outcomes.push(s.strike(x, y));
            }
            (s.stone, outcomes)
        };
        assert_eq!(play(9), play(9));
        assert_ne!(play(9).0, play(10).0);
    }

    // ---- how hard is it? ---------------------------------------------
    //
    // Bots play the real rules with the shipped outlines so the quality
    // curve is tuned against measurements, not guessed.

    use crate::blocks::BlockRegistry;
    use crate::crafting::RecipeBook;
    use crate::item_registry::ItemRegistry;

    fn shipped_outlines() -> Vec<(String, Outline)> {
        let blocks = BlockRegistry::with_defaults();
        let items = ItemRegistry::with_defaults(&blocks);
        let book = RecipeBook::with_defaults(&blocks, &items);
        let found: Vec<_> = book.recipes.iter().filter_map(|r| r.knap.as_ref().map(|k| (r.id.clone(), k.outline.clone()))).collect();
        assert!(!found.is_empty(), "no knapping recipes shipped");
        found
    }

    fn cells() -> impl Iterator<Item = (usize, usize)> {
        (0..GRID).flat_map(|y| (0..GRID).map(move |x| (x, y)))
    }

    /// Strikes the best-scoring cell (judged on force alone, as a player
    /// can) among those at most `max_depth` in from open air, until nothing
    /// improves the score.
    fn greedy_bot(session: &mut Session, max_depth: u8, max_strikes: u32) {
        while session.strikes < max_strikes {
            let depth = session.stone.depths();
            let base = raw_score(&session.stone, &session.outline);
            let mut best: Option<((usize, usize), f32)> = None;
            for (x, y) in cells() {
                if depth[y][x] == 0 || depth[y][x] > max_depth {
                    continue;
                }
                let mut trial = session.stone.clone();
                trial.apply_force(x, y);
                let gain = raw_score(&trial, &session.outline) - base;
                if gain > 1e-6 && best.is_none_or(|(_, g)| gain > g) {
                    best = Some(((x, y), gain));
                }
            }
            let Some(((x, y), _)) = best else { return };
            session.strike(x, y);
        }
    }

    /// Greedy heavy strikes, then greedy light taps on whatever still
    /// improves the score, until the session refuses another tap.
    fn two_phase_bot(session: &mut Session) {
        greedy_bot(session, 1, 200);
        for _ in 0..50 {
            let base = raw_score(&session.stone, &session.outline);
            let mut best: Option<((usize, usize), f32)> = None;
            for (x, y) in cells() {
                if !session.stone.present(x, y) {
                    continue;
                }
                let mut trial = session.stone.clone();
                trial.apply_tap(x, y);
                let gain = raw_score(&trial, &session.outline) - base;
                if gain > 1e-6 && best.is_none_or(|(_, g)| gain > g) {
                    best = Some(((x, y), gain));
                }
            }
            let Some(((x, y), _)) = best else { return };
            if session.tap(x, y).is_none() {
                return;
            }
        }
    }

    /// Strikes random cells of the stone, anywhere.
    fn reckless_bot(session: &mut Session, strikes: u32, rng: &mut Rng) {
        for _ in 0..strikes {
            let present: Vec<_> = cells().filter(|&(x, y)| session.stone.present(x, y)).collect();
            if present.is_empty() {
                return;
            }
            let (x, y) = present[(rng.unit() * present.len() as f32) as usize % present.len()];
            session.strike(x, y);
        }
    }

    /// Counts of Trash/Common/Rare/Epic/Legendary over `seeds` attempts.
    fn distribution(play: impl Fn(&mut Session, u64), seeds: u64) -> ([u32; 5], f32) {
        let rarities = crate::tools::RarityBook::with_defaults();
        let mut counts = [0u32; 5];
        let mut total_strikes = 0;
        let mut attempts = 0;
        for (_, outline) in shipped_outlines() {
            for seed in 0..seeds {
                let mut session = Session::new(outline.clone(), seed);
                play(&mut session, seed);
                total_strikes += session.strikes;
                attempts += 1;
                counts[rarities.index_at(session.quality())] += 1;
            }
        }
        (counts, total_strikes as f32 / attempts as f32)
    }

    fn pct(counts: [u32; 5]) -> [f32; 5] {
        let n: u32 = counts.iter().sum();
        counts.map(|c| 100.0 * c as f32 / n as f32)
    }

    #[test]
    fn a_careful_player_usually_makes_rare_tools_sometimes_epic_and_legendary_is_a_long_shot() {
        // The bot knows exactly what a strike does, which a person doesn't,
        // so this is a strong player. Measured (seeds 0..300 on each of the
        // five shipped outlines): ~0% trash, ~6% common, ~61% rare, ~28%
        // epic, ~4% legendary, in about 26 clicks.
        let (counts, clicks) = distribution(|s, _| two_phase_bot(s), 300);
        let p = pct(counts);
        assert!(p[0] < 2.0, "{p:?}");
        assert!(p[2] + p[3] > 70.0, "most careful tools should be rare or epic: {p:?}");
        assert!(p[3] > 10.0, "epic should be within reach: {p:?}");
        assert!(counts[4] > 0, "legendary has to be reachable at all: {p:?}");
        assert!(p[4] < 10.0, "legendary should stand out, not be routine: {p:?}");
        assert!((18.0..=36.0).contains(&clicks), "{clicks} clicks a tool");
    }

    #[test]
    fn striking_at_random_wrecks_the_stone() {
        let (counts, _) = distribution(|s, seed| reckless_bot(s, 25, &mut Rng::new(seed + 1000)), 150);
        assert_eq!(counts[0], counts.iter().sum::<u32>(), "random hits should always be trash: {counts:?}");
    }

    #[test]
    fn heavy_strikes_alone_cannot_make_a_perfect_tool() {
        // Without the taps, no heavy-only play ever matched an outline: the
        // hardest part of the stone is the cells beside the outline, which a
        // full strike can't clear without wrecking the outline itself.
        let (counts, _) = distribution(|s, _| greedy_bot(s, 1, 200), 200);
        assert_eq!(counts[4], 0, "{counts:?}");
        let (counts, _) = distribution(|s, _| greedy_bot(s, 2, 200), 200);
        assert_eq!(counts[4], 0, "{counts:?}");
    }

    #[test]
    fn a_tap_breaks_exactly_the_cell_it_lands_on_and_only_dents_the_rest() {
        for seed in 0..30 {
            let mut s = Session::new(square(), seed);
            let before = s.stone.clone();
            let (x, y) = (8, 8);
            let outcome = s.tap(x, y).unwrap();
            assert_eq!(outcome.chipped, 1, "seed {seed}: a tap clears one cell, however tough it was");
            assert!(!s.stone.present(x, y));
            assert!(!outcome.cracked, "a tap can't crack");
            for (nx, ny) in [(8, 7), (9, 8), (7, 8), (8, 9)] {
                assert!(s.stone.present(nx, ny), "a neighbour broke");
                assert!(s.stone.integrity(nx, ny) < before.integrity(nx, ny), "neighbours are dented");
            }
            assert_eq!(s.stone.integrity(8, 6), before.integrity(8, 6), "two away is untouched");
        }
        // A tap even in the deep middle is safe, unlike a full strike.
        let mut s = Session::new(square(), 3);
        assert!(!s.tap(8, 8).unwrap().cracked);
    }

    #[test]
    fn taps_run_out() {
        let mut s = Session::new(square(), 1);
        assert_eq!(s.taps_left(), MAX_TAPS);
        for i in 0..MAX_TAPS {
            assert!(s.tap(8, 8 - i as usize).is_some() || s.tap(8, 9).is_some());
        }
        assert_eq!(s.taps_left(), 0);
        assert_eq!(s.tap(4, 8), None, "no taps left");
        assert!(s.strike(4, 8).is_some(), "full strikes aren't limited");
    }

    #[test]
    fn every_shipped_outline_fits_inside_every_stone_and_is_one_piece() {
        for (name, outline) in shipped_outlines() {
            for seed in 0..500 {
                let stone = Stone::blob(&mut Rng::new(seed));
                for (x, y) in cells().filter(|&(x, y)| outline.contains(x, y)) {
                    assert!(stone.present(x, y), "{name}: ({x},{y}) is outside the stone with seed {seed}");
                }
            }
            // One 4-connected piece, or the tool head couldn't hold together.
            let wanted: Vec<_> = cells().filter(|&(x, y)| outline.contains(x, y)).collect();
            let mut seen = std::collections::HashSet::from([wanted[0]]);
            let mut stack = vec![wanted[0]];
            while let Some((x, y)) = stack.pop() {
                for (dx, dy) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx < 0 || ny < 0 || nx >= GRID as i32 || ny >= GRID as i32 {
                        continue;
                    }
                    let n = (nx as usize, ny as usize);
                    if outline.contains(n.0, n.1) && seen.insert(n) {
                        stack.push(n);
                    }
                }
            }
            assert_eq!(seen.len(), wanted.len(), "{name} is in more than one piece");
        }
    }
}
