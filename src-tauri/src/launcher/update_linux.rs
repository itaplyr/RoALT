//! Roblox client updates on Linux. Cordial and Sober update their own Roblox build, so RoALT
//! does not download or run Roblox's Windows installer here. The UI still gets a truthful
//! `RobloxClient` (the launcher path and that it is managed for us).
#![allow(dead_code)]


use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use super::client;
use crate::App;
use crate::error::{AppError, Result};
use crate::model::RobloxClient;

pub const CLIENT_EVENT: &str = "roblox://client";
pub const INSTALL_EVENT: &str = "roblox://install";
pub const INSTALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);

#[derive(Default)]
pub struct Updater {
    sent: Mutex<Option<RobloxClient>>,
    installing: AtomicBool,
}

impl Updater {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn installing(&self) -> bool {
        self.installing.load(Ordering::SeqCst)
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn is_version(s: &str) -> bool {
    s.strip_prefix("version-").is_some_and(|h| h.len() == 16 && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}

pub fn installed_version(_exe: &Path) -> Option<String> {
    None
}

pub fn outdated(version: Option<&str>, latest: Option<&str>) -> bool {
    matches!((version, latest), (Some(v), Some(l)) if v != l)
}

pub fn may_auto_update(_enabled: bool, _c: &RobloxClient, _clients_running: usize, _busy: bool) -> bool {
    false
}

pub fn installer_url(_version: &str) -> Option<String> {
    None
}

pub async fn update_hold(_s: &App, _closing: usize) -> Option<String> {
    None
}

/// Linux has no RoALT-managed client update; the launcher is supplied by Cordial/Sober. The
/// production channel flag is what a normal launch uses.
pub async fn launch_client(_s: &App) -> Result<(client::ClientExe, bool)> {
    Ok((client::find()?, true))
}

pub fn client(_s: &App) -> RobloxClient {
    let exe = client::find().ok();
    RobloxClient {
        path: exe.map(|e| e.path.display().to_string()),
        bootstrapper: true,
        version: None,
        latest: None,
        outdated: false,
    }
}

pub fn publish(app: &AppHandle) -> RobloxClient {
    let s = app.state::<App>();
    let c = client(&s);
    let mut sent = lock(&s.update.sent);
    if sent.as_ref() != Some(&c) {
        let _ = app.emit(CLIENT_EVENT, &c);
        *sent = Some(c.clone());
    }
    c
}

pub fn spawn(_app: AppHandle) {}

pub async fn install(_app: &AppHandle) -> Result<RobloxClient> {
    Err(AppError::Invalid(tr!(
        "Roblox updates are handled by Cordial or Sober on Linux. Update the launcher with your package manager, then launch again."
    )))
}
