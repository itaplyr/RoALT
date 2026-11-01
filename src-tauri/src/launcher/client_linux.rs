//! Finding and starting Roblox on Linux through Cordial or Sober.
//!
//! Cordial supports one profile per account and several clients at once. RoALT launches the
//! loader (`cordial-run`) directly with `--profile roalt_<user id>` and `--join-url`, which is
//! what the Cordial shell itself does after the user picks a profile. That works with Cordial
//! 0.21 (whose shell has no `--profile` command-line option) and with newer shells. If the
//! loader/build isn't available, it falls back to `cordial --profile ... --play ...` on newer
//! Cordials, then a plain link, then Sober. A system `roblox-player` handler is the last resort.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::error::{AppError, Result};
use crate::vault::Secret;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherKind {
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
const SOBER_ID: &str = "org.vinegarhq.Sober";

pub fn find() -> Result<ClientExe> {
    if let Some(path) = std::env::var_os("ROALT_LAUNCHER") {
        let path = PathBuf::from(path);
        let kind = kind_for(&path);
        let profiles = supports_profiles(&path, kind);
        return Ok(ClientExe { path, bootstrapper: true, kind, profiles, direct: None });
    }
    // Preferred: bypass the shell and give the loader the profile directly. The shell's
    // profile picker is what produced "Roblox link waiting" and put every account on the
    // same profile. ROALT_CORDIAL_RUN points at a self-built engine.
    let loader = std::env::var_os("ROALT_CORDIAL_RUN").map(PathBuf::from).or_else(|| in_path("cordial-run"));
    if let Some(loader) = loader {
        if let Some(build) = latest_build() {
            return Ok(ClientExe {
                path: loader.clone(),
                bootstrapper: true,
                kind: LauncherKind::Cordial,
                profiles: true,
                direct: Some(CordialBuild { loader, ..build }),
            });
        }
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
    if let Some(path) = in_path("sober") {
        return Ok(ClientExe { path, bootstrapper: true, kind: LauncherKind::Sober, profiles: false, direct: None });
    }
    if flatpak_installed(SOBER_ID) {
        return Ok(ClientExe {
            path: PathBuf::from("flatpak"),
            bootstrapper: true,
            kind: LauncherKind::FlatpakSober,
            profiles: false,
            direct: None,
        });
    }
    if let Some(path) = in_path("xdg-open") {
        return Ok(ClientExe { path, bootstrapper: true, kind: LauncherKind::Handler, profiles: false, direct: None });
    }
    Err(AppError::Invalid(tr!(
        "Roblox on Linux needs Cordial or Sober. Install Cordial (recommended for multiple clients) or Sober, then try again."
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

/// Cordial's cache root for downloaded Roblox builds.
fn builds_dir() -> Option<PathBuf> {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(cache.join("cordial").join("builds"))
}

/// The newest complete build under Cordial's cache.
fn latest_build() -> Option<CordialBuild> {
    let dir = builds_dir()?;
    let mut builds: Vec<(Vec<u64>, PathBuf)> = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.join("libroblox.so").is_file() && p.join("base.apk").is_file())
        .filter_map(|p| {
            let name = p.file_name()?.to_str()?;
            let key = name.split('.').map(|n| n.parse().unwrap_or(0)).collect();
            Some((key, p))
        })
        .collect();
    builds.sort_by(|a, b| a.0.cmp(&b.0));
    let dir = builds.pop()?.1;
    Some(CordialBuild { loader: in_path("cordial-run")?, lib_dir: dir.clone(), apk: dir.join("base.apk") })
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
pub fn devctl_socket(user_id: i64) -> Option<PathBuf> {
    profile_dir(user_id).map(|dir| dir.join("devctl.sock"))
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
    // succeeds writes `.roalt-good`; a launch that reports the link was dropped removes it
    // (`watch_launch`). So the next launch starts from clean engine storage, automatically.
    if !dir.join(".roalt-good").exists() {
        reset_engine_state(&dir);
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

/// Follow this launch's log: mark the profile good once the link is accepted, and remove the
/// marker if Cordial says the app shell dropped it. The next launch then clears the engine's
/// cached state and tries again.
fn watch_launch(log: PathBuf, profile_dir: PathBuf) {
    std::thread::Builder::new()
        .name("cordial-launch".into())
        .spawn(move || {
            let marker = profile_dir.join(".roalt-good");
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
            while std::time::Instant::now() < deadline {
                if let Ok(text) = std::fs::read_to_string(&log) {
                    if text.contains("the app shell asked to launch an experience")
                        || text.contains("[cordial] game: joined")
                    {
                        let _ = std::fs::write(&marker, b"1\n");
                        return;
                    }
                    if text.contains("this link did not reach an experience") {
                        log::warn!("Cordial dropped the join link; the next launch will reset this profile's engine state");
                        let _ = std::fs::remove_file(&marker);
                        return;
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        })
        .ok();
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
    if name.contains("cordial") {
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
    if let Some(build) = &exe.direct {
        if headless && in_path("cage").is_none() {
            return Err(AppError::Invalid(tr!(
                "Headless launch needs Cage. Install cage, or turn Headless off in Settings."
            )));
        }
        write_cookies(user_id, cookie)?;
        write_identity(user_id, username, display_name)?;
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
    }
    if matches!(exe.kind, LauncherKind::Cordial | LauncherKind::FlatpakCordial) {
        // Cordial drops the launch ticket unless asked; without it the profile stays signed out.
        cmd.env("CORDIAL_DEEPLINK_CARRY_TICKET", "1");
        if let Some(flags) = write_launch_flags(&profile, profile_dir.as_deref(), fps_cap, linux_flags) {
            cmd.env("CORDIAL_FLAGS", flags);
        }
    }
    // Launch arguments are Roblox flags for the Windows client; the loader does not take them.
    let _ = extra;
    let log = client_log(browser_tracker_id).map(|(file, _)| file);
    spawn_tracked(cmd, log)
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
    fn knows_the_launcher_from_its_name() {
        assert_eq!(kind_for(Path::new("/usr/bin/cordial")), LauncherKind::Cordial);
        assert_eq!(kind_for(Path::new("/usr/bin/sober")), LauncherKind::Sober);
        assert_eq!(kind_for(Path::new("flatpak")), LauncherKind::FlatpakCordial);
        assert_eq!(kind_for(Path::new("/usr/bin/xdg-open")), LauncherKind::Handler);
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
