//! Linux has no main-thread hang watchdog: the Windows version reads the stuck thread's Win32
//! stack and restarts in the tray, neither of which exists here. These are the no-op counterparts
//! of its API, so the shared start-up, exit and `--tray` handling compiles and behaves as before.

use tauri::AppHandle;

/// The restarted RoALT's `--tray` argument (`--after` is Windows only). Kept for the shared
/// start-up path, which checks for it.
pub const TRAY_ARG: &str = "--tray";

/// Nothing to watch: the Windows watchdog is not started on Linux.
pub fn spawn(_app: AppHandle) {}

/// No restarted process to wait for (`--after` is Windows only).
pub fn wait_for_previous() {}

/// Nothing restarts RoALT on Linux, so quitting has nothing to tell the watchdog.
pub fn exiting() {}
