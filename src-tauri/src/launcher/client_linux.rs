//! Finding and starting Roblox on Linux through Mocktail or Cordial.
//!
//! Mocktail is used when it is installed: RoALT points it at its own data/auth roots with
//! `MOCKTAIL_*`, seeds the account's `roblox.cookie`, and passes the join link with
//! `--launch-uri`. Mocktail takes one client at a time — its lock is keyed by uid and
//! executable, not by data root, and a second launch forwards its URI to the running process —
//! so a second launch is refused rather than silently joining the wrong account.
//!
//! Cordial remains the fallback and is the only launcher here with several profiles at once
//! and a preview/devctl socket. RoALT launches its loader (`cordial-run`) directly with
//! `--profile roalt_<user id>` and `--join-url`, which is what the Cordial shell itself does
//! after the user picks a profile. That works with Cordial 0.21 (whose shell has no `--profile`
//! command-line option) and with newer shells. If the direct loader/build isn't available, it
//! falls back to Cordial's shell or Flatpak Cordial. It deliberately does not fall back to Sober.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{AppError, Result};
use crate::vault::Secret;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherKind {
    Mocktail,
    Cordial,
    FlatpakCordial,
    Sober,
    FlatpakSober,
    Handler,
}

/// The Cordial loader and the Roblox build it should run, found in Cordial's own cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CordialBuild {
    pub loader: PathBuf,
    pub lib_dir: PathBuf,
    pub apk: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientExe {
    pub path: PathBuf,
    pub bootstrapper: bool,
    pub kind: LauncherKind,
    /// Cordial 0.27+ has `--profile`/`--play`; older shells only take a `roblox-player:` link.
    pub profiles: bool,
    /// Present when `cordial-run` and a downloaded build were found: launch the game directly.
    pub direct: Option<CordialBuild>,
}

const CORDIAL_ID: &str = "io.github.luohua97.Cordial";
/// Only reachable through an explicit `ROALT_LAUNCHER`; the automatic finder picks Mocktail or
/// Cordial, never Sober.
const SOBER_ID: &str = "org.vinegarhq.Sober";

/// Set by `launcher::launch` from the user's multi-instance setting. With it, Mocktail runs
/// one copied launcher and one engine-data root per account instead of its unsupported shared
/// instance; without it, the one-client guard stays.
static MOCKTAIL_MULTI: AtomicBool = AtomicBool::new(false);

/// Whether the next Mocktail launch may use the per-account isolated-copy path.
pub fn set_mocktail_multi(on: bool) {
    MOCKTAIL_MULTI.store(on, Ordering::Relaxed);
}

fn mocktail_multi() -> bool {
    MOCKTAIL_MULTI.load(Ordering::Relaxed)
}

pub fn find() -> Result<ClientExe> {
    if let Some(path) = std::env::var_os("ROALT_LAUNCHER") {
        let path = PathBuf::from(path);
        let kind = kind_for(&path);
        let profiles = supports_profiles(&path, kind);
        return Ok(ClientExe { path, bootstrapper: true, kind, profiles, direct: None });
    }
    // Trial launcher: Mocktail when it is installed. It owns its Roblox build and takes one
    // client at a time; Cordial, which supports several profiles, stays the fallback.
    if let Some(path) = mocktail_binary() {
        log::info!("launcher: Mocktail at {}", path.display());
        return Ok(ClientExe { path, bootstrapper: true, kind: LauncherKind::Mocktail, profiles: false, direct: None });
    }
    // Preferred Cordial path: bypass the shell and give the loader the profile directly. The
    // shell's profile picker is what produced "Roblox link waiting" and put every account on the
    // same profile. ROALT_CORDIAL_RUN points at a self-built engine.
    if let Some(loader) = cordial_loader() {
        if let Some(build) = latest_build(&loader) {
            return Ok(ClientExe {
                path: loader.clone(),
                bootstrapper: true,
                kind: LauncherKind::Cordial,
                profiles: true,
                direct: Some(CordialBuild { loader, ..build }),
            });
        }
        // Not silent: without a complete build this run cannot use the direct
        // `--join-url` contract, and Cordial's shell is the path that produced the home/login
        // landing page before. Say which stores were checked.
        let stores = build_dirs().iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join(", ");
        log::warn!(
            "found {} but no complete Roblox build (libroblox.so + base.apk) under [{}]; \
             falling back to Cordial's shell",
            loader.display(),
            stores
        );
    }
    if let Some(path) = in_path("cordial") {
        let profiles = supports_profiles(&path, LauncherKind::Cordial);
        return Ok(ClientExe { path, bootstrapper: true, kind: LauncherKind::Cordial, profiles, direct: None });
    }
    if flatpak_installed(CORDIAL_ID) {
        let path = PathBuf::from("flatpak");
        let profiles = supports_profiles(&path, LauncherKind::FlatpakCordial);
        return Ok(ClientExe { path, bootstrapper: true, kind: LauncherKind::FlatpakCordial, profiles, direct: None });
    }
    Err(AppError::Invalid(tr!(
        "Roblox on Linux needs Cordial. Install Cordial, or set ROALT_CORDIAL_RUN to its cordial-run."
    )))
}

/// Whether this Cordial understands `--profile`/`--play` (0.27 and later).
fn supports_profiles(path: &Path, kind: LauncherKind) -> bool {
    let mut cmd = match kind {
        LauncherKind::Cordial => Command::new(path),
        LauncherKind::FlatpakCordial => {
            let mut c = Command::new("flatpak");
            c.args(["run", CORDIAL_ID]);
            c
        }
        _ => return false,
    };
    cmd.arg("--help")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("--profile"))
}

