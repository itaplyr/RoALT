#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
mod i18n;
mod commands;
mod diagnostics;
mod discord;
mod error;
mod instances;
mod launcher;
mod model;
#[cfg(windows)]
mod network;
#[cfg(not(windows))]
#[path = "network_linux.rs"]
mod network;
mod performance;
mod roblox_api;
mod rpc;
#[cfg(windows)]
mod self_update;
#[cfg(not(windows))]
#[path = "self_update_linux.rs"]
mod self_update;
#[cfg(windows)]
mod splash;
#[cfg(not(windows))]
#[path = "splash_linux.rs"]
mod splash;
mod storage;
mod tray;
mod vault;
#[cfg(windows)]
mod webview;
#[cfg(not(windows))]
#[path = "webview_linux.rs"]
mod webview;

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::window::Color;
use tauri::{App as TauriApp, AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_global_shortcut::ShortcutState;
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};

use crate::error::{AppError, Result};
use crate::model::VaultMode;
use crate::vault::{Secret, cookie_aad};

/// Everything the commands share.
pub struct App {
    pub db: storage::Db,
    pub vault: vault::Vault,
    pub api: roblox_api::Client,
    pub data_dir: PathBuf,
    /// Last cookie put on the clipboard, until it is cleared.
    pub copied: Mutex<Option<Secret>>,
    pub tracker: instances::Tracker,
    pub multi: instances::multi::Multi,
    pub limiter: instances::limits::Limiter,
    pub metrics: instances::metrics::Sampler,
    pub bulk: launcher::bulk::Bulk,
    pub discord: discord::Discord,
    pub update: launcher::update::Updater,
    pub self_update: self_update::SelfUpdater,
    /// Whether a Public network is connected (`network`).
    pub network: network::Network,
    /// When the shutdown RoALT scheduled is due (Unix ms; `commands::shutdown_schedule`).
    pub shutdown: Mutex<Option<i64>>,
    /// Rotated cookies that arrived while the vault was locked, stored at the next unlock.
    pub rotated_later: Mutex<Vec<(i64, vault::Secret)>>,
}

impl App {
    /// Roblox handed out a new `.ROBLOSECURITY` for a stored account: keep it.
    pub fn store_rotated(&self, account_id: i64, cookie: Secret) {
        if self.vault.is_locked() {
            self.rotated_later.lock().unwrap_or_else(|e| e.into_inner()).push((account_id, cookie));
            return;
        }
        let stored = self
            .db
            .account_cookie(account_id)
            .and_then(|(user_id, _)| self.vault.encrypt(&cookie_aad(user_id), cookie.expose().as_bytes()))
            .and_then(|blob| self.db.set_cookie(account_id, &blob));
        match stored {
            Ok(()) => log::info!("stored rotated cookie for account {account_id}"),
            Err(e) => log::warn!("could not store rotated cookie for account {account_id}: {e}"),
        }
    }
}

pub const DB_FILE: &str = "roalt.db";
/// The database, its journal and its key (`storage::key_path`): all a data folder move ever
/// deletes, from the old folder.
pub const DB_FILES: [&str; 4] = [DB_FILE, "roalt.db-wal", "roalt.db-shm", "roalt.db.key"];
const LOCATION: &str = "location.json";

/// `%LOCALAPPDATA%\RoALT\location.json`: where the data folder was moved to (and from, until
/// the next start has deleted the old database).
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    pub data_dir: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moved_from: Option<PathBuf>,
}

/// The data folder unless moved, and where location.json lives.
#[cfg(windows)]
pub fn default_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("RoALT")
}

/// `$XDG_DATA_HOME/roalt` (usually `~/.local/share/roalt`).
#[cfg(not(windows))]
pub fn default_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))
        .unwrap_or_else(std::env::temp_dir)
        .join("roalt")
}

/// A test run (`ROALT_DATA_DIR`): its data folder can't be moved.
pub fn data_dir_overridden() -> bool {
    std::env::var_os("ROALT_DATA_DIR").is_some()
}

/// `ROALT_DATA_DIR` (test runs), else the folder `%LOCALAPPDATA%\RoALT\location.json` names
/// (`dataDir`), else `%LOCALAPPDATA%\RoALT`. After a move, also the folder it left
/// (`movedFrom`): `init` deletes its database files once the new database has opened, then
/// drops `movedFrom`. Err: the moved folder isn't available (a message for `fatal`).
fn data_dir() -> std::result::Result<(PathBuf, Option<PathBuf>), String> {
    if let Some(d) = std::env::var_os("ROALT_DATA_DIR") {
        return Ok((d.into(), None));
    }
    let default = default_dir();
    let json = std::fs::read_to_string(default.join(LOCATION)).ok();
    chosen_dir(default, json.as_deref())
}

