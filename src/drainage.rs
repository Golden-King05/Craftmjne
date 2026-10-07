//! The world's drainage network: which way water flows from every point on
//! land, how much land drains through each point, and how high a river's
//! surface stands there. Rivers in `terrain.rs` are drawn from this.
//!
//! The world is divided into `CELL`-block cells, each with one node (its
//! center, jittered so river paths meander instead of following a grid).
//! Every node drains to whichever of its 8 neighbours is lowest by a
//! *routing* height - real terrain plus a gentle tilt toward the sea - so
//! water heads downhill locally but still finds its way to the coast
//! instead of dead-ending in every dip.
//!
//! **Nothing here is bounded to a region.** A node's flow direction depends
//! only on its neighbours, so the network is one continuous thing across
//! the whole world. The non-local quantities - how much land drains through
//! a node (`accumulation`) and its water level (`water_level`, which needs
//! everything upstream) - are computed on first use by walking upstream,
//! and memoized per node in `Tile`s, so each is computed once no matter how
//! many chunks ask. Every value is a pure function of the terrain, so the
//! order chunks generate in never changes the answer.
//!
//! **Water never just stops in a hollow.** Steepest descent on any real
//! surface finds pits - low spots with nothing lower around them - and on
//! flat plains the sea-ward tilt is too weak to rule them out. A real
//! depression fills up and overflows at the lowest point of its rim, so
//! here every pit has a *spill path* (`breach`): flooding outward from the
//! pit, lowest rim first, until reaching a cell that drains somewhere
//! strictly lower than the pit itself. The cells along that path flow along
//! it instead of by steepest descent (`downstream`), and the river carries
//! on. Each spill leads to strictly lower ground than the pit it left, so
//! following spills can never come back round - and like everything else
//! here, a spill path is a pure function of the terrain.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use crate::noise::hash2;

/// Size of one drainage cell, in blocks.
pub const CELL: i32 = 16;
/// Cells per side of one cache `Tile`.
const TILE: i32 = 32;
/// How far (as a fraction of `CELL`) a node may sit from its cell's
/// center - enough to bend river paths off the grid, not so much that two
/// neighbours' nodes cross.
const JITTER: f64 = 0.35;

/// What a drainage network needs to know about the land - implemented by
/// `terrain::TerrainGenerator`, kept as a trait so this module's algorithms
/// can be driven by a hand-built landscape in tests.
pub trait Landscape: Sync {
    /// Height water is routed by - real terrain plus any sea-ward tilt.
    /// Flow always goes to a strictly lower routing height, so the network
    /// can never contain a cycle.
    fn routing_height(&self, x: f64, z: f64) -> f64;
    /// Real (pre-river) terrain height - what a river's banks stand at.
    fn ground_height(&self, x: f64, z: f64) -> f64;
    /// Whether this point is open water already (the sea): rivers end
    /// there rather than continuing across the sea floor.
    fn is_sea(&self, x: f64, z: f64) -> bool;
    /// How far a river here sits below its banks (`0.0` = flush).
    fn incision(&self, x: f64, z: f64) -> f64;
    /// The lowest a river's surface can ever be (sea level).
    fn base_level(&self) -> f64;
    /// Whether the depression around the pit at `(x, z)`, covering `cells`
    /// drainage cells when filled to its rim, keeps its water instead of
    /// overflowing - a salt sea, where rivers end. Decided from the basin
    /// alone, never from how much flows into it, since whether it spills
    /// changes where water flows.
    fn keeps_its_water(&self, _x: f64, _z: f64, _cells: usize) -> bool {
        false
    }
}

/// One node's memoized values. Each is computed at most once (`OnceLock`)
/// and is a pure function of the landscape, so two threads racing to fill
/// the same one just compute the same number.
#[derive(Default)]
struct Node {
    route: OnceLock<f32>,
    /// Steepest descent: an index into `NEIGHBOURS`, or `PIT` if nothing
    /// around is lower (or this is the sea).
    down: OnceLock<u8>,
    /// Where water actually goes - `down`, unless this cell lies on a pit's
    /// spill path (see `downstream`).
    flow: OnceLock<u8>,
    /// Where following `down` alone ends up: the sea, or a pit.
    terminal: OnceLock<Cell>,
    /// `Landscape::is_sea` at this node - asked constantly (every column
    /// checks the basins around it) and not cheap to evaluate.
    sea: OnceLock<bool>,
    accumulation: OnceLock<f32>,
    /// The longest flow path upstream of this cell, in cells, including it.
    length: OnceLock<f32>,
    water: OnceLock<f32>,
}