/// Mocktail's executable: `PATH`, then the per-user bin folder a desktop launch often loses,
/// then the usual system location. `ROALT_LAUNCHER` overrides this in `find`.
fn mocktail_binary() -> Option<PathBuf> {
    let path = in_path("mocktail")
        .or_else(|| env_home().map(|h| h.join(".local").join("bin").join("mocktail")))
        .unwrap_or_else(|| PathBuf::from("/usr/bin/mocktail"));
    path.is_file().then_some(path)
}

/// The Cordial loader: the explicit override, PATH, or the usual per-user bin folders.
///
/// A desktop-launched RoALT often does not inherit the shell's `~/.local/bin` in `PATH`, which
/// is why a `cordial-run` that `which` finds can otherwise be missed.
fn cordial_loader() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("ROALT_CORDIAL_RUN").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(path);
    }
    in_path("cordial-run")
        .or_else(|| env_home().map(|h| h.join(".local").join("bin").join("cordial-run")))
        .or_else(|| env_home().map(|h| h.join("bin").join("cordial-run")))
        .filter(|p| p.is_file())
}

fn env_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Cordial's build stores: the data store used since 0.25, then the old cache for upgrades.
fn build_dirs() -> Vec<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")));
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")));
    [data.map(|d| d.join("cordial").join("builds")), cache.map(|c| c.join("cordial").join("builds"))]
        .into_iter()
        .flatten()
        .collect()
}

/// Whether a build directory holds both files and neither is empty.
///
/// A directory name is reserved before the download finishes, so presence alone is not enough:
/// running a truncated engine or APK is a launch that never reaches a place.
fn build_is_complete(dir: &Path) -> bool {
    ["libroblox.so", "base.apk"]
        .iter()
        .all(|name| std::fs::metadata(dir.join(name)).is_ok_and(|m| m.is_file() && m.len() > 0))
}

/// The newest complete build under either Cordial store, with `loader` as the executable.
fn latest_build(loader: &Path) -> Option<CordialBuild> {
    let mut builds: Vec<(Vec<u64>, PathBuf)> = build_dirs()
        .into_iter()
        .flat_map(|dir| std::fs::read_dir(dir).into_iter().flatten().flatten())
        .map(|e| e.path())
        .filter(|p| build_is_complete(p))
        .filter_map(|p| {
            let name = p.file_name()?.to_str()?;
            let key = name.split('.').map(|n| n.parse().unwrap_or(0)).collect();
            Some((key, p))
        })
        .collect();
    builds.sort_by(|a, b| a.0.cmp(&b.0));
    let dir = builds.pop()?.1;
    log::info!("Cordial direct launch: {} running {}", loader.display(), dir.display());
    Some(CordialBuild { loader: loader.to_owned(), lib_dir: dir.clone(), apk: dir.join("base.apk") })
}

/// `$XDG_DATA_HOME/cordial/profiles` (or `~/.local/share/cordial/profiles`).
fn profiles_root() -> Option<PathBuf> {
    if let Some(root) = std::env::var_os("CORDIAL_PROFILE_ROOT") {
        return Some(PathBuf::from(root));
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))?;
    Some(data.join("cordial").join("profiles"))
}

pub fn profile_name(user_id: i64) -> String {
    format!("roalt_{user_id}")
}

/// This account's Cordial profile directory.
pub fn profile_dir(user_id: i64) -> Option<PathBuf> {
    profiles_root().map(|root| root.join(profile_name(user_id)))
}

/// Cordial's development-control socket in this account's profile.
///
/// Preview is Cordial-only: a Mocktail client has no dev-control socket. A profile RoALT only
/// created for an account (`ensure_profile`), or one that never had a Cordial launch, has no
/// cookie store yet, so this is `None` and the preview stays "no window" instead of waiting for
/// a socket that will never appear. Cordial's own launch seeds `cookies` before the client
/// starts, so a running Cordial client keeps its socket.
pub fn devctl_socket(user_id: i64) -> Option<PathBuf> {
    profile_dir(user_id)
        .filter(|dir| dir.is_dir() && dir.join("cookies").is_file())
        .map(|dir| dir.join("devctl.sock"))
}