/// The folder location.json points at, with `movedFrom`: its `dataDir` when that is a full
/// path; else `default`. Missing or malformed JSON counts as no move. A moved folder without
/// a roalt.db (a drive not connected) is an error, not a fresh empty vault in `default`.
fn chosen_dir(default: PathBuf, json: Option<&str>) -> std::result::Result<(PathBuf, Option<PathBuf>), String> {
    match json.and_then(|j| serde_json::from_str::<Location>(j).ok()) {
        Some(l) if l.data_dir.is_absolute() => {
            if l.data_dir.join(DB_FILE).is_file() {
                Ok((l.data_dir, l.moved_from))
            } else if same_dir(&l.data_dir, &default) {
                Ok((default, None))
            } else {
                Err(tr!(
                    "RoALT's data folder {folder} isn't available (no {db}). Connect that drive and start RoALT again, \
                     or delete {location} to start fresh in the default folder.",
                    folder = l.data_dir.display(),
                    db = DB_FILE,
                    location = default.join(LOCATION).display()
                ))
            }
        }
        _ => Ok((default, None)),
    }
}

/// Writes location.json through a temporary file, so a crash never leaves half of it.
pub fn write_location(l: &Location) -> std::io::Result<()> {
    let dir = default_dir();
    std::fs::create_dir_all(&dir)?;
    let tmp = dir.join("location.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(l)?)?;
    std::fs::rename(tmp, dir.join(LOCATION))
}

/// An absolute path with `.` and `..` resolved lexically (no filesystem or symlink lookup).
fn normalized(p: &Path) -> PathBuf {
    let p = std::path::absolute(p).unwrap_or_else(|_| p.to_owned());
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            _ => out.push(c.as_os_str()),
        }
    }
    out
}

/// A path as the app compares it: absolute, `..` resolved, by component, ignoring case.
fn path_key(p: &Path) -> Vec<String> {
    normalized(p).components().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect()
}

/// The same folder: judged on disk when both exist (junctions, short names), else by path.
pub fn same_dir(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => path_key(a) == path_key(b),
    }
}

/// Where a data folder move to `path` goes: a full path that isn't the current folder or
/// inside it, and holds no RoALT database yet.
pub fn move_target(current: &Path, path: &str) -> Result<PathBuf> {
    let refuse = |why: String| Err(AppError::Invalid(why));
    let path = Path::new(path.trim());
    if !path.is_absolute() {
        return refuse(tr!("Pick a full folder path, like D:\\RoALT."));
    }
    let target = normalized(path);
    if same_dir(&target, current) {
        return refuse(tr!("RoALT already keeps its data there."));
    }
    if path_key(&target).starts_with(&path_key(current)) {
        return refuse(tr!("Pick a folder outside the current data folder."));
    }
    if DB_FILES.iter().any(|f| target.join(f).exists()) {
        return refuse(tr!("That folder already has a RoALT database. Pick another one."));
    }
    Ok(target)
}

/// The moved database opened: the old folder's database files go and location.json forgets
/// the old folder (kept while a file couldn't be deleted, so the next start tries again).
fn finish_move(data_dir: &Path, old: &Path) {
    if !remove_old_db(data_dir, old) {
        return;
    }
    if let Err(e) = write_location(&Location { data_dir: data_dir.to_owned(), moved_from: None }) {
        log::warn!("could not update {LOCATION}: {e}");
    }
}

/// Deletes roalt.db and its journal files from `old`: nothing else, and nothing when `old` is
/// the folder in use. True when none of them is left there.
fn remove_old_db(data_dir: &Path, old: &Path) -> bool {
    if same_dir(old, data_dir) {
        return true;
    }
    let mut gone = true;
    for f in DB_FILES {
        match std::fs::remove_file(old.join(f)) {
            Ok(()) => log::info!("deleted {f} from the old data folder {}", old.display()),
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => {
                log::warn!("could not delete {f} from the old data folder {}: {e}", old.display());
                gone = false;
            }
        }
    }
    gone
}

