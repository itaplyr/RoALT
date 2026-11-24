//! Launching the custom RakNet client instead of Cordial, behind `ROALT_CUSTOM_CLIENT`.
//!
//! The custom client is a CLI (the GPU-free `client/roblox` build) that performs its own HTTPS
//! join and ticket, so RoALT does not fetch a Roblox ticket or build a `roblox-player:` URI for
//! this backend. It still uses the account cookie (written to a short-lived 0600 file, never
//! argv) and the target place/job from the launch.
//!
//! Switch: `ROALT_CUSTOM_CLIENT` = absolute path to the custom binary. Unset (or empty): every
//! launch is Cordial, exactly as before. Related optional variables:
//! - `ROALT_CUSTOM_EXEC` = a Lua file passed as `--exec`.
//! - `ROALT_CUSTOM_NO_WORLD` = set: launch with `RBX_NO_WORLD=1` (session-only mode).
//!
//! Tracking: the returned PID/start time is the custom process itself (`via_bootstrapper` is
//! false), so instance tracking, killing and disposal work unchanged. The client does not write
//! Roblox's own `_Player_` log format, so log-derived state stays at `Starting`/`Joining`; process
//! liveness decides when it exits. Rejoin heuristics therefore use the generic crash path only.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::error::{AppError, Result};
use crate::model::{LaunchInfo, LaunchTarget};
use crate::roblox_api::Client;
use crate::vault::Secret;

/// How long the cookie file is kept after the child starts: enough for its one startup read,
/// then it is unlinked even if the client is still running.
const COOKIE_FILE_GRACE_SECS: u64 = 5;

/// The custom backend is selected, and the binary exists. Empty means unset (Cordial).
pub fn configured() -> Result<Option<PathBuf>> {
    let Some(value) = std::env::var_os("ROALT_CUSTOM_CLIENT") else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(None);
    }
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(AppError::Invalid(tr!(
            "ROALT_CUSTOM_CLIENT must be an absolute path."
        )));
    }
    if !path.is_file() {
        return Err(AppError::Invalid(tr!(
            "The custom client doesn't exist: {path}",
            path = path.display()
        )));
    }
    Ok(Some(path))
}

/// What the custom client can join: a place (optionally one job) or a universe whose root place
/// still has to be looked up. Private servers, share links and follow targets carry join
/// semantics the CLI cannot express, so they are refused instead of silently launching Cordial.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CustomTarget {
    Place {
        place_id: i64,
        job_id: Option<String>,
    },
    Universe {
        universe_id: i64,
    },
}

pub fn target_join(target: &LaunchTarget) -> Result<CustomTarget> {
    match target {
        LaunchTarget::Place { place_id, .. } => Ok(CustomTarget::Place {
            place_id: *place_id,
            job_id: None,
        }),
        LaunchTarget::Server { place_id, job_id } => Ok(CustomTarget::Place {
            place_id: *place_id,
            job_id: Some(job_id.clone()),
        }),
        LaunchTarget::Universe { universe_id } => Ok(CustomTarget::Universe {
            universe_id: *universe_id,
        }),
        LaunchTarget::PrivateLink { .. } | LaunchTarget::PrivateAccess { .. } => {
            Err(AppError::Invalid(tr!(
                "The custom RakNet client can't join private servers. Use Cordial for this link."
            )))
        }
        LaunchTarget::ShareLink { .. } | LaunchTarget::ShareLinkV2 { .. } => {
            Err(AppError::Invalid(tr!(
                "The custom RakNet client can't resolve share links. Paste the game link instead, or use Cordial."
            )))
        }
        LaunchTarget::FollowUser { .. } => Err(AppError::Invalid(tr!(
            "The custom RakNet client can't follow a player. Use a place or server link, or use Cordial."
        ))),
    }
}

/// The place id and optional job id to hand the CLI. A universe costs one API call; the join
/// itself is still the custom client's.
pub async fn resolve_target(api: &Client, target: &LaunchTarget) -> Result<(i64, Option<String>)> {
    match target_join(target)? {
        CustomTarget::Place { place_id, job_id } => Ok((place_id, job_id)),
        CustomTarget::Universe { universe_id } => {
            Ok((super::root_place(api, universe_id).await?, None))
        }
    }
}

/// The exact command line (without argv[0]) for one custom launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomSpec {
    pub place_id: i64,
    pub job_id: Option<String>,
    pub headless: bool,
    pub cookie_file: PathBuf,
    pub exec: Option<PathBuf>,
}