/// Mocktail's own data root, where its managed payload and `rbx_bin` live: `$XDG_DATA_HOME/mocktail`
/// or `~/.local/share/mocktail`. RoALT shares it (the payload is read-only) and isolates only auth,
/// cache and state per account; pointing this at RoALT's own store hid the payload and made Mocktail
/// fail on `rbx_bin/libroblox.so`.
fn mocktail_data_root() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| env_home().map(|h| h.join(".local").join("share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("mocktail")
}

/// RoALT's Mocktail store: `$XDG_DATA_HOME/roalt/mocktail` (or `~/.local/share/roalt/mocktail`).
///
/// It is shared by every account; only `auth/<user id>` is per account. Mocktail's own lock is
/// per uid and executable rather than per data root, so this sharing costs nothing extra.
fn mocktail_root() -> PathBuf {
    crate::default_dir().join("mocktail")
}

/// This account's Mocktail auth root, where Mocktail keeps `roblox.cookie`.
fn mocktail_auth_dir(user_id: i64) -> PathBuf {
    mocktail_root().join("auth").join(user_id.to_string())
}

/// Creates a directory only this user can read; Mocktail refuses an auth root that others can.
fn private_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// The folder holding Mocktail's runtime bridges (`libEGL.so`, `libGLESv2.so`, ...). It reads
/// `MOCKTAIL_RUNTIME_LIBRARY_DIR` before falling back to its executable's own folder, which the
/// per-account copy does not share. The installed layout is `<prefix>/lib/mocktail`.
fn mocktail_runtime_library_dir(source: &Path) -> Option<PathBuf> {
    let dir = source.parent()?;
    [dir.to_path_buf(), dir.join("..").join("lib").join("mocktail")]
        .into_iter()
        .filter_map(|p| p.canonicalize().ok())
        .find(|p| p.join("libEGL.so").is_file())
}

/// The folders Mocktail's shared libraries live in, relative to the installed binary: its own
/// folder and `<prefix>/lib/mocktail` (its `$ORIGIN/../lib/mocktail` RUNPATH). A per-account copy
/// lives in RoALT's data folder, where that RUNPATH no longer points at the libraries, so the
/// loader must be told where they are.
fn mocktail_lib_dirs(source: &Path) -> Vec<PathBuf> {
    let Some(dir) = source.parent() else { return Vec::new() };
    [dir.to_path_buf(), dir.join("..").join("lib").join("mocktail"), dir.join("..").join("lib")]
        .into_iter()
        .filter_map(|p| p.canonicalize().ok())
        .filter(|p| p.is_dir())
        .collect()
}

/// A per-account copy of Mocktail. Its executable path is what Mocktail hashes for the
/// single-instance lock, so copies are what let several clients coexist. The copy is refreshed
/// when the installed binary changes and is created 0755.
fn mocktail_copy(source: &Path, user_id: i64) -> Result<PathBuf> {
    let bin = mocktail_root().join("bin");
    private_dir(&bin).map_err(|e| {
        AppError::Internal(tr!("Could not create Mocktail's data folder: {why}", why = e))
    })?;
    let dest = bin.join(format!("mocktail-{user_id}"));
    let same = std::fs::metadata(source)
        .ok()
        .zip(std::fs::metadata(&dest).ok())
        .is_some_and(|(a, b)| a.len() == b.len() && a.modified().ok() == b.modified().ok());
    if !same {
        let tmp = bin.join(format!("mocktail-{user_id}.new"));
        std::fs::copy(source, &tmp).map_err(|e| {
            AppError::Internal(tr!("Could not copy Mocktail for multi-instance: {why}", why = e))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| AppError::Internal(e.to_string()))?;
        }
        std::fs::rename(&tmp, &dest).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            AppError::Internal(tr!("Could not copy Mocktail for multi-instance: {why}", why = e))
        })?;
    }
    Ok(dest)
}

/// Seeds this account's `roblox.cookie`. Mocktail wants exactly `.ROBLOSECURITY=<value>` with no
/// newline and a private mode; anything else is rejected. The value is never logged.
fn write_mocktail_cookie(auth: &Path, cookie: &Secret) -> Result<()> {
    let path = auth.join("roblox.cookie");
    let tmp = auth.join("roblox.cookie.roalt");
    let body = format!(".ROBLOSECURITY={}", cookie.expose());
    let write = || -> std::io::Result<()> {
        std::fs::write(&tmp, body.as_bytes())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(&tmp, &path)
    };
    write().map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        AppError::Internal(tr!("Could not write Mocktail's session cookie: {why}", why = e))
    })
}

/// Creates this account's Cordial profile (0700) so a launch has somewhere to sign in and
/// keep that account's Roblox storage. Safe to call repeatedly.
pub fn ensure_profile(user_id: i64) {
    let Some(root) = profiles_root() else { return };
    let dir = root.join(profile_name(user_id));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("could not create Cordial profile {}: {e}", dir.display());
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)) {
            log::warn!("could not restrict Cordial profile {}: {e}", dir.display());
        }
    }
    // The engine's own storage can reach a signed-out app state that makes it drop the
    // cold-start join link even though Cordial restored the cookie and identity. A launch that
    // loads a real place writes `.roalt-good`. A launch in flight writes `.roalt-launching`, and
    // `watch_launch` clears it on success or failure. Anything else means the previous launch
    // never proved it loaded a place (a failure, a crash, or RoALT exiting first), so the next
    // launch starts from clean engine storage. This is what makes the retry deterministic instead
    // of leaving it to the same cold-start race.
    let good = dir.join(".roalt-good");
    let launching = dir.join(".roalt-launching");
    if !good.exists() || launching.exists() {
        reset_engine_state(&dir);
        let _ = std::fs::remove_file(&good);
        let _ = std::fs::remove_file(&launching);
    }
    log::info!("Cordial profile ready at {}", dir.display());
}

/// Clear the engine state that can wedge the app shell on a signed-out landing page. The
/// session (`cookies`, `identity`), flags and plugin grants are deliberately kept.
fn reset_engine_state(dir: &Path) {
    for sub in ["data", "run"] {
        match std::fs::remove_dir_all(dir.join(sub)) {
            Ok(()) => log::info!("reset Cordial {sub} for {}", dir.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("could not reset Cordial {sub} for {}: {e}", dir.display()),
        }
    }
    for file in ["local-storage-secrets.json", "window.json", "game-window.json"] {
        match std::fs::remove_file(dir.join(file)) {
            Ok(()) => log::info!("reset Cordial {file} for {}", dir.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("could not reset Cordial {file} for {}: {e}", dir.display()),
        }
    }
}

/// Write the account's `.ROBLOSECURITY` into the profile's Cordial cookie store. The launch
/// pins `CORDIAL_SECRET_STORE=file`, so the loader reads this instead of the keyring. Cordial
/// rewrites the file itself when Roblox rotates or clears the session.
fn write_cookies(user_id: i64, cookie: &Secret) -> Result<()> {
    let Some(root) = profiles_root() else {
        return Err(AppError::Internal(tr!("Cordial's profile folder could not be found.")));
    };
    let dir = root.join(profile_name(user_id));
    ensure_profile(user_id);
    let path = dir.join("cookies");
    // Cordial keeps more than this one cookie after a real sign-in. Overwriting its store on
    // every launch sent the engine through a fresh sign-in, which is what dropped the join
    // link on a second instance. Only seed a profile that has no session yet.
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 0) {
        return Ok(());
    }
    let host_jar = format!(".ROBLOSECURITY={}; path=/", cookie.expose());
    let mut body = String::from(
        "# cordial cookie store v1 -- a live Roblox session. Treat it as a password.\n",
    );
    for host in [".roblox.com", "roblox.com", "www.roblox.com", "apis.roblox.com", "auth.roblox.com"] {
        body.push_str(host);
        body.push('\t');
        body.push_str(&host_jar);
        body.push('\n');
    }
    let tmp = dir.join("cookies.roalt");
    let write = || -> std::io::Result<()> {
        std::fs::write(&tmp, body.as_bytes())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(&tmp, &path)
    };
    write().map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        AppError::Internal(tr!("Could not write the Cordial cookie store: {why}", why = e))
    })
}

