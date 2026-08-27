//! Where the launcher keeps everything on disk.
//!
//! Deliberately rooted at the *same* per-user application directory the game
//! itself uses (`craftmjne`'s `save::app_data_dir`, duplicated here rather
//! than shared because the launcher must not depend on the game crate - it
//! has to build and run without Bevy). Keeping one root means the shared
//! `saves/` folder every instance reads and writes is exactly the folder a
//! directly-installed copy of the game already uses, so adopting the
//! launcher doesn't strand anyone's existing worlds.
//!
//! ```text
//! <app data>/
//! ├── saves/                     the game's worlds - shared by every instance
//! ├── versions/<version>/        one extracted game build per release
//! │   └── craftmjne[.exe], blocks/, textures/
//! └── launcher/
//!     ├── instances.json         see instances.rs
//!     └── staged/                a downloaded launcher update, pre-swap
//! ```

use std::path::PathBuf;

/// The per-user application directory, matching `craftmjne`'s
/// `save::app_data_dir` exactly - if you change one, change the other.
pub fn app_data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local).join("Craftmjne");
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join("Library/Application Support/Craftmjne");
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(xdg).join("craftmjne");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".local/share/craftmjne");
        }
    }
    PathBuf::from(".craftmjne")
}

/// Every file the launcher owns lives under one root so tests can point the
/// whole thing at a scratch directory, and so "uninstall the launcher"
/// stays a single folder to delete.
#[derive(Clone, Debug)]
pub struct Paths {
    root: PathBuf,
}

impl Default for Paths {
    fn default() -> Self {
        Self::at(app_data_dir())
    }
}

impl Paths {
    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    /// Where downloaded game builds are extracted, one directory per release
    /// version. Its existence *is* the "already downloaded" cache: a version
    /// with a directory here never gets fetched again.
    pub fn versions_dir(&self) -> PathBuf {
        self.root.join("versions")
    }

    pub fn version_dir(&self, version: &str) -> PathBuf {
        self.versions_dir().join(version)
    }

    /// The game executable inside an installed version's directory.
    pub fn game_exe(&self, version: &str) -> PathBuf {
        self.version_dir(version).join(format!("craftmjne{}", std::env::consts::EXE_SUFFIX))
    }

    pub fn launcher_dir(&self) -> PathBuf {
        self.root.join("launcher")
    }

    pub fn instances_file(&self) -> PathBuf {
        self.launcher_dir().join("instances.json")
    }

    /// Scratch space for a downloaded launcher build waiting to be swapped in.
    pub fn staging_dir(&self) -> PathBuf {
        self.launcher_dir().join("staged")
    }

    /// The shared worlds folder. The launcher never writes here - it's listed
    /// only so the UI can point at it - but every instance the launcher
    /// starts uses it, because the game resolves it the same way.
    pub fn saves_dir(&self) -> PathBuf {
        self.root.join("saves")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_path_stays_inside_the_one_root() {
        let paths = Paths::at(PathBuf::from("/tmp/scratch-root"));
        for p in [
            paths.versions_dir(),
            paths.version_dir("1.2.3"),
            paths.game_exe("1.2.3"),
            paths.launcher_dir(),
            paths.instances_file(),
            paths.staging_dir(),
            paths.saves_dir(),
        ] {
            assert!(p.starts_with("/tmp/scratch-root"), "{p:?} escaped the root");
        }
    }

    #[test]
    fn a_versions_directory_is_named_after_its_version() {
        let paths = Paths::at(PathBuf::from("/root"));
        assert_eq!(paths.version_dir("1.2.3"), PathBuf::from("/root/versions/1.2.3"));
        assert!(paths.game_exe("1.2.3").starts_with("/root/versions/1.2.3"));
    }

    #[test]
    fn the_game_executable_carries_the_platforms_extension() {
        let paths = Paths::at(PathBuf::from("/root"));
        let exe = paths.game_exe("1.0.0");
        let name = exe.file_name().unwrap().to_string_lossy().to_string();
        assert_eq!(name, format!("craftmjne{}", std::env::consts::EXE_SUFFIX));
    }
}
