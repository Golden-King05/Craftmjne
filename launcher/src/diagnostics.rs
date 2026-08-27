//! Making startup failures visible.
//!
//! Release builds set `windows_subsystem = "windows"` (see `main.rs`) so the
//! launcher doesn't drag a console window around behind its UI. The cost is
//! that a process with no console has nowhere to print: a panic, or an `Err`
//! returned from `main`, is written to a stderr that doesn't exist. The
//! launcher exits with a non-zero code and *absolutely nothing else happens* -
//! no window, no message, no log. That is indistinguishable from
//! double-clicking a file that isn't executable at all, and it's exactly what
//! "I clicked the exe and nothing happened" looks like from the outside.
//!
//! So every startup failure has to be recorded somewhere the user can
//! actually reach:
//!
//! - **A log file**, always, at `<app data>/launcher/launcher.log`. Written
//!   from the first line of `main` onward, so even a failure inside
//!   `run_native` leaves a trail.
//! - **A native message box** on Windows for anything fatal, so the failure
//!   is visible without knowing a log file exists in the first place.
//! - **The parent console**, when there is one. `AttachConsole` reconnects
//!   stdout/stderr if the launcher was started from a terminal, which makes
//!   `--version` and error output work there while still costing a
//!   double-clicked launcher nothing.
//!
//! This module never returns errors of its own. Diagnostics that can fail
//! while reporting a failure are worse than no diagnostics - every write here
//! is best-effort, matching the same "never crash on an external environment
//! failure" stance as the game's texture loading and the old updater's
//! self-replace guard.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Where the log lives. Kept next to `instances.json` rather than beside the
/// executable: the install directory may not be writable (Program Files), and
/// this path is per-user and always writable by the person who ran it.
pub fn log_path() -> PathBuf {
    crate::paths::Paths::default().launcher_dir().join("launcher.log")
}

/// Seconds since the Unix epoch. Deliberately not a formatted date - that
/// would mean a `chrono`/`time` dependency for a line nobody reads unless
/// something already went wrong, and a raw epoch is still enough to tell
/// this run's entries from last week's.
fn stamp() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Append one line to the log. Best-effort: a failure to log is dropped
/// rather than escalated, since the only thing left to report it *to* is the
/// log that just failed.
pub fn log(msg: &str) {
    let path = log_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{}] {msg}", stamp());
    }
}

/// Report an unrecoverable startup failure: log it, then put it on screen.
///
/// The message box is what makes this worth having - a log file only helps
/// someone who already suspects a log file exists, whereas a dialog turns
/// "nothing happened" into a sentence you can act on (or paste into a bug
/// report).
pub fn fatal(msg: &str) {
    log(&format!("FATAL: {msg}"));
    let full = format!(
        "{msg}\n\nA full log is at:\n{}",
        log_path().display()
    );
    message_box("Craftmjne Launcher", &full);
}

#[cfg(windows)]
fn message_box(title: &str, msg: &str) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }
    let (title, msg) = (wide(title), wide(msg));
    // SAFETY: both pointers are NUL-terminated UTF-16 buffers that outlive
    // the call, and a null HWND is documented as "no owner window".
    unsafe {
        MessageBoxW(std::ptr::null_mut(), msg.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR);
    }
}

/// Everywhere else keeps a console by default, so stderr is a real
/// destination and no dialog is needed.
#[cfg(not(windows))]
fn message_box(title: &str, msg: &str) {
    eprintln!("{title}: {msg}");
}

/// Reconnect stdio to the terminal that launched us, if there was one.
///
/// A `windows_subsystem = "windows"` binary starts with no console at all,
/// so even running it from an open PowerShell prints nothing. `AttachConsole`
/// with the parent-process pseudo-handle borrows that terminal's console when
/// it exists, and fails harmlessly when it doesn't (a double-click), which is
/// why this is unconditional rather than gated on a flag.
#[cfg(windows)]
pub fn attach_console() {
    use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    // SAFETY: no arguments to get wrong; documented to fail cleanly when the
    // process has no parent console.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
pub fn attach_console() {}

/// Route panics into the log and a dialog instead of a stderr that may not
/// exist. Without this, a panic anywhere during startup is silent.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "unknown location".to_string());
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".to_string());
        fatal(&format!("The launcher crashed at {location}:\n{payload}"));
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_lives_under_the_launcher_directory() {
        let path = log_path();
        assert!(path.starts_with(crate::paths::Paths::default().launcher_dir()));
        assert_eq!(path.file_name().unwrap(), "launcher.log");
    }

    #[test]
    fn a_stamp_is_a_plausible_unix_time() {
        // Sanity check that the clock is being read at all, rather than
        // silently falling through to the 0 fallback on every call.
        assert!(stamp() > 1_700_000_000, "epoch seconds look wrong");
    }
}
