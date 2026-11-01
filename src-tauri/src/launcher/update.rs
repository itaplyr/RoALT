//! Roblox client updates: the version check, auto-update and the built-in installer.
//!
//! - Check: `clientsettingscdn.roblox.com/v2/client-version/WindowsPlayer` → `clientVersionUpload`,
//!   compared with the `Versions\version-…` folder the client is in. At start and every 30 min;
//!   `roblox://client` carries the new `RobloxClient` when anything changed.
//! - Install (`roblox_install`, or auto-update): refused while any Roblox client runs (an update
//!   ends them all), while a launch, batch or auto-rejoin runs, and over a bootstrapper install.
//!   Downloads `setup.rbxcdn.com/<version>-RobloxPlayerInstaller.exe` (≤ 50 MB) into a fresh
//!   temp folder, write-locks it, checks its Authenticode signature is Roblox Corporation's
//!   (`authenticode`) and only then runs it, waits (≤ 10 min), finds the client again and
//!   deletes the folder. Progress goes out as `roblox://install`. Launches are refused meanwhile.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

use super::{authenticode, client};
use crate::App;
use crate::error::{AppError, Result};
use crate::model::{InstallProgress, RobloxClient};
use crate::roblox_api::endpoints;

pub const CLIENT_EVENT: &str = "roblox://client";
pub const INSTALL_EVENT: &str = "roblox://install";
const CHECK_EVERY: Duration = Duration::from_secs(30 * 60);
const MAX_INSTALLER: u64 = 50_000_000;
pub const INSTALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Win32 `FILE_SHARE_READ`: others may read (and run) the file, never write or delete it.
pub(crate) const FILE_SHARE_READ: u32 = 0x1;
fn bootstrapper() -> String {
    tr!("A bootstrapper (e.g. Bloxstrap) manages this Roblox install and keeps it up to date; RoALT leaves it alone.")
}

#[derive(Default)]
pub struct Updater {
    /// Roblox's live version, once checked.
    latest: Mutex<Option<String>>,
    /// What `roblox://client` last said.
    sent: Mutex<Option<RobloxClient>>,
    installing: AtomicBool,
    /// The version auto-update last tried: a failed install isn't retried every 30 minutes.
    auto_tried: Mutex<Option<String>>,
}

impl Updater {
    pub fn new() -> Self {
        Self::default()
    }

    /// The installer runs (or is being downloaded): launches are refused.
    pub fn installing(&self) -> bool {
        self.installing.load(Ordering::SeqCst)
    }

    fn begin(&self) -> Result<Installing<'_>> {
        self.installing
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| AppError::Invalid(tr!("Roblox is already being installed.")))?;
        Ok(Installing(&self.installing))
    }
}

struct Installing<'a>(&'a AtomicBool);