/// Remove the plaintext Cordial session and identity once no client is using the profile,
/// and this account's Mocktail cookie. RoALT's encrypted vault still has the account; the next
/// launch seeds the profile again.
pub fn remove_profile_secrets(user_id: i64) {
    if let Some(root) = profiles_root() {
        let dir = root.join(profile_name(user_id));
        for name in ["cookies", "identity", "cookies.roalt", "identity.roalt"] {
            match std::fs::remove_file(dir.join(name)) {
                Ok(()) => log::info!("removed Cordial profile secret {name} for {user_id}"),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => log::warn!(
                    "could not remove Cordial profile {} for {user_id}: {e}",
                    name
                ),
            }
        }
    }
    // Mocktail's cookie is the same live session in a different file. It is only kept while the
    // client runs, so a later launch seeds it from the vault again.
    let auth = mocktail_auth_dir(user_id);
    for name in ["roblox.cookie", "roblox.cookie.roalt"] {
        match std::fs::remove_file(auth.join(name)) {
            Ok(()) => log::info!("removed Mocktail session {name} for {user_id}"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("could not remove Mocktail session {name} for {user_id}: {e}"),
        }
    }
}

/// Cordial's identity store, so the app shell routes straight to the game on the first launch
/// of a new profile instead of landing on the home screen while the cookie is applied.
fn write_identity(user_id: i64, username: &str, display_name: &str) -> Result<()> {
    let Some(root) = profiles_root() else {
        return Err(AppError::Internal(tr!("Cordial's profile folder could not be found.")));
    };
    let dir = root.join(profile_name(user_id));
    ensure_profile(user_id);
    let path = dir.join("identity");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 0) {
        return Ok(());
    }
    let body = serde_json::json!({
        "schema": 1,
        "warning": "a live Roblox identity; treat it as personal data and keep it out of any bug report",
        "userId": user_id,
        "username": username,
        "displayName": display_name,
        "membershipType": 0,
        "isUnder13": false,
        "hasRobloxSubscription": false,
        "countryCode": "",
    });
    let tmp = dir.join("identity.roalt");
    let write = || -> std::io::Result<()> {
        std::fs::write(&tmp, serde_json::to_vec(&body).unwrap_or_default())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(&tmp, &path)
    };
    write().map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        AppError::Internal(tr!("Could not write the Cordial identity store: {why}", why = e))
    })
}

/// A client log RoALT can follow: it starts with the browser tracker id the tracker searches
/// for, and Cordial's stdout/stderr are appended after it.
fn client_log(browser_tracker_id: i64) -> Option<(std::fs::File, PathBuf)> {
    let dir = crate::instances::logwatch::logs_dir()?;
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("could not create the client log folder {}: {e}", dir.display());
        return None;
    }
    let path = dir.join(format!("{browser_tracker_id}_Player_last.log"));
    let mut file = match std::fs::OpenOptions::new().create(true).write(true).truncate(true).open(&path) {
        Ok(f) => f,
        Err(e) => {
            log::warn!("could not create client log {}: {e}", path.display());
            return None;
        }
    };
    use std::io::Write;
    if let Err(e) = writeln!(file, "[RoALT] web launch, websiteBTId is {browser_tracker_id}") {
        log::warn!("could not write client log {}: {e}", path.display());
        return None;
    }
    Some((file, path))
}

/// The place id the engine actually finished loading, from its own `game loaded` line.
///
/// `place 0` is the app shell's own root and appears on every launch, including the runs that end
/// on Home, so only a nonzero id is a join. Two earlier signals were wrong: "the app shell asked
/// to launch an experience" fires before the place loads, and "this link did not reach an
/// experience" fires on runs whose log then shows a real place loaded. Neither is used here.
fn loaded_place(line: &str) -> Option<i64> {
    let id = line.split_once("game loaded: place ")?.1.split_whitespace().next()?.parse::<i64>().ok()?;
    (id > 0).then_some(id)
}

