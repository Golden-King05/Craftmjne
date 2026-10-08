//! Talking to GitHub: what versions exist, and downloading one into the
//! local library.
//!
//! The download/extract machinery is `self_update`'s, the same crate the
//! game's old in-game updater used. What's gone is the part that was
//! actually causing trouble: a running process rewriting its own
//! executable. Here the process doing the downloading (the launcher) is
//! never the process being replaced (a game build under `versions/`), which
//! is why this design fixes the update problem rather than relocating it.

use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub const REPO_OWNER: &str = "golden-king05";
pub const REPO_NAME: &str = "craftmjne";

/// One published game release the launcher can offer to install.
#[derive(Clone, Debug, PartialEq)]
pub struct RemoteVersion {
    /// Release version with any leading `v` stripped, e.g. `1.2.2` - this is
    /// also the directory name it installs to.
    pub version: String,
    pub name: String,
    pub date: String,
    pub notes: Option<String>,
    pub download_url: String,
}

/// The version-slot name the rolling dev build installs under
/// (`versions/dev/`), and the tag `.github/workflows/dev-build.yml`
/// publishes to. There's exactly one of these at a time - a new push to
/// main replaces the previous dev build rather than adding another - so
/// unlike a real `RemoteVersion` there's no list of them to choose from.
pub const DEV_VERSION_SLOT: &str = "dev";

/// The rolling "dev" release: the game built straight from `main`, not a
/// tagged version. See [`fetch_dev_build`].
#[derive(Clone, Debug, PartialEq)]
pub struct DevBuild {
    /// Full 40-character commit sha this build was made from - what
    /// `Library::dev_commit` compares an installed dev build against to
    /// decide whether it's stale.
    pub commit: String,
    /// First 7 characters of `commit`, for display.
    pub short_commit: String,
    pub download_url: String,
}

/// The one small JSON file `dev-build.yml` publishes alongside the platform
/// archives on the "dev" release - see that workflow's own comments for why
/// the commit has to travel this way rather than being inferred from the
/// release's name/date.
#[derive(serde::Deserialize)]
struct DevManifest {
    commit: String,
}

/// Public download URL of a file attached to the rolling dev release.
///
/// This is `github.com/.../releases/download/...`, a plain file download,
/// and deliberately *not* the REST API (`api.github.com`). The API allows
/// only 60 unauthenticated requests an hour per IP address, and the dev
/// build is re-checked every minute while the launcher is open - that alone
/// used the whole allowance, so GitHub started answering 403 for everything,
/// including the Versions list. The dev release's tag and file names are
/// fixed by `.github/workflows/dev-build.yml`, so there's nothing the API
/// would tell us that we can't name directly.
fn dev_asset_url(file_name: &str) -> String {
    format!("https://github.com/{REPO_OWNER}/{REPO_NAME}/releases/download/{DEV_VERSION_SLOT}/{file_name}")
}

/// The archive `dev-build.yml` publishes for the platform we're running on.
fn dev_archive_name() -> String {
    let ext = if cfg!(windows) { "zip" } else { "tar.gz" };
    format!("craftmjne-{}.{ext}", self_update::get_target())
}

/// Whether a download error was GitHub saying the file doesn't exist.
fn is_not_found(err: &str) -> bool {
    err.contains("status: 404")
}

/// Looks up the current rolling dev build. `Ok(None)` means no dev release
/// has been published yet (the workflow has never run) - not an error, just
/// nothing to offer right now. A release that exists but has no archive for
/// this platform (its build leg failed) shows up as an error at install
/// time instead, since telling that apart up front would cost another
/// request every minute.
pub fn fetch_dev_build() -> Result<Option<DevBuild>, String> {
    let bytes = match download_bytes(&dev_asset_url("dev-manifest.json")) {
        Ok(bytes) => bytes,
        Err(err) if is_not_found(&err) => return Ok(None),
        Err(err) => return Err(err),
    };
    let manifest: DevManifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let short_commit = manifest.commit.chars().take(7).collect();

    Ok(Some(DevBuild {
        commit: manifest.commit,
        short_commit,
        download_url: dev_asset_url(&dev_archive_name()),
    }))
}

/// Turns a GitHub error into something a player can act on. A 403 or 429
/// from the API almost always means its limit of 60 requests an hour for
/// unauthenticated clients was hit (by this launcher, or by anything else
/// on the same network) - "NetworkError: ... status: 403" reads like
/// GitHub is down or the repo is gone, which it isn't.
pub fn explain_github_error(err: &str) -> String {
    if err.contains("status: 403") || err.contains("status: 429") {
        "GitHub is limiting requests from your network for now (it allows 60 an hour \
         without signing in). Wait a while, then press Refresh."
            .to_string()
    } else {
        err.to_string()
    }
}

