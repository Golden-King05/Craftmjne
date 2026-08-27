//! The local library of downloaded game versions - the "download it once"
//! half of the launcher.
//!
//! There is no index file and no manifest: the directories under
//! `Paths::versions_dir` *are* the record. A version counts as installed
//! when its directory holds a runnable game executable, which means the
//! state on disk can never disagree with a catalogue that got out of sync -
//! deleting a folder by hand uninstalls that version, and a download
//! interrupted halfway simply doesn't count as installed (the exe never
//! arrived), so the next attempt re-downloads instead of launching
//! something incomplete.

use std::path::PathBuf;

use crate::paths::Paths;

pub struct Library {
    paths: Paths,
}

impl Library {
    pub fn new(paths: Paths) -> Self {
        Self { paths }
    }

    /// Whether `version` is downloaded and runnable right now.
    pub fn is_installed(&self, version: &str) -> bool {
        self.paths.game_exe(version).is_file()
    }

    /// Every installed version, newest-looking first (see
    /// [`compare_versions`]).
    pub fn installed(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(self.paths.versions_dir()) else {
            return Vec::new();
        };
        let mut versions: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|name| self.is_installed(name))
            .collect();
        versions.sort_by(|a, b| compare_versions(b, a));
        versions
    }

    pub fn exe(&self, version: &str) -> PathBuf {
        self.paths.game_exe(version)
    }

    pub fn dir(&self, version: &str) -> PathBuf {
        self.paths.version_dir(version)
    }

    /// Deletes a downloaded version. The shared `saves/` folder lives
    /// outside `versions/` precisely so this can never take worlds with it.
    pub fn remove(&self, version: &str) -> std::io::Result<()> {
        let dir = self.paths.version_dir(version);
        if dir.is_dir() {
            std::fs::remove_dir_all(dir)?;
        }
        Ok(())
    }

    /// How much disk the whole library is using, for the UI to show.
    pub fn total_bytes(&self) -> u64 {
        self.installed().iter().map(|v| dir_size(&self.paths.version_dir(v))).sum()
    }
}

