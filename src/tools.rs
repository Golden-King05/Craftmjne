//! Tools: what kinds exist, what tiers they come in, and how a single tool's
//! *quality* moves it within its tier.
//!
//! **Tiers** are data (`tiers/*.json`), one file per material:
//!
//! ```json
//! { "id": "stone", "name": "Stone", "mining_speed": 2.0, "damage": 3.0,
//!   "quality_band": [0.75, 1.15] }
//! ```
//!
//! `mining_speed` and `damage` are the tier's base stats. `mining_speed` is
//! how many times faster than a bare hand the tier's tools mine the blocks
//! they're made for (3.0 turns a 45-second log into 15 seconds), so it's the
//! one number to leave headroom in: later tiers are bigger numbers. `quality_band`
//! is what the stats are multiplied by at quality 0% and at quality 100%
//! (defaults to `[0.75, 1.15]`); a tool in between gets the straight-line
//! blend. **Tiers are independent of each other** - nothing requires one
//! tier's worst tool to beat the previous tier's best, so a 100% iron sword
//! can out-damage a 10% steel one, and which of two tools is better is
//! just arithmetic on the numbers. The overlap is a tuning knob (bases and
//! bands), not a rule.
//!
//! A tool item names its tier and kind in `items/*.json`:
//!
//! ```json
//! { "id": "stone_pickaxe", "name": "Stone Pickaxe",
//!   "tool": { "kind": "pickaxe", "tier": "stone" } }
//! ```
//!
//! **Quality** is a whole percent, `0..=100`, stored on the `ItemStack`
//! itself (so tools never stack - a tool's `max_stack` is forced to 1).
//! It means "how good is this one, for its tier", and the tooltip shows a
//! [`Quality`] name instead of the number: Trash, Common, Rare, Epic, and
//! Legendary only at exactly 100.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

use crate::blocks::{find_data_dir, BlockDef};

/// The best a tool can be.
pub const MAX_QUALITY: u8 = 100;

/// What a tool made some way other than by hand gets - Creative's item list,
/// for now. Knapping and forging will set real values.
pub const DEFAULT_TOOL_QUALITY: u8 = 50;

/// Seconds of mining per point of block `hardness`, at speed 1 (a bare
/// hand, or a tool of the wrong kind). Dirt (0.5) takes 2.5s, gravel (1.0)
/// 5s.
pub const SECONDS_PER_HARDNESS: f32 = 5.0;

/// How much longer a block that `requires_tool` takes *by hand* than its
/// hardness alone says: stone (hardness 1.5, 7.5s) is 60s by hand, a log
/// (1.125) 45s. A tool doesn't remove this - it divides the whole time by
/// its own speed, so a tool's `mining_speed` is simply "how many times
/// faster than a bare hand" and a better tier is just a bigger number.
pub const NO_TOOL_PENALTY: f32 = 8.0;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolKind {
    Axe,
    Pickaxe,
    Shovel,
    Hoe,
    Dagger,
    Longsword,
    Greatsword,
}

/// The tool in a player's hand, as far as mining is concerned: its kind and
/// its speed (the tier's `mining_speed` scaled by this tool's quality).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct HeldTool {
    pub kind: ToolKind,
    pub speed: f32,
}

/// Whether the tool in hand is the kind `block_tool` asks for.
fn is_right_tool(block_tool: Option<ToolKind>, held: Option<HeldTool>) -> bool {
    matches!((block_tool, held), (Some(want), Some(h)) if want == h.kind)
}

/// Seconds to mine `block` with `held` in hand: the time it takes by hand
/// (its hardness, times `NO_TOOL_PENALTY` if it `requires_tool`) divided by
/// the tool's speed - only if the tool is the block's kind, and only as much
/// of it as the block's `tool_response` allows (the speed raised to that
/// power). A bare hand or the wrong kind is speed 1, i.e. the hand time.
pub fn mining_seconds(block: &BlockDef, held: Option<HeldTool>) -> f32 {
    let speed = if is_right_tool(block.tool, held) {
        held.map_or(1.0, |h| h.speed).max(0.01).powf(block.tool_response)
    } else {
        1.0
    };
    let by_hand = block.hardness.max(0.0) * SECONDS_PER_HARDNESS * if block.requires_tool { NO_TOOL_PENALTY } else { 1.0 };
    by_hand / speed
}

/// Whether mining `block` with what's in hand yields its drops: always,
/// unless it `requires_tool` and this isn't it.
pub fn harvests(block: &BlockDef, held: Option<HeldTool>) -> bool {
    !block.requires_tool || is_right_tool(block.tool, held)
}