/// Downloads a small file (not a release archive - no extraction) fully
/// into memory. Reuses `self_update::Download`, the same primitive
/// `download_and_extract` builds on, rather than pulling in a second HTTP
/// client just for this.
fn download_bytes(url: &str) -> Result<Vec<u8>, String> {
    let mut download = self_update::Download::from_url(url);
    download.set_header(http::header::ACCEPT, "application/octet-stream".parse().unwrap());
    let mut buf = Vec::new();
    download.download_to(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

/// Live progress for an in-flight download, shared with the UI thread.
/// Bytes only - GitHub's release listing doesn't carry asset sizes, so
/// there's no honest total to show a percentage against, and inventing one
/// would be worse than showing the real number climbing.
#[derive(Clone, Default)]
pub struct Progress(Arc<AtomicU64>);

impl Progress {
    pub fn bytes(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

/// Wraps a writer to publish how much has been written so far.
struct CountingWriter<W> {
    inner: W,
    progress: Progress,
}

impl<W: Write> Write for CountingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.progress.0.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// Every published *game* release that ships a build for the platform we're
/// running on, newest first. A release with no matching asset (a platform
/// that failed to build, or one added later) is skipped rather than listed
/// as something that can't actually be installed.
///
/// This repo also publishes releases that are not game versions at all -
/// `launcher-v1.0.2` (the launcher's own updates, see `selfupdate.rs`) and
/// `dev` (see [`fetch_dev_build`]) - and both would otherwise show up here
/// too: `self_update::Release::version` is just the tag with a *leading* `v`
/// stripped (`"launcher-v1.0.2".trim_start_matches('v')` doesn't touch it,
/// since the tag doesn't start with `v`), and their assets independently
/// happen to contain the same platform-target substring `asset_for` matches
/// on. A real game version always starts with a digit (`"1.3.0"`); neither
/// `"launcher-v1.0.2"` nor `"dev"` do, which is what this filters on rather
/// than hardcoding the specific other tag prefixes that exist today.
fn looks_like_a_game_version(version: &str) -> bool {
    version.starts_with(|c: char| c.is_ascii_digit())
}

pub fn fetch_releases() -> Result<Vec<RemoteVersion>, String> {
    let releases = self_update::backends::github::ReleaseList::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .build()
        .and_then(|list| list.fetch())
        .map_err(|e| e.to_string())?;

    let target = self_update::get_target();
    Ok(releases
        .into_iter()
        .filter(|release| looks_like_a_game_version(&release.version))
        .filter_map(|release| {
            let asset = release.asset_for(target, None)?;
            Some(RemoteVersion {
                version: release.version,
                name: release.name,
                date: release.date,
                notes: release.body.filter(|b| !b.trim().is_empty()),
                download_url: asset.download_url,
            })
        })
        .collect())
}

/// Downloads and extracts one release into `dest` (its final
/// `versions/<version>` directory).
///
/// Extraction goes to a sibling scratch directory first and only then gets
/// renamed into place, so an interrupted or failed download can never leave
/// a half-populated version directory behind. That matters because
/// `Library::is_installed` trusts what's on disk: a partially extracted
/// build with the executable already written would otherwise read as
/// installed and launch into a crash from its missing `blocks/` folder.
pub fn install(download_url: &str, dest: &Path, progress: &Progress) -> Result<(), String> {
    let parent = dest.parent().ok_or("version directory has no parent")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;

    let scratch = parent.join(format!(
        ".incoming-{}",
        dest.file_name().and_then(|n| n.to_str()).unwrap_or("version")
    ));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;

    let result = download_and_extract(download_url, &scratch, progress);
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&scratch);
        return result;
    }

    let _ = std::fs::remove_dir_all(dest);
    std::fs::rename(&scratch, dest).map_err(|e| {
        let _ = std::fs::remove_dir_all(&scratch);
        e.to_string()
    })
}

fn download_and_extract(url: &str, into: &Path, progress: &Progress) -> Result<(), String> {
    let tmp = self_update::TempDir::new().map_err(|e| e.to_string())?;
    // The archive's own name decides how `Extract` picks a decompressor, so
    // it has to keep the extension the release asset was published with.
    let archive_path = tmp.path().join(archive_name(url));
    let file = std::fs::File::create(&archive_path).map_err(|e| e.to_string())?;

    let mut download = self_update::Download::from_url(url);
    // GitHub's release-asset endpoint returns JSON metadata unless the
    // request explicitly asks for the raw bytes.
    download.set_header(http::header::ACCEPT, "application/octet-stream".parse().unwrap());
    download
        .download_to(CountingWriter { inner: file, progress: progress.clone() })
        .map_err(|e| e.to_string())?;

    self_update::Extract::from_source(&archive_path)
        .extract_into(into)
        .map_err(|e| e.to_string())
}

/// The file name to save a download under, taken from the tail of its URL.
/// Only the extension really matters (it selects the archive format), so an
/// unrecognizable URL falls back to a name that still carries one rather
/// than failing outright.
pub fn archive_name(url: &str) -> String {
    let tail = url.rsplit('/').next().unwrap_or("");
    let tail = tail.split('?').next().unwrap_or(tail);
    if tail.ends_with(".zip") || tail.ends_with(".tar.gz") || tail.ends_with(".tgz") {
        tail.to_string()
    } else {
        format!("download{}", if cfg!(windows) { ".zip" } else { ".tar.gz" })
    }
}

/// Human-readable byte count for the UI.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[0])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_versions_are_told_apart_from_the_launchers_own_releases_and_dev_builds() {
        // Both `"launcher-v1.0.2"` (`self_update`'s own `version` field is
        // just the tag with a *leading* `v` stripped, which does nothing
        // here since the tag starts with "launcher") and `"dev"` have,
        // historically, ended up in the game's version list purely because
        // their release assets also happen to contain the target-triple
        // substring `asset_for` matches on - this is the guard that keeps
        // them out.
        assert!(looks_like_a_game_version("1.3.0"));
        assert!(looks_like_a_game_version("0.1.0"));
        assert!(!looks_like_a_game_version("launcher-v1.0.2"));
        assert!(!looks_like_a_game_version(DEV_VERSION_SLOT));
        assert!(!looks_like_a_game_version(""));
    }

    #[test]
    fn dev_build_files_are_fetched_as_plain_downloads_not_api_calls() {
        let manifest = dev_asset_url("dev-manifest.json");
        assert_eq!(manifest, "https://github.com/golden-king05/craftmjne/releases/download/dev/dev-manifest.json");
        assert!(!manifest.contains("api.github.com"));
        // Must match what dev-build.yml packages, extension included -
        // `Extract` picks its decompressor from it.
        let archive = dev_asset_url(&dev_archive_name());
        assert!(archive.ends_with(&format!("/dev/craftmjne-{}.zip", self_update::get_target()))
            || archive.ends_with(&format!("/dev/craftmjne-{}.tar.gz", self_update::get_target())));
        assert!(archive_name(&archive).starts_with("craftmjne-"));
    }

    #[test]
    fn only_a_404_counts_as_no_dev_build_yet() {
        assert!(is_not_found("Download request failed with status: 404"));
        assert!(!is_not_found("Download request failed with status: 403"));
        assert!(!is_not_found("error sending request for url"));
    }

    #[test]
    fn a_rate_limited_request_is_explained_not_shown_raw() {
        let raw = "NetworkError: api request failed with status: 403 - for: \"https://api.github.com/repos/golden-king05/craftmjne/releases?per_page=100\"";
        let friendly = explain_github_error(raw);
        assert!(friendly.contains("60 an hour"), "{friendly}");
        assert!(!friendly.contains("NetworkError"));
        // Anything else passes through untouched, so a real failure still
        // says what it was.
        assert_eq!(explain_github_error("dns error"), "dns error");
    }

    #[test]
    fn archive_name_keeps_the_extension_that_selects_the_decompressor() {
        assert_eq!(
            archive_name("https://github.com/o/r/releases/download/v1.0.0/craftmjne-x86_64-pc-windows-msvc.zip"),
            "craftmjne-x86_64-pc-windows-msvc.zip"
        );
        assert_eq!(
            archive_name("https://example.com/a/b/craftmjne-x86_64-unknown-linux-gnu.tar.gz"),
            "craftmjne-x86_64-unknown-linux-gnu.tar.gz"
        );
    }

    #[test]
    fn a_query_string_does_not_become_part_of_the_file_name() {
        assert_eq!(archive_name("https://example.com/thing.zip?token=abc"), "thing.zip");
    }

    #[test]
    fn an_unrecognizable_url_still_produces_a_usable_archive_name() {
        let name = archive_name("https://example.com/download");
        assert!(
            name.ends_with(".zip") || name.ends_with(".tar.gz"),
            "expected a name carrying an archive extension, got {name:?}"
        );
    }

    #[test]
    fn byte_counts_read_in_the_largest_sensible_unit() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2048), "2.0 KB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn a_counting_writer_reports_exactly_what_it_wrote() {
        let progress = Progress::default();
        let mut writer = CountingWriter { inner: Vec::new(), progress: progress.clone() };
        writer.write_all(&[0u8; 100]).unwrap();
        assert_eq!(progress.bytes(), 100);
        writer.write_all(&[0u8; 23]).unwrap();
        assert_eq!(progress.bytes(), 123);
        assert_eq!(writer.inner.len(), 123);
    }

    #[test]
    fn a_failed_install_leaves_no_half_written_version_directory() {
        let root = std::env::temp_dir()
            .join(format!("craftmjne-install-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dest = root.join("versions").join("9.9.9");

        // An unreachable URL stands in for any mid-download failure.
        let err = install("http://127.0.0.1:1/nope.zip", &dest, &Progress::default());

        assert!(err.is_err(), "expected the download to fail");
        assert!(!dest.exists(), "a failed install must not leave a version directory");
        let leftovers: Vec<_> = std::fs::read_dir(root.join("versions"))
            .map(|d| d.filter_map(|e| e.ok()).map(|e| e.file_name()).collect())
            .unwrap_or_default();
        assert!(leftovers.is_empty(), "scratch directory was left behind: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