fn main_window(app: &TauriApp, data_dir: &Path, skip_splash: bool) -> tauri::Result<()> {
    // The dark theme's --bg (theme.css) in either theme: the splash is dark, and this is what
    // shows before the UI paints and while the window grows.
    let bg = Color(5, 5, 6, 255);

    let mut builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("RoALT")
        .inner_size(splash::DEFAULT.0, splash::DEFAULT.1)
        // The minimum size is set once the splash has grown (`splash`); shown by it too.
        .visible(false)
        .decorations(false)
        .background_color(bg)
        // The UI's own WebView2 profile; never used for roblox.com.
        .data_directory(data_dir.join("webview"))
        .on_navigation(in_app);
    // The splash decides before any setting could reach the UI.
    if skip_splash {
        builder = builder.initialization_script("window.__ROALT_SKIP_SPLASH = true;");
    }
    // Debug builds only: DevTools protocol for automated UI checks on a hidden desktop.
    #[cfg(debug_assertions)]
    if let Ok(port) = std::env::var("ROALT_CDP_PORT") {
        builder = builder.additional_browser_args(&format!(
            "--remote-debugging-port={port} --disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection"
        ));
    }
    let w = builder.build()?;
    webview::log_process_failures(&w);
    splash::start(app.handle(), &w, skip_splash)
}

/// Only the RoALT window: pages in Roblox windows are never to reach a command, whatever origin
/// they manage to appear as.
fn main_only(handler: impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static) -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    move |invoke| {
        if invoke.message.webview_ref().label() != "main" {
            log::warn!("refused command {} from window {}", invoke.message.command(), invoke.message.webview_ref().label());
            invoke.resolver.reject("not allowed");
            return true;
        }
        handler(invoke)
    }
}

/// The RoALT window stays on the app's own pages.
fn in_app(url: &url::Url) -> bool {
    let host = url.host_str();
    (host == Some("tauri.localhost") && url.port().is_none() && url.username().is_empty() && url.password().is_none())
        || url.scheme() == "tauri"
        // `tauri dev`.
        || (cfg!(debug_assertions) && host == Some("localhost"))
}

/// Keeps a folder RoALT creates from being writable by other users of the PC: what lands in it
/// includes the UI's WebView2 profile. A drive without permissions (exFAT) can't, and is left so.
#[cfg(windows)]
pub fn restrict_to_user(dir: &Path) {
    use std::os::windows::process::CommandExt;
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| r"C:\Windows".into());
    let user = std::env::var("USERNAME").unwrap_or_default();
    let done = std::process::Command::new(root.join(r"System32\icacls.exe"))
        .arg(dir)
        .args(["/inheritance:r", "/grant:r", &format!("{user}:(OI)(CI)F"), "*S-1-5-18:(OI)(CI)F", "*S-1-5-32-544:(OI)(CI)F"])
        .creation_flags(0x0800_0000)
        .output();
    match done {
        Ok(o) if o.status.success() => {}
        Ok(o) => log::warn!("could not restrict the data folder: icacls exit {:?}", o.status.code()),
        Err(e) => log::warn!("could not restrict the data folder: {e}"),
    }
}

#[cfg(not(windows))]
pub fn restrict_to_user(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Err(e) = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)) {
        log::warn!("could not restrict {}: {e}", dir.display());
    }
}

/// Master-password mode: lock after the configured idle time.
fn spawn_auto_lock(handle: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(20)).await;
            let s = handle.state::<App>();
            if s.vault.mode() != VaultMode::Master || s.vault.is_locked() {
                continue;
            }
            // Using a Roblox window (signing in, a session) is activity too: its cookie is
            // encrypted when it closes, which needs the vault open. So is a client auto-rejoin
            // or auto-restart follows: relaunching needs its cookie.
            if handle.webview_windows().keys().any(|l| l.starts_with("session-") || l.starts_with("login-"))
                || (s.tracker.any_running()
                    && s.db
                        .settings()
                        .inspect_err(|e| log::debug!("auto-lock: could not read settings: {e}"))
                        .is_ok_and(|v| v.auto_rejoin || v.auto_restart_on_crash))
            {
                s.vault.touch();
                continue;
            }
            let minutes = s.db.settings().map(|v| v.auto_lock_minutes).unwrap_or_else(|e| {
                log::debug!("auto-lock: could not read settings, using 15 min: {e}");
                15
            });
            if minutes > 0 && s.vault.idle() >= Duration::from_secs(minutes as u64 * 60) {
                commands::lock_vault(&handle);
            }
        }
    });
}