fn dir_size(dir: &std::path::Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    entries
        .filter_map(|e| e.ok())
        .map(|entry| match entry.file_type() {
            Ok(t) if t.is_dir() => dir_size(&entry.path()),
            Ok(_) => entry.metadata().map(|m| m.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

/// Orders two version strings newest-last, comparing dotted numeric parts
/// numerically so `1.10.0` correctly sorts above `1.9.0` (a plain string
/// compare gets that backwards, which would put the newest release in the
/// middle of the list). Anything non-numeric falls back to comparing the
/// raw text, so an unexpected tag name still sorts deterministically
/// instead of panicking or being dropped.
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |v: &str| -> Vec<Option<u64>> {
        v.trim_start_matches('v').split('.').map(|p| p.parse::<u64>().ok()).collect()
    };
    let (pa, pb) = (parts(a), parts(b));
    for i in 0..pa.len().max(pb.len()) {
        match (pa.get(i).copied().flatten(), pb.get(i).copied().flatten()) {
            (Some(x), Some(y)) if x != y => return x.cmp(&y),
            (Some(_), Some(_)) => {}
            // A missing component counts as 0, so `1.2` and `1.2.0` are equal
            // rather than one arbitrarily outranking the other.
            (Some(x), None) if x != 0 => return std::cmp::Ordering::Greater,
            (None, Some(y)) if y != 0 => return std::cmp::Ordering::Less,
            (None, None) => return a.cmp(b),
            _ => {}
        }
    }
    std::cmp::Ordering::Equal
}

/// `true` when `candidate` is strictly newer than `current`.
pub fn is_newer(current: &str, candidate: &str) -> bool {
    compare_versions(candidate, current) == std::cmp::Ordering::Greater
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;
    use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempRoot(PathBuf);
    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn temp_root() -> TempRoot {
        let n = COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
        let dir = std::env::temp_dir()
            .join(format!("craftmjne-library-test-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        TempRoot(dir)
    }

    /// Fakes a downloaded version by creating the one file `is_installed`
    /// actually looks for.
    fn install(paths: &Paths, version: &str, bytes: usize) {
        let dir = paths.version_dir(version);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(paths.game_exe(version), vec![0u8; bytes]).unwrap();
    }

    #[test]
    fn numeric_version_components_compare_numerically_not_alphabetically() {
        assert_eq!(compare_versions("1.10.0", "1.9.0"), Ordering::Greater);
        assert_eq!(compare_versions("1.2.3", "1.2.10"), Ordering::Less);
        assert_eq!(compare_versions("2.0.0", "1.99.99"), Ordering::Greater);
        assert_eq!(compare_versions("1.2.3", "1.2.3"), Ordering::Equal);
    }

    #[test]
    fn a_leading_v_and_a_missing_component_do_not_change_the_ordering() {
        assert_eq!(compare_versions("v1.2.3", "1.2.3"), Ordering::Equal);
        assert_eq!(compare_versions("1.2", "1.2.0"), Ordering::Equal);
        assert_eq!(compare_versions("1.2.1", "1.2"), Ordering::Greater);
    }

    #[test]
    fn is_newer_only_accepts_a_strict_upgrade() {
        assert!(is_newer("1.2.2", "1.2.3"));
        assert!(is_newer("1.9.0", "1.10.0"));
        assert!(!is_newer("1.2.3", "1.2.3"));
        assert!(!is_newer("1.2.3", "1.2.2"));
    }

    #[test]
    fn a_version_only_counts_as_installed_once_its_executable_exists() {
        let root = temp_root();
        let paths = Paths::at(root.0.clone());
        let library = Library::new(paths.clone());

        // A directory alone is what a half-finished download leaves behind -
        // it must not read as installed, or the launcher would try to start
        // a game that isn't there.
        std::fs::create_dir_all(paths.version_dir("1.0.0")).unwrap();
        assert!(!library.is_installed("1.0.0"));
        assert!(library.installed().is_empty());

        install(&paths, "1.0.0", 10);
        assert!(library.is_installed("1.0.0"));
        assert_eq!(library.installed(), vec!["1.0.0"]);
    }

    #[test]
    fn installed_versions_come_back_newest_first() {
        let root = temp_root();
        let paths = Paths::at(root.0.clone());
        let library = Library::new(paths.clone());
        for v in ["1.9.0", "1.10.0", "1.2.0"] {
            install(&paths, v, 1);
        }
        assert_eq!(library.installed(), vec!["1.10.0", "1.9.0", "1.2.0"]);
    }

    #[test]
    fn removing_a_version_deletes_only_that_version() {
        let root = temp_root();
        let paths = Paths::at(root.0.clone());
        let library = Library::new(paths.clone());
        install(&paths, "1.0.0", 1);
        install(&paths, "2.0.0", 1);

        library.remove("1.0.0").unwrap();
        assert!(!library.is_installed("1.0.0"));
        assert!(library.is_installed("2.0.0"));
        // Removing something that isn't there is a no-op, not an error.
        library.remove("1.0.0").unwrap();
    }

    #[test]
    fn removing_a_version_never_touches_the_shared_saves_folder() {
        let root = temp_root();
        let paths = Paths::at(root.0.clone());
        let library = Library::new(paths.clone());
        install(&paths, "1.0.0", 1);
        std::fs::create_dir_all(paths.saves_dir()).unwrap();
        let world = paths.saves_dir().join("my-world.json");
        std::fs::write(&world, "precious").unwrap();

        library.remove("1.0.0").unwrap();

        assert!(world.is_file(), "uninstalling a version must not delete worlds");
    }

    #[test]
    fn total_bytes_adds_up_every_installed_version() {
        let root = temp_root();
        let paths = Paths::at(root.0.clone());
        let library = Library::new(paths.clone());
        assert_eq!(library.total_bytes(), 0);
        install(&paths, "1.0.0", 100);
        install(&paths, "2.0.0", 250);
        assert_eq!(library.total_bytes(), 350);
    }

    #[test]
    fn an_absent_versions_directory_reads_as_an_empty_library() {
        let library = Library::new(Paths::at(PathBuf::from("/nonexistent/launcher-root")));
        assert!(library.installed().is_empty());
        assert_eq!(library.total_bytes(), 0);
        assert!(!library.is_installed("1.0.0"));
    }
}
