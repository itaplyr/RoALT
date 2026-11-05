//! RoALT's own updates on Linux.
//!
//! Releases are published on the Linux fork (`itaplyr/RoALT`) with four-part tags:
//! `v<upstream version>.<linux revision>`, for example `v0.9.0.1`. The upstream version stays
//! in `Cargo.toml`/`tauri.conf.json`; `LINUX_REVISION` here is bumped for each Linux release.
//! That keeps the fork's releases from colliding with upstream's `v0.9.1` and later.
//!
//! The app checks GitHub at start and every five minutes while `Settings.self_update` is on.
//! Installing downloads the matching `roalt-linux-x86_64` release asset, checks its SHA-256
//! sidecar, replaces the running executable and restarts RoALT. Nothing is installed without
//! the user's click.

use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use reqwest::header::ACCEPT;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};

use crate::error::{AppError, Result};
use crate::model::AppUpdate;

pub const UPDATE_EVENT: &str = "app://update";
/// Bumped for each Linux release: `v<Cargo version>.<this>`.
pub const LINUX_REVISION: u32 = 2;

const LATEST: &str = "https://api.github.com/repos/itaplyr/RoALT/releases/latest";
const CHECK_EVERY: Duration = Duration::from_secs(5 * 60);
const MAX_JSON: u64 = 1_000_000;
const MAX_BINARY: u64 = 200_000_000;
const MAX_SHA: u64 = 4_096;
const BIN_ASSET: &str = "roalt-linux-x86_64";
const SHA_ASSET: &str = "roalt-linux-x86_64.sha256";

#[derive(Default)]
pub struct SelfUpdater {
    available: Mutex<Option<Release>>,
    installing: AtomicBool,
}

