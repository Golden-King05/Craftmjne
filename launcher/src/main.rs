//! Craftmjne Launcher — downloads, manages and starts game versions.
//!
//! Why this exists: the game used to update itself by rewriting its own
//! running executable, which is fragile (antivirus and EDR are built to
//! catch exactly that), invisible when it silently failed, and impossible to
//! undo. The launcher replaces all of it. Game builds are ordinary folders
//! under `versions/<version>/`, downloaded once and kept, so switching or
//! rolling back a version is just picking a different folder to run - no
//! executable ever gets rewritten in place, and there is no in-game update
//! prompt at all any more.
//!
//! The one thing that *does* update itself is this launcher, and it does so
//! from its own dedicated branch (`src/selfupdate.rs`) so launcher updates
//! are completely independent of game releases.
//!
//! Module map:
//! - `paths`      — where everything lives on disk
//! - `library`    — installed versions (the download-once cache)
//! - `remote`     — GitHub releases: what exists, and fetching one
//! - `instances`  — named, editable launch configurations
//! - `launch`     — starting a game build
//! - `selfupdate` — the launcher updating itself
//! - `jobs`       — background work, so the window never blocks
//! - `app`        — the window

// Without this a Windows release build pops a console window behind the UI.
// Debug builds keep it, since that's where a panic message is worth seeing.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod instances;
mod jobs;
mod launch;
mod library;
mod paths;
mod remote;
mod selfupdate;

fn main() -> eframe::Result<()> {
    // `--version` without opening a window, so the release workflow (and
    // anyone debugging an install) can ask what build this is.
    if std::env::args().skip(1).any(|a| a == "--version" || a == "-V") {
        println!("craftmjne-launcher {}", selfupdate::CURRENT_VERSION);
        return Ok(());
    }

    let paths = paths::Paths::default();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([880.0, 560.0])
            .with_min_inner_size([640.0, 420.0])
            .with_title("Craftmjne Launcher"),
        ..Default::default()
    };
    eframe::run_native(
        "Craftmjne Launcher",
        options,
        Box::new(move |_cc| Ok(Box::new(app::LauncherApp::new(paths)))),
    )
}