/// The depression around a pit: the cells water would cover filling it to
/// its rim (`footprint`, the pit included), and - unless it keeps its
/// water - its way out: the cells from the pit to its `outlet`, each mapped
/// to the next one along.
pub struct Basin {
    pub footprint: HashSet<Cell>,
    next: HashMap<Cell, Cell>,
    outlet: Option<Cell>,
}

impl Basin {
    /// No way out: a salt sea, or a true sink with no outlet in reach.
    pub fn keeps_its_water(&self) -> bool {
        self.outlet.is_none()
    }
}

/// A true sink's footprint is everything its failed search covered, far too
/// much to call a lake; only its innermost (lowest-filling) cells count.
const SINK_FOOTPRINT: usize = 48;

/// How many cells a pit's flood may cover looking for its way out before
/// giving up and staying a true sink - an inland basin with no outlet in
/// reach. Real pits on this terrain are shallow and small; this bound is
/// only what keeps a pathological one from running away.
const MAX_SPILL_SEARCH: usize = 4096;
/// How far `terminal` follows steepest descent before deciding the water
/// simply drains away - a continent is under a thousand cells across, so a
/// chain still going after this many is heading downhill indefinitely,
/// which only a synthetic landscape can do, and is as good as reaching the
/// sea.
const MAX_CHAIN: usize = 8192;
/// The terminal of a chain cut off at `MAX_CHAIN`.
const DRAINS_AWAY: Cell = (i32::MIN, i32::MIN);

const PIT: u8 = u8::MAX;

/// The largest catchment (in cells) `accumulation` ever reports - roughly a
/// 5 km x 5 km basin, far past the size where rivers stop getting any wider.
/// Real terrain's catchments are bounded by its divides, so this is purely a
/// safety valve: an upstream walk that finds more cells than this stops and
/// reports the cap, so no pathological landscape can make one walk run away.
/// The cap is decided by the catchment's *true* size (you only stop after
/// actually finding more than this many cells), so it is still the same
/// answer whichever chunk happens to ask first.
pub const MAX_ACCUMULATION: f32 = 100_000.0;
const NEIGHBOURS: [(i32, i32); 8] = [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)];

struct Tile {
    nodes: Box<[Node]>,
}

impl Tile {
    fn new() -> Self {
        Self { nodes: (0..TILE * TILE).map(|_| Node::default()).collect() }
    }
}

/// A cell coordinate (`x / CELL`, `z / CELL`, floored).
pub type Cell = (i32, i32);

pub struct Network {
    seed: u32,
    tiles: Mutex<HashMap<Cell, Arc<Tile>>>,
    /// Each pit's spill path, or `None` for a true sink. Pits are rare, so
    /// these live in one map rather than on every node.
    spills: Mutex<HashMap<Cell, Arc<Basin>>>,
}

/// An `f32` as a key that sorts in numeric order, negatives included.
fn ordered(v: f32) -> u32 {
    let bits = v.to_bits();
    if bits >> 31 == 1 { !bits } else { bits | 1 << 31 }
}

impl Network {
    pub fn new(seed: u32) -> Self {
        Self { seed, tiles: Mutex::new(HashMap::new()), spills: Mutex::new(HashMap::new()) }
    }

    fn with_node<R>(&self, cell: Cell, f: impl FnOnce(&Node) -> R) -> R {
        let key = (cell.0.div_euclid(TILE), cell.1.div_euclid(TILE));
        let tile = self.tiles.lock().unwrap().entry(key).or_insert_with(|| Arc::new(Tile::new())).clone();
        let (lx, lz) = (cell.0.rem_euclid(TILE), cell.1.rem_euclid(TILE));
        f(&tile.nodes[(lx + TILE * lz) as usize])
    }

