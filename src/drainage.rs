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

use std::collections::HashMap;
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
}

/// One node's memoized values. Each is computed at most once (`OnceLock`)
/// and is a pure function of the landscape, so two threads racing to fill
/// the same one just compute the same number.
#[derive(Default)]
struct Node {
    route: OnceLock<f32>,
    /// Index into `NEIGHBOURS`, or `PIT` if nothing around is lower.
    down: OnceLock<u8>,
    accumulation: OnceLock<f32>,
    water: OnceLock<f32>,
}

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
}

impl Network {
    pub fn new(seed: u32) -> Self {
        Self { seed, tiles: Mutex::new(HashMap::new()) }
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

    /// Where water at `cell` flows next, or `None` if it stops here (a
    /// pit, or the sea).
    pub fn downstream(&self, land: &impl Landscape, cell: Cell) -> Option<Cell> {
        let code = self.with_node(cell, |n| n.down.get().copied());
        let code = code.unwrap_or_else(|| {
            let pos = self.node_pos(cell);
            let code = if land.is_sea(pos.0, pos.1) {
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
        (code != PIT).then(|| {
            let (dx, dz) = NEIGHBOURS[code as usize];
            (cell.0 + dx, cell.1 + dz)
        })
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
                self.with_node(c, |n| {
                    let _ = n.accumulation.set(total.min(MAX_ACCUMULATION));
                });
            }
        }
        self.accumulation_known(cell)
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
}