/// Follow this launch's log and mark the profile good only once a real place loaded. If no place
/// loads before the deadline, the marker is removed so `ensure_profile` clears the engine's
/// cached state before the next launch. That makes the retry deterministic instead of leaving the
/// next launch to the same cold-start race.
fn watch_launch(log: PathBuf, profile_dir: PathBuf) {
    std::thread::Builder::new()
        .name("cordial-launch".into())
        .spawn(move || {
            let marker = profile_dir.join(".roalt-good");
            let launching = profile_dir.join(".roalt-launching");
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
            while std::time::Instant::now() < deadline {
                if let Ok(text) = std::fs::read_to_string(&log)
                    && text.lines().any(|line| loaded_place(line).is_some())
                {
                    let _ = std::fs::write(&marker, b"1\n");
                    let _ = std::fs::remove_file(&launching);
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            log::warn!(
                "launch never loaded a place; the next launch will reset this profile's engine state"
            );
            let _ = std::fs::remove_file(&marker);
            let _ = std::fs::remove_file(&launching);
        })
        .ok();
}

/// The direct build to run for this launch, rediscovering it if `find` ran before the build was
/// complete. An explicit `ROALT_LAUNCHER` is the user's choice and is never overridden.
fn direct_build(exe: &ClientExe) -> Option<CordialBuild> {
    if let Some(build) = &exe.direct {
        return Some(build.clone());
    }
    if exe.kind == LauncherKind::Cordial && std::env::var_os("ROALT_LAUNCHER").is_none() {
        return cordial_loader().and_then(|loader| latest_build(&loader));
    }
    None
}

/// The title-bar choice from Cordial's own settings (`~/.config/cordial/shell.json`). The
/// shell normally passes it to the client; a direct launch has to do that itself.
fn cordial_title_bar() -> Option<&'static str> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?
        .join("cordial")
        .join("shell.json");
    let text = std::fs::read_to_string(config).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    match v.get("title_bar").and_then(|v| v.as_str()) {
        Some("hidden") => Some("hidden"),
        Some("compact") => Some("compact"),
        _ => None,
    }
}

/// Cordial's window throttle choice from `shell.json`: visible (the default), unfocused, or off.
fn cordial_throttle() -> Option<&'static str> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?
        .join("cordial")
        .join("shell.json");
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(config).ok()?).ok()?;
    match v.get("throttle").and_then(|v| v.as_str()) {
        Some("visible") => Some("visible"),
        Some("unfocused") => Some("unfocused"),
        Some("off") => Some("off"),
        _ => None,
    }
}

fn kind_for(path: &Path) -> LauncherKind {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_ascii_lowercase();
    if name.contains("mocktail") {
        LauncherKind::Mocktail
    } else if name.contains("cordial") {
        LauncherKind::Cordial
    } else if name.contains("sober") {
        LauncherKind::Sober
    } else if name == "flatpak" {
        LauncherKind::FlatpakCordial
    } else {
        LauncherKind::Handler
    }
}

fn in_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths).map(|d| d.join(name)).find(|p| p.is_file())
}

