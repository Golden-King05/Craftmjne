//! The launcher updating itself, from its own dedicated branch.
//!
//! Checked once on every startup. The check reads a small JSON manifest
//! committed to [`LAUNCHER_BRANCH`] rather than looking at the repo's
//! releases, which is what keeps launcher updates completely independent of
//! game releases: publishing a new launcher means editing one file on one
//! branch, and it neither requires nor triggers a game version bump. It also
//! sidesteps GitHub's unauthenticated API rate limit, since `raw.github
//! usercontent.com` is a plain file fetch rather than an API call.
//!
//! ## Why self-replace is fine *here*, having been a problem in the game
//!
//! The game used to rewrite its own running executable, which is exactly the
//! behaviour antivirus and EDR heuristics exist to catch, and it happened
//! mid-session with a whole game's worth of state in the way. The launcher
//! is a different situation on every count: it's a small binary whose entire
//! job is managing installs, the swap happens seconds after startup with
//! nothing else going on, and if it fails the launcher still works - you
//! just keep running the old one and it says so. Nothing about a game
//! version's install depends on it, because game builds live in
//! `versions/<version>/` and are never swapped in place at all.

use serde::Deserialize;
use std::path::PathBuf;

use crate::remote::{REPO_NAME, REPO_OWNER};

/// The branch the launcher publishes its own updates from.
pub const LAUNCHER_BRANCH: &str = "launcher";

/// This build's version, which the manifest is compared against.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn manifest_url() -> String {
    format!(
        "https://raw.githubusercontent.com/{REPO_OWNER}/{REPO_NAME}/{LAUNCHER_BRANCH}/launcher/manifest.json"
    )
}

/// The launcher update manifest, as committed to [`LAUNCHER_BRANCH`] at
/// `launcher/manifest.json`:
///
/// ```json
/// {
///   "version": "1.0.1",
///   "assets": {
///     "x86_64-pc-windows-msvc": "https://github.com/.../CraftmjneLauncher-x86_64-pc-windows-msvc.zip",
///     "x86_64-unknown-linux-gnu": "https://github.com/.../craftmjne-launcher-x86_64-unknown-linux-gnu.tar.gz"
///   }
/// }
/// ```
///
/// A platform simply absent from `assets` means "no launcher update for you
/// right now", which is the correct outcome when one platform's build failed
/// - not an error, and not a reason to offer a download that can't work.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Manifest {
    pub version: String,
    #[serde(default)]
    pub assets: std::collections::HashMap<String, String>,
    /// Optional human-readable summary the launcher shows next to the
    /// update prompt.
    #[serde(default)]
    pub notes: Option<String>,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|e| e.to_string())
    }

    /// The download for the platform we're running on, if the manifest has
    /// one.
    pub fn asset_for(&self, target: &str) -> Option<&str> {
        self.assets.get(target).map(String::as_str)
    }
}

/// What the startup check found. Every variant except `Applied` leaves the
/// running launcher perfectly usable - an update failing is never allowed to
/// be the reason you can't get into the game.
#[derive(Clone, Debug, PartialEq)]
pub enum SelfUpdate {
    UpToDate,
    /// Swapped in successfully; the new version takes effect on restart.
    Applied { version: String, notes: Option<String> },
    /// Checked, but couldn't complete - shown quietly, never blocking.
    Failed(String),
}

/// Decides what the check should do, given a fetched manifest. Split out
/// from the network and disk work so the actual decision - the part with
/// rules in it - is testable without either.
pub fn decide(manifest: &Manifest, current_version: &str, target: &str) -> Result<String, SelfUpdate> {
    if !crate::library::is_newer(current_version, &manifest.version) {
        return Err(SelfUpdate::UpToDate);
    }
    match manifest.asset_for(target) {
        Some(url) => Ok(url.to_string()),
        None => Err(SelfUpdate::Failed(format!(
            "launcher {} is available but has no build for {target}",
            manifest.version
        ))),
    }
}