/// The log folder once known: `fatal` writes there itself when the logger isn't up yet.
static LOG_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// A startup failure the user must see: there is no window to show it in.
#[cfg(windows)]
fn fatal(message: &str) -> ! {
    use std::io::Write;
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    log::error!("{message}");
    // Before the log plugin exists `log::error!` goes nowhere, so append the line directly.
    if let Some(dir) = LOG_DIR.get()
        && let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("roalt.log"))
    {
        let _ = writeln!(f, "[fatal] {message}");
    }
    // SAFETY: a plain modal message box with no owner window.
    unsafe { MessageBoxW(None, &windows::core::HSTRING::from(message), windows::core::w!("RoALT"), MB_ICONERROR | MB_OK) };
    std::process::exit(1)
}

/// A startup failure on Linux: stderr, the log, and a desktop dialog when one is available.
#[cfg(not(windows))]
fn fatal(message: &str) -> ! {
    use std::io::Write;
    log::error!("{message}");
    eprintln!("RoALT: {message}");
    if let Some(dir) = LOG_DIR.get()
        && let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("roalt.log"))
    {
        let _ = writeln!(f, "[fatal] {message}");
    }
    if std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some() {
        let _ = std::process::Command::new("zenity").args(["--error", "--title=RoALT", "--text", message]).status();
    }
    std::process::exit(1)
}

/// Panics go to the log (release has no console and aborts on panic, so nothing else sees them).
fn panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let what = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".into());
        let at = info.location().map(|l| l.to_string()).unwrap_or_default();
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        log::error!("panic in thread {thread} at {at}: {what}");
        log::logger().flush();
        previous(info);
    }));
}

fn init(app: &TauriApp, data_dir: &Path, moved_from: Option<&Path>) -> std::result::Result<(), String> {
    webview::sweep_temp_profiles(data_dir);
    let db_path = data_dir.join(DB_FILE);
    let db = storage::Db::open(&db_path).map_err(|e| tr!("RoALT can't open its database {path}: {why}", path = db_path.display(), why = e.message()))?;
    let record = db.vault_record().map_err(|e| e.message())?;
    // Only a database with a vault is the copy that replaces the old one.
    let moved_from = moved_from.filter(|_| record.is_some());
    let vault = match record {
        Some(rec) => vault::Vault::open(&rec).map_err(|e| {
            tr!(
                "RoALT can't open its vault on this Windows account ({why}).\n\nIf this data folder came from another PC or \
                 Windows account, rename {path} and import a .roalt backup.",
                why = e.message(),
                path = db_path.display()
            )
        })?,
        None => {
            let (v, rec) = vault::Vault::create().map_err(|e| e.message())?;
            db.save_vault_record(&rec).map_err(|e| e.message())?;
            v
        }
    };
    if let Some(old) = moved_from {
        finish_move(data_dir, old);
    }
    let handle = app.handle().clone();
    let api = roblox_api::Client::new(Arc::new(move |id, cookie| handle.state::<App>().store_rotated(id, cookie)));
    let settings = db.settings().map_err(|e| e.message())?;
    i18n::set(&settings.language);
    let shutdown = commands::pending_shutdown(&db);
    let multi = instances::multi::Multi::new();
    if settings.multi_instance
        && let Err(e) = multi.enable().inspect(|()| {
            // Clients from before a restart keep their protection.
            if settings.protect_shared_login && multi.has_survivors() {
                multi.lock_shared();
            }
        })
    {
        // Roblox was already running: the UI shows why multi-instance is not active.
        log::info!("multi-instance not active at start: {e}");
    }
    app.manage(App {
        rotated_later: Mutex::new(Vec::new()),
        db,
        vault,
        api,
        data_dir: data_dir.to_owned(),
        copied: Mutex::new(None),
        tracker: instances::Tracker::new(),
        multi,
        limiter: instances::limits::Limiter::new(),
        metrics: instances::metrics::Sampler::new(),
        bulk: launcher::bulk::Bulk::new(),
        discord: discord::Discord::new(),
        update: launcher::update::Updater::new(),
        self_update: self_update::SelfUpdater::new(),
        network: network::Network::new(),
        shutdown: Mutex::new(shutdown),
    });
    if let Err(e) = commands::set_panic_hotkey(app.handle(), "", &settings.panic_hotkey) {
        let handle = app.handle().clone();
        // Said once the window is up.
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            commands::notice(&handle, true, tr!("{why} The panic hotkey is off; change it in Settings, Automation.", why = e.message()));
        });
    }
    if let Err(e) = tray::setup(app.handle()) {
        log::warn!("no tray icon: {e}");
    }
    // Before anything else is started, so a newer RoALT is offered as soon as possible.
    self_update::spawn(app.handle().clone());
    app.manage(splash::Pending::default());
    main_window(app, data_dir, settings.skip_splash).map_err(|e| {
        tr!(
            "RoALT can't open its window ({why}).\n\nIf this PC was very busy, start RoALT again. Otherwise it needs the Microsoft Edge WebView2 Runtime: \
             https://go.microsoft.com/fwlink/p/?LinkId=2124703",
            why = e
        )
    })?;
    spawn_auto_lock(app.handle().clone());
    instances::Tracker::spawn(app.handle().clone());
    instances::afk::spawn(app.handle().clone());
    network::spawn(app.handle().clone());
    launcher::flags::restore();
    launcher::client_settings::spawn(app.handle().clone());
    discord::spawn(app.handle().clone());
    rpc::spawn(app.handle().clone());
    launcher::update::spawn(app.handle().clone());
    remind_bandwidth(app.handle().clone());
    Ok(())
}