impl Drop for Installing<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// `version-` and 16 lowercase hex digits: the only thing that goes into the installer URL.
pub fn is_version(s: &str) -> bool {
    s.strip_prefix("version-").is_some_and(|h| h.len() == 16 && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}

/// The installed version of a client at `...\Versions\version-xxxx\RobloxPlayerBeta.exe`.
pub fn installed_version(exe: &Path) -> Option<String> {
    let dir = exe.parent()?;
    if !dir.parent()?.file_name()?.eq_ignore_ascii_case("Versions") {
        return None;
    }
    let name = dir.file_name()?.to_str()?.to_ascii_lowercase();
    is_version(&name).then_some(name)
}

/// Both versions known and different.
pub fn outdated(version: Option<&str>, latest: Option<&str>) -> bool {
    matches!((version, latest), (Some(v), Some(l)) if v != l)
}

/// Auto-update runs only when turned on, Roblox's own client is installed and outdated, no
/// Roblox client runs and nothing (launch, batch, auto-rejoin, another install) is busy.
pub fn may_auto_update(enabled: bool, c: &RobloxClient, clients_running: usize, busy: bool) -> bool {
    enabled && c.outdated && c.path.is_some() && !c.bootstrapper && clients_running == 0 && !busy
}

/// `https://setup.rbxcdn.com/<version>-RobloxPlayerInstaller.exe`; None unless `is_version`.
pub fn installer_url(version: &str) -> Option<String> {
    is_version(version).then(|| format!("{}{version}-RobloxPlayerInstaller.exe", endpoints::SETUP))
}

/// A bootstrapper with an older Roblox than the live one closes every client as it updates, so
/// with clients running a launch through it waits (pure). Anything unknown doesn't hold it up.
// ponytail: Bloxstrap's optional background updates don't close clients, but only under conditions
// it decides at launch; such a user is asked to close clients too. Read its settings if they mind.
fn bootstrapper_would_close(installed: Option<&str>, latest: Option<&str>, running: usize) -> bool {
    running > 0 && outdated(installed, latest)
}

/// The live version on this PC's release channel, asked now (a bootstrapper follows the channel);
/// None when Roblox can't be asked, so nothing is held up on a guess.
async fn live_version(s: &App) -> Option<String> {
    let on_channel = match client::channel() {
        Some(channel) => s.api.client_version_on(&channel).await,
        None => s.api.client_version().await,
    };
    match on_channel {
        Ok(v) => Some(v),
        // Unknown to Roblox or closed to this PC: bootstrappers go back to production then.
        Err(AppError::NotFound | AppError::CookieInvalid | AppError::Roblox(_)) if client::channel().is_some() => s.api.client_version().await.ok(),
        Err(e) => {
            log::info!("bootstrapper check: Roblox version unknown: {e}");
            None
        }
    }
}

/// Why this launch must wait for an update that would close running clients, or None: a
/// bootstrapper's (`bootstrapper_would_close`), or production's build missing (`launch_client`
/// would start a client that runs Roblox's installer). `closing`: clients about to be closed for
/// it, which don't count as running.
pub async fn update_hold(s: &App, closing: usize) -> Option<String> {
    let exe = client::find().ok()?;
    let running = crate::instances::process::roblox_processes().len().saturating_sub(closing);
    if running == 0 {
        return None;
    }
    if let Some(installed) = client::bootstrapper_version(&exe) {
        let live = live_version(s).await?;
        return bootstrapper_would_close(Some(&installed), Some(&live), running)
            .then(|| tr!("Roblox has an update, and your bootstrapper (e.g. Bloxstrap) would close every running client to install it. Close your clients first, then launch."));
    }
    if client::managed_by_bootstrapper(&exe) {
        return None;
    }
    let live = production_version(s).await?;
    client::installed_build(&live)
        .is_none()
        .then(|| tr!("Roblox has an update, and installing it would close every running client. Close your clients first, then launch."))
}

/// Production's live version, asked now, else the last one known; None when never known.
async fn production_version(s: &App) -> Option<String> {
    match s.api.client_version().await {
        Ok(v) => {
            *lock(&s.update.latest) = Some(v.clone());
            Some(v)
        }
        Err(e) => {
            log::info!("launch: Roblox version check failed, using the last one known: {e}");
            lock(&s.update.latest).clone()
        }
    }
}

/// The client a launch starts, and whether its URI asks for production (`uri::player_uri`).
///
/// Roblox rewrites `clientExe` and the registry's channel for whichever account last played (each
/// is enrolled in a channel of its own). A client whose build doesn't match the channel it asks
/// about runs the foreground installer, which closes every running client (research multi-instance
/// M9). So: production's build with an empty `channel:`, as they always agree. Over a bootstrapper,
/// or with the version unknown, what `client::find` says, as is. Production's build missing: the
/// client found, asked for production, updates itself (`update_hold` kept it from closing others).
pub async fn launch_client(s: &App) -> Result<(client::ClientExe, bool)> {
    let exe = client::find()?;
    let managed = client::managed_by_bootstrapper(&exe);
    let live = if managed { None } else { production_version(s).await };
    let chosen = launch_choice(exe, managed, live.as_deref(), client::installed_build);
    log::info!(
        "launching {} (production: {}, bootstrapper manages this install: {managed}, Roblox's live version: {live:?}, channel: {:?}, roblox-player handler: {:?})",
        chosen.0.path.display(),
        chosen.1,
        client::channel(),
        client::handler(),
    );
    // Asked for production without having it: this client updates itself into a new folder.
    if chosen.1 && live.as_deref().is_some_and(|v| client::installed_build(v).is_none()) {
        log::warn!("Roblox's live version isn't installed: the updated client won't have this launch's graphics flags, which go in the old version's folder");
    }
    Ok(chosen)
}

/// `launch_client`'s choice (pure but for `installed`).
fn launch_choice(exe: client::ClientExe, managed: bool, live: Option<&str>, installed: impl FnOnce(&str) -> Option<PathBuf>) -> (client::ClientExe, bool) {
    let Some(live) = live.filter(|_| !managed) else { return (exe, false) };
    match installed(live) {
        Some(path) => (client::ClientExe { path, bootstrapper: false }, true),
        None => (exe, true),
    }
}

/// The Roblox client as found now, with the last known live version.
pub fn client(s: &App) -> RobloxClient {
    let exe = client::find().ok();
    let bootstrapper = exe.as_ref().is_some_and(client::managed_by_bootstrapper);
    let version = exe.as_ref().filter(|_| !bootstrapper).and_then(|e| installed_version(&e.path));
    let latest = lock(&s.update.latest).clone();
    RobloxClient {
        path: exe.map(|e| e.path.display().to_string()),
        bootstrapper,
        outdated: outdated(version.as_deref(), latest.as_deref()),
        version,
        latest,
    }
}

/// `client`, sent as `roblox://client` when it changed since last time.
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

/// Checks the version a few seconds after start, then every 30 minutes.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(3)).await;
        loop {
            check(&app).await;
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

async fn check(app: &AppHandle) {
    let s = app.state::<App>();
    match s.api.client_version().await {
        Ok(v) => *lock(&s.update.latest) = Some(v),
        Err(e) => log::info!("Roblox version check failed: {e}"),
    }
    let c = publish(app);
    let enabled = s.db.settings().inspect_err(|e| log::warn!("auto-update: settings unreadable, skipped: {e}")).is_ok_and(|v| v.auto_update);
    let running = crate::instances::process::roblox_processes().len();
    if !may_auto_update(enabled, &c, running, s.bulk.any_busy() || s.update.installing() || s.self_update.installing()) {
        return;
    }
    {
        let mut tried = lock(&s.update.auto_tried);
        if *tried == c.latest {
            return;
        }
        tried.clone_from(&c.latest);
    }
    log::info!("auto-update: {:?} → {:?}", c.version, c.latest);
    match install(app).await {
        Ok(c) => crate::commands::notice(
            app,
            false,
            match c.version.as_deref() {
                Some(v) => tr!("Roblox was updated to {version}.", version = v),
                None => tr!("Roblox was updated to the latest version."),
            },
        ),
        Err(e) => crate::commands::notice(app, true, tr!("Roblox couldn't be updated: {why}", why = e.message())),
    }
}

/// Downloads, verifies and runs Roblox's installer (see the module notes). Ends with
/// `Done` or `Error` on `roblox://install` and a fresh `roblox://client`.
pub async fn install(app: &AppHandle) -> Result<RobloxClient> {
    let s = app.state::<App>();
    let installing = s.update.begin()?;
    let result = run_install(app, &s).await;
    drop(installing);
    let _ = app.emit(
        INSTALL_EVENT,
        match &result {
            Ok(()) => InstallProgress::Done,
            Err(e) => InstallProgress::Error { message: e.message() },
        },
    );
    if let Err(e) = &result {
        log::warn!("Roblox install failed: {e}");
    }
    result.map(|()| publish(app))
}

async fn run_install(app: &AppHandle, s: &App) -> Result<()> {
    // `installing` is set first: a launch from now on is refused, one already going is seen here.
    if s.bulk.any_busy() {
        return Err(AppError::Invalid(tr!("Wait for the running launch, batch or auto-rejoin to finish, then install Roblox.")));
    }
    if !crate::instances::process::roblox_processes().is_empty() {
        return Err(AppError::Invalid(tr!("Close every Roblox client first: installing Roblox ends them.")));
    }
    if client::find().is_ok_and(|e| client::managed_by_bootstrapper(&e)) {
        return Err(AppError::Invalid(bootstrapper()));
    }
    let emit = |p: InstallProgress| {
        let _ = app.emit(INSTALL_EVENT, p);
    };
    emit(InstallProgress::Downloading { percent: 0 });
    let version = s.api.client_version().await?;
    *lock(&s.update.latest) = Some(version.clone());
    let url = installer_url(&version).ok_or_else(|| AppError::Roblox("unexpected version name".into()))?;

    let dir = TempDir::new("roalt-install-")?;
    let mut shown = 0;
    let bytes = s
        .api
        .download(&url, MAX_INSTALLER, |got, total| {
            let percent = total.map_or(0, |t| (got.saturating_mul(100) / t.max(1)).min(100) as u8);
            if percent != shown {
                shown = percent;
                emit(InstallProgress::Downloading { percent });
            }
        })
        .await?;
    let exe = dir.0.join("RobloxPlayerInstaller.exe");
    File::create_new(&exe)?.write_all(&bytes)?;
    drop(bytes);
    // Nothing can change the file between the check and the run: no write sharing until it ends.
    let pinned = OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).open(&exe)?;

    emit(InstallProgress::Verifying);
    let checked = exe.clone();
    blocking(move || authenticode::verify(&checked, authenticode::ROBLOX)).await?;

    emit(InstallProgress::Installing);
    log::info!("running Roblox's installer ({version}, signature checked)");
    let code = blocking(move || run(&exe, INSTALL_TIMEOUT)).await?;
    drop(pinned);
    if code == Some(0) {
        log::info!("Roblox's installer exited with {code:?}");
    } else {
        log::warn!("Roblox's installer exited with {code:?}");
    }
    match client::find() {
        Ok(e) => {
            let installed = installed_version(&e.path);
            if installed.as_deref() != Some(version.as_str()) {
                return Err(AppError::Internal(match installed.as_deref() {
                    Some(v) => tr!("Roblox's installer finished, but Roblox is still on {version}.", version = v),
                    None => tr!("Roblox's installer finished, but Roblox is still on an unknown version."),
                }));
            }
            Ok(())
        }
        Err(_) => Err(AppError::Internal(tr!("Roblox's installer finished, but Roblox wasn't found. Install it from roblox.com."))),
    }
}

/// Runs `exe` in its folder and waits for it, at most `timeout` (then it is ended and Err).
fn run(exe: &Path, timeout: Duration) -> Result<Option<i32>> {
    let dir = exe.parent().ok_or_else(|| AppError::Internal("installer folder unknown".into()))?;
    let mut child = Command::new(exe)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| AppError::Internal(tr!("Could not start Roblox's installer: {why}", why = e)))?;
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status.code());
        }
        if Instant::now() > deadline {
            // Ended, so launches aren't let in under a live installer and its folder can go.
            if let Err(e) = child.kill() {
                log::warn!("could not end Roblox's installer (pid {}): {e}", child.id());
            }
            if let Err(e) = child.wait() {
                log::warn!("could not wait for Roblox's installer (pid {}): {e}", child.id());
            }
            return Err(AppError::Internal(tr!("Roblox's installer took over 10 minutes and was stopped. Try again, or install Roblox from roblox.com.")));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

pub(crate) async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| AppError::Internal(e.to_string()))?
}

