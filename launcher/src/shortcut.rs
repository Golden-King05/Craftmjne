//! Desktop / Start Menu shortcuts for the launcher itself.
//!
//! `installer/craftmjne.nsi` already drops these at install time
//! (`$DESKTOP\Craftmjne.lnk`, `$SMPROGRAMS\Craftmjne\Craftmjne.lnk`), but
//! that only happens for someone who actually ran the installer. Anyone who
//! got the launcher another way - the rolling self-update in
//! `selfupdate.rs` replacing a portable .exe, a manual download, a dev
//! build - never gets one, and has no way to add one short of finding the
//! install folder and right-click-pinning it by hand. This offers the exact
//! same shortcut as a button in the launcher itself, pointed at whichever
//! executable is currently running.
//!
//! This whole module only exists on Windows (`main.rs` gates `mod
//! shortcut;` on it), matching the NSIS installer this mirrors: there's no
//! Desktop or Start Menu concept to target on macOS/Linux, and `mslnk` (the
//! crate that writes the `.lnk` binary format) reaches into
//! `std::os::windows` directly and simply doesn't compile elsewhere.
//! `app.rs`'s own "Add shortcut" buttons are gated the same way, so a
//! non-Windows build never shows a button for a feature that can't do
//! anything there rather than showing one that only ever errors.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use windows_sys::core::GUID;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::UI::Shell::{FOLDERID_Desktop, FOLDERID_Programs, SHGetKnownFolderPath};

/// Where to put the shortcut. Mirrors `installer/craftmjne.nsi`'s own two
/// shortcut locations.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Location {
    Desktop,
    StartMenu,
}

impl Location {
    pub fn label(self) -> &'static str {
        match self {
            Location::Desktop => "Desktop",
            Location::StartMenu => "Start Menu",
        }
    }
}

/// Resolves a known folder through the real Shell API rather than a
/// `%USERPROFILE%`-relative guess, so this still lands in the right place
/// under folder redirection (OneDrive Known Folder Move, a roaming
/// profile) - the same reason NSIS's own `$DESKTOP`/`$SMPROGRAMS`
/// variables don't hardcode a path either.
fn known_folder(id: &GUID) -> Result<PathBuf, String> {
    let mut raw = std::ptr::null_mut();
    // SAFETY: `id` is a valid GUID reference for the call's duration, `raw`
    // is an out-param the call itself initializes, and a null token is
    // documented as meaning "the current user".
    let hr = unsafe { SHGetKnownFolderPath(id as *const GUID, 0, std::ptr::null_mut(), &mut raw) };
    if hr < 0 || raw.is_null() {
        return Err(format!("Windows couldn't locate that folder (0x{hr:08X})."));
    }
    // SAFETY: `raw` was just confirmed non-null, and SHGetKnownFolderPath
    // documents its output as a NUL-terminated UTF-16 string allocated with
    // CoTaskMemAlloc - walking to the NUL is the documented way to find its
    // length, and CoTaskMemFree is the documented way to free it, exactly
    // once, after we're done reading it.
    let path = unsafe {
        let mut len = 0usize;
        while *raw.add(len) != 0 {
            len += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(raw, len));
        CoTaskMemFree(raw as *const c_void);
        s
    };
    Ok(PathBuf::from(path))
}

/// The folder a shortcut for `location` actually belongs in. `StartMenu`
/// nests under a `Craftmjne` subfolder, same as the installer's
/// `$SMPROGRAMS\Craftmjne\` - Desktop shortcuts don't get a subfolder
/// anywhere else in this project either, so this doesn't.
fn dir(location: Location) -> Result<PathBuf, String> {
    match location {
        Location::Desktop => known_folder(&FOLDERID_Desktop),
        Location::StartMenu => known_folder(&FOLDERID_Programs).map(|d| d.join("Craftmjne")),
    }
}

