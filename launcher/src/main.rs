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
mod diagnostics;
mod instances;
mod jobs;
mod launch;
mod library;
mod paths;
mod remote;
mod selfupdate;

use eframe::Renderer;

fn main() {
    diagnostics::attach_console();
    diagnostics::install_panic_hook();

    // `--version` without opening a window, so the release workflow (and
    // anyone debugging an install) can ask what build this is.
    if std::env::args().skip(1).any(|a| a == "--version" || a == "-V") {
        println!("craftmjne-launcher {}", selfupdate::CURRENT_VERSION);
        return;
    }

    diagnostics::log(&format!(
        "starting craftmjne-launcher {} on {}",
        selfupdate::CURRENT_VERSION,
        std::env::consts::OS
    ));

    // Try wgpu first (DX12/Vulkan/Metal), then fall back to glow (OpenGL).
    // A machine that can't give wgpu an adapter - an older GPU, a remote
    // desktop session, a VM with no 3D acceleration, a driver that needs
    // updating - would otherwise fail here with no recourse, and until the
    // diagnostics above existed it failed *silently*. OpenGL is a much
    // lower bar and is very often available where DX12/Vulkan isn't, so
    // trying both turns a hard failure into a slower-but-working window.
    //
    // Note both arms below: a graphics backend that can't start is at least
    // as likely to *panic* as to return `Err` (wgpu panics outright when no
    // backend is compiled in). An earlier version only matched on `Err`, so
    // the panicking case blew straight past the fallback it was standing
    // next to. Anything with a fallback behind it has to catch both.
    for renderer in [Renderer::Wgpu, Renderer::Glow] {
        diagnostics::log(&format!("trying {renderer:?} renderer"));
        // Only the last attempt is allowed to interrupt the user - an earlier
        // one that fails is about to be retried, so a "crashed" dialog for it
        // would be actively misleading.
        diagnostics::suppress_dialogs(renderer != Renderer::Glow);
        match std::panic::catch_unwind(|| run(renderer)) {
            Ok(Ok(())) => {
                diagnostics::log("launcher exited normally");
                return;
            }
            Ok(Err(err)) => diagnostics::log(&format!("{renderer:?} failed: {err}")),
            // The panic hook has already logged the message and location by
            // the time this is reached; there's nothing useful in the
            // payload that isn't already in the log.
            Err(_) => diagnostics::log(&format!("{renderer:?} panicked; trying the next renderer")),
        }
    }

    diagnostics::suppress_dialogs(false);
    diagnostics::fatal(
        "Couldn't open the launcher window.\n\n\
         Both the DX12/Vulkan and OpenGL renderers failed to start, which \
         usually means the graphics driver needs updating, or this machine \
         has no 3D acceleration available (a virtual machine or a remote \
         desktop session, for example).",
    );
    std::process::exit(1);
}

fn run(renderer: Renderer) -> eframe::Result<()> {
    let paths = paths::Paths::default();
    let options = eframe::NativeOptions {
        renderer,
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