/// A fresh, uniquely named folder in %TEMP% (`prefix` + a UUID), deleted with everything in it on drop.
pub(crate) struct TempDir(pub(crate) PathBuf);

impl TempDir {
    pub(crate) fn new(prefix: &str) -> Result<Self> {
        let dir = std::env::temp_dir().join(format!("{prefix}{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir)?;
        Ok(Self(dir))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_dir_all(&self.0) {
            log::warn!("could not delete {}: {e}", self.0.display());
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_names() {
        assert!(is_version("version-9ac8595712b447a7"));
        for bad in [
            "",
            "version-",
            "version-9ac8595712b447a",
            "version-9ac8595712b447a77",
            "version-9AC8595712B447A7",
            "version-9ac8595712b447g7",
            "Version-9ac8595712b447a7",
            "version-9ac8595712b447a7/../x",
            " version-9ac8595712b447a7",
        ] {
            assert!(!is_version(bad), "{bad:?}");
        }
        assert_eq!(
            installer_url("version-9ac8595712b447a7").as_deref(),
            Some("https://setup.rbxcdn.com/version-9ac8595712b447a7-RobloxPlayerInstaller.exe")
        );
        assert_eq!(installer_url("version-x?y"), None);
    }

    #[test]
    fn version_folders() {
        let v = |p: &str| installed_version(Path::new(p));
        let own = r"C:\Users\you\AppData\Local\Roblox\Versions\version-9ac8595712b447a7\RobloxPlayerBeta.exe";
        assert_eq!(v(own).as_deref(), Some("version-9ac8595712b447a7"));
        assert_eq!(v(r"D:\R\VERSIONS\Version-9AC8595712B447A7\RobloxPlayerBeta.exe").as_deref(), Some("version-9ac8595712b447a7"));
        assert_eq!(v(r"C:\Users\you\AppData\Local\Bloxstrap\Bloxstrap.exe"), None);
        assert_eq!(v(r"C:\Roblox\Other\version-9ac8595712b447a7\RobloxPlayerBeta.exe"), None);
        assert_eq!(v(r"C:\Roblox\Versions\version-xyz\RobloxPlayerBeta.exe"), None);
        assert_eq!(v("RobloxPlayerBeta.exe"), None);
    }

    fn roblox(version: Option<&str>, latest: Option<&str>, bootstrapper: bool) -> RobloxClient {
        RobloxClient {
            path: Some(r"C:\x\RobloxPlayerBeta.exe".into()),
            bootstrapper,
            version: version.map(Into::into),
            latest: latest.map(Into::into),
            outdated: outdated(version, latest),
        }
    }

    #[test]
    fn launches_production_unless_a_bootstrapper_manages_it_or_the_version_is_unknown() {
        let found = client::ClientExe { path: PathBuf::from(r"C:\R\version-test\RobloxPlayerBeta.exe"), bootstrapper: false };
        let prod = PathBuf::from(r"C:\R\version-live\RobloxPlayerBeta.exe");
        let there = |v: &str| (v == "version-live").then(|| prod.clone());
        let (exe, production) = launch_choice(found.clone(), false, Some("version-live"), there);
        assert_eq!((exe.path, production), (prod.clone(), true));
        assert_eq!(launch_choice(found.clone(), false, Some("version-new"), there), (found.clone(), true), "missing: the client updates itself");
        assert_eq!(launch_choice(found.clone(), false, None, there), (found.clone(), false), "unknown: as before");
        assert_eq!(launch_choice(found.clone(), true, Some("version-live"), there), (found, false), "a bootstrapper's own");
    }

    #[test]
    fn holds_a_launch_only_when_a_bootstrapper_would_close_clients() {
        let (a, b) = ("version-aaaaaaaaaaaaaaaa", "version-bbbbbbbbbbbbbbbb");
        assert!(bootstrapper_would_close(Some(a), Some(b), 1));
        assert!(!bootstrapper_would_close(Some(a), Some(b), 0), "nothing to close");
        assert!(!bootstrapper_would_close(Some(a), Some(a), 3), "up to date");
        assert!(!bootstrapper_would_close(None, Some(b), 3), "unknown");
        assert!(!bootstrapper_would_close(Some(a), None, 3), "unknown");
    }

    #[test]
    fn outdated_and_auto_update_gate() {
        let (a, b) = ("version-aaaaaaaaaaaaaaaa", "version-bbbbbbbbbbbbbbbb");
        assert!(outdated(Some(a), Some(b)));
        assert!(!outdated(Some(a), Some(a)));
        assert!(!outdated(None, Some(b)), "unknown install: not outdated");
        assert!(!outdated(Some(a), None), "not checked yet");

        let old = roblox(Some(a), Some(b), false);
        assert!(may_auto_update(true, &old, 0, false));
        assert!(!may_auto_update(false, &old, 0, false), "off");
        assert!(!may_auto_update(true, &old, 1, false), "a client runs");
        assert!(!may_auto_update(true, &old, 0, true), "a launch, batch or install runs");
        assert!(!may_auto_update(true, &roblox(Some(a), Some(a), false), 0, false), "up to date");
        assert!(!may_auto_update(true, &RobloxClient { bootstrapper: true, ..old.clone() }, 0, false), "bootstrapper");
        assert!(!may_auto_update(true, &RobloxClient { path: None, ..old }, 0, false), "not installed");
    }

    /// `run` on a copy of PING.EXE (never Roblox's installer) in a temp folder, write-locked
    /// as the installer is: it starts, is waited for, and the folder is deleted afterwards.
    #[test]
    fn run_waits_for_a_write_locked_program() {
        let system = std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        let dir = TempDir::new("roalt-install-").unwrap();
        let exe = dir.0.join("ping-copy.exe");
        std::fs::copy(system.join(r"System32\PING.EXE"), &exe).unwrap();
        let pinned = OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).open(&exe).unwrap();
        assert!(OpenOptions::new().write(true).open(&exe).is_err(), "no writer while pinned");
        // PING without arguments prints its usage and exits 1.
        assert_eq!(run(&exe, Duration::from_secs(30)).unwrap(), Some(1));
        drop(pinned);
        let path = dir.0.clone();
        drop(dir);
        assert!(!path.exists());
        assert!(run(&path.join("missing.exe"), Duration::from_secs(1)).is_err());
    }
}