    /// A cell's node position, in world blocks.
    pub fn node_pos(&self, cell: Cell) -> (f64, f64) {
        let jx = (hash2(cell.0, cell.1, self.seed ^ 0x5bd1_e995) as f64 - 0.5) * 2.0 * JITTER;
        let jz = (hash2(cell.0, cell.1, self.seed ^ 0x27d4_eb2d) as f64 - 0.5) * 2.0 * JITTER;
        (((cell.0 as f64) + 0.5 + jx) * CELL as f64, ((cell.1 as f64) + 0.5 + jz) * CELL as f64)
    }

    /// Routing samples each cell at its true center, not its jittered node:
    /// jitter is only for *drawing* rivers off the grid. Routing by the
    /// jittered position let a node pushed a few blocks up a steep valley
    /// side out-rank its own downstream neighbour, leaving a fake pit
    /// halfway down a perfectly good valley.
    fn route(&self, land: &impl Landscape, cell: Cell) -> f32 {
        let (x, z) = (((cell.0 as f64) + 0.5) * CELL as f64, ((cell.1 as f64) + 0.5) * CELL as f64);
        self.with_node(cell, |n| *n.route.get_or_init(|| land.routing_height(x, z) as f32))
    }

    /// Where water at `cell` flows next, or `None` if it stops here: the
    /// sea, or a true sink with no way out in reach. Steepest descent,
    /// except along a pit's spill path - see this module's doc comment.
    ///
    /// A cell can lie on the spill paths of several pits in the chain its
    /// water passes through (its own terminal pit, the pit *that* spills
    /// into, and so on, each strictly lower than the last). It follows the
    /// lowest one's: that's where the water was going anyway.
    pub fn downstream(&self, land: &impl Landscape, cell: Cell) -> Option<Cell> {
        let code = self.with_node(cell, |n| n.flow.get().copied()).unwrap_or_else(|| {
            let natural = self.natural_down_code(land, cell);
            let mut code = natural;
            let mut pit = self.terminal(land, cell);
            for _ in 0..64 {
                if self.ends_at_sea(land, pit) {
                    break;
                }
                let spill = self.basin(land, pit);
                let Some(outlet) = spill.outlet else { break };
                if let Some(&next) = spill.next.get(&cell) {
                    let delta = (next.0 - cell.0, next.1 - cell.1);
                    code = NEIGHBOURS.iter().position(|&d| d == delta).unwrap() as u8;
                }
                pit = self.terminal(land, outlet);
            }
            self.with_node(cell, |n| *n.flow.get_or_init(|| code))
        });
        (code != PIT).then(|| {
            let (dx, dz) = NEIGHBOURS[code as usize];
            (cell.0 + dx, cell.1 + dz)
        })
    }

    fn ends_at_sea(&self, land: &impl Landscape, cell: Cell) -> bool {
        if cell == DRAINS_AWAY {
            return true;
        }
        self.with_node(cell, |n| n.sea.get().copied()).unwrap_or_else(|| {
            let pos = self.node_pos(cell);
            let sea = land.is_sea(pos.0, pos.1);
            self.with_node(cell, |n| *n.sea.get_or_init(|| sea))
        })
    }

    /// Steepest descent only - where water would go with no spill paths.
    fn natural_downstream(&self, land: &impl Landscape, cell: Cell) -> Option<Cell> {
        let code = self.natural_down_code(land, cell);
        (code != PIT).then(|| {
            let (dx, dz) = NEIGHBOURS[code as usize];
            (cell.0 + dx, cell.1 + dz)
        })
    }

    /// Where following steepest descent from `cell` ends: a sea cell, or a
    /// pit. Memoized along the whole path walked.
    fn terminal(&self, land: &impl Landscape, cell: Cell) -> Cell {
        let mut path = Vec::new();
        let mut c = cell;
        let end = loop {
            if let Some(t) = self.with_node(c, |n| n.terminal.get().copied()) {
                break t;
            }
            path.push(c);
            if path.len() > MAX_CHAIN {
                break DRAINS_AWAY;
            }
            match self.natural_downstream(land, c) {
                Some(next) => c = next,
                None => break c,
            }
        };
        for c in path {
            self.with_node(c, |n| {
                let _ = n.terminal.set(end);
            });
        }
        end
    }