/// What a tool item is: its kind and which tier's stats it uses.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ToolSpec {
    pub kind: ToolKind,
    pub tier: String,
}

/// The default `quality_band`: a 0% tool is a quarter worse than the tier's
/// base, a 100% one 15% better.
const DEFAULT_BAND: [f32; 2] = [0.75, 1.15];

#[derive(Clone, Debug)]
pub struct Tier {
    pub id: String,
    pub name: String,
    pub mining_speed: f32,
    pub damage: f32,
    /// The multiplier at quality 0 and at quality 100.
    pub band: (f32, f32),
}

impl Tier {
    /// What a tool of `quality` multiplies this tier's base stats by.
    /// Never decreases as quality rises.
    pub fn multiplier(&self, quality: u8) -> f32 {
        let t = quality.min(MAX_QUALITY) as f32 / MAX_QUALITY as f32;
        self.band.0 + (self.band.1 - self.band.0) * t
    }

    pub fn mining_speed_at(&self, quality: u8) -> f32 {
        self.mining_speed * self.multiplier(quality)
    }

    pub fn damage_at(&self, quality: u8) -> f32 {
        self.damage * self.multiplier(quality)
    }
}

#[derive(Deserialize)]
struct TierFile {
    id: String,
    name: Option<String>,
    mining_speed: Option<f32>,
    damage: Option<f32>,
    quality_band: Option<[f32; 2]>,
}

/// Every tier, by id.
#[derive(Clone, Default, Debug)]
pub struct TierBook {
    tiers: Vec<Tier>,
    by_id: HashMap<String, usize>,
}

impl TierBook {
    /// Every `tiers/*.json` file, found like the other shipped data folders.
    pub fn with_defaults() -> Self {
        Self::load_from_dir(&find_data_dir("tiers"))
    }

    /// Panics, with a clear message, on an unreadable or malformed file, a
    /// duplicate id, or numbers that make no sense - startup content.
    pub fn load_from_dir(dir: &Path) -> Self {
        let mut paths: Vec<_> = std::fs::read_dir(dir)
            .unwrap_or_else(|err| panic!("could not read tier definitions from {dir:?}: {err}"))
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
            .collect();
        paths.sort();
        let mut book = Self::default();
        for path in paths {
            let text = std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("failed to read tier {path:?}: {err}"));
            let file: TierFile =
                serde_json::from_str(&text).unwrap_or_else(|err| panic!("failed to parse tier {path:?}: {err}"));
            book.add(file, &path);
        }
        book
    }

    fn add(&mut self, file: TierFile, source: &Path) {
        let band = file.quality_band.unwrap_or(DEFAULT_BAND);
        assert!(!file.id.contains(char::is_whitespace), "tier {source:?}: id {:?} must not contain spaces", file.id);
        assert!(
            band[0] > 0.0 && band[0] <= band[1],
            "tier {source:?}: quality_band {band:?} needs 0 < low <= high (a higher quality never makes a tool worse)"
        );
        let (mining_speed, damage) = (file.mining_speed.unwrap_or(1.0), file.damage.unwrap_or(1.0));
        assert!(mining_speed > 0.0 && damage > 0.0, "tier {source:?}: mining_speed and damage must be positive");
        assert!(!self.by_id.contains_key(&file.id), "tier {:?} already defined", file.id);
        self.by_id.insert(file.id.clone(), self.tiers.len());
        self.tiers.push(Tier {
            name: file.name.unwrap_or_else(|| crate::blocks::title_case(&file.id)),
            id: file.id,
            mining_speed,
            damage,
            band: (band[0], band[1]),
        });
    }

    pub fn get(&self, id: &str) -> Option<&Tier> {
        self.by_id.get(id).map(|&i| &self.tiers[i])
    }

    pub fn iter(&self) -> impl Iterator<Item = &Tier> {
        self.tiers.iter()
    }
}

/// The name a quality is shown as. Bands are fixed: 0-19 Trash, 20-59
/// Common, 60-84 Rare, 85-99 Epic, and Legendary only at exactly 100.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Quality {
    Trash,
    Common,
    Rare,
    Epic,
    Legendary,
}

impl Quality {
    pub fn from_percent(percent: u8) -> Self {
        match percent {
            0..=19 => Self::Trash,
            20..=59 => Self::Common,
            60..=84 => Self::Rare,
            85..=99 => Self::Epic,
            _ => Self::Legendary,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Trash => "Trash",
            Self::Common => "Common",
            Self::Rare => "Rare",
            Self::Epic => "Epic",
            Self::Legendary => "Legendary",
        }
    }

