//! Aggregates texture load status from every subsystem that can fall back
//! to the "missing texture" placeholder (`atlas::TextureStatus`) - block/UI
//! tiles from `atlas::AtlasData::texture_status`, sun/moon from `sky.rs` -
//! into one `Resource` so `/texture-report` (see `commands.rs`) can show a
//! single honest breakdown instead of each subsystem needing its own ad hoc
//! reporting. Both `world::compile_content` (blocks) and `sky::spawn_sky`
//! (sun/moon) push their own entries into the same resource at Startup;
//! system ordering between them doesn't matter since each only appends its
//! own names.

use bevy::prelude::*;

use crate::atlas::TextureStatus;

#[derive(Resource, Default)]
pub struct TextureReport {
    entries: Vec<(String, TextureStatus)>,
    /// Names a subsystem expected to resolve to *something* but that never
    /// showed up in its final lookup table - a real invariant check
    /// (`world::compile_content` diffs `BlockRegistry::texture_names()`
    /// against the built atlas's indices), not a hardcoded "always zero"
    /// stub. Currently always empty, because `Painters::ensure_registered`
    /// is called for every required name before the atlas is ever built -
    /// but this recomputes that fact fresh every startup rather than just
    /// assuming it holds.
    missing: Vec<String>,
}

impl TextureReport {
    pub fn extend(&mut self, entries: impl IntoIterator<Item = (String, TextureStatus)>) {
        self.entries.extend(entries);
    }

    pub fn set_missing(&mut self, missing: Vec<String>) {
        self.missing = missing;
    }

    /// (working, broken-but-functioning, completely-broken) counts - green,
    /// yellow, and red in `/texture-report`.
    pub fn counts(&self) -> (usize, usize, usize) {
        let working = self.entries.iter().filter(|(_, s)| *s == TextureStatus::Working).count();
        let placeholder = self.entries.len() - working;
        (working, placeholder, self.missing.len())
    }

    /// Placeholder (yellow) and completely-broken (red) names, both sorted
    /// for stable output - what `/texture-report`'s detail section lists.
    pub fn broken_names(&self) -> (Vec<&str>, Vec<&str>) {
        let mut placeholder: Vec<&str> = self
            .entries
            .iter()
            .filter(|(_, s)| *s == TextureStatus::Placeholder)
            .map(|(n, _)| n.as_str())
            .collect();
        placeholder.sort_unstable();
        let mut missing: Vec<&str> = self.missing.iter().map(|s| s.as_str()).collect();
        missing.sort_unstable();
        (placeholder, missing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_split_working_from_placeholder_and_track_missing_separately() {
        let mut report = TextureReport::default();
        report.extend([
            ("stone".to_string(), TextureStatus::Working),
            ("dirt".to_string(), TextureStatus::Working),
            ("ruby".to_string(), TextureStatus::Placeholder),
        ]);
        report.set_missing(vec!["ghost".to_string()]);
        assert_eq!(report.counts(), (2, 1, 1));
    }

    #[test]
    fn broken_names_are_sorted_and_split_by_severity() {
        let mut report = TextureReport::default();
        report.extend([
            ("zebra".to_string(), TextureStatus::Placeholder),
            ("apple".to_string(), TextureStatus::Placeholder),
            ("fine".to_string(), TextureStatus::Working),
        ]);
        report.set_missing(vec!["zzz".to_string(), "aaa".to_string()]);
        let (placeholder, missing) = report.broken_names();
        assert_eq!(placeholder, vec!["apple", "zebra"]);
        assert_eq!(missing, vec!["aaa", "zzz"]);
    }

    #[test]
    fn a_fresh_report_is_all_zero() {
        let report = TextureReport::default();
        assert_eq!(report.counts(), (0, 0, 0));
        assert_eq!(report.broken_names(), (vec![], vec![]));
    }

    #[test]
    fn extend_accumulates_across_multiple_calls() {
        // Mirrors how compile_content (blocks) and spawn_sky (sun/moon) both
        // push into the same resource independently at Startup.
        let mut report = TextureReport::default();
        report.extend([("stone".to_string(), TextureStatus::Working)]);
        report.extend([("sun".to_string(), TextureStatus::Working), ("moon".to_string(), TextureStatus::Placeholder)]);
        assert_eq!(report.counts(), (2, 1, 0));
    }
}