fn flatpak_installed(id: &str) -> bool {
    Command::new("flatpak")
        .args(["info", id])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Cordial's flags file with the launch's FPS cap added. Everything already in the profile's
/// own file stays, so only the frame target is forced for this launch. `CORDIAL_FLAGS` takes
/// precedence over the profile file, which is why a user's own
/// `DFIntTaskSchedulerTargetFps` no longer silently beats RoALT's setting.
fn flags_with_launch(existing: Option<&str>, fps: u32, extra: &[(&str, &str)]) -> String {
    let mut flags = existing
        .and_then(|text| serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(text).ok())
        .unwrap_or_default();
    for (key, value) in extra {
        flags.insert((*key).into(), (*value).into());
    }
    if fps > 0 {
        flags.insert("DFIntTaskSchedulerTargetFps".into(), serde_json::Value::from(fps));
    }
    serde_json::to_string_pretty(&flags).unwrap_or_else(|_| String::from("{}"))
}

/// Writes a per-profile flags file for this launch. None when there is nothing to apply.
fn write_launch_flags(profile: &str, profile_dir: Option<&Path>, fps: u32, extra: &[(&str, &str)]) -> Option<PathBuf> {
    if fps == 0 && extra.is_empty() {
        return None;
    }
    let source = std::env::var_os("CORDIAL_FLAGS").map(PathBuf::from).or_else(|| profile_dir.map(|d| d.join("flags.json")));
    let existing = source.and_then(|path| std::fs::read_to_string(path).ok());
    let dir = std::env::var_os("ROALT_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(crate::default_dir)
        .join("cordial-flags");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("could not create Cordial flags folder {}: {e}", dir.display());
        return None;
    }
    let path = dir.join(format!("{profile}.json"));
    match std::fs::write(&path, flags_with_launch(existing.as_deref(), fps, extra)) {
        Ok(()) => Some(path),
        Err(e) => {
            log::warn!("could not write Cordial flags file {}: {e}", path.display());
            None
        }
    }
}

/// WebKit workarounds are for RoALT's own window, not the Cordial/Roblox child. Remove the ones
/// this process set for RoALT's own window before spawning the game.
fn clear_webview_env(cmd: &mut Command) {
    if std::env::var_os("ROALT_DESKTOP_GRAPHICS").is_none() {
        return;
    }
    for key in [
        "GDK_BACKEND",
        "WEBKIT_DISABLE_DMABUF_RENDERER",
        "WEBKIT_DISABLE_COMPOSITING_MODE",
        "ROALT_DESKTOP_GRAPHICS",
    ] {
        cmd.env_remove(key);
    }
    if std::env::var_os("ROALT_DESKTOP_LIBGL").is_some() {
        cmd.env_remove("LIBGL_ALWAYS_SOFTWARE");
        cmd.env_remove("ROALT_DESKTOP_LIBGL");
    }
}

/// Starts Mocktail with RoALT's own roots, the join link and, when asked, `--headless`. The FPS
/// cap goes through `MOCKTAIL_FRAME_RATE_LIMIT`; RoALT's Cordial flags, Cage, the preview socket
/// and the Windows launch arguments have no Mocktail equivalent and are not passed.
#[allow(clippy::too_many_arguments)]
fn start_mocktail(
    exe: &ClientExe,
    uri: &Secret,
    user_id: i64,
    cookie: &Secret,
    browser_tracker_id: i64,
    headless: bool,
    keep_running_on_close: bool,
    fps_cap: u32,
) -> Result<(u32, u64)> {
    // Mocktail's lock is per uid and executable, not per data root, and a second launch forwards
    // its URI to the first process. With multi-instance off, refuse rather than let Mocktail
    // reshape this launch. With it on, a per-account copy gets its own executable hash and lock;
    // each copy also gets its own Android data/cache roots below.
    let multi = mocktail_multi();
    if !multi && crate::instances::process::mocktail_running() {
        return Err(AppError::Invalid(tr!(
            "Mocktail supports one Roblox client at a time. Close the running client first."
        )));
    }
    let root = mocktail_root();
    let auth = mocktail_auth_dir(user_id);
    let instance = root.join("instances").join(user_id.to_string());
    // Mocktail's Android runtime root: the platform-cache migration requires the files directory to
    // sit under `<runtime root>/data` (its appStorage lives there), so data/files/cache share that root.
    let android = instance.join("android");
    let android_data = android.join("data");
    let (cache, state, android_runtime, android_files, android_cache, android_external) = if multi {
        (
            instance.join("cache"),
            instance.join("state"),
            android.clone(),
            android_data.join("files"),
            android_data.join("cache"),
            android.join("external"),
        )
    } else {
        (root.join("cache"), root.join("state"), PathBuf::new(), PathBuf::new(), PathBuf::new(), PathBuf::new())
    };
    let dirs = if multi {
        vec![
            &root,
            &auth,
            &instance,
            &cache,
            &state,
            &android_runtime,
            &android_data,
            &android_files,
            &android_cache,
            &android_external,
        ]
    } else {
        vec![&root, &auth, &cache, &state]
    };
    for dir in dirs {
        private_dir(dir).map_err(|e| {
            AppError::Internal(tr!("Could not create Mocktail's data folder: {why}", why = e))
        })?;
    }
    write_mocktail_cookie(&auth, cookie)?;
    if keep_running_on_close {
        log::info!("Mocktail has no keep-running-on-close switch; the setting is not applied");
    }
    let launcher = if multi { mocktail_copy(&exe.path, user_id)? } else { exe.path.clone() };
    let mut cmd = Command::new(&launcher);
    cmd.env("MOCKTAIL_DATA_ROOT", mocktail_data_root())
        .env("MOCKTAIL_AUTH_ROOT", &auth)
        .env("MOCKTAIL_CACHE_ROOT", &cache)
        .env("MOCKTAIL_STATE_ROOT", &state);
    if let Some(dir) = mocktail_runtime_library_dir(&exe.path) {
        cmd.env("MOCKTAIL_RUNTIME_LIBRARY_DIR", dir);
    }
    if multi {
        // The copy sits outside Mocktail's install tree, so its RUNPATH misses the bundle's
        // libraries (e.g. libmocktail_audio_sdl.so). Add them ahead of any existing search path.
        let mut libs = mocktail_lib_dirs(&exe.path);
        if let Some(existing) = std::env::var_os("LD_LIBRARY_PATH") {
            libs.extend(std::env::split_paths(&existing));
        }
        if !libs.is_empty()
            && let Ok(dirs) = std::env::join_paths(libs)
        {
            cmd.env("LD_LIBRARY_PATH", dirs);
        }
    }
    if multi {
        cmd.env("MOCKTAIL_RUNTIME_ROOT", &android_runtime)
            .env("MOCKTAIL_ANDROID_DATA_DIR", &android_data)
            .env("MOCKTAIL_ANDROID_FILES_DIR", &android_files)
            .env("MOCKTAIL_ANDROID_CACHE_DIR", &android_cache)
            .env("MOCKTAIL_ANDROID_EXTERNAL_BASE_DIR", &android_external)
            .env("MOCKTAIL_ANDROID_EXTERNAL_DIR", android_external.join("files"));
    }
    if headless {
        cmd.arg("--headless");
    }
    if fps_cap > 0 {
        cmd.env("MOCKTAIL_FRAME_RATE_LIMIT", fps_cap.to_string());
    }
    // A roblox-player: or roblox: link, the same one the browser's Play button opens.
    cmd.arg("--launch-uri").arg(uri.expose());
    clear_webview_env(&mut cmd);
    let (log, _log_path) = match client_log(browser_tracker_id) {
        Some((file, path)) => (Some(file), Some(path)),
        None => (None, None),
    };
    let (pid, started) = spawn_tracked(cmd, log)?;
    log::info!("Mocktail launch for account {user_id} (pid {pid}, headless: {headless})");
    Ok((pid, started))
}

/// Starts the game with the URI as its argument. Returns the PID of the launcher process and its
/// start time; that process lives as long as the game for Cordial and Sober.
#[allow(clippy::too_many_arguments)]
pub fn start(
    exe: &ClientExe,
    uri: &Secret,
    mobile_uri: Option<&str>,
    extra: &[String],
    _account_id: i64,
    user_id: i64,
    username: &str,
    display_name: &str,
    cookie: &Secret,
    browser_tracker_id: i64,
    headless: bool,
    keep_running_on_close: bool,
    fps_cap: u32,
    linux_flags: &[(&str, &str)],
    headless_resolution: Option<(u32, u32)>,
) -> Result<(u32, u64)> {
    let profile = profile_name(user_id);
    let profile_dir = profiles_root().map(|root| root.join(profile_name(user_id)));
    // The shell/Flatpak fallback also reads the profile's cookie and identity. Write them before
    // branching: a fresh account otherwise lands on Roblox's login page even though RoALT has
    // its cookie.
    if matches!(exe.kind, LauncherKind::Cordial | LauncherKind::FlatpakCordial) {
        write_cookies(user_id, cookie)?;
        write_identity(user_id, username, display_name)?;
        // `ensure_profile` has just consumed the previous outcome; mark this launch in flight so
        // an early close, a crash, or RoALT exiting before the watcher finishes still makes the
        // next launch start clean.
        if let Some(dir) = &profile_dir
            && let Err(e) = std::fs::write(dir.join(".roalt-launching"), b"1\n")
        {
            log::warn!("could not mark Cordial profile {} in flight: {e}", dir.display());
        }
    }
    if let Some(build) = direct_build(exe) {
        // A build directory that lost a file after `find` (an interrupted update, a prune) must
        // not be spawned: a missing engine is a launch that lands on Home instead of a place.
        if !build.loader.is_file() || !build_is_complete(&build.lib_dir) {
            let why = if build.loader.is_file() {
                "Cordial's Roblox build is incomplete"
            } else {
                "Cordial's loader is missing"
            };
            return Err(AppError::Internal(tr!("Could not start Roblox: {why}", why = why)));
        }
        if headless && in_path("cage").is_none() {
            return Err(AppError::Invalid(tr!(
                "Headless launch needs Cage. Install cage, or turn Headless off in Settings."
            )));
        }
        let log = client_log(browser_tracker_id);
        let mut cmd = Command::new(&build.loader);
        cmd.arg("--lib-dir")
            .arg(&build.lib_dir)
            .arg("--apk")
            .arg(&build.apk)
            .args(["--host-libc", "--game-activity", "--run", "0"])
            .arg("--profile")
            .arg(&profile);
        if let Some(flags) = write_launch_flags(&profile, profile_dir.as_deref(), fps_cap, linux_flags) {
            cmd.env("CORDIAL_FLAGS", flags);
        }
        if let Some((w, h)) = headless_resolution
            && std::env::var_os("CORDIAL_RESOLUTION").is_none()
        {
            cmd.env("CORDIAL_RESOLUTION", format!("{w}x{h}"));
        }
        if headless {
            cmd.arg("--headless");
            // FMOD's AAudio path reports every stream as unavailable and gives up, which leaves
            // no output and no fallback to the Java AudioDevice. A headless client has no one to
            // hear it, so skip the mixer and its PipeWire connection entirely.
            if std::env::var_os("CORDIAL_AUDIO").is_none() {
                cmd.env("CORDIAL_AUDIO", "aaudio-refuse");
            }
        }
        cmd.arg("--join-url")
            .arg(mobile_uri.unwrap_or(uri.expose()))
            // Do not carry the one-time browser ticket. Cordial's default is to drop it, and
            // carrying it made the engine abort the whole link when that ticket was not
            // accepted (one account always landed on Home). The profile has the session cookie
            // and identity instead, so the link only needs place/server ids.
            .env("CORDIAL_DEEPLINK_CARRY_TICKET", "0")
            .env("CORDIAL_SECRET_STORE", "file")
            .env("CORDIAL_DEV_CONTROL", "1");
        // These are the settings that measured ~1.0 GB PSS per client on this NVIDIA/Hyprland
        // machine. Each one is a default: an inherited variable from the user still wins.
        for (key, value) in [
            ("CORDIAL_PERFORMANCE", "latency"),
            ("CORDIAL_GAMEMODE", "0"),
            ("CORDIAL_PRESENT_MODE", "immediate"),
        ] {
            if std::env::var_os(key).is_none() {
                cmd.env(key, value);
            }
        }
        // X11 needs a DISPLAY to fall back to. On a Wayland-only session the native backend is
        // left alone rather than being forced into an X server that is not there.
        if std::env::var_os("DISPLAY").is_some() && std::env::var_os("CORDIAL_X11").is_none() {
            cmd.env("CORDIAL_X11", "1");
        }
        if let Some(v) = cordial_title_bar() {
            cmd.env("CORDIAL_TITLE_BAR", v);
        }
        if let Some(v) = cordial_throttle() {
            cmd.env("CORDIAL_THROTTLE", v);
        }
        if keep_running_on_close {
            // Cordial's own switch: closing the game window does not end the client.
            cmd.env("CORDIAL_NO_CLOSE_EXIT", "1");
        }
        clear_webview_env(&mut cmd);
        let (log, path) = match log {
            Some((file, path)) => (Some(file), Some(path)),
            None => (None, None),
        };
        let (pid, started) = spawn_tracked(cmd, log)?;
        if let (Some(path), Some(profile_dir)) = (path, profile_dir) {
            watch_launch(path, profile_dir);
        }
        return Ok((pid, started));
    }
    let mut cmd = Command::new(&exe.path);
    match exe.kind {
        LauncherKind::Cordial if exe.profiles => {
            cmd.args(["--profile", profile.as_str(), "--play", uri.expose()]);
        }
        LauncherKind::FlatpakCordial if exe.profiles => {
            cmd.args(["run", CORDIAL_ID, "--profile", profile.as_str(), "--play", uri.expose()]);
        }
        LauncherKind::Cordial => {
            cmd.arg(uri.expose());
        }
        LauncherKind::FlatpakCordial => {
            cmd.args(["run", CORDIAL_ID, uri.expose()]);
        }
        LauncherKind::Sober => {
            cmd.arg(uri.expose());
        }
        LauncherKind::FlatpakSober => {
            cmd.args(["run", SOBER_ID, uri.expose()]);
        }
        LauncherKind::Handler => {
            cmd.arg(uri.expose());
        }
        // Mocktail gets its own roots and link argument, never the Cordial-only environment
        // applied below.
        LauncherKind::Mocktail => {
            return start_mocktail(
                exe,
                uri,
                user_id,
                cookie,
                browser_tracker_id,
                headless,
                keep_running_on_close,
                fps_cap,
            );
        }
    }
    let cordshell = matches!(exe.kind, LauncherKind::Cordial | LauncherKind::FlatpakCordial);
    if cordshell {
        // The shell hands this environment to the `cordial-run` child it starts, so a host
        // Cordial reads RoALT's file cookie/identity store instead of the desktop keyring. This
        // is the fallback path that used to land on the login page with a perfectly good cookie.
        // A Flatpak's profile lives inside its sandbox home, so its own store choice is left alone.
        if exe.kind == LauncherKind::Cordial {
            cmd.env("CORDIAL_SECRET_STORE", "file");
        }
        // Do not force the one-time browser ticket through. Cordial's own setting decides (its
        // shell only exports "1" when the user enabled it), and its source documents the ticket
        // as unverified and dropped by default.
        cmd.env("CORDIAL_DEEPLINK_CARRY_TICKET", "0");
        if let Some(flags) = write_launch_flags(&profile, profile_dir.as_deref(), fps_cap, linux_flags) {
            cmd.env("CORDIAL_FLAGS", flags);
        }
    }
    // Launch arguments are Roblox flags for the Windows client; the loader does not take them.
    let _ = extra;
    let log = client_log(browser_tracker_id);
    let (log, path) = match log {
        Some((file, path)) => (Some(file), Some(path)),
        None => (None, None),
    };
    let started = spawn_tracked(cmd, log);
    if started.is_ok()
        && cordshell
        && let (Some(path), Some(profile_dir)) = (path, profile_dir)
    {
        watch_launch(path, profile_dir);
    }
    started
}

fn spawn_tracked(mut cmd: Command, log: Option<std::fs::File>) -> Result<(u32, u64)> {
    if let Some(file) = log {
        let err = file.try_clone().map_err(|e| AppError::Internal(e.to_string()))?;
        cmd.stdout(Stdio::from(file)).stderr(Stdio::from(err));
    }
    let child = cmd
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| AppError::Internal(tr!("Could not start Roblox: {why}", why = e)))?;
    let started = crate::instances::process::start_time(child.id()).unwrap_or_else(|| {
        log::warn!("Roblox launcher pid {} start time unreadable; tracking it without one", child.id());
        0
    });
    Ok((child.id(), started))
}

