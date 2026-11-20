//! The default Roblox launcher on Linux: nothing RoALT can hold. `roblox-player:` and `roblox:`
//! links belong to the desktop (the browser's own protocol handling or the Cordial/Sober
//! launcher's desktop entry), and there is no registry to point at RoALT. These keep the same
//! shape as the Windows `links` module so the commands and the settings page compile unchanged;
//! the page shows no launcher to pick.

use tauri::AppHandle;

use crate::App;
use crate::error::{AppError, Result};
use crate::model::{LauncherKind, RobloxLauncher};

/// No default Roblox launcher can be set on Linux, so there is nothing to offer.
pub fn list() -> Vec<RobloxLauncher> {
    Vec::new()
}

/// Refuses: Roblox's links are not RoALT's to take here.
pub fn set(_s: &App, _path: &str) -> Result<LauncherKind> {
    Err(AppError::Invalid(tr!("Roblox links are opened by your desktop, not by RoALT, on Linux.")))
}

/// Nothing to take back after an install.
#[allow(dead_code)]
pub fn reclaim(_s: &App) {}

/// A `roblox-player:` link while RoALT was opened with it: the account isn't known, and Cordial's
/// profiles are per account, so this can't start one the way the Windows path does. Said in the
/// log; the link is never logged.
pub fn open(_app: &AppHandle, arg: &str) {
    let kind = if arg.split_once(':').is_some_and(|(s, _)| s.eq_ignore_ascii_case("roblox-player")) {
        "a roblox-player: link"
    } else {
        "a Roblox link"
    };
    log::warn!("ignored {kind}: RoALT can't open Roblox links on Linux, so no client was started");
}

/// Nothing starts a client from a link here, so the window never has to wait.
pub async fn started() {}