/// Writes (or overwrites) a `.lnk` named `{name}.lnk` in `dest_dir`,
/// pointing at `target`. The one function that actually touches disk;
/// [`create`] resolves `dest_dir` from the real Desktop/Start Menu folders
/// and calls straight through to this, and a test can point `dest_dir` at a
/// scratch directory instead (same "give tests a way to inject the
/// controlled input a real startup path resolves automatically" split as
/// `atlas::build_atlas_from_dir`). `target` has to be a real, existing path,
/// since `mslnk` reads its metadata to build the shortcut, but nothing about
/// the `.lnk` format itself needs a real Windows environment to write,
/// which is what lets this get a real `cargo test` despite the module only
/// *compiling* on Windows.
fn write_shortcut(target: &Path, dest_dir: &Path, name: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dest_dir)
        .map_err(|e| format!("Couldn't create {}: {e}", dest_dir.display()))?;
    let lnk = dest_dir.join(format!("{name}.lnk"));
    // `ShellLink::new` already sets the working directory to `target`'s own
    // parent internally, so there's nothing left to configure here.
    let link = mslnk::ShellLink::new(target)
        .map_err(|e| format!("Couldn't read {}: {e}", target.display()))?;
    link.create_lnk(&lnk).map_err(|e| format!("Couldn't write {}: {e}", lnk.display()))?;
    Ok(lnk)
}

/// Creates a shortcut to the currently-running launcher executable at
/// `location`. Every failure mode - can't resolve the folder, can't create
/// it, can't write the `.lnk` - comes back as a plain string for the status
/// line rather than a panic, matching this crate's usual "never crash on an
/// external environment failure" stance (`diagnostics.rs`).
pub fn create(location: Location) -> Result<String, String> {
    let exe =
        std::env::current_exe().map_err(|e| format!("Couldn't find my own executable: {e}"))?;
    let dest_dir = dir(location)?;
    let name = exe.file_stem().and_then(|s| s.to_str()).unwrap_or("Craftmjne Launcher");
    write_shortcut(&exe, &dest_dir, name)?;
    Ok(format!("Added a {} shortcut.", location.label()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique scratch directory per call, mirroring every other temp-dir
    /// test helper in this repo (`AtomicU64`, not the fixture's own
    /// contents) - parallel test threads must not race over the same path.
    fn scratch_dir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("craftmjne-shortcut-test-{}-{n}", std::process::id()))
    }

    #[test]
    fn writes_a_real_lnk_file_pointing_at_the_target() {
        let root = scratch_dir();
        let target = root.join("fake-launcher.exe");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&target, b"not a real PE, mslnk only reads metadata + the path").unwrap();
        let dest = root.join("dest");

        let lnk = write_shortcut(&target, &dest, "Craftmjne").unwrap();

        assert_eq!(lnk, dest.join("Craftmjne.lnk"));
        assert!(lnk.is_file());
        // The `.lnk` binary format starts with a fixed 4-byte header size
        // field, always exactly this value - a cheap, real assertion that
        // this is genuinely shortcut data and not an empty or garbage file.
        let bytes = std::fs::read(&lnk).unwrap();
        assert_eq!(&bytes[0..4], &0x4Cu32.to_le_bytes());
    }

    #[test]
    fn writing_again_overwrites_rather_than_erroring() {
        let root = scratch_dir();
        let target = root.join("fake-launcher.exe");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&target, b"v1").unwrap();
        let dest = root.join("dest");

        write_shortcut(&target, &dest, "Craftmjne").unwrap();
        // Re-running (as the UI button does every click) must not fail just
        // because the file - or the destination directory - already exists.
        let lnk = write_shortcut(&target, &dest, "Craftmjne").unwrap();
        assert!(lnk.is_file());
    }

    #[test]
    fn a_target_that_does_not_exist_is_a_real_error_not_a_panic() {
        let root = scratch_dir();
        let missing = root.join("does-not-exist.exe");
        let dest = root.join("dest");
        assert!(write_shortcut(&missing, &dest, "Craftmjne").is_err());
    }
}
