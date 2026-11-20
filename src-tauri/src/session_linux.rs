//! Linux has no `WM_WTSSESSION_CHANGE`/`WM_POWERBROADCAST` equivalent wired up here, so a locked
//! desktop never locks the vault or pauses anti-AFK through this module. These are the no-op
//! counterparts of the Windows API, keeping the shared callers (start-up, anti-AFK) compiling.

use tauri::AppHandle;

/// Whether the desktop is locked, as last told: never on Linux (no session watcher).
#[allow(dead_code)]
pub fn locked() -> bool {
    false
}

/// Nothing to follow: the Windows session/power watcher is not started on Linux.
pub fn spawn(_app: AppHandle) {}