/// Fetches the manifest, and if it names a newer launcher with a build for
/// this platform, downloads it and swaps it in.
pub fn check_and_apply(staging_dir: PathBuf) -> SelfUpdate {
    let manifest = match fetch_manifest() {
        Ok(manifest) => manifest,
        // No manifest yet (the branch may not exist until the first launcher
        // release), no network, offline - all completely normal, and none of
        // them are worth interrupting someone who just wants to play.
        Err(err) => return SelfUpdate::Failed(err),
    };

    let target = self_update::get_target();
    let url = match decide(&manifest, CURRENT_VERSION, target) {
        Ok(url) => url,
        Err(outcome) => return outcome,
    };

    let _ = std::fs::remove_dir_all(&staging_dir);
    if let Err(err) = std::fs::create_dir_all(&staging_dir) {
        return SelfUpdate::Failed(err.to_string());
    }
    if let Err(err) = crate::remote::install(&url, &staging_dir.join("new"), &Default::default()) {
        return SelfUpdate::Failed(err);
    }

    let exe_name = format!("craftmjne-launcher{}", std::env::consts::EXE_SUFFIX);
    let staged = staging_dir.join("new").join(&exe_name);
    if !staged.is_file() {
        return SelfUpdate::Failed(format!("launcher archive did not contain {exe_name}"));
    }
    match self_update::self_replace::self_replace(&staged) {
        Ok(()) => {
            let _ = std::fs::remove_dir_all(&staging_dir);
            SelfUpdate::Applied { version: manifest.version, notes: manifest.notes }
        }
        Err(err) => SelfUpdate::Failed(err.to_string()),
    }
}

fn fetch_manifest() -> Result<Manifest, String> {
    let mut buf: Vec<u8> = Vec::new();
    self_update::Download::from_url(&manifest_url())
        .download_to(&mut buf)
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8(buf).map_err(|e| e.to_string())?;
    Manifest::parse(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOWS: &str = "x86_64-pc-windows-msvc";
    const LINUX: &str = "x86_64-unknown-linux-gnu";

    fn manifest(version: &str) -> Manifest {
        Manifest::parse(&format!(
            r#"{{"version":"{version}","assets":{{
                "{WINDOWS}":"https://example.com/launcher-win.zip",
                "{LINUX}":"https://example.com/launcher-linux.tar.gz"
            }}}}"#
        ))
        .unwrap()
    }

    #[test]
    fn the_manifest_url_points_at_the_launchers_own_branch() {
        let url = manifest_url();
        assert!(url.contains(LAUNCHER_BRANCH), "{url}");
        assert!(url.starts_with("https://raw.githubusercontent.com/"), "{url}");
        assert!(url.ends_with("/launcher/manifest.json"), "{url}");
    }

    #[test]
    fn a_manifest_parses_its_version_and_per_platform_assets() {
        let m = manifest("1.2.0");
        assert_eq!(m.version, "1.2.0");
        assert_eq!(m.asset_for(WINDOWS), Some("https://example.com/launcher-win.zip"));
        assert_eq!(m.asset_for("some-unbuilt-target"), None);
    }

    #[test]
    fn a_manifest_with_only_a_version_is_valid() {
        // `assets` and `notes` both default, so the smallest legal manifest
        // is just a version - it simply won't offer anything to download.
        let m = Manifest::parse(r#"{"version":"2.0.0"}"#).unwrap();
        assert_eq!(m.version, "2.0.0");
        assert!(m.assets.is_empty());
        assert_eq!(m.notes, None);
    }

    #[test]
    fn a_malformed_manifest_is_an_error_rather_than_a_panic() {
        assert!(Manifest::parse("not json").is_err());
        assert!(Manifest::parse(r#"{"assets":{}}"#).is_err(), "version is required");
    }

    #[test]
    fn an_older_or_equal_manifest_version_means_up_to_date() {
        assert_eq!(decide(&manifest("1.0.0"), "1.0.0", WINDOWS), Err(SelfUpdate::UpToDate));
        assert_eq!(decide(&manifest("0.9.0"), "1.0.0", WINDOWS), Err(SelfUpdate::UpToDate));
    }

    #[test]
    fn a_newer_manifest_version_returns_this_platforms_download() {
        assert_eq!(
            decide(&manifest("1.1.0"), "1.0.0", LINUX),
            Ok("https://example.com/launcher-linux.tar.gz".to_string())
        );
        // Numeric comparison, not alphabetical - 1.10 really is newer than 1.9.
        assert!(decide(&manifest("1.10.0"), "1.9.0", LINUX).is_ok());
    }

    #[test]
    fn a_newer_version_with_no_build_for_this_platform_reports_it_instead_of_pretending() {
        let outcome = decide(&manifest("1.1.0"), "1.0.0", "aarch64-unknown-weird");
        match outcome {
            Err(SelfUpdate::Failed(msg)) => {
                assert!(msg.contains("1.1.0"), "{msg}");
                assert!(msg.contains("aarch64-unknown-weird"), "{msg}");
            }
            other => panic!("expected a reported failure, got {other:?}"),
        }
    }

    #[test]
    fn notes_ride_along_when_the_manifest_supplies_them() {
        let m = Manifest::parse(
            r#"{"version":"1.1.0","assets":{},"notes":"Adds instance duplication"}"#,
        )
        .unwrap();
        assert_eq!(m.notes.as_deref(), Some("Adds instance duplication"));
    }
}
