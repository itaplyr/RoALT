//! Roblox login / session windows on Linux (WebKitGTK). Cookie reading and writing go through
//! Tauri's webview cookie API; each window gets its own profile directory where the platform
//! supports it.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tauri::webview::Cookie;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

use crate::error::{AppError, Result};
use crate::roblox_api::endpoints;
use crate::vault::Secret;

const TMP_DIR: &str = "tmp-webview";
const POLL: Duration = Duration::from_millis(700);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const HOME_PAGE: &str = "https://www.roblox.com/home";

static LOGIN_ACTIVE: AtomicBool = AtomicBool::new(false);

pub async fn capture_login(app: &AppHandle, data_dir: &Path) -> Result<Secret> {
    let slot = LoginSlot::take(app)?;
    let id = uuid::Uuid::new_v4();
    let label = format!("login-{id}");
    let profile = data_dir.join(TMP_DIR).join(id.to_string());
    let built = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(parse_url(endpoints::LOGIN_PAGE)?))
        .title(tr!("Sign in to Roblox — RoALT"))
        .inner_size(480.0, 760.0)
        .center()
        .data_directory(profile.clone())
        .incognito(true)
        .devtools(cfg!(debug_assertions))
        .general_autofill_enabled(false)
        .build();
    let window = match built {
        Ok(w) => w,
        Err(e) => {
            cleanup(profile).await;
            return Err(e.into());
        }
    };
    let cookie = wait_for_cookie(app.clone(), label).await;
    if let Err(e) = window.destroy() {
        log::debug!("could not destroy login window login-{id}: {e}");
    }
    drop(slot);
    cleanup(profile).await;
    cookie
}

pub async fn open_session(
    app: &AppHandle,
    data_dir: &Path,
    title: &str,
    cookie: Secret,
    on_close: impl FnOnce(Option<Secret>) + Send + 'static,
) -> Result<()> {
    let home = parse_url(HOME_PAGE)?;
    let id = uuid::Uuid::new_v4();
    let label = format!("session-{id}");
    let profile = data_dir.join(TMP_DIR).join(id.to_string());
    let built = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(parse_url("about:blank")?))
        .title(title)
        .inner_size(1180.0, 800.0)
        .center()
        .data_directory(profile.clone())
        .incognito(true)
        .devtools(cfg!(debug_assertions))
        .general_autofill_enabled(false)
        .build();
    let window = match built {
        Ok(w) => w,
        Err(e) => {
            cleanup(profile).await;
            return Err(e.into());
        }
    };
    let c = Cookie::build((".ROBLOSECURITY", cookie.expose().to_owned()))
        .domain(".roblox.com")
        .path("/")
        .secure(true)
        .http_only(true)
        .same_site(tauri::webview::cookie::SameSite::Lax)
        .build();
    if let Err(e) = window.set_cookie(c) {
        if let Err(e) = window.destroy() {
            log::debug!("could not destroy session window {label}: {e}");
        }
        cleanup(profile).await;
        return Err(AppError::Internal(tr!("Could not set Roblox's cookie: {why}", why = e)));
    }
    let (app, title) = (app.clone(), title.to_owned());
    let pending = Mutex::new(Some(move || end_session(&app, &label, &title, &profile, on_close)));
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            if let Some(end) = take_once(&pending) {
                tauri::async_runtime::spawn_blocking(end);
            }
        }
    });
    window.navigate(home)?;
    Ok(())
}

pub fn sweep_temp_profiles(data_dir: &Path) {
    let entries = match std::fs::read_dir(data_dir.join(TMP_DIR)) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return,
        Err(e) => {
            log::warn!("could not list old browser profiles: {e}");
            return;
        }
    };
    for path in entries.filter_map(|e| e.inspect_err(|e| log::debug!("could not read an old browser profile entry: {e}")).ok()).map(|e| e.path()) {
        let removed = if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        if let Err(e) = removed {
            log::warn!("could not delete old browser profile {}: {e}", path.display());
        }
    }
}

pub fn clear_webview2_env() {}

struct LoginSlot;

impl LoginSlot {
    fn take(app: &AppHandle) -> Result<Self> {
        if !LOGIN_ACTIVE.swap(true, Ordering::AcqRel) {
            return Ok(Self);
        }
        if let Some(w) = app.webview_windows().into_iter().find_map(|(l, w)| l.starts_with("login-").then_some(w)) {
            let _ = w.unminimize();
            let _ = w.set_focus();
        }
        Err(AppError::Invalid(tr!("Finish the Roblox sign-in window that's already open.")))
    }
}