pub fn args(spec: &CustomSpec) -> Vec<String> {
    let mut args = vec![
        "--connection-type=RakNet".to_owned(),
        "--place-id".to_owned(),
        spec.place_id.to_string(),
    ];
    if let Some(job_id) = &spec.job_id {
        args.push("--job-id".to_owned());
        args.push(job_id.clone());
    }
    if spec.headless {
        args.push("--headless".to_owned());
    }
    args.push("--cookie-file".to_owned());
    args.push(spec.cookie_file.to_string_lossy().into_owned());
    args.push("--no-scripts".to_owned());
    args.push("--no-gui".to_owned());
    if let Some(exec) = &spec.exec {
        args.push("--exec".to_owned());
        args.push(exec.to_string_lossy().into_owned());
    }
    args.push("--seconds".to_owned());
    args.push("0".to_owned());
    args
}

/// `ROALT_CUSTOM_EXEC`, when it names a file. A set but missing path is only a warning (no
/// secret is involved); the launch continues without user Lua.
pub fn exec_script() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os("ROALT_CUSTOM_EXEC")?);
    if path.is_file() {
        Some(path)
    } else {
        log::warn!("ROALT_CUSTOM_EXEC is not a file: {}", path.display());
        None
    }
}

/// A cookie file removed when this value drops. Created with mode 0600 in `XDG_RUNTIME_DIR`
/// (or the temp dir), under a random name.
struct CookieFile {
    path: PathBuf,
}

impl CookieFile {
    fn write(cookie: &Secret) -> Result<Self> {
        let dir = cookie_dir();
        let path = dir.join(format!("roalt-cookie-{}.cookie", uuid::Uuid::new_v4()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path).map_err(|e| {
            AppError::Internal(tr!(
                "Could not write the custom client cookie file: {why}",
                why = e
            ))
        })?;
        // The client reads the raw `.ROBLOSECURITY` value once at startup.
        file.write_all(cookie.expose().as_bytes())
            .and_then(|()| file.flush())
            .map_err(|e| {
                let _ = std::fs::remove_file(&path);
                AppError::Internal(tr!(
                    "Could not write the custom client cookie file: {why}",
                    why = e
                ))
            })?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for CookieFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn cookie_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(std::env::temp_dir)
}

/// Starts the custom client and returns its PID and start time, like `client::start`.
/// The cookie file is unlinked shortly after the child starts; when spawning fails it is
/// removed immediately on drop.
#[allow(clippy::too_many_arguments)]
pub fn start(
    exe: &Path,
    place_id: i64,
    job_id: Option<&str>,
    headless: bool,
    cookie: &Secret,
    exec: Option<PathBuf>,
    log: Option<std::fs::File>,
) -> Result<(u32, u64)> {
    let cookie_file = CookieFile::write(cookie)?;
    let spec = CustomSpec {
        place_id,
        job_id: job_id.map(str::to_owned),
        headless,
        cookie_file: cookie_file.path().to_path_buf(),
        exec,
    };
    let mut cmd = Command::new(exe);
    cmd.args(args(&spec)).stdin(Stdio::null());
    match log {
        Some(file) => {
            let err = file
                .try_clone()
                .map_err(|e| AppError::Internal(e.to_string()))?;
            cmd.stdout(Stdio::from(file)).stderr(Stdio::from(err));
        }
        None => {
            cmd.stdout(Stdio::null()).stderr(Stdio::null());
        }
    }
    if std::env::var_os("ROALT_CUSTOM_NO_WORLD").is_some() {
        cmd.env("RBX_NO_WORLD", "1");
    }
    let child = cmd.spawn().map_err(|e| {
        AppError::Internal(tr!("Could not start the custom client: {why}", why = e))
    })?;
    let pid = child.id();
    let started = crate::instances::process::start_time(pid).unwrap_or_else(|| {
        log::warn!("custom client pid {pid} start time unreadable; tracking it without one");
        0
    });
    // The client reads the cookie once at startup. Keep the file just long enough for that,
    // then unlink it even if the client is still running. If the thread can't be started, the
    // moved `CookieFile` is dropped with the closure and the file is removed at once.
    if let Err(e) = std::thread::Builder::new()
        .name("custom-cookie-cleanup".into())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(COOKIE_FILE_GRACE_SECS));
            drop(cookie_file);
        })
    {
        log::warn!("could not schedule the custom client cookie cleanup: {e}");
    }
    Ok((pid, started))
}