    /// The depression whose water collects at `cell`, if `cell` is in one -
    /// the pit its water runs to, and that pit's basin. `None` for anywhere
    /// that drains to the sea, or that lies outside the basin it drains to
    /// (on the slopes above the rim).
    pub fn basin_of(&self, land: &impl Landscape, cell: Cell) -> Option<(Cell, Arc<Basin>)> {
        let pit = self.terminal(land, cell);
        if self.ends_at_sea(land, pit) {
            return None;
        }
        let basin = self.basin(land, pit);
        basin.footprint.contains(&cell).then_some((pit, basin))
    }

    /// The depression around `pit`. Floods outward from the pit in order of
    /// how high water would have to rise to get there (the lowest rim
    /// first, as a filling depression overflows), until it reaches a cell
    /// whose steepest descent ends at the sea or at a pit strictly lower
    /// than this one: the outlet. Everything flooded before that is the
    /// footprint. The landscape then decides whether it overflows there or
    /// keeps its water (`Landscape::keeps_its_water`).
    pub fn basin(&self, land: &impl Landscape, pit: Cell) -> Arc<Basin> {
        if let Some(known) = self.spills.lock().unwrap().get(&pit) {
            return known.clone();
        }
        let floor = self.route(land, pit);
        let mut heap = BinaryHeap::new();
        let mut parent: HashMap<Cell, Cell> = HashMap::new();
        parent.insert(pit, pit);
        heap.push(Reverse((ordered(floor), pit)));
        let mut found = None;
        let mut popped = 0;
        let mut footprint = Vec::new();
        while let Some(Reverse((level, c))) = heap.pop() {
            popped += 1;
            if popped > MAX_SPILL_SEARCH {
                break;
            }
            if c != pit {
                let end = self.terminal(land, c);
                if end != pit && (self.ends_at_sea(land, end) || self.route(land, end) < floor) {
                    found = Some(c);
                    break;
                }
            }
            footprint.push(c);
            for (dx, dz) in NEIGHBOURS {
                let n = (c.0 + dx, c.1 + dz);
                if let std::collections::hash_map::Entry::Vacant(e) = parent.entry(n) {
                    e.insert(c);
                    heap.push(Reverse((level.max(ordered(self.route(land, n))), n)));
                }
            }
        }
        let pos = self.node_pos(pit);
        let outlet = found.filter(|_| !land.keeps_its_water(pos.0, pos.1, footprint.len()));
        if found.is_none() {
            footprint.truncate(SINK_FOOTPRINT);
        }
        let mut next = HashMap::new();
        if let Some(outlet) = outlet {
            let mut c = outlet;
            while c != pit {
                let p = parent[&c];
                next.insert(p, c);
                c = p;
            }
        }
        let basin = Arc::new(Basin { footprint: footprint.into_iter().collect(), next, outlet });
        self.spills.lock().unwrap().entry(pit).or_insert(basin).clone()
    }

    fn natural_down_code(&self, land: &impl Landscape, cell: Cell) -> u8 {
        let code = self.with_node(cell, |n| n.down.get().copied());
        let code = code.unwrap_or_else(|| {
            let code = if self.ends_at_sea(land, cell) {
                PIT
            } else {
                let here = self.route(land, cell);
                let mut best: Option<(u8, f32)> = None;
                for (i, (dx, dz)) in NEIGHBOURS.iter().enumerate() {
                    let h = self.route(land, (cell.0 + dx, cell.1 + dz));
                    // Diagonal neighbours are farther away, so compare
                    // slopes rather than raw drops.
                    let dist = if *dx != 0 && *dz != 0 { std::f32::consts::SQRT_2 } else { 1.0 };
                    let slope = (here - h) / dist;
                    if slope > 0.0 && best.is_none_or(|(_, s)| slope > s) {
                        best = Some((i as u8, slope));
                    }
                }
                best.map_or(PIT, |(i, _)| i)
            };
            self.with_node(cell, |n| *n.down.get_or_init(|| code))
        });
        code
    }

    /// The neighbours whose water flows into `cell`.
    pub fn donors(&self, land: &impl Landscape, cell: Cell) -> impl Iterator<Item = Cell> {
        let mut out = Vec::with_capacity(8);
        for (dx, dz) in NEIGHBOURS {
            let n = (cell.0 + dx, cell.1 + dz);
            if self.downstream(land, n) == Some(cell) {
                out.push(n);
            }
        }
        out.into_iter()
    }

