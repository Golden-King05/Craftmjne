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
//! `mining_speed` and `damage` are the tier's base stats. `quality_band`
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

use crate::blocks::find_data_dir;

/// The best a tool can be.
pub const MAX_QUALITY: u8 = 100;

/// What a tool made some way other than by hand gets - Creative's item list,
/// for now. Knapping and forging will set real values.
pub const DEFAULT_TOOL_QUALITY: u8 = 50;

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
}
