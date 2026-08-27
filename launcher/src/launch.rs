//! Starting a game build.
//!
//! Deliberately fire-and-forget: the launcher spawns the game and does not
//! wait on it, so you can start an instance, close the launcher, and the
//! game keeps running - and so a crashed game can never take the launcher
//! down with it.

use std::process::Command;

use crate::instances::Instance;
use crate::library::Library;

/// Builds the command that would start `instance`, without running it.
/// Separated from [`launch`] so the arguments and working directory - the
/// parts with actual decisions in them - can be checked without spawning a
/// process.
pub fn command_for(library: &Library, instance: &Instance) -> Result<Command, String> {
    let exe = library.exe(&instance.version);
    if !exe.is_file() {
        return Err(format!(
            "Craftmjne {} isn't downloaded yet - install it from the Versions tab.",
            instance.version
        ));
    }
    let mut command = Command::new(&exe);
    command.args(instance.launch_args());
    // Run from the version's own directory. The game finds `blocks/` next to
    // its executable regardless (see its `find_blocks_dir`), but starting
    // there means anything it writes relative to the working directory - a
    // crash log, a smoke-test screenshot - lands with the build it came
    // from rather than wherever the launcher happened to be started from.
    command.current_dir(library.dir(&instance.version));
    Ok(command)
}

/// Spawns the game for `instance`.
pub fn launch(library: &Library, instance: &Instance) -> Result<(), String> {
    command_for(library, instance)?
        .spawn()
        .map(|_child| ())
        .map_err(|e| format!("couldn't start Craftmjne {}: {e}", instance.version))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;
    use std::path::PathBuf;

    struct TempRoot(PathBuf);
    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A scratch library holding one fake version. The directory name has to
    /// be unique per *call*, not per version: tests run in parallel, and two
    /// sharing a path would have one's cleanup delete the other's fixture
    /// out from under it.
    fn library_with(version: &str) -> (TempRoot, Library) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir()
            .join(format!("craftmjne-launch-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let paths = Paths::at(dir.clone());
        std::fs::create_dir_all(paths.version_dir(version)).unwrap();
        std::fs::write(paths.game_exe(version), b"not a real binary").unwrap();
        (TempRoot(dir), Library::new(paths))
    }

    #[test]
    fn launching_a_version_that_is_not_downloaded_explains_itself() {
        let (_temp, library) = library_with("1.0.0");
        let err = command_for(&library, &Instance::new("Missing", "9.9.9")).unwrap_err();
        assert!(err.contains("9.9.9"), "{err}");
        assert!(err.contains("isn't downloaded"), "{err}");
    }

    #[test]
    fn the_command_runs_the_selected_versions_executable_from_its_own_directory() {
        let (_temp, library) = library_with("1.0.0");
        let command = command_for(&library, &Instance::new("Main", "1.0.0")).unwrap();

        assert_eq!(command.get_program(), library.exe("1.0.0").as_os_str());
        assert_eq!(command.get_current_dir(), Some(library.dir("1.0.0").as_path()));
    }

    #[test]
    fn an_instances_options_reach_the_command_line() {
        let (_temp, library) = library_with("1.0.0");
        let mut instance = Instance::new("Tuned", "1.0.0");
        instance.render_distance = Some(4);
        instance.extra_args = "--no-update-check".to_string();

        let command = command_for(&library, &instance).unwrap();
        let args: Vec<String> =
            command.get_args().map(|a| a.to_string_lossy().to_string()).collect();
        assert_eq!(args, vec!["--render-distance", "4", "--no-update-check"]);
    }
}