    /// How many cells (including itself) drain through `cell`, capped at
    /// `MAX_ACCUMULATION`. Computed by an explicit-stack post-order walk
    /// upstream (a big river's catchment is far too deep for recursion),
    /// memoizing every node it finishes.
    pub fn accumulation(&self, land: &impl Landscape, cell: Cell) -> f32 {
        if let Some(a) = self.with_node(cell, |n| n.accumulation.get().copied()) {
            return a;
        }
        // (cell, donors already pushed?)
        let mut stack = vec![(cell, false)];
        let mut explored = 0usize;
        while let Some((c, expanded)) = stack.pop() {
            if self.with_node(c, |n| n.accumulation.get().is_some()) {
                continue;
            }
            let donors: Vec<Cell> = self.donors(land, c).collect();
            if !expanded {
                explored += 1;
                if explored as f32 > MAX_ACCUMULATION {
                    // More cells upstream than the cap, all genuinely in
                    // this catchment: the answer is the cap. Nodes this walk
                    // started but didn't finish stay unset, to be computed
                    // properly if anything asks for them directly.
                    self.with_node(cell, |n| {
                        let _ = n.accumulation.set(MAX_ACCUMULATION);
                        let _ = n.length.set(f32::INFINITY);
                    });
                    break;
                }
                stack.push((c, true));
                for d in donors {
                    if self.with_node(d, |n| n.accumulation.get().is_none()) {
                        stack.push((d, false));
                    }
                }
            } else {
                let total = 1.0 + donors.iter().map(|&d| self.accumulation_known(d)).sum::<f32>();
                let longest = 1.0 + donors.iter().map(|&d| self.with_node(d, |n| *n.length.get().unwrap())).fold(0.0, f32::max);
                self.with_node(c, |n| {
                    let _ = n.accumulation.set(total.min(MAX_ACCUMULATION));
                    let _ = n.length.set(longest);
                });
            }
        }
        self.accumulation_known(cell)
    }

    /// The longest flow path upstream of `cell` and through it, in cells -
    /// how far its water has come from the most distant source. Infinite
    /// for a catchment at the `MAX_ACCUMULATION` cap.
    pub fn length(&self, land: &impl Landscape, cell: Cell) -> f32 {
        self.accumulation(land, cell);
        self.with_node(cell, |n| n.length.get().copied()).unwrap_or(f32::INFINITY)
    }

    fn accumulation_known(&self, cell: Cell) -> f32 {
        self.with_node(cell, |n| *n.accumulation.get().expect("upstream finished first"))
    }