/// A launcher manages its own Roblox build, so RoALT never installs a Windows client over it.
pub fn managed_by_bootstrapper(_exe: &ClientExe) -> bool {
    true
}

pub fn channel() -> Option<String> {
    None
}

pub fn bootstrapper_version(_exe: &ClientExe) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mocktail_copy_finds_the_runtime_bridges() {
        let root = std::env::temp_dir().join(format!("roalt-mocktail-egl-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::create_dir_all(root.join("lib").join("mocktail")).unwrap();
        std::fs::write(root.join("bin").join("mocktail"), b"").unwrap();
        std::fs::write(root.join("lib").join("mocktail").join("libEGL.so"), b"").unwrap();
        assert_eq!(
            mocktail_runtime_library_dir(&root.join("bin").join("mocktail")),
            Some(root.join("lib").join("mocktail").canonicalize().unwrap())
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_mocktail_copy_finds_the_bundled_libraries() {
        let root = std::env::temp_dir().join(format!("roalt-mocktail-libs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::create_dir_all(root.join("lib").join("mocktail")).unwrap();
        std::fs::write(root.join("bin").join("mocktail"), b"").unwrap();
        let dirs = mocktail_lib_dirs(&root.join("bin").join("mocktail"));
        assert!(dirs.contains(&root.join("bin").canonicalize().unwrap()));
        assert!(dirs.contains(&root.join("lib").join("mocktail").canonicalize().unwrap()));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_mocktail_cookie_is_the_bare_cookie_with_a_private_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("roalt-mocktail-{}", uuid::Uuid::new_v4()));
        private_dir(&dir).unwrap();
        assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
        write_mocktail_cookie(&dir, &Secret::new("COOKIEVALUE")).unwrap();
        let path = dir.join("roblox.cookie");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), ".ROBLOSECURITY=COOKIEVALUE");
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        // A second launch replaces it, with no temporary file left behind.
        write_mocktail_cookie(&dir, &Secret::new("SECOND")).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), ".ROBLOSECURITY=SECOND");
        assert!(!dir.join("roblox.cookie.roalt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn knows_the_launcher_from_its_name() {
        assert_eq!(kind_for(Path::new("/usr/bin/mocktail")), LauncherKind::Mocktail);
        assert_eq!(kind_for(Path::new("/home/you/.local/bin/mocktail")), LauncherKind::Mocktail);
        assert_eq!(kind_for(Path::new("/usr/bin/cordial")), LauncherKind::Cordial);
        assert_eq!(kind_for(Path::new("/usr/bin/sober")), LauncherKind::Sober);
        assert_eq!(kind_for(Path::new("flatpak")), LauncherKind::FlatpakCordial);
        assert_eq!(kind_for(Path::new("/usr/bin/xdg-open")), LauncherKind::Handler);
    }

    #[test]
    fn a_loaded_place_is_only_the_engine_reporting_a_real_place() {
        assert_eq!(loaded_place("[roblox] game loaded: place 72858262610964"), Some(72858262610964));
        assert_eq!(loaded_place("  [roblox] game loaded: place 0"), None);
        assert_eq!(loaded_place("the app shell asked to launch an experience"), None);
        assert_eq!(loaded_place("this link did not reach an experience"), None);
    }

    #[test]
    fn an_fps_cap_overrides_the_flag_but_keeps_the_other_ones() {
        let text = flags_with_launch(Some(r#"{"DFIntTaskSchedulerTargetFps":"60","FFlagOther":true}"#), 30, &[("FFlagDebugSkyGray", "True")]);
        let flags: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(flags["DFIntTaskSchedulerTargetFps"], 30);
        assert_eq!(flags["FFlagOther"], true);
        assert_eq!(flags["FFlagDebugSkyGray"], "True");
        assert_eq!(flags_with_launch(None, 240, &[]), "{\n  \"DFIntTaskSchedulerTargetFps\": 240\n}");
        assert_eq!(flags_with_launch(None, 0, &[]), "{}");
    }
}