    /// The colour the name is drawn in (sRGB): grey, white, blue, purple, gold.
    pub fn color(self) -> [f32; 3] {
        match self {
            Self::Trash => [0.6, 0.6, 0.6],
            Self::Common => [1.0, 1.0, 1.0],
            Self::Rare => [0.35, 0.55, 1.0],
            Self::Epic => [0.7, 0.35, 0.95],
            Self::Legendary => [1.0, 0.75, 0.2],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tier(id: &str, base: f32, band: (f32, f32)) -> Tier {
        Tier { id: id.into(), name: id.into(), mining_speed: base, damage: base, band }
    }

    #[test]
    fn quality_moves_a_tool_from_the_low_end_of_its_band_to_the_high_end() {
        let t = tier("stone", 2.0, (0.75, 1.15));
        assert!((t.multiplier(0) - 0.75).abs() < 1e-6);
        assert!((t.multiplier(100) - 1.15).abs() < 1e-6);
        assert!((t.multiplier(50) - 0.95).abs() < 1e-6);
        assert!((t.mining_speed_at(100) - 2.3).abs() < 1e-5);
        assert!((t.damage_at(0) - 1.5).abs() < 1e-5);
        assert_eq!(t.multiplier(250), t.multiplier(100), "out-of-range quality is clamped");
    }

    #[test]
    fn a_higher_quality_never_makes_a_tool_worse() {
        let t = tier("any", 3.0, (0.6, 1.4));
        let mut last = 0.0;
        for q in 0..=MAX_QUALITY {
            let v = t.damage_at(q);
            assert!(v >= last, "quality {q} is worse than {}", q.saturating_sub(1));
            last = v;
        }
    }

    #[test]
    fn tiers_may_overlap_so_a_great_iron_tool_can_beat_a_poor_steel_one() {
        let iron = tier("iron", 6.0, (0.8, 1.2));
        let steel = tier("steel", 7.0, (0.8, 1.2));
        assert!(iron.damage_at(100) > steel.damage_at(10), "the perfect iron sword wins");
        assert!(steel.damage_at(100) > iron.damage_at(100), "but a perfect steel one wins overall");
        assert!(steel.damage_at(0) < iron.damage_at(60), "so does the choice depend on both qualities");
    }

    #[test]
    fn quality_names_follow_the_bands_and_legendary_is_only_a_perfect_hundred() {
        let name = |q| Quality::from_percent(q);
        assert_eq!((name(0), name(19)), (Quality::Trash, Quality::Trash));
        assert_eq!((name(20), name(59)), (Quality::Common, Quality::Common));
        assert_eq!((name(60), name(84)), (Quality::Rare, Quality::Rare));
        assert_eq!((name(85), name(99)), (Quality::Epic, Quality::Epic));
        assert_eq!(name(100), Quality::Legendary);
        assert_eq!(Quality::Legendary.label(), "Legendary");
        assert_ne!(Quality::Epic.color(), Quality::Legendary.color());
    }

    #[test]
    fn the_shipped_stone_tier_loads() {
        let book = TierBook::with_defaults();
        let stone = book.get("stone").expect("no stone tier");
        assert!(stone.mining_speed > 1.0, "stone must out-mine a bare hand");
        assert!(stone.band.0 < 1.0 && stone.band.1 > 1.0);
        assert!(book.get("diamond").is_none());
    }

    fn scratch(files: &[(&str, &str)]) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!("craftmjne-tiers-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, text) in files {
            std::fs::write(dir.join(name), text).unwrap();
        }
        dir
    }

    #[test]
    fn a_tier_file_defaults_the_band_and_name() {
        let dir = scratch(&[("iron_age.json", r#"{ "id": "iron_age", "mining_speed": 4.0 }"#)]);
        let book = TierBook::load_from_dir(&dir);
        let t = book.get("iron_age").unwrap();
        assert_eq!(t.name, "Iron Age");
        assert_eq!(t.band, (DEFAULT_BAND[0], DEFAULT_BAND[1]));
        assert_eq!(t.damage, 1.0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    #[should_panic(expected = "never makes a tool worse")]
    fn a_band_that_runs_backwards_is_refused() {
        let dir = scratch(&[("bad.json", r#"{ "id": "bad", "quality_band": [1.2, 0.8] }"#)]);
        TierBook::load_from_dir(&dir);
    }

    #[test]
    #[should_panic(expected = "already defined")]
    fn two_tiers_with_one_id_are_refused() {
        let dir = scratch(&[("a.json", r#"{ "id": "same" }"#), ("b.json", r#"{ "id": "same" }"#)]);
        TierBook::load_from_dir(&dir);
    }

    // ---- mining -------------------------------------------------------

    use crate::blocks::BlockRegistry;
    use crate::item_registry::{ItemRegistry, ItemStack};

    fn shipped() -> (BlockRegistry, ItemRegistry) {
        let blocks = BlockRegistry::with_defaults();
        let items = ItemRegistry::with_defaults(&blocks);
        (blocks, items)
    }

    /// Seconds to mine `block` with `held` in hand, from the shipped data.
    fn secs(blocks: &BlockRegistry, block: &str, held: Option<HeldTool>) -> f32 {
        mining_seconds(blocks.def(blocks.id(block)), held)
    }

    fn tool_in_hand(items: &ItemRegistry, name: &str, quality: u8) -> Option<HeldTool> {
        items.held_tool(&ItemStack::new(items.id(name), 1).with_quality(quality))
    }

    #[test]
    fn bare_hands_take_the_times_the_game_was_designed_around() {
        let (blocks, _items) = shipped();
        let near = |name: &str, want: f32| {
            let got = secs(&blocks, name, None);
            assert!((got - want).abs() < 1e-3, "{name} took {got}s by hand, want {want}s");
        };
        near("dirt", 2.5);
        near("grass", 2.5);
        near("gravel", 5.0);
        near("stone", 60.0);
        near("log", 45.0);
        near("planks", 45.0);
    }

    #[test]
    fn the_right_tool_is_much_faster_and_quality_makes_it_faster_still() {
        let (blocks, items) = shipped();
        for (block, tool) in [("stone", "stone_pickaxe"), ("log", "stone_axe"), ("dirt", "stone_shovel"), ("gravel", "stone_shovel")] {
            let hand = secs(&blocks, block, None);
            let poor = secs(&blocks, block, tool_in_hand(&items, tool, 0));
            let great = secs(&blocks, block, tool_in_hand(&items, tool, 100));
            assert!(great < poor && poor < hand, "{block}: hand {hand}s, poor {poor}s, great {great}s");
        }
    }

    #[test]
    fn soft_blocks_take_real_time_even_with_a_good_shovel_but_a_better_tool_still_helps() {
        let (blocks, items) = shipped();
        for name in ["dirt", "grass", "sand", "mud", "salt"] {
            let t = |q| secs(&blocks, name, tool_in_hand(&items, "stone_shovel", q));
            assert!(t(100) > 1.2, "{name}: even the best stone shovel takes {}s", t(100));
            assert!(t(0) > t(100), "{name}: quality still matters");
            assert!(t(0) < secs(&blocks, name, None), "{name}: and a shovel still beats a hand");
        }
        let gravel = |q| secs(&blocks, "gravel", tool_in_hand(&items, "stone_shovel", q));
        assert!(gravel(100) > 2.5 && gravel(0) < 5.0, "gravel {}s..{}s", gravel(100), gravel(0));
        // A far better tier helps more, but never to instant: speed 40 is a
        // 6x speedup here, not 40x.
        let dirt = blocks.def(blocks.id("dirt"));
        let fast = Some(HeldTool { kind: ToolKind::Shovel, speed: 40.0 });
        let t = mining_seconds(dirt, fast);
        assert!((0.3..0.6).contains(&t), "an end-game shovel takes {t}s on dirt");
    }

    #[test]
    fn hard_blocks_take_the_tools_full_speedup() {
        let (blocks, items) = shipped();
        let log = blocks.def(blocks.id("log"));
        assert_eq!(log.tool_response, 1.0);
        let axe = tool_in_hand(&items, "stone_axe", 50).unwrap();
        assert!((mining_seconds(log, None) / mining_seconds(log, Some(axe)) - axe.speed).abs() < 1e-3);
    }

    #[test]
    fn stone_tools_are_slow_enough_to_leave_room_for_better_tiers() {
        let (blocks, items) = shipped();
        // The best stone axe ever made still takes real time on a log, so
        // iron, steel and whatever follows each have somewhere to go.
        let best = secs(&blocks, "log", tool_in_hand(&items, "stone_axe", 100));
        let typical = secs(&blocks, "log", tool_in_hand(&items, "stone_axe", 50));
        let worst = secs(&blocks, "log", tool_in_hand(&items, "stone_axe", 0));
        assert!(best >= 10.0, "a perfect stone axe fells a log in {best}s");
        assert!((13.0..=19.0).contains(&typical), "a typical stone axe takes {typical}s");
        assert!(worst < secs(&blocks, "log", None), "even a poor stone axe beats a bare hand");
        assert!(secs(&blocks, "stone", tool_in_hand(&items, "stone_pickaxe", 100)) >= 12.0);
        // A tool is simply `speed` times faster than the hand on its block.
        let speed = tool_in_hand(&items, "stone_axe", 50).unwrap().speed;
        assert!((secs(&blocks, "log", None) / typical - speed).abs() < 1e-3);
    }

    #[test]
    fn the_wrong_tool_mines_at_hand_speed_and_so_do_weapons() {
        let (blocks, items) = shipped();
        for (block, wrong) in [
            ("stone", "stone_shovel"),
            ("stone", "stone_axe"),
            ("stone", "stone_dagger"),
            ("log", "stone_pickaxe"),
            ("dirt", "stone_pickaxe"),
            ("dirt", "stone_dagger"),
            ("dirt", "stone_hoe"),
        ] {
            assert_eq!(
                secs(&blocks, block, tool_in_hand(&items, wrong, 100)),
                secs(&blocks, block, None),
                "{wrong} on {block} should be no better than a hand"
            );
        }
    }

    #[test]
    fn blocks_that_need_a_tool_drop_nothing_without_it() {
        let (blocks, items) = shipped();
        let drops = |block: &str, held: Option<HeldTool>| {
            harvests(blocks.def(blocks.id(block)), held)
        };
        assert!(!drops("log", None), "a log punched by hand gives nothing");
        assert!(!drops("planks", None));
        assert!(!drops("stone", None));
        assert!(!drops("stone", tool_in_hand(&items, "stone_axe", 100)), "the wrong tool doesn't count");
        assert!(drops("log", tool_in_hand(&items, "stone_axe", 0)), "any quality of the right tool will do");
        assert!(drops("stone", tool_in_hand(&items, "stone_pickaxe", 0)));
        // Blocks with only a preferred tool drop either way.
        assert!(drops("dirt", None));
        assert!(drops("gravel", None));
        assert!(drops("leaves", None));
    }

    #[test]
    fn what_each_block_is_mined_with() {
        let (blocks, _items) = shipped();
        let of = |name: &str| {
            let d = blocks.def(blocks.id(name));
            (d.tool, d.requires_tool)
        };
        for name in ["stone", "cobblestone", "bricks", "coal_ore", "iron_ore"] {
            assert_eq!(of(name), (Some(ToolKind::Pickaxe), true), "{name}");
        }
        for name in ["log", "planks", "crafting_bench"] {
            assert_eq!(of(name), (Some(ToolKind::Axe), true), "{name}");
        }
        for name in ["dirt", "grass", "gravel", "sand", "mud", "snow", "salt"] {
            assert_eq!(of(name), (Some(ToolKind::Shovel), false), "{name}");
        }
        assert_eq!(of("leaves"), (None, false));
        assert_eq!(of("torch"), (None, false));
    }

    #[test]
    fn held_tool_reads_a_stacks_speed_at_its_quality() {
        let (_blocks, items) = shipped();
        let pick = items.id("stone_pickaxe");
        let tier = items.tier_of(pick).unwrap();
        let held = items.held_tool(&ItemStack::new(pick, 1).with_quality(83)).unwrap();
        assert_eq!(held.kind, ToolKind::Pickaxe);
        assert!((held.speed - tier.mining_speed_at(83)).abs() < 1e-6);
        assert_eq!(items.held_tool(&ItemStack::EMPTY), None, "a bare hand");
        assert_eq!(items.held_tool(&ItemStack::new(items.id("stick"), 3)), None, "a stick is no tool");
        assert_eq!(items.held_tool(&ItemStack::new(items.id("dirt"), 3)), None);
    }

    #[test]
    fn instant_and_zero_hardness_blocks_stay_instant_with_any_tool() {
        let block = |hardness| BlockDef { hardness, tool: Some(ToolKind::Axe), requires_tool: true, ..BlockDef::default() };
        assert_eq!(mining_seconds(&block(0.0), None), 0.0);
        let held = Some(HeldTool { kind: ToolKind::Axe, speed: 2.0 });
        assert_eq!(mining_seconds(&block(0.0), held), 0.0);
    }
}