    /// A river's water surface at `cell` (only meaningful where the cell is
    /// a river - `accumulation > threshold`): its banks' height minus its
    /// incision, capped at the lowest surface of every river cell flowing
    /// into it, so a surface can only ever stay level or step *down*
    /// moving downstream - never climb - and never below sea level. Same
    /// explicit-stack walk as `accumulation`, over river cells only.
    ///
    /// Bounded by the same cap: upstream river cells are a subset of the
    /// catchment, so a cell below `MAX_ACCUMULATION` walks fewer than that.
    /// A cell *at* the cap (a basin bigger than any real terrain produces)
    /// skips the walk and takes its own banks' level - water could then
    /// step up where a tributary joins such a trunk, an accepted cost for a
    /// case that only exists as a safety valve.
    pub fn water_level(&self, land: &impl Landscape, cell: Cell, threshold: f32) -> f32 {
        if let Some(w) = self.with_node(cell, |n| n.water.get().copied()) {
            return w;
        }
        let own = |c: Cell| {
            let pos = self.node_pos(c);
            ((land.ground_height(pos.0, pos.1) - land.incision(pos.0, pos.1)) as f32).max(land.base_level() as f32)
        };
        if self.accumulation(land, cell) >= MAX_ACCUMULATION {
            let w = own(cell);
            return self.with_node(cell, |n| *n.water.get_or_init(|| w));
        }
        let mut stack = vec![(cell, false)];
        while let Some((c, expanded)) = stack.pop() {
            if self.with_node(c, |n| n.water.get().is_some()) {
                continue;
            }
            let donors: Vec<Cell> = self
                .donors(land, c)
                .filter(|&d| {
                    let a = self.accumulation(land, d);
                    a > threshold && a < MAX_ACCUMULATION
                })
                .collect();
            if !expanded {
                stack.push((c, true));
                for d in donors {
                    if self.with_node(d, |n| n.water.get().is_none()) {
                        stack.push((d, false));
                    }
                }
            } else {
                let cap = donors.iter().map(|&d| self.with_node(d, |n| *n.water.get().unwrap())).fold(f32::INFINITY, f32::min);
                let w = own(c).min(cap).max(land.base_level() as f32);
                self.with_node(c, |n| {
                    let _ = n.water.set(w);
                });
            }
        }
        self.with_node(cell, |n| *n.water.get().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A straight valley running down toward the sea at low z, with steep
    /// sides funnelling every node toward `x = 0`, closed off by ridges
    /// (at `|x| = 160`, `z = 1600`) so its catchment is finite like real
    /// terrain's. `incision` switches by z so tests can make a river incised
    /// upstream and flush downstream.
    struct Valley {
        base: f64,
        incision: fn(f64) -> f64,
    }

    impl Landscape for Valley {
        fn routing_height(&self, x: f64, z: f64) -> f64 {
            self.ground_height(x, z)
        }
        fn ground_height(&self, x: f64, z: f64) -> f64 {
            let (across, along) = (x.abs().min(320.0 - x.abs()), z.min(3200.0 - z));
            self.base + 0.5 * across + 0.05 * along
        }
        fn is_sea(&self, _x: f64, z: f64) -> bool {
            z < 0.0
        }
        fn incision(&self, _x: f64, z: f64) -> f64 {
            (self.incision)(z)
        }
        fn base_level(&self) -> f64 {
            26.0
        }
    }

    fn valley(base: f64, incision: fn(f64) -> f64) -> (Valley, Network) {
        (Valley { base, incision }, Network::new(1))
    }

    /// The cells of the main channel, walked downstream from a start cell
    /// well up the valley until the river ends.
    fn channel(net: &Network, land: &Valley, from: Cell) -> Vec<Cell> {
        let mut out = vec![from];
        while let Some(next) = net.downstream(land, *out.last().unwrap()) {
            out.push(next);
        }
        out
    }

    #[test]
    fn water_follows_the_valley_floor_all_the_way_to_the_sea() {
        let (land, net) = valley(60.0, |_| 0.0);
        let path = channel(&net, &land, (5, 60));
        let end = *path.last().unwrap();
        let (_, z) = net.node_pos(end);
        assert!(land.is_sea(0.0, z), "the river stopped short of the sea at {end:?}");
        // It found the valley floor and stayed on it.
        assert!(path.iter().skip(10).all(|&(x, _)| x.abs() <= 1), "{path:?}");
    }

    #[test]
    fn a_river_gathers_more_water_at_every_step_downstream() {
        let (land, net) = valley(60.0, |_| 0.0);
        let path = channel(&net, &land, (0, 60));
        let acc: Vec<f32> = path.iter().map(|&c| net.accumulation(&land, c)).collect();
        assert!(acc.windows(2).all(|w| w[1] > w[0]), "{acc:?}");
        // Side slopes feed it too, so it gathers more than its own length.
        assert!(*acc.last().unwrap() > 3.0 * path.len() as f32);
    }

    #[test]
    fn the_answer_does_not_depend_on_which_cell_is_asked_first() {
        let (land, a) = valley(60.0, |_| 0.0);
        let b = Network::new(1);
        // Fresh networks, queried in opposite orders.
        let cells: Vec<Cell> = (0..40).flat_map(|z| (-4..=4).map(move |x| (x, z))).collect();
        let fwd: Vec<f32> = cells.iter().map(|&c| a.accumulation(&land, c)).collect();
        let mut rev: Vec<f32> = cells.iter().rev().map(|&c| b.accumulation(&land, c)).collect();
        rev.reverse();
        assert_eq!(fwd, rev);
    }

    #[test]
    fn flat_ground_has_nowhere_to_flow() {
        struct Flat;
        impl Landscape for Flat {
            fn routing_height(&self, _: f64, _: f64) -> f64 {
                50.0
            }
            fn ground_height(&self, _: f64, _: f64) -> f64 {
                50.0
            }
            fn is_sea(&self, _: f64, _: f64) -> bool {
                false
            }
            fn incision(&self, _: f64, _: f64) -> f64 {
                0.0
            }
            fn base_level(&self) -> f64 {
                26.0
            }
        }
        let net = Network::new(1);
        for c in [(0, 0), (5, -3), (-7, 9)] {
            assert_eq!(net.downstream(&Flat, c), None);
            assert_eq!(net.accumulation(&Flat, c), 1.0);
        }
    }

    #[test]
    fn a_rivers_surface_never_rises_downstream_even_where_its_banks_get_lower() {
        // Deeply incised upstream (high z), flush downstream: without the
        // cap, the flush stretch's surface would jump *up* to its banks
        // where the incision ends - water climbing uphill.
        let (land, net) = valley(60.0, |z| if z > 400.0 { 6.0 } else { 0.0 });
        let path = channel(&net, &land, (0, 60));
        let levels: Vec<f32> = path.iter().map(|&c| net.water_level(&land, c, 0.0)).collect();
        assert!(levels.windows(2).all(|w| w[1] <= w[0]), "surface rose downstream: {levels:?}");
    }

    #[test]
    fn a_flush_river_runs_level_with_its_banks() {
        let (land, net) = valley(60.0, |_| 0.0);
        // The river's true head: the first cell down the channel that
        // counts as a river, so no river flows into it to cap its level.
        let threshold = 20.0;
        let head = (0..100)
            .flat_map(|z| (-9..=9).map(move |x| (x, z)))
            .find(|&c| {
                net.accumulation(&land, c) > threshold
                    && net.donors(&land, c).all(|d| net.accumulation(&land, d) <= threshold)
            })
            .expect("the valley has at least one river");
        let (x, z) = net.node_pos(head);
        assert_eq!(net.water_level(&land, head, threshold), land.ground_height(x, z) as f32);
    }

    #[test]
    fn a_river_meeting_the_sea_never_drops_below_it() {
        // A valley whose floor sits well below sea level near the coast.
        let (land, net) = valley(10.0, |_| 3.0);
        let path = channel(&net, &land, (0, 60));
        assert!(path.iter().all(|&c| net.water_level(&land, c, 0.0) >= 26.0));
    }

    /// The valley with a hollow in its floor halfway down - a dip deep
    /// enough that steepest descent alone gets stuck in it.
    struct HollowValley(Valley);

    impl Landscape for HollowValley {
        fn routing_height(&self, x: f64, z: f64) -> f64 {
            self.ground_height(x, z)
        }
        fn ground_height(&self, x: f64, z: f64) -> f64 {
            let dip = 8.0 * (-((z - 800.0) / 60.0).powi(2) - (x / 60.0).powi(2)).exp();
            self.0.ground_height(x, z) - dip
        }
        fn is_sea(&self, x: f64, z: f64) -> bool {
            self.0.is_sea(x, z)
        }
        fn incision(&self, x: f64, z: f64) -> f64 {
            self.0.incision(x, z)
        }
        fn base_level(&self) -> f64 {
            self.0.base_level()
        }
    }

    /// A river that runs into a hollow fills it and carries on over its
    /// lowest rim, rather than ending there.
    #[test]
    fn a_river_spills_out_of_a_hollow_and_carries_on_to_the_sea() {
        let land = HollowValley(Valley { base: 60.0, incision: |_| 0.0 });
        let net = Network::new(1);
        // Test setup: steepest descent alone really does get stuck.
        let mut c = (0, 90);
        while let Some(next) = net.natural_downstream(&land, c) {
            c = next;
        }
        let (_, stuck_z) = net.node_pos(c);
        assert!(!land.is_sea(0.0, stuck_z), "the hollow should trap plain steepest descent");

        let mut path = vec![(0, 90)];
        while let Some(next) = net.downstream(&land, *path.last().unwrap()) {
            assert!(path.len() < 10_000, "flow went round in a loop");
            path.push(next);
        }
        let (_, z) = net.node_pos(*path.last().unwrap());
        assert!(land.is_sea(0.0, z), "the river stopped at {:?} instead of reaching the sea", path.last());
        assert!(path.contains(&c), "it should pass through the hollow it filled, not around it");
    }
}