/// One custom launch: use the account cookie and target, then start the CLI. No ticket, no
/// player URI, no assertion.
pub async fn launch(
    s: &crate::App,
    account_id: i64,
    target: &LaunchTarget,
    fps_cap: Option<u32>,
    headless: bool,
    exe: PathBuf,
) -> Result<LaunchInfo> {
    let cookie = super::account_cookie(s, account_id)?;
    let (place_id, job_id) = resolve_target(&s.api, target).await?;
    let exec = exec_script();
    let browser_tracker_id = super::uri::new_browser_tracker_id();
    let started_at = crate::commands::now_ms();
    let log = super::client::client_log(browser_tracker_id).map(|(file, _)| file);
    let (pid, pid_start) = start(
        &exe,
        place_id,
        job_id.as_deref(),
        headless,
        &cookie,
        exec,
        log,
    )?;
    log::info!(
        "launched account {account_id} with the custom RakNet client (pid {pid}, place {place_id})"
    );
    Ok(LaunchInfo {
        account_id,
        pid,
        browser_tracker_id,
        place_id: Some(place_id),
        started_at,
        via_bootstrapper: false,
        warning: None,
        fps_cap,
        pid_start: Some(pid_start),
    })
}

/// `launch_check` for the custom backend: the cookie decrypts and the target can be joined.
pub async fn check(
    s: &crate::App,
    account_id: i64,
    target: &LaunchTarget,
    exe: &Path,
) -> Result<()> {
    let _cookie = super::account_cookie(s, account_id)?;
    resolve_target(&s.api, target).await?;
    log::debug!("custom client check: {}", exe.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_is_exact() {
        let cookie = PathBuf::from("/run/user/1000/roalt-cookie-abc.cookie");
        let place = CustomSpec {
            place_id: 1818,
            job_id: None,
            headless: true,
            cookie_file: cookie.clone(),
            exec: None,
        };
        assert_eq!(
            args(&place),
            [
                "--connection-type=RakNet",
                "--place-id",
                "1818",
                "--headless",
                "--cookie-file",
                "/run/user/1000/roalt-cookie-abc.cookie",
                "--no-scripts",
                "--no-gui",
                "--seconds",
                "0",
            ]
        );

        let job = CustomSpec {
            place_id: 4483381587,
            job_id: Some("0d6c3f4e-5b2a-4c1d-9e8f-7a6b5c4d3e2f".into()),
            headless: false,
            cookie_file: cookie.clone(),
            exec: Some(PathBuf::from("/home/me/join.lua")),
        };
        assert_eq!(
            args(&job),
            [
                "--connection-type=RakNet",
                "--place-id",
                "4483381587",
                "--job-id",
                "0d6c3f4e-5b2a-4c1d-9e8f-7a6b5c4d3e2f",
                "--cookie-file",
                "/run/user/1000/roalt-cookie-abc.cookie",
                "--no-scripts",
                "--no-gui",
                "--exec",
                "/home/me/join.lua",
                "--seconds",
                "0",
            ]
        );
        // The cookie value never appears on the command line, only its file path.
        assert!(!args(&place).iter().any(|a| a.contains(".ROBLOSECURITY")));
    }

    #[test]
    fn targets_map_to_place_and_job_or_fail_clearly() {
        assert_eq!(
            target_join(&LaunchTarget::Place {
                place_id: 1,
                launch_data: None
            })
            .unwrap(),
            CustomTarget::Place {
                place_id: 1,
                job_id: None
            }
        );
        assert_eq!(
            target_join(&LaunchTarget::Server {
                place_id: 2,
                job_id: "j".into()
            })
            .unwrap(),
            CustomTarget::Place {
                place_id: 2,
                job_id: Some("j".into())
            }
        );
        assert_eq!(
            target_join(&LaunchTarget::Universe { universe_id: 3 }).unwrap(),
            CustomTarget::Universe { universe_id: 3 }
        );
        for target in [
            LaunchTarget::PrivateLink {
                place_id: 1,
                link_code: "123".into(),
            },
            LaunchTarget::PrivateAccess {
                place_id: 1,
                access_code: "0d6c3f4e-5b2a-4c1d-9e8f-7a6b5c4d3e2f".into(),
            },
            LaunchTarget::ShareLink {
                code: "abc".into(),
                link_type: "Server".into(),
            },
            LaunchTarget::ShareLinkV2 { code: "abc".into() },
            LaunchTarget::FollowUser { user_id: 1 },
        ] {
            assert!(target_join(&target).is_err(), "{target:?}");
        }
    }
}