impl Drop for LoginSlot {
    fn drop(&mut self) {
        LOGIN_ACTIVE.store(false, Ordering::Release);
    }
}

pub fn log_process_failures(_window: &WebviewWindow) {}

fn parse_url(s: &str) -> Result<url::Url> {
    s.parse().map_err(|e: url::ParseError| AppError::Internal(e.to_string()))
}

fn take_once<T>(slot: &Mutex<Option<T>>) -> Option<T> {
    slot.lock().unwrap_or_else(|e| e.into_inner()).take()
}

fn end_session(app: &AppHandle, label: &str, title: &str, profile: &Path, on_close: impl FnOnce(Option<Secret>)) {
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.hide();
        let last = match window.cookies().map(pick_roblosecurity) {
            Ok(Some(c)) => Some(c),
            Ok(None) => {
                log::warn!("session window \"{title}\" closed signed out of Roblox; stored cookie not changed");
                None
            }
            Err(e) => {
                log::warn!("could not read the cookies of session window \"{title}\": {e}");
                None
            }
        };
        on_close(last);
        if let Err(e) = window.destroy() {
            log::debug!("could not destroy session window {label}: {e}");
        }
    }
    remove_profile(profile);
}

async fn wait_for_cookie(app: AppHandle, label: String) -> Result<Secret> {
    tauri::async_runtime::spawn_blocking(move || poll_cookie(&app, &label)).await?
}

fn poll_cookie(app: &AppHandle, label: &str) -> Result<Secret> {
    let deadline = Instant::now() + LOGIN_TIMEOUT;
    let mut failures = 0;
    while Instant::now() < deadline {
        let Some(window) = app.get_webview_window(label) else {
            return Err(AppError::Cancelled);
        };
        app.state::<crate::App>().vault.touch();
        match window.cookies() {
            Ok(all) => {
                failures = 0;
                if let Some(cookie) = pick_roblosecurity(all) {
                    return Ok(cookie);
                }
            }
            Err(e) => {
                failures += 1;
                log::debug!("login window {label}: reading cookies failed ({failures}/5): {e}");
                if failures == 5 {
                    return Err(e.into());
                }
            }
        }
        std::thread::sleep(POLL);
    }
    Err(AppError::Invalid(tr!("The Roblox sign-in window timed out after 15 minutes. Try again.")))
}

fn pick_roblosecurity(cookies: Vec<Cookie<'static>>) -> Option<Secret> {
    cookies
        .into_iter()
        .find(|c| c.name() == ".ROBLOSECURITY" && !c.value().is_empty() && c.domain().is_some_and(is_roblox))
        .map(|c| Secret::new(c.value()))
}

fn is_roblox(domain: &str) -> bool {
    let d = domain.trim_start_matches('.').to_ascii_lowercase();
    d == "roblox.com" || d.ends_with(".roblox.com")
}

async fn cleanup(profile: PathBuf) {
    let _ = tauri::async_runtime::spawn_blocking(move || remove_profile(&profile)).await;
}

fn remove_profile(dir: &Path) {
    for attempt in 1..=5 {
        match std::fs::remove_dir_all(dir) {
            Ok(()) => return,
            Err(e) if e.kind() == ErrorKind::NotFound => return,
            Err(e) if attempt == 5 => log::warn!("could not delete browser profile {}: {e}", dir.display()),
            Err(_) => std::thread::sleep(Duration::from_millis(500)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cookie(name: &str, value: &str, domain: &str) -> Cookie<'static> {
        Cookie::build((name.to_owned(), value.to_owned())).domain(domain.to_owned()).build()
    }

    fn picked(cookies: Vec<Cookie<'static>>) -> Option<String> {
        pick_roblosecurity(cookies).map(|s| s.expose().to_owned())
    }

    #[test]
    fn picks_only_a_non_empty_roblox_cookie() {
        let jar = vec![
            cookie(".ROBLOSECURITY", "fake", "evil.com"),
            cookie(".ROBLOSECURITY", "", ".roblox.com"),
            cookie(".ROBLOSECURITY", "good", ".roblox.com"),
        ];
        assert_eq!(picked(jar), Some("good".into()));
        assert_eq!(picked(vec![cookie(".ROBLOSECURITY", "x", "roblox.com.evil.com")]), None);
        assert_eq!(picked(vec![Cookie::new(".ROBLOSECURITY", "no-domain")]), None);
    }

    #[test]
    fn close_path_runs_once() {
        let pending = Mutex::new(Some(7));
        assert_eq!(take_once(&pending), Some(7));
        assert_eq!(take_once(&pending), None);
    }
}