/// Windows drops the bandwidth policy when it restarts; RoALT never sets it again on its own
/// (that needs admin rights), so it says so once the window is up.
fn remind_bandwidth(handle: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        if commands::bandwidth(&handle.state::<App>())
            .inspect_err(|e| log::debug!("bandwidth reminder: could not read the limit: {e}"))
            .is_ok_and(|b| b.kbps > 0 && !b.active)
        {
            commands::notice(
                &handle,
                true,
                tr!("The Roblox bandwidth limit is off: Windows clears it when it restarts. Apply it again in Performance > More settings."),
            );
        }
    });
}

/// Closing the main window hides it while `settings.close_to_tray` is on.
fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event
        && window.label() == "main"
        && window.try_state::<App>().is_some_and(|s| {
            s.db.settings().inspect_err(|e| log::debug!("close: could not read settings, exiting: {e}")).is_ok_and(|v| v.close_to_tray)
        })
    {
        api.prevent_close();
        if let Err(e) = window.hide() {
            log::warn!("could not hide the main window to the tray: {e}");
        }
    }
}

fn main() {
    webview::clear_webview2_env();
    // Windows' language until the settings are read.
    i18n::set("");
    let (data_dir, moved_from) = data_dir().unwrap_or_else(|e| fatal(&e));
    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        fatal(&tr!("RoALT can't create its data folder {path}: {why}", path = data_dir.display(), why = e));
    }
    let log_dir = data_dir.join("logs");
    let _ = std::fs::create_dir_all(&log_dir);
    let _ = LOG_DIR.set(log_dir.clone());

    tauri::Builder::default()
        // Must be first: a second launch shows the running window instead.
        .plugin(tauri_plugin_single_instance::init(|app, _, _| tray::show_main(app)))
        .plugin(
            tauri_plugin_log::Builder::new()
                .clear_targets()
                .target(Target::new(TargetKind::Folder { path: log_dir, file_name: Some("roalt".into()) }))
                // The Debug tab (`log://log` events); its history comes from `commands::debug_tail`.
                .target(Target::new(TargetKind::Webview))
                .level(log::LevelFilter::Info)
                .level_for("roalt", log::LevelFilter::Debug)
                // roalt.log plus the 4 before it, 5 MB each.
                .max_file_size(5 << 20)
                .rotation_strategy(RotationStrategy::KeepSome(5))
                .build(),
        )
        // The panic hotkey (`settings.panic_hotkey`, registered by `commands::set_panic_hotkey`).
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _, e| {
                    if e.state == ShortcutState::Pressed {
                        tray::kill_all(app);
                    }
                })
                .build(),
        )
        // "main" is restored by `splash::start`, which shows it small first.
        .plugin(tauri_plugin_window_state::Builder::new().with_filter(|label| label == "main").skip_initial_state("main").build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(move |app| {
            panic_hook();
            if let Err(message) = init(app, &data_dir, moved_from.as_deref()) {
                fatal(&message);
            }
            Ok(())
        })
        .on_window_event(on_window_event)
        .invoke_handler(main_only(tauri::generate_handler![
            commands::vault_status,
            commands::vault_unlock,
            commands::vault_lock,
            commands::vault_set_mode,
            commands::vault_recover,
            commands::vault_recovery_create,
            commands::vault_recovery_save,
            commands::vault_recovery_copy,
            commands::app_activity,
            splash::splash_done,
            commands::accounts_list,
            commands::account_add_cookie,
            commands::accounts_import_cookies,
            commands::accounts_import_ram,
            commands::account_add_login,
            commands::account_update,
            commands::accounts_reorder,
            commands::accounts_remove,
            commands::accounts_validate,
            commands::account_reveal_cookie,
            commands::account_copy_cookie,
            commands::account_info,
            commands::accounts_headshots,
            commands::accounts_presence,
            commands::account_open_session,
            commands::target_parse,
            commands::user_lookup,
            commands::launch,
            commands::favourites_list,
            commands::favourite_save,
            commands::favourite_remove,
            commands::favourites_reorder,
            commands::join_candidates,
            commands::instances_list,
            commands::instance_close,
            commands::instances_close_all,
            commands::instances_clear_ended,
            commands::instance_set_limits,
            commands::instance_trim,
            commands::instance_set_anti_afk,
            commands::instances_metrics,
            commands::system_usage,
            commands::instance_stop_relaunch,
            commands::instances_retry_relaunch,
            commands::instance_thumbnail,
            commands::instance_show,
            commands::instances_show_all,
            commands::instance_micro,
            commands::instances_tile,
            commands::bandwidth_status,
            commands::bandwidth_apply,
            commands::multi_instance_status,
            commands::multi_instance_set,
            commands::multi_instance_take_over,
            commands::roblox_close_all,
            commands::panic_kill,
            commands::crashes_list,
            commands::diagnostics_text,
            commands::diagnostics_save,
            commands::server_history,
            commands::crash_open_log,
            commands::discord_get,
            commands::discord_set,
            commands::discord_test,
            commands::bulk_launch,
            commands::bulk_cancel,
            commands::bulk_current,
            commands::servers_list,
            commands::game_info,
            commands::target_place,
            commands::roblox_client,
            commands::roblox_install,
            commands::self_update_status,
            commands::self_update_check,
            commands::self_update_install,
            commands::data_dir_info,
            commands::data_dir_open,
            commands::data_dir_move,
            commands::logs_open,
            commands::logs_clean,
            commands::debug_tail,
            commands::debug_log,
            commands::background_set,
            commands::background_clear,
            commands::background_get,
            commands::shutdown_schedule,
            commands::shutdown_cancel,
            commands::shutdown_status,
            commands::private_servers_list,
            commands::groups_list,
            commands::group_save,
            commands::group_remove,
            commands::backup_export,
            commands::backup_import,
            commands::performance_profile,
            commands::performance_export,
            commands::performance_read,
            commands::performance_parse,
            commands::settings_get,
            commands::settings_set,
            commands::network_status,
        ]))
        .build(tauri::generate_context!())
        .unwrap_or_else(|e| fatal(&tr!("RoALT failed to start: {why}", why = e)))
        .run(|app, event| {
            // Before the Exit event: the clipboard plugin has cleaned up by then.
            if let tauri::RunEvent::ExitRequested { .. } = event {
                commands::clear_copied_cookie(app);
            }
            if let tauri::RunEvent::Exit = event {
                log::info!("shutting down");
                // The next start follows the running clients again (`Tracker::readopt`).
                if let Some(s) = app.try_state::<App>() {
                    s.tracker.persist(&s.db, true);
                    s.limiter.release_all();
                    launcher::client_settings::restore(&s.db);
                    launcher::flags::restore();
                    for (_, pid, ..) in s.tracker.clients() {
                        instances::window::unpark_client(pid);
                    }
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("roalt-datadir-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }

    fn json(data_dir: &Path, moved_from: Option<&Path>) -> String {
        serde_json::to_string(&Location { data_dir: data_dir.into(), moved_from: moved_from.map(Into::into) }).unwrap()
    }

    #[test]
    fn location_json_picks_a_folder_with_a_database() {
        let tmp = temp_dir();
        let (default, moved, empty) = (tmp.join("default"), tmp.join("moved"), tmp.join("empty"));
        touch(&moved.join("roalt.db"));
        std::fs::create_dir_all(&empty).unwrap();
        let pick = |j: Option<&str>| chosen_dir(default.clone(), j).unwrap();

        assert_eq!(pick(None), (default.clone(), None));
        for bad in ["", "{", "[]", "null", r#"{"dataDir": 5}"#, r#"{"movedFrom": "C:\\x"}"#] {
            assert_eq!(pick(Some(bad)), (default.clone(), None), "{bad}");
        }
        assert_eq!(pick(Some(r#"{"dataDir": "relative\\moved"}"#)), (default.clone(), None));
        // Moved back to the default folder, which then lost its database: a fresh start there.
        assert_eq!(pick(Some(&json(&default, Some(&moved)))), (default.clone(), None));
        // A moved folder without a database (a drive not connected) is no fresh start.
        for gone in [empty.clone(), tmp.join("gone")] {
            let err = chosen_dir(default.clone(), Some(&json(&gone, Some(&default)))).unwrap_err();
            assert!(err.contains(&gone.display().to_string()) && err.contains(LOCATION), "{err}");
        }

        assert_eq!(pick(Some(&json(&moved, None))), (moved.clone(), None));
        assert_eq!(pick(Some(&json(&moved, Some(&default)))), (moved.clone(), Some(default.clone())));
        let extra = format!(r#"{{"dataDir": {}, "later": true}}"#, serde_json::to_string(&moved).unwrap());
        assert_eq!(pick(Some(&extra)), (moved.clone(), None));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn move_targets() {
        let tmp = temp_dir();
        let current = tmp.join("Current");
        touch(&current.join("roalt.db"));
        let refused = |p: &Path| matches!(move_target(&current, &p.display().to_string()), Err(AppError::Invalid(_)));

        assert!(matches!(move_target(&current, r"relative\folder"), Err(AppError::Invalid(_))));
        assert!(matches!(move_target(&current, ""), Err(AppError::Invalid(_))));
        assert!(refused(&current));
        assert!(refused(&tmp.join("current")));
        #[cfg(windows)]
        {
            assert!(refused(&tmp.join(r"current\.\")));
            assert!(refused(&tmp.join(r"other\..\CURRENT\sub")));
        }
        assert!(refused(&current.join("sub")));
        for f in DB_FILES {
            let taken = tmp.join(format!("taken-{f}"));
            touch(&taken.join(f));
            assert!(refused(&taken), "{f}");
        }

        // A new folder, or an existing one without a database (other files are fine).
        #[cfg(windows)]
        let new = tmp.join(r"a\..\new");
        #[cfg(not(windows))]
        let new = tmp.join("a").join("..").join("new");
        assert_eq!(move_target(&current, &format!("  {}  ", new.display())).unwrap(), tmp.join("new"));
        let existing = tmp.join("existing");
        touch(&existing.join("notes.txt"));
        assert_eq!(move_target(&current, &existing.display().to_string()).unwrap(), existing);
        // Beside the current folder, or above it.
        assert!(move_target(&current, &tmp.join("Current2").display().to_string()).is_ok());
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_finished_move_deletes_only_the_old_database() {
        let tmp = temp_dir();
        let (old, new) = (tmp.join("old"), tmp.join("new"));
        for f in DB_FILES.iter().chain(&["notes.txt", r"logs\roalt.log", r"webview\x"]) {
            touch(&old.join(f));
            touch(&new.join(f));
        }
        let left = |dir: &Path| DB_FILES.iter().filter(|f| dir.join(f).exists()).count();

        // Never from the folder in use, however it is spelled.
        assert!(remove_old_db(&new, &tmp.join(r"NEW\.\")));
        assert_eq!(left(&new), DB_FILES.len());

        // Still open elsewhere (no delete sharing, like SQLite's): reported, so it is retried.
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            let held = std::fs::File::options().read(true).share_mode(1).open(old.join(DB_FILE)).unwrap();
            assert!(!remove_old_db(&new, &old));
            assert_eq!(left(&old), 1);
            drop(held);
        }

        assert!(remove_old_db(&new, &old));
        assert_eq!(left(&old), 0);
        assert_eq!(left(&new), DB_FILES.len());
        assert!(old.join("notes.txt").exists() && old.join(r"logs\roalt.log").exists() && old.join(r"webview\x").exists());
        // Already gone: nothing to do.
        assert!(remove_old_db(&new, &old));
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