impl SelfUpdater {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn installing(&self) -> bool {
        self.installing.load(Ordering::SeqCst)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    version: String,
    binary: String,
    sha256: String,
}

#[derive(Deserialize)]
struct Latest {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

fn current_version() -> String {
    format!("{}.{}", env!("CARGO_PKG_VERSION"), LINUX_REVISION)
}

/// `X.Y.Z` or `X.Y.Z.N`, numbers without leading zeros. No prereleases.
fn version_key(s: &str) -> Option<(u64, u64, u64, u64)> {
    let s = s.strip_prefix('v').unwrap_or(s);
    let mut parts = s.split('.');
    let mut n = [0u64; 4];
    let mut count = 0;
    for part in &mut parts {
        if count == 4 || part.is_empty() || part.len() > 9 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        if part.len() > 1 && part.starts_with('0') {
            return None;
        }
        n[count] = part.parse().ok()?;
        count += 1;
    }
    (3..=4).contains(&count).then_some((n[0], n[1], n[2], n[3]))
}

/// The version in `tag` when it is newer than `current`; never the same or older.
pub fn newer(current: &str, tag: &str) -> Option<String> {
    let version = tag.strip_prefix('v')?;
    let new = version_key(version)?;
    (new > version_key(current)?).then(|| version.to_owned())
}

fn pick(body: &[u8], current: &str) -> Result<Option<Release>> {
    let latest: Latest = serde_json::from_slice(body)
        .map_err(|e| AppError::Invalid(format!("GitHub's answer couldn't be read: {e}")))?;
    if latest.draft || latest.prerelease {
        return Ok(None);
    }
    let Some(version) = newer(current, &latest.tag_name) else { return Ok(None) };
    let url = |name: &str| latest.assets.iter().find(|a| a.name == name).map(|a| a.browser_download_url.clone());
    match (url(BIN_ASSET), url(SHA_ASSET)) {
        (Some(binary), Some(sha256)) => Ok(Some(Release { version, binary, sha256 })),
        _ => Err(AppError::Invalid(format!(
            "RoALT {version} is out, but its Linux binary or SHA-256 file is missing from the release."
        ))),
    }
}

fn http() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(format!("RoALT/{}", current_version()))
        .build()
        .map_err(|e| AppError::Internal(format!("RoALT's HTTP client couldn't start: {e}")))
}

async fn get(http: &reqwest::Client, url: &str, accept: &'static str, max: u64) -> Result<Vec<u8>> {
    let mut response = http.get(url).header(ACCEPT, accept).timeout(Duration::from_secs(30)).send().await
        .map_err(|e| AppError::Internal(format!("GitHub could not be reached: {e}")))?;
    if !response.status().is_success() {
        return Err(AppError::Invalid(format!("GitHub answered HTTP {}.", response.status().as_u16())));
    }
    if response.content_length().is_some_and(|n| n > max) {
        return Err(AppError::Invalid("That download is larger than expected.".into()));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| AppError::Internal(format!("The download broke off: {e}")))? {
        if (body.len() + chunk.len()) as u64 > max {
            return Err(AppError::Invalid("That download is larger than expected.".into()));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

async fn latest(http: &reqwest::Client) -> Result<Option<Release>> {
    let body = get(http, LATEST, "application/vnd.github+json", MAX_JSON).await?;
    pick(&body, &current_version())
}

pub fn status(s: &crate::App) -> AppUpdate {
    AppUpdate {
        current: current_version(),
        available: s.self_update.available.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|r| r.version.clone()),
    }
}

pub async fn check(app: &AppHandle) -> Result<AppUpdate> {
    let release = latest(&http()?).await?;
    let s = app.state::<crate::App>();
    *s.self_update.available.lock().unwrap_or_else(|e| e.into_inner()) = release;
    Ok(status(&s))
}

pub fn switched(app: &AppHandle, on: bool) {
    if !on {
        *app.state::<crate::App>().self_update.available.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let _ = app.emit(UPDATE_EVENT, status(&app.state::<crate::App>()));
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match check(&app).await {
            Ok(u) => {
                let _ = app.emit(UPDATE_EVENT, &u);
            }
            Err(e) => log::info!("RoALT update check failed: {e}"),
        }
    });
}

/// Checks at start and every five minutes while the setting is on.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let on = app.state::<crate::App>().db.settings().is_ok_and(|v| v.self_update);
            if on {
                match check(&app).await {
                    Ok(u) if u.available.is_some() => {
                        let _ = app.emit(UPDATE_EVENT, &u);
                    }
                    Ok(_) => {}
                    Err(e) => log::info!("RoALT update check failed: {e}"),
                }
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

pub async fn install(app: &AppHandle) -> Result<()> {
    let s = app.state::<crate::App>();
    if s.bulk.any_busy() {
        return Err(AppError::Invalid(tr!("Wait for the running launch, batch or auto-rejoin to finish, then update RoALT.")));
    }
    let release = {
        let cached = s.self_update.available.lock().unwrap_or_else(|e| e.into_inner()).clone();
        match cached {
            Some(r) => r,
            None => latest(&http()?).await?.ok_or_else(|| AppError::Invalid(tr!("RoALT is up to date.")))?,
        }
    };
    if !s.self_update.installing.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
        return Err(AppError::Invalid(tr!("RoALT is already being updated.")));
    }
    let result = install_release(app, &release).await;
    if result.is_err() {
        s.self_update.installing.store(false, Ordering::SeqCst);
    }
    result
}

async fn install_release(app: &AppHandle, release: &Release) -> Result<()> {
    let http = http()?;
    let binary = get(&http, &release.binary, "application/octet-stream", MAX_BINARY).await?;
    let sha = String::from_utf8(get(&http, &release.sha256, "text/plain", MAX_SHA).await?)
        .map_err(|_| AppError::Invalid("The SHA-256 file is not text.".into()))?;
    let wanted = sha.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
    let actual = format!("{:x}", Sha256::digest(&binary));
    if wanted.len() != 64 || wanted != actual {
        return Err(AppError::Invalid(tr!("The downloaded update failed its SHA-256 check.")));
    }

    let current = std::env::current_exe().map_err(|e| AppError::Internal(format!("RoALT's executable path is unknown: {e}")))?;
    let dir = current.parent().ok_or_else(|| AppError::Internal("RoALT's folder is unknown.".into()))?;
    let tmp = dir.join(format!(".roalt-update-{}", std::process::id()));
    std::fs::write(&tmp, &binary).map_err(|e| AppError::Internal(format!("Could not write the update beside RoALT: {e}")))?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| AppError::Internal(format!("Could not make the update executable: {e}")))?;
    std::fs::rename(&tmp, &current).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        AppError::Invalid(format!(
            "Could not replace {} (is RoALT installed by a package manager?): {e}",
            current.display()
        ))
    })?;
    log::info!("RoALT {} installed; restarting", release.version);
    app.request_restart();
    Ok(())
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_part_linux_tags_are_ordered() {
        assert_eq!(newer("0.9.0.1", "v0.9.0.2"), Some("0.9.0.2".into()));
        assert_eq!(newer("0.9.0.1", "v0.9.0.1"), None);
        assert_eq!(newer("0.9.0.1", "v0.9.1"), Some("0.9.1".into()));
        assert_eq!(newer("0.9.1.1", "v0.9.1"), None);
        assert_eq!(newer("0.9.0.1", "v0.10.0"), Some("0.10.0".into()));
        assert_eq!(newer("0.9.0.1", "v0.9.0.2-beta"), None);
    }

    #[test]
    fn release_needs_the_linux_binary_and_checksum() {
        let body = br#"{
          "tag_name": "v0.9.0.2",
          "draft": false,
          "prerelease": false,
          "assets": [
            {"name": "roalt-linux-x86_64", "browser_download_url": "https://github.com/itaplyr/RoALT/releases/download/v0.9.0.2/roalt-linux-x86_64"},
            {"name": "roalt-linux-x86_64.sha256", "browser_download_url": "https://github.com/itaplyr/RoALT/releases/download/v0.9.0.2/roalt-linux-x86_64.sha256"}
          ]
        }"#;
        let r = pick(body, "0.9.0.1").unwrap().unwrap();
        assert_eq!(r.version, "0.9.0.2");
        assert!(r.binary.ends_with("/roalt-linux-x86_64"));
        assert!(r.sha256.ends_with(".sha256"));
    }
}
